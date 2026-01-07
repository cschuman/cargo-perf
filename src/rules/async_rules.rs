use super::visitor::VisitorState;
use super::{Diagnostic, Rule, Severity};
use crate::engine::AnalysisContext;
use syn::visit::Visit;
use syn::{Expr, ExprCall, ExprMethodCall, ExprPath, ItemFn, Member};

// ============================================================================
// Unbounded Channel Detection
// ============================================================================

/// Detects unbounded channel creation that can cause memory exhaustion.
///
/// Unbounded channels have no backpressure - producers can send unlimited messages
/// while consumers struggle to keep up, leading to unbounded memory growth.
///
/// # Detected Patterns
/// - `std::sync::mpsc::channel()` - unbounded by default
/// - `tokio::sync::mpsc::unbounded_channel()` - explicitly unbounded
/// - `crossbeam_channel::unbounded()` / `crossbeam::channel::unbounded()`
/// - `flume::unbounded()`
/// - `async_channel::unbounded()`
///
/// # Example
/// ```rust,ignore
/// // Bad: Unbounded channel can grow without limit
/// let (tx, rx) = std::sync::mpsc::channel();
/// let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
///
/// // Good: Bounded channel provides backpressure
/// let (tx, rx) = std::sync::mpsc::sync_channel(100);
/// let (tx, rx) = tokio::sync::mpsc::channel(100);
/// ```
pub struct UnboundedChannelRule;

impl Rule for UnboundedChannelRule {
    fn id(&self) -> &'static str {
        "unbounded-channel"
    }

    fn name(&self) -> &'static str {
        "Unbounded Channel"
    }

    fn description(&self) -> &'static str {
        "Detects unbounded channels that can cause memory exhaustion under load"
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check(&self, ctx: &AnalysisContext) -> Vec<Diagnostic> {
        let mut visitor = UnboundedChannelVisitor {
            ctx,
            diagnostics: Vec::new(),
            state: VisitorState::new(),
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct UnboundedChannelVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    state: VisitorState,
}

/// Unbounded channel patterns and their bounded alternatives
/// Format: (pattern_suffix, alternative, require_exact_match)
/// If require_exact_match is true, the pattern must match the entire path
const UNBOUNDED_CHANNELS: &[(&str, &str, bool)] = &[
    // std::sync::mpsc::channel() is unbounded - but we must NOT match tokio::sync::mpsc::channel
    // So we require that if "mpsc::channel" matches, it must NOT be preceded by "tokio"
    // We handle this case specially below

    // tokio unbounded - explicit function name
    ("unbounded_channel", "tokio::sync::mpsc::channel(N)", false),
    // crossbeam unbounded
    ("crossbeam_channel::unbounded", "crossbeam_channel::bounded(N)", false),
    ("crossbeam::channel::unbounded", "crossbeam::channel::bounded(N)", false),
    // flume unbounded
    ("flume::unbounded", "flume::bounded(N)", false),
    // async-channel unbounded
    ("async_channel::unbounded", "async_channel::bounded(N)", false),
];

/// Patterns that need special handling to avoid false positives
const STD_MPSC_CHANNEL_PATTERNS: &[&str] = &[
    "std::sync::mpsc::channel",
    "sync::mpsc::channel",
    "mpsc::channel",
];

impl UnboundedChannelVisitor<'_> {
    fn report(&mut self, span: proc_macro2::Span, pattern: &str, alternative: &str) {
        let line = span.start().line;
        let column = span.start().column;

        self.diagnostics.push(Diagnostic {
            rule_id: "unbounded-channel",
            severity: Severity::Warning,
            message: format!(
                "Unbounded channel `{}` can cause memory exhaustion. Use `{}` instead.",
                pattern, alternative
            ),
            file_path: self.ctx.file_path.to_path_buf(),
            line,
            column,
            end_line: None,
            end_column: None,
            suggestion: Some(format!(
                "Use a bounded channel with explicit capacity: `{}`",
                alternative
            )),
            fix: None,
        });
    }

    fn check_unbounded_channel(&mut self, path_str: &str, span: proc_macro2::Span) {
        // Special case: std::sync::mpsc::channel is unbounded, but tokio::sync::mpsc::channel is bounded
        // We need to detect std's channel without flagging tokio's
        for &pattern in STD_MPSC_CHANNEL_PATTERNS {
            if path_str.ends_with(pattern) {
                let prefix_len = path_str.len().saturating_sub(pattern.len());
                let is_boundary = prefix_len == 0 || path_str[..prefix_len].ends_with("::");

                if is_boundary {
                    // Make sure it's NOT tokio's channel
                    if !path_str.contains("tokio") {
                        self.report(span, pattern, "std::sync::mpsc::sync_channel(N)");
                        return;
                    }
                }
            }
        }

        // Check other unbounded channel patterns
        for &(pattern, alternative, _) in UNBOUNDED_CHANNELS {
            if path_str.ends_with(pattern) {
                let prefix_len = path_str.len().saturating_sub(pattern.len());
                let is_boundary = prefix_len == 0 || path_str[..prefix_len].ends_with("::");

                if is_boundary {
                    self.report(span, pattern, alternative);
                    return;
                }
            }
        }
    }
}

impl<'ast> Visit<'ast> for UnboundedChannelVisitor<'_> {
    fn visit_expr(&mut self, node: &'ast syn::Expr) {
        if self.state.should_bail() {
            return;
        }
        self.state.enter_expr();
        syn::visit::visit_expr(self, node);
        self.state.exit_expr();
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Expr::Path(ExprPath { path, .. }) = &*node.func {
            let path_str = path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join("::");

            let span = path
                .segments
                .last()
                .map(|s| s.ident.span())
                .unwrap_or_else(proc_macro2::Span::call_site);

            self.check_unbounded_channel(&path_str, span);
        }
        syn::visit::visit_expr_call(self, node);
    }
}

// ============================================================================
// Unbounded Task Spawn Detection
// ============================================================================

/// Detects unbounded task spawning in loops that can cause resource exhaustion.
///
/// Spawning tasks in a loop without concurrency limits can:
/// - Exhaust memory by creating too many task handles
/// - Overwhelm the runtime scheduler
/// - Cause thundering herd problems on shared resources
///
/// # Detected Patterns
/// - `tokio::spawn` in loops
/// - `tokio::task::spawn` in loops
/// - `async_std::task::spawn` in loops
/// - `smol::spawn` in loops
///
/// # Example
/// ```rust,ignore
/// // Bad: Unbounded spawning
/// for id in ids {
///     tokio::spawn(process(id));
/// }
///
/// // Good: Use buffered streams or semaphores
/// let semaphore = Arc::new(Semaphore::new(100));
/// for id in ids {
///     let permit = semaphore.clone().acquire_owned().await?;
///     tokio::spawn(async move {
///         let _permit = permit;
///         process(id).await
///     });
/// }
///
/// // Good: Use futures::stream::buffer_unordered
/// futures::stream::iter(ids)
///     .map(|id| process(id))
///     .buffer_unordered(100)
///     .collect::<Vec<_>>()
///     .await;
/// ```
pub struct UnboundedSpawnRule;

impl Rule for UnboundedSpawnRule {
    fn id(&self) -> &'static str {
        "unbounded-spawn"
    }

    fn name(&self) -> &'static str {
        "Unbounded Task Spawning"
    }

    fn description(&self) -> &'static str {
        "Detects task spawning in loops without concurrency limits"
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check(&self, ctx: &AnalysisContext) -> Vec<Diagnostic> {
        let mut visitor = UnboundedSpawnVisitor {
            ctx,
            diagnostics: Vec::new(),
            state: VisitorState::new(),
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct UnboundedSpawnVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    state: VisitorState,
}

/// Task spawn functions that should be bounded when used in loops
const SPAWN_FUNCTIONS: &[&str] = &[
    "spawn",
    "spawn_local",
    "spawn_blocking",
];

/// Prefixes that indicate async runtime spawn functions
const SPAWN_PREFIXES: &[&str] = &[
    "tokio",
    "async_std",
    "smol",
];

impl UnboundedSpawnVisitor<'_> {
    fn check_spawn_call(&mut self, path_str: &str, span: proc_macro2::Span) {
        // Check if this is a spawn function
        for &spawn_fn in SPAWN_FUNCTIONS {
            if path_str.ends_with(spawn_fn) {
                // Verify it looks like an async runtime spawn
                let is_runtime_spawn = SPAWN_PREFIXES.iter().any(|p| path_str.contains(p))
                    || path_str == spawn_fn; // bare `spawn` after `use`

                if is_runtime_spawn {
                    let line = span.start().line;
                    let column = span.start().column;

                    self.diagnostics.push(Diagnostic {
                        rule_id: "unbounded-spawn",
                        severity: Severity::Warning,
                        // cargo-perf-ignore: format-in-loop
                        message: format!(
                            "Task `{}` in loop without concurrency limit can exhaust resources",
                            spawn_fn
                        ),
                        file_path: self.ctx.file_path.to_path_buf(),
                        line,
                        column,
                        end_line: None,
                        end_column: None,
                        suggestion: Some(
                            "Use a Semaphore, buffer_unordered(), or JoinSet with limits".to_string(),
                        ),
                        fix: None,
                    });
                    return;
                }
            }
        }
    }
}

impl<'ast> Visit<'ast> for UnboundedSpawnVisitor<'_> {
    fn visit_expr_for_loop(&mut self, node: &'ast syn::ExprForLoop) {
        if self.state.should_bail() {
            return;
        }
        self.state.enter_loop();
        syn::visit::visit_expr_for_loop(self, node);
        self.state.exit_loop();
    }

    fn visit_expr_while(&mut self, node: &'ast syn::ExprWhile) {
        if self.state.should_bail() {
            return;
        }
        self.state.enter_loop();
        syn::visit::visit_expr_while(self, node);
        self.state.exit_loop();
    }

    fn visit_expr_loop(&mut self, node: &'ast syn::ExprLoop) {
        if self.state.should_bail() {
            return;
        }
        self.state.enter_loop();
        syn::visit::visit_expr_loop(self, node);
        self.state.exit_loop();
    }

    fn visit_expr(&mut self, node: &'ast syn::Expr) {
        if self.state.should_bail() {
            return;
        }
        self.state.enter_expr();
        syn::visit::visit_expr(self, node);
        self.state.exit_expr();
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if self.state.in_loop() {
            if let Expr::Path(ExprPath { path, .. }) = &*node.func {
                let path_str = path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::");

                let span = path
                    .segments
                    .last()
                    .map(|s| s.ident.span())
                    .unwrap_or_else(proc_macro2::Span::call_site);

                self.check_spawn_call(&path_str, span);
            }
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if self.state.in_loop() {
            let method_name = node.method.to_string();
            // Check for .spawn() method calls on known async types
            if (method_name == "spawn" || method_name == "spawn_local")
                && Self::is_async_spawn_receiver(&node.receiver)
            {
                let line = node.method.span().start().line;
                let column = node.method.span().start().column;

                self.diagnostics.push(Diagnostic {
                    rule_id: "unbounded-spawn",
                    severity: Severity::Warning,
                    message: format!(
                        "Task `.{}()` in loop without concurrency limit can exhaust resources",
                        method_name
                    ),
                    file_path: self.ctx.file_path.to_path_buf(),
                    line,
                    column,
                    end_line: None,
                    end_column: None,
                    suggestion: Some(
                        "Use a Semaphore, buffer_unordered(), or JoinSet with limits".to_string(),
                    ),
                    fix: None,
                });
            }
        }
        syn::visit::visit_expr_method_call(self, node);
    }
}

impl UnboundedSpawnVisitor<'_> {
    /// Check if the receiver expression looks like an async runtime type.
    ///
    /// Returns true for known async runtime types that have spawn methods,
    /// false for other types to avoid false positives.
    fn is_async_spawn_receiver(receiver: &Expr) -> bool {
        match receiver {
            // Check for known async types in paths: JoinSet, TaskPool, etc.
            Expr::Path(path) => {
                let path_str = path
                    .path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::");
                // Check both type names (JoinSet) and variable names (join_set)
                Self::is_known_async_spawn_type(&path_str)
                    || Self::is_likely_async_spawn_field(&path_str)
            }
            // Method chains: check if it's a field access on a known type
            Expr::Field(field) => {
                // e.g., self.join_set.spawn() - check the field name
                let field_name = match &field.member {
                    Member::Named(ident) => ident.to_string(),
                    Member::Unnamed(index) => index.index.to_string(),
                };
                Self::is_likely_async_spawn_field(&field_name)
            }
            // Variable reference - check if the name suggests an async type
            Expr::Reference(reference) => Self::is_async_spawn_receiver(&reference.expr),
            // For other expressions, be conservative and don't flag
            // This prevents false positives on custom .spawn() methods
            _ => false,
        }
    }

    /// Check if a type name is a known async runtime spawn type.
    fn is_known_async_spawn_type(type_name: &str) -> bool {
        // Known types that have spawn methods we want to flag
        const KNOWN_ASYNC_TYPES: &[&str] = &[
            "JoinSet",
            "tokio::task::JoinSet",
            "task::JoinSet",
            "LocalSet",
            "tokio::task::LocalSet",
            "TaskPool",
            "bevy::tasks::TaskPool",
            "ComputeTaskPool",
            "AsyncComputeTaskPool",
            "IoTaskPool",
        ];

        KNOWN_ASYNC_TYPES
            .iter()
            .any(|&known| type_name.contains(known))
    }

    /// Check if a field name suggests an async spawn type.
    fn is_likely_async_spawn_field(field_name: &str) -> bool {
        // Field names that commonly hold async spawn types
        const LIKELY_ASYNC_FIELDS: &[&str] = &[
            "join_set",
            "joinset",
            "task_pool",
            "taskpool",
            "local_set",
            "localset",
            "executor",
            "runtime",
        ];

        let lower = field_name.to_lowercase();
        LIKELY_ASYNC_FIELDS.iter().any(|&f| lower.contains(f))
    }
}

// ============================================================================
// Blocking Call Detection
// ============================================================================

/// Detects blocking calls inside async functions
pub struct AsyncBlockInAsyncRule;

impl Rule for AsyncBlockInAsyncRule {
    fn id(&self) -> &'static str {
        "async-block-in-async"
    }

    fn name(&self) -> &'static str {
        "Blocking Call in Async Function"
    }

    fn description(&self) -> &'static str {
        "Detects blocking std calls inside async functions that should use async alternatives"
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, ctx: &AnalysisContext) -> Vec<Diagnostic> {
        let mut visitor = AsyncBlockingVisitor {
            ctx,
            diagnostics: Vec::new(),
            in_async_fn: false,
            state: VisitorState::new(),
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct AsyncBlockingVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    in_async_fn: bool,
    state: VisitorState,
}

/// Known blocking calls that should be avoided in async contexts
/// Format: (module_path, function_name, suggested_alternative)
const BLOCKING_CALLS: &[(&str, &str, &str)] = &[
    // File system operations - order matters, longer matches first
    ("std::fs", "read_to_string", "tokio::fs::read_to_string"),
    ("std::fs", "read_dir", "tokio::fs::read_dir"),
    ("std::fs", "read_link", "tokio::fs::read_link"),
    ("std::fs", "read", "tokio::fs::read"),
    ("std::fs", "write", "tokio::fs::write"),
    ("std::fs", "metadata", "tokio::fs::metadata"),
    ("std::fs", "remove_file", "tokio::fs::remove_file"),
    ("std::fs", "remove_dir", "tokio::fs::remove_dir"),
    ("std::fs", "remove_dir_all", "tokio::fs::remove_dir_all"),
    ("std::fs", "create_dir", "tokio::fs::create_dir"),
    ("std::fs", "create_dir_all", "tokio::fs::create_dir_all"),
    ("std::fs", "copy", "tokio::fs::copy"),
    ("std::fs", "rename", "tokio::fs::rename"),
    ("std::fs::File", "open", "tokio::fs::File::open"),
    ("std::fs::File", "create", "tokio::fs::File::create"),
    // Thread operations
    ("std::thread", "sleep", "tokio::time::sleep"),
    // Network operations
    ("std::net::TcpStream", "connect", "tokio::net::TcpStream::connect"),
    ("std::net::TcpListener", "bind", "tokio::net::TcpListener::bind"),
    ("std::net::UdpSocket", "bind", "tokio::net::UdpSocket::bind"),
    // Process operations
    ("std::process::Command", "output", "tokio::process::Command::output"),
    ("std::process::Command", "status", "tokio::process::Command::status"),
    ("std::process::Command", "spawn", "tokio::process::Command::spawn"),
    // IO operations
    ("std::io::stdin", "read_line", "tokio::io::AsyncBufReadExt::read_line"),
    ("std::io::Stdin", "read_line", "tokio::io::AsyncBufReadExt::read_line"),
];

impl AsyncBlockingVisitor<'_> {
    fn check_blocking_call(&mut self, full_path: &str, span: proc_macro2::Span) {
        // Find the best (most specific) match
        let mut best_match: Option<(&str, &str)> = None;
        let mut best_match_len = 0;

        for (_module_path, func_name, alternative) in BLOCKING_CALLS {
            // Check if the path ends with the function name
            // e.g., "std::fs::read_to_string" ends with "read_to_string"
            // or "thread::sleep" ends with "sleep"
            if full_path.ends_with(func_name) {
                // Verify it's a word boundary (preceded by :: or start of string)
                let prefix_len = full_path.len().saturating_sub(func_name.len());
                let is_boundary = prefix_len == 0
                    || full_path[..prefix_len].ends_with("::");

                if is_boundary && func_name.len() > best_match_len {
                    best_match = Some((func_name, alternative));
                    best_match_len = func_name.len();
                }
            }
        }

        if let Some((func_name, alternative)) = best_match {
            let line = span.start().line;
            let column = span.start().column;

            self.diagnostics.push(Diagnostic {
                rule_id: "async-block-in-async",
                severity: Severity::Error,
                message: format!(
                    "Blocking call `{}` inside async function. Use `{}` instead.",
                    func_name, alternative
                ),
                file_path: self.ctx.file_path.to_path_buf(),
                line,
                column,
                end_line: None,
                end_column: None,
                suggestion: Some(format!("Replace with `{}`", alternative)),
                fix: None,
            });
        }
    }
}

impl<'ast> Visit<'ast> for AsyncBlockingVisitor<'_> {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        if self.state.should_bail() { return; }
        self.state.enter_expr();
        let was_async = self.in_async_fn;
        self.in_async_fn = node.sig.asyncness.is_some();
        syn::visit::visit_item_fn(self, node);
        self.in_async_fn = was_async;
        self.state.exit_expr();
    }

    fn visit_expr(&mut self, node: &'ast syn::Expr) {
        if self.state.should_bail() { return; }
        self.state.enter_expr();
        syn::visit::visit_expr(self, node);
        self.state.exit_expr();
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if self.in_async_fn {
            // Extract the function path
            if let Expr::Path(ExprPath { path, .. }) = &*node.func {
                let path_str = path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::");
                self.check_blocking_call(&path_str, path.segments.first().map(|s| s.ident.span()).unwrap_or_else(proc_macro2::Span::call_site));
            }
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if self.in_async_fn {
            let method_name = node.method.to_string();
            self.check_blocking_call(&method_name, node.method.span());
        }
        syn::visit::visit_expr_method_call(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::AnalysisContext;
    use crate::Config;
    use std::path::Path;

    fn check_blocking_code(source: &str) -> Vec<Diagnostic> {
        let ast = syn::parse_file(source).expect("Failed to parse test code");
        let config = Config::default();
        let ctx = AnalysisContext::new(Path::new("test.rs"), source, &ast, &config);
        AsyncBlockInAsyncRule.check(&ctx)
    }

    fn check_channel_code(source: &str) -> Vec<Diagnostic> {
        let ast = syn::parse_file(source).expect("Failed to parse test code");
        let config = Config::default();
        let ctx = AnalysisContext::new(Path::new("test.rs"), source, &ast, &config);
        UnboundedChannelRule.check(&ctx)
    }

    fn check_spawn_code(source: &str) -> Vec<Diagnostic> {
        let ast = syn::parse_file(source).expect("Failed to parse test code");
        let config = Config::default();
        let ctx = AnalysisContext::new(Path::new("test.rs"), source, &ast, &config);
        UnboundedSpawnRule.check(&ctx)
    }

    // ========================================================================
    // Unbounded Spawn Tests
    // ========================================================================

    #[test]
    fn test_detects_tokio_spawn_in_loop() {
        let source = r#"
            async fn bad(ids: Vec<i32>) {
                for id in ids {
                    tokio::spawn(process(id));
                }
            }
        "#;
        let diagnostics = check_spawn_code(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("spawn"));
    }

    #[test]
    fn test_detects_tokio_task_spawn_in_loop() {
        let source = r#"
            async fn bad(ids: Vec<i32>) {
                for id in ids {
                    tokio::task::spawn(process(id));
                }
            }
        "#;
        let diagnostics = check_spawn_code(source);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_detects_async_std_spawn_in_loop() {
        let source = r#"
            async fn bad(ids: Vec<i32>) {
                for id in ids {
                    async_std::task::spawn(process(id));
                }
            }
        "#;
        let diagnostics = check_spawn_code(source);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_detects_spawn_method_in_loop() {
        let source = r#"
            async fn bad(ids: Vec<i32>) {
                let mut join_set = JoinSet::new();
                for id in ids {
                    join_set.spawn(process(id));
                }
            }
        "#;
        let diagnostics = check_spawn_code(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains(".spawn()"));
    }

    #[test]
    fn test_detects_spawn_in_while_loop() {
        let source = r#"
            async fn bad(rx: Receiver<i32>) {
                while let Some(id) = rx.recv().await {
                    tokio::spawn(process(id));
                }
            }
        "#;
        let diagnostics = check_spawn_code(source);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_no_detection_spawn_outside_loop() {
        let source = r#"
            async fn good() {
                tokio::spawn(process(1));
                tokio::spawn(process(2));
            }
        "#;
        let diagnostics = check_spawn_code(source);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_no_detection_non_runtime_spawn() {
        let source = r#"
            fn good(ids: Vec<i32>) {
                for id in ids {
                    // This is std::thread::spawn, not async runtime spawn
                    // We don't flag this since it's a different pattern
                    my_custom_spawn(id);
                }
            }
        "#;
        let diagnostics = check_spawn_code(source);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_unbounded_spawn_rule_metadata() {
        let rule = UnboundedSpawnRule;
        assert_eq!(rule.id(), "unbounded-spawn");
        assert_eq!(rule.name(), "Unbounded Task Spawning");
        assert_eq!(rule.default_severity(), Severity::Warning);
    }

    // ========================================================================
    // Unbounded Channel Tests
    // ========================================================================

    #[test]
    fn test_detects_std_mpsc_channel() {
        let source = r#"
            fn bad() {
                let (tx, rx) = std::sync::mpsc::channel();
            }
        "#;
        let diagnostics = check_channel_code(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("mpsc::channel"));
        assert!(diagnostics[0].message.contains("sync_channel"));
    }

    #[test]
    fn test_detects_mpsc_channel_use_import() {
        let source = r#"
            use std::sync::mpsc;
            fn bad() {
                let (tx, rx) = mpsc::channel();
            }
        "#;
        let diagnostics = check_channel_code(source);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_detects_tokio_unbounded_channel() {
        let source = r#"
            fn bad() {
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            }
        "#;
        let diagnostics = check_channel_code(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("unbounded_channel"));
    }

    #[test]
    fn test_detects_crossbeam_unbounded() {
        let source = r#"
            fn bad() {
                let (tx, rx) = crossbeam_channel::unbounded();
            }
        "#;
        let diagnostics = check_channel_code(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("unbounded"));
    }

    #[test]
    fn test_detects_flume_unbounded() {
        let source = r#"
            fn bad() {
                let (tx, rx) = flume::unbounded();
            }
        "#;
        let diagnostics = check_channel_code(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("flume::unbounded"));
    }

    #[test]
    fn test_detects_async_channel_unbounded() {
        let source = r#"
            fn bad() {
                let (tx, rx) = async_channel::unbounded();
            }
        "#;
        let diagnostics = check_channel_code(source);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_no_detection_sync_channel() {
        let source = r#"
            fn good() {
                let (tx, rx) = std::sync::mpsc::sync_channel(100);
            }
        "#;
        let diagnostics = check_channel_code(source);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_no_detection_tokio_bounded() {
        let source = r#"
            fn good() {
                let (tx, rx) = tokio::sync::mpsc::channel(100);
            }
        "#;
        let diagnostics = check_channel_code(source);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_no_detection_crossbeam_bounded() {
        let source = r#"
            fn good() {
                let (tx, rx) = crossbeam_channel::bounded(100);
            }
        "#;
        let diagnostics = check_channel_code(source);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_no_detection_flume_bounded() {
        let source = r#"
            fn good() {
                let (tx, rx) = flume::bounded(100);
            }
        "#;
        let diagnostics = check_channel_code(source);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_unbounded_channel_rule_metadata() {
        let rule = UnboundedChannelRule;
        assert_eq!(rule.id(), "unbounded-channel");
        assert_eq!(rule.name(), "Unbounded Channel");
        assert_eq!(rule.default_severity(), Severity::Warning);
    }

    // ========================================================================
    // Blocking Call Tests
    // ========================================================================

    // Keep backward compatibility with existing test function name
    fn check_code(source: &str) -> Vec<Diagnostic> {
        check_blocking_code(source)
    }

    #[test]
    fn test_detects_fs_read_in_async() {
        let source = r#"
            async fn bad() {
                let _ = std::fs::read_to_string("file.txt");
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("read_to_string"));
        assert!(diagnostics[0].message.contains("tokio::fs::read_to_string"));
    }

    #[test]
    fn test_detects_thread_sleep_in_async() {
        let source = r#"
            async fn bad() {
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("sleep"));
        assert!(diagnostics[0].message.contains("tokio::time::sleep"));
    }

    #[test]
    fn test_no_detection_in_sync_function() {
        let source = r#"
            fn sync_fn() {
                let _ = std::fs::read_to_string("file.txt");
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        "#;
        let diagnostics = check_code(source);
        assert!(diagnostics.is_empty(), "Should not flag blocking calls in sync functions");
    }

    #[test]
    fn test_nested_functions_tracked_correctly() {
        let source = r#"
            async fn outer() {
                fn inner_sync() {
                    // This should NOT be flagged - inner_sync is not async
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
                // This SHOULD be flagged - we're back in async context
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        "#;
        let diagnostics = check_code(source);
        // Should only detect the one in the outer async fn, not the inner sync fn
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_detects_multiple_blocking_calls() {
        let source = r#"
            async fn bad() {
                let _ = std::fs::read("file1");
                let _ = std::fs::write("file2", b"data");
                let _ = std::fs::metadata("file3");
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 3);
    }

    #[test]
    fn test_specific_match_over_substring() {
        // Ensure read_to_string is detected as read_to_string, not just read
        let source = r#"
            async fn test() {
                let _ = std::fs::read_to_string("file.txt");
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 1);
        // Should suggest tokio::fs::read_to_string, not tokio::fs::read
        assert!(diagnostics[0].suggestion.as_ref().unwrap().contains("read_to_string"));
    }
}
