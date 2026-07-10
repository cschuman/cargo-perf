use super::resolve::{is_std_root, ImportOracle};
use super::visitor::VisitorState;
use super::{Diagnostic, Fix, Replacement, Rule, Severity, MAX_FIX_TEXT_SIZE};
use crate::engine::AnalysisContext;
use syn::visit::Visit;
use syn::{Expr, ExprCall, ExprMethodCall, ExprPath, ImplItemFn, ItemFn, Member};

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
            imports: ImportOracle::from_file(ctx.ast),
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct UnboundedChannelVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    state: VisitorState,
    imports: ImportOracle,
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
    (
        "crossbeam_channel::unbounded",
        "crossbeam_channel::bounded(N)",
        false,
    ),
    (
        "crossbeam::channel::unbounded",
        "crossbeam::channel::bounded(N)",
        false,
    ),
    // flume unbounded
    ("flume::unbounded", "flume::bounded(N)", false),
    // async-channel unbounded
    (
        "async_channel::unbounded",
        "async_channel::bounded(N)",
        false,
    ),
];

/// Patterns that need special handling to avoid false positives
const STD_MPSC_CHANNEL_PATTERNS: &[&str] = &[
    "std::sync::mpsc::channel",
    "sync::mpsc::channel",
    "mpsc::channel",
];

/// Channel replacement patterns for auto-fix
/// Format: (match_suffix, replacement_fn, replacement_call)
const CHANNEL_FIXES: &[(&str, &str, &str)] = &[
    // std::sync::mpsc patterns - replace `channel()` with `sync_channel(32)`
    ("mpsc::channel", "sync_channel", "sync_channel(32)"),
    // tokio patterns - replace `unbounded_channel()` with `channel(32)`
    ("unbounded_channel", "channel", "channel(32)"),
    // crossbeam patterns - replace `unbounded()` with `bounded(32)`
    ("crossbeam_channel::unbounded", "bounded", "bounded(32)"),
    ("crossbeam::channel::unbounded", "bounded", "bounded(32)"),
    // flume patterns
    ("flume::unbounded", "bounded", "bounded(32)"),
    // async-channel patterns
    ("async_channel::unbounded", "bounded", "bounded(32)"),
];

impl UnboundedChannelVisitor<'_> {
    fn report(
        &mut self,
        span: proc_macro2::Span,
        pattern: &str,
        alternative: &str,
        fix: Option<Fix>,
    ) {
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
            fix,
        });
    }

    /// Generate a fix for unbounded channel calls.
    fn generate_fix(&self, node: &ExprCall, path_str: &str) -> Option<Fix> {
        use syn::spanned::Spanned;

        // Find matching fix pattern
        for &(match_suffix, _replacement_fn, replacement_call) in CHANNEL_FIXES {
            if path_str.ends_with(match_suffix) {
                // Skip tokio's bounded channel (already bounded)
                if path_str.contains("tokio") && !path_str.contains("unbounded") {
                    return None;
                }

                // Get the span of the function path (before the parens)
                if let Expr::Path(ExprPath { path, .. }) = &*node.func {
                    let path_span = path.span();
                    let (path_start, path_end) = self.ctx.span_to_byte_range(path_span)?;

                    // Skip fix generation for very large expressions
                    let path_size = path_end.saturating_sub(path_start);
                    if path_size > MAX_FIX_TEXT_SIZE {
                        return None;
                    }

                    // Build the new path by replacing the function name
                    let original_path = self.ctx.source.get(path_start..path_end)?;

                    // Find where the function name starts (after last ::)
                    let fn_name_start = if let Some(pos) = original_path.rfind("::") {
                        pos + 2
                    } else {
                        0
                    };

                    let prefix = &original_path[..fn_name_start];
                    let new_path = format!("{}{}", prefix, replacement_call);

                    // Get the full call span (including parens)
                    let call_span = node.span();
                    let (call_start, call_end) = self.ctx.span_to_byte_range(call_span)?;

                    return Some(Fix {
                        description: format!("Replace with bounded channel: `{}`", new_path),
                        replacements: vec![Replacement {
                            file_path: self.ctx.file_path.to_path_buf(),
                            start_byte: call_start,
                            end_byte: call_end,
                            new_text: new_path,
                        }],
                    });
                }
            }
        }
        None
    }

    fn check_unbounded_channel(
        &mut self,
        node: &ExprCall,
        path_str: &str,
        span: proc_macro2::Span,
    ) {
        // A locally-defined item shadows the outer name: a user `fn
        // unbounded_channel` / `mod flume` is not the runtime primitive.
        let leading = path_str.split("::").next().unwrap_or(path_str);
        if self.imports.is_local_item(leading) {
            return;
        }

        // Special case: std::sync::mpsc::channel is unbounded, but tokio::sync::mpsc::channel is bounded
        // We need to detect std's channel without flagging tokio's
        for &pattern in STD_MPSC_CHANNEL_PATTERNS {
            if path_str.ends_with(pattern) {
                let prefix_len = path_str.len().saturating_sub(pattern.len());
                let is_boundary = prefix_len == 0 || path_str[..prefix_len].ends_with("::");

                if is_boundary {
                    // Make sure it's NOT tokio's channel
                    if !path_str.contains("tokio") {
                        let fix = self.generate_fix(node, path_str);
                        self.report(span, pattern, "std::sync::mpsc::sync_channel(N)", fix);
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
                    let fix = self.generate_fix(node, path_str);
                    self.report(span, pattern, alternative, fix);
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

            self.check_unbounded_channel(node, &path_str, span);
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
const SPAWN_FUNCTIONS: &[&str] = &["spawn", "spawn_local", "spawn_blocking"];

/// Prefixes that indicate async runtime spawn functions
const SPAWN_PREFIXES: &[&str] = &["tokio", "async_std", "smol"];

impl UnboundedSpawnVisitor<'_> {
    fn check_spawn_call(&mut self, path_str: &str, span: proc_macro2::Span) {
        // Check if this is a spawn function
        for &spawn_fn in SPAWN_FUNCTIONS {
            if path_str.ends_with(spawn_fn) {
                // Verify it looks like an async runtime spawn
                let is_runtime_spawn =
                    SPAWN_PREFIXES.iter().any(|p| path_str.contains(p)) || path_str == spawn_fn; // bare `spawn` after `use`

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
                            "Use a Semaphore, buffer_unordered(), or JoinSet with limits"
                                .to_string(),
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
            imports: ImportOracle::from_file(ctx.ast),
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
    imports: ImportOracle,
}

/// If `path` contains a segment equal to `type_leaf`, return the leading
/// segments up to and including it as owned strings, e.g. path
/// `tokio::process::Command::new` with leaf `Command` -> `["tokio", "process",
/// "Command"]`. Returns `None` if the leaf is absent.
fn path_segments_up_to(path: &syn::Path, type_leaf: &str) -> Option<Vec<String>> {
    let idents: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    let pos = idents.iter().position(|s| s == type_leaf)?;
    Some(idents[..=pos].to_vec())
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
    (
        "std::net::TcpStream",
        "connect",
        "tokio::net::TcpStream::connect",
    ),
    (
        "std::net::TcpListener",
        "bind",
        "tokio::net::TcpListener::bind",
    ),
    ("std::net::UdpSocket", "bind", "tokio::net::UdpSocket::bind"),
    // Process operations
    (
        "std::process::Command",
        "output",
        "tokio::process::Command::output",
    ),
    (
        "std::process::Command",
        "status",
        "tokio::process::Command::status",
    ),
    (
        "std::process::Command",
        "spawn",
        "tokio::process::Command::spawn",
    ),
    // IO operations
    (
        "std::io::stdin",
        "read_line",
        "tokio::io::AsyncBufReadExt::read_line",
    ),
    (
        "std::io::Stdin",
        "read_line",
        "tokio::io::AsyncBufReadExt::read_line",
    ),
];

impl AsyncBlockingVisitor<'_> {
    /// Last `::`-separated segment of a module path,
    /// e.g. `"std::process::Command"` -> `"Command"`, `"std::fs"` -> `"fs"`.
    fn module_leaf(module_path: &str) -> &str {
        module_path.rsplit("::").next().unwrap_or(module_path)
    }

    /// True if `full_path` ends with `needle` on a `::` (or start-of-string)
    /// boundary, so `"std::fs::read"` matches `"fs::read"` but `"my_fs::read"`
    /// does not match `"fs::read"`.
    fn path_ends_with_boundary(full_path: &str, needle: &str) -> bool {
        if !full_path.ends_with(needle) {
            return false;
        }
        let prefix_len = full_path.len() - needle.len();
        prefix_len == 0 || full_path[..prefix_len].ends_with("::")
    }

    /// If the receiver chain of a method call syntactically mentions `type_leaf`
    /// as a path segment, return the path segments *up to and including* that
    /// leaf, e.g. `tokio::process::Command::new("ls")` with leaf `Command`
    /// yields `["tokio", "process", "Command"]` and a bare `Command::new(..)`
    /// yields `["Command"]`. Returning the qualifier (rather than a bare bool)
    /// lets the caller canonicalize it through the import oracle and decide
    /// whether it is really the std type — so `.output()` fires on a genuine
    /// `std::process::Command` but not on a user type or the tokio async form.
    fn receiver_type_path(expr: &Expr, type_leaf: &str, depth: usize) -> Option<Vec<String>> {
        if depth > 16 {
            return None;
        }
        match expr {
            Expr::MethodCall(m) => Self::receiver_type_path(&m.receiver, type_leaf, depth + 1),
            Expr::Call(c) => {
                if let Expr::Path(p) = &*c.func {
                    if let Some(segs) = path_segments_up_to(&p.path, type_leaf) {
                        return Some(segs);
                    }
                }
                Self::receiver_type_path(&c.func, type_leaf, depth + 1)
            }
            Expr::Path(p) => path_segments_up_to(&p.path, type_leaf),
            Expr::Reference(r) => Self::receiver_type_path(&r.expr, type_leaf, depth + 1),
            Expr::Paren(p) => Self::receiver_type_path(&p.expr, type_leaf, depth + 1),
            Expr::Field(f) => Self::receiver_type_path(&f.base, type_leaf, depth + 1),
            Expr::Try(t) => Self::receiver_type_path(&t.expr, type_leaf, depth + 1),
            Expr::Await(a) => Self::receiver_type_path(&a.base, type_leaf, depth + 1),
            _ => None,
        }
    }

    /// Path-form blocking call, e.g. `std::fs::read_to_string(..)`. Requires the
    /// module/type qualifier to be present (`fs::read_to_string`,
    /// `Command::output`) so that unrelated paths sharing only a trailing name —
    /// notably `tokio::spawn` vs. `std::process::Command::spawn` — never match.
    fn check_blocking_path_call(
        &mut self,
        full_path: &str,
        span: proc_macro2::Span,
        call_node: Option<&ExprCall>,
    ) {
        // A locally-defined item shadows the outer name: a user `mod fs` /
        // `mod net` / `struct Command` is not the std item, so its calls never
        // block. This is checked before canonicalization because a local item
        // is authoritative regardless of any same-named import.
        let leading = full_path.split("::").next().unwrap_or(full_path);
        if self.imports.is_local_item(leading) {
            return;
        }

        // Rewrite the leading segment through the `use` map so aliases resolve
        // to their canonical path: `sfs::read_to_string` -> `std::fs::…`.
        let canon = self.imports.canonicalize(full_path);

        // Never flag the async replacements themselves when written in full
        // (or reached via an alias that canonicalizes into them).
        if canon.starts_with("tokio::") || canon.starts_with("async_std::") {
            return;
        }

        let mut best: Option<(&str, &str)> = None;
        let mut best_len = 0;
        for (module_path, func_name, alternative) in BLOCKING_CALLS {
            let leaf = Self::module_leaf(module_path);
            let needle = format!("{leaf}::{func_name}");
            if Self::path_ends_with_boundary(&canon, &needle) && needle.len() > best_len {
                best = Some((func_name, alternative));
                best_len = needle.len();
            }
        }

        if let Some((func_name, alternative)) = best {
            let fix = call_node.and_then(|node| self.generate_blocking_fix(node, alternative));
            self.emit_blocking(func_name, alternative, span, fix);
        }
    }

    /// Method-form blocking call, e.g. `cmd.output()`. A bare method name is far
    /// too ambiguous to flag on its own (`builder.output()`, `pool.connect()`,
    /// `atomic.load()` are all common), so this only fires when the receiver
    /// chain corroborates the std type (`Command::new(..).output()`).
    fn check_blocking_method_call(
        &mut self,
        method_name: &str,
        receiver: &Expr,
        span: proc_macro2::Span,
    ) {
        let mut best: Option<(&str, &str)> = None;
        let mut best_len = 0;
        for (module_path, func_name, alternative) in BLOCKING_CALLS {
            if *func_name != method_name {
                continue;
            }
            let leaf = Self::module_leaf(module_path);
            let Some(segs) = Self::receiver_type_path(receiver, leaf, 0) else {
                continue;
            };
            // The receiver mentions the type name; now decide whether it is
            // really the std type. A locally-defined item of that name shadows
            // it (`struct Command`), an alias / import may re-root it, and the
            // tokio/async_std forms are the recommended fix, not a defect.
            let qualifier = segs.first().map(String::as_str).unwrap_or("");
            if self.imports.is_local_item(qualifier) {
                continue;
            }
            let canon = self.imports.canonicalize(&segs.join("::"));
            if canon.starts_with("tokio::") || canon.starts_with("async_std::") {
                continue;
            }
            // Require positive evidence that the receiver type is std-rooted:
            // a fully-qualified `std::…::Command` or a `use` that canonicalizes
            // into std. A bare, unqualified `Command` with no in-file evidence
            // is left alone (precision-first — cargo-perf can't resolve it).
            if !is_std_root(&canon) {
                continue;
            }
            if func_name.len() > best_len {
                best = Some((func_name, alternative));
                best_len = func_name.len();
            }
        }

        if let Some((func_name, alternative)) = best {
            self.emit_blocking(func_name, alternative, span, None);
        }
    }

    fn emit_blocking(
        &mut self,
        func_name: &str,
        alternative: &str,
        span: proc_macro2::Span,
        fix: Option<Fix>,
    ) {
        self.diagnostics.push(Diagnostic {
            rule_id: "async-block-in-async",
            severity: Severity::Error,
            message: format!(
                "Blocking call `{}` inside async function. Use `{}.await` instead.",
                func_name, alternative
            ),
            file_path: self.ctx.file_path.to_path_buf(),
            line: span.start().line,
            column: span.start().column,
            end_line: None,
            end_column: None,
            suggestion: Some(format!("Replace with `{}.await`", alternative)),
            fix,
        });
    }

    /// Generate a fix by replacing the entire call expression with the async alternative + .await.
    fn generate_blocking_fix(&self, node: &ExprCall, alternative: &str) -> Option<Fix> {
        use syn::spanned::Spanned;

        // Get the full call span (function + args)
        let call_span = node.span();
        let (call_start, call_end) = self.ctx.span_to_byte_range(call_span)?;

        // Skip fix generation for very large expressions
        let call_size = call_end.saturating_sub(call_start);
        if call_size > MAX_FIX_TEXT_SIZE {
            return None;
        }

        // Get the original arguments from the source
        // We need to find the args portion (everything after the function path, including parens)
        let original_call = self.ctx.source.get(call_start..call_end)?;

        // Find the opening paren - args start from there
        let paren_pos = original_call.find('(')?;
        let args_with_parens = &original_call[paren_pos..];

        // Build the new call: alternative + args + .await
        let new_text = format!("{}{}.await", alternative, args_with_parens);

        Some(Fix {
            description: format!("Replace with `{}.await`", alternative),
            replacements: vec![Replacement {
                file_path: self.ctx.file_path.to_path_buf(),
                start_byte: call_start,
                end_byte: call_end,
                new_text,
            }],
        })
    }
}

impl<'ast> Visit<'ast> for AsyncBlockingVisitor<'_> {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        if self.state.should_bail() {
            return;
        }
        self.state.enter_expr();
        let was_async = self.in_async_fn;
        self.in_async_fn = node.sig.asyncness.is_some();
        syn::visit::visit_item_fn(self, node);
        self.in_async_fn = was_async;
        self.state.exit_expr();
    }

    fn visit_impl_item_fn(&mut self, node: &'ast ImplItemFn) {
        if self.state.should_bail() {
            return;
        }
        self.state.enter_expr();
        // Async methods in impl blocks (inherent and trait impls) are async
        // functions too; without tracking their asyncness, blocking calls in their
        // bodies were systematically missed (D3, D4).
        let was_async = self.in_async_fn;
        self.in_async_fn = node.sig.asyncness.is_some();
        syn::visit::visit_impl_item_fn(self, node);
        self.in_async_fn = was_async;
        self.state.exit_expr();
    }

    fn visit_expr(&mut self, node: &'ast syn::Expr) {
        if self.state.should_bail() {
            return;
        }
        self.state.enter_expr();
        syn::visit::visit_expr(self, node);
        self.state.exit_expr();
    }

    fn visit_expr_async(&mut self, node: &'ast syn::ExprAsync) {
        // An `async { .. }` block is an async context on its own, independent of
        // the enclosing function. A blocking call inside
        // `tokio::spawn(async { std::fs::read(..) })` written in a *sync* fn still
        // blocks the runtime worker that polls it, so track it (D5). Reached only
        // via `visit_expr`, which already manages recursion depth.
        let was_async = self.in_async_fn;
        self.in_async_fn = true;
        syn::visit::visit_expr_async(self, node);
        self.in_async_fn = was_async;
    }

    fn visit_expr_closure(&mut self, node: &'ast syn::ExprClosure) {
        // A closure body is a fresh execution context. Sync closures are routinely
        // handed to offloaders (`spawn_blocking`, `thread::spawn`, rayon) where
        // blocking is the *correct* thing to do, so a sync closure body is NOT an
        // async context — even inside an async fn — or every offloaded blocking
        // call is a false positive (D5). An `async` closure keeps async context; a
        // nested `async { .. }` re-enables it via `visit_expr_async`. A blocking
        // call in a sync closure that is instead invoked inline is a deliberate
        // known gap: the offload false positive is far more common than that miss.
        let was_async = self.in_async_fn;
        self.in_async_fn = node.asyncness.is_some();
        syn::visit::visit_expr_closure(self, node);
        self.in_async_fn = was_async;
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
                let display_span = path
                    .segments
                    .first()
                    .map(|s| s.ident.span())
                    .unwrap_or_else(proc_macro2::Span::call_site);
                // Pass the full call node for fix generation (to include args and add .await)
                self.check_blocking_path_call(&path_str, display_span, Some(node));
            }
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if self.in_async_fn {
            let method_name = node.method.to_string();
            // Method calls only fire when the receiver chain corroborates the
            // std type; a bare method name is too ambiguous to flag.
            self.check_blocking_method_call(&method_name, &node.receiver, node.method.span());
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
    // Async methods in impl / trait-impl blocks (D3, D4)
    // ========================================================================

    #[test]
    fn test_blocking_call_in_inherent_impl_async_method() {
        // D3: a blocking `std::fs::read_to_string` inside an async method of an
        // inherent impl must fire, just as it does in a free async fn.
        let source = r#"
            struct Worker;
            impl Worker {
                async fn load(&self) -> String {
                    std::fs::read_to_string("config.toml").unwrap()
                }
            }
        "#;
        assert_eq!(
            check_blocking_code(source).len(),
            1,
            "blocking call in an inherent-impl async method must fire: {:?}",
            check_blocking_code(source)
        );
    }

    #[test]
    fn test_blocking_call_in_trait_impl_async_method() {
        // D4: a blocking `std::process::Command::..output()` inside an async method
        // of a trait impl must fire.
        let source = r#"
            trait Runner {
                async fn go(&self);
            }
            struct Shell;
            impl Runner for Shell {
                async fn go(&self) {
                    let _ = std::process::Command::new("ls").arg("-la").output();
                }
            }
        "#;
        assert_eq!(
            check_blocking_code(source).len(),
            1,
            "blocking call in a trait-impl async method must fire: {:?}",
            check_blocking_code(source)
        );
    }

    #[test]
    fn test_blocking_call_in_sync_impl_method_is_silent() {
        // Guard: a blocking call in a NON-async impl method must stay silent — the
        // impl-method handling must track asyncness, not flag every impl method.
        let source = r#"
            struct Worker;
            impl Worker {
                fn load(&self) -> String {
                    std::fs::read_to_string("config.toml").unwrap()
                }
            }
        "#;
        assert!(
            check_blocking_code(source).is_empty(),
            "blocking call in a sync impl method must not fire: {:?}",
            check_blocking_code(source)
        );
    }

    // ========================================================================
    // Blocking-call receiver gating (false-positive guards)
    // ========================================================================

    #[test]
    fn test_tokio_spawn_is_not_blocking() {
        // `tokio::spawn` shares a trailing name with `Command::spawn` but must
        // never be flagged as a blocking call.
        let diags = check_blocking_code(r#"async fn run() { tokio::spawn(async {}); }"#);
        assert!(diags.is_empty(), "tokio::spawn flagged: {diags:?}");
    }

    #[test]
    fn test_custom_method_names_are_not_blocking() {
        // `.output()`, `.connect()`, `.bind()` on arbitrary receivers are common
        // and must not match std::process / std::net blocking calls.
        let diags = check_blocking_code(
            r#"
            async fn run() {
                let _ = Builder.output();
                let _ = pool().connect().await;
                let _ = q().bind(5);
            }
            struct Builder;
            impl Builder { fn output(&self) -> u8 { 0 } }
            "#,
        );
        assert!(diags.is_empty(), "custom methods flagged: {diags:?}");
    }

    #[test]
    fn test_real_command_output_is_blocking() {
        // The receiver chain `Command::new(..).output()` corroborates the std
        // type, so the true positive must survive gating.
        let diags = check_blocking_code(
            r#"async fn run() { let _ = std::process::Command::new("ls").output(); }"#,
        );
        assert_eq!(diags.len(), 1, "expected one blocking finding: {diags:?}");
        assert_eq!(diags[0].rule_id, "async-block-in-async");
    }

    #[test]
    fn test_path_form_std_fs_still_blocks() {
        let diags =
            check_blocking_code(r#"async fn run() { let _ = std::fs::read_to_string("x"); }"#);
        assert_eq!(
            diags.len(),
            1,
            "std::fs::read_to_string must still flag: {diags:?}"
        );
    }

    // ========================================================================
    // Import/shadow oracle gating (D1, D2, D6, D7, D8)
    // ========================================================================

    #[test]
    fn test_user_struct_named_command_not_blocking() {
        // D1: a locally-defined `struct Command` with an `.output()` method is
        // NOT std::process::Command; the receiver `Command::new(..)` mentions
        // "Command" but the item is shadowed in-file.
        let diags = check_blocking_code(
            r#"
            struct Command { label: String }
            impl Command {
                fn new(label: &str) -> Self { Command { label: label.to_string() } }
                fn output(&self) -> String { self.label.clone() }
            }
            async fn run() -> String { Command::new("x").output() }
            "#,
        );
        assert!(diags.is_empty(), "user struct Command flagged: {diags:?}");
    }

    #[test]
    fn test_user_mod_net_tcpstream_not_blocking() {
        // D2: a local `mod net` with its own `TcpStream::connect` is not
        // std::net::TcpStream.
        let diags = check_blocking_code(
            r#"
            mod net {
                pub struct TcpStream;
                impl TcpStream { pub fn connect(_a: &str) -> Self { TcpStream } }
            }
            async fn run() { let _ = net::TcpStream::connect("127.0.0.1:8080"); }
            "#,
        );
        assert!(
            diags.is_empty(),
            "user mod net::TcpStream flagged: {diags:?}"
        );
    }

    #[test]
    fn test_user_mod_fs_read_not_blocking() {
        // D6: a local `mod fs` with a free `read` fn is not std::fs::read.
        let diags = check_blocking_code(
            r#"
            mod fs { pub fn read(_p: &str) -> u8 { 0 } }
            async fn run() { let _ = fs::read("x"); }
            "#,
        );
        assert!(diags.is_empty(), "user mod fs::read flagged: {diags:?}");
    }

    #[test]
    fn test_aliased_std_fs_still_blocks() {
        // D7: `use std::fs as sfs;` then `sfs::read_to_string(..)` is a real
        // blocking call and must fire.
        let diags = check_blocking_code(
            r#"
            use std::fs as sfs;
            async fn run() { let _ = sfs::read_to_string("x"); }
            "#,
        );
        assert_eq!(diags.len(), 1, "aliased std::fs must flag: {diags:?}");
    }

    #[test]
    fn test_tokio_command_output_await_not_blocking() {
        // D8: the CORRECT async form must never be flagged — it is the fix the
        // rule itself recommends.
        let diags = check_blocking_code(
            r#"
            async fn run() -> std::process::Output {
                tokio::process::Command::new("ls").output().await.unwrap()
            }
            "#,
        );
        assert!(
            diags.is_empty(),
            "tokio::process::Command...output().await flagged: {diags:?}"
        );
    }

    #[test]
    fn test_use_std_process_command_output_still_blocks() {
        // Guard the other direction: `use std::process::Command;` then
        // `Command::new(..).output()` must still fire via the use-map.
        let diags = check_blocking_code(
            r#"
            use std::process::Command;
            async fn run() { let _ = Command::new("ls").output(); }
            "#,
        );
        assert_eq!(
            diags.len(),
            1,
            "imported std Command::output must flag: {diags:?}"
        );
    }

    #[test]
    fn test_user_unbounded_channel_fn_not_flagged() {
        // D35: a local `fn unbounded_channel` is unrelated to tokio.
        let diags = check_channel_code(
            r#"
            fn unbounded_channel() -> Vec<u8> { Vec::new() }
            fn run() { let _q = unbounded_channel(); }
            "#,
        );
        assert!(
            diags.is_empty(),
            "user unbounded_channel fn flagged: {diags:?}"
        );
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

    #[test]
    fn test_unbounded_channel_fix_std_mpsc() {
        let source = r#"fn test() { let (tx, rx) = std::sync::mpsc::channel(); }"#;
        let diagnostics = check_channel_code(source);
        assert_eq!(diagnostics.len(), 1);

        let fix = diagnostics[0].fix.as_ref().expect("Should have a fix");
        assert_eq!(fix.replacements.len(), 1);

        let replacement = &fix.replacements[0];
        let mut result = source.to_string();
        result.replace_range(
            replacement.start_byte..replacement.end_byte,
            &replacement.new_text,
        );

        assert!(
            result.contains("sync_channel(32)"),
            "Fix should use sync_channel: {}",
            result
        );
        assert!(
            !result.contains("mpsc::channel()"),
            "Fix should remove unbounded channel: {}",
            result
        );
    }

    #[test]
    fn test_unbounded_channel_fix_tokio() {
        let source = r#"fn test() { let (tx, rx) = tokio::sync::mpsc::unbounded_channel(); }"#;
        let diagnostics = check_channel_code(source);
        assert_eq!(diagnostics.len(), 1);

        let fix = diagnostics[0].fix.as_ref().expect("Should have a fix");
        let replacement = &fix.replacements[0];
        let mut result = source.to_string();
        result.replace_range(
            replacement.start_byte..replacement.end_byte,
            &replacement.new_text,
        );

        assert!(
            result.contains("channel(32)"),
            "Fix should use bounded channel: {}",
            result
        );
        assert!(
            !result.contains("unbounded_channel"),
            "Fix should remove unbounded: {}",
            result
        );
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
        assert!(
            diagnostics.is_empty(),
            "Should not flag blocking calls in sync functions"
        );
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
        assert!(diagnostics[0]
            .suggestion
            .as_ref()
            .unwrap()
            .contains("read_to_string"));
    }

    #[test]
    fn test_async_blocking_fix_fs_read() {
        let source = r#"async fn test() { let _ = std::fs::read_to_string("file.txt"); }"#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 1);

        let fix = diagnostics[0].fix.as_ref().expect("Should have a fix");
        assert_eq!(fix.replacements.len(), 1);

        let replacement = &fix.replacements[0];
        let mut result = source.to_string();
        result.replace_range(
            replacement.start_byte..replacement.end_byte,
            &replacement.new_text,
        );

        assert!(
            result.contains("tokio::fs::read_to_string(\"file.txt\").await"),
            "Fix should use tokio alternative with .await: {}",
            result
        );
        assert!(
            !result.contains("std::fs::read_to_string"),
            "Fix should remove std call: {}",
            result
        );
    }

    #[test]
    fn test_async_blocking_fix_thread_sleep() {
        let source =
            r#"async fn test() { std::thread::sleep(std::time::Duration::from_secs(1)); }"#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 1);

        let fix = diagnostics[0].fix.as_ref().expect("Should have a fix");
        let replacement = &fix.replacements[0];
        let mut result = source.to_string();
        result.replace_range(
            replacement.start_byte..replacement.end_byte,
            &replacement.new_text,
        );

        assert!(
            result.contains("tokio::time::sleep(std::time::Duration::from_secs(1)).await"),
            "Fix should use tokio sleep with .await: {}",
            result
        );
        assert!(
            !result.contains("std::thread::sleep"),
            "Fix should remove std sleep: {}",
            result
        );
    }

    // ========================================================================
    // Batch 9 (D5): async blocks vs sync closures
    // ========================================================================

    #[test]
    fn test_blocking_in_async_block_in_sync_fn_flagged() {
        // D5: a blocking call inside an `async { .. }` block blocks the runtime
        // worker that eventually polls it, even when the *enclosing* function is
        // sync (the block is spawned onto the runtime). The async block, not the
        // fn signature, defines the async context.
        let source = r#"
            fn spawn_work() {
                tokio::spawn(async {
                    let _ = std::fs::read_to_string("config.toml");
                });
            }
        "#;
        assert_eq!(
            check_blocking_code(source).len(),
            1,
            "blocking call in an async block must fire even in a sync fn"
        );
    }

    #[test]
    fn test_blocking_in_sync_closure_in_async_fn_not_flagged() {
        // D5: a sync closure is a new execution context, commonly handed to an
        // offloader (`spawn_blocking`, `thread::spawn`, rayon) where blocking is
        // correct. We must not treat a sync closure body as async, even inside an
        // async fn — otherwise every offloaded blocking call is a false positive.
        let source = r#"
            async fn process() {
                let read_it = || {
                    let _ = std::fs::read_to_string("config.toml");
                };
                let _ = read_it;
            }
        "#;
        assert!(
            check_blocking_code(source).is_empty(),
            "blocking call in a sync closure must not fire: {:?}",
            check_blocking_code(source)
        );
    }

    #[test]
    fn test_blocking_in_async_block_inside_sync_closure_flagged() {
        // The sync-closure suppression must not swallow a nested `async { .. }`:
        // async fn (async) -> sync closure (not async) -> async block (async again).
        // The blocking call sits in the async block, so it must fire.
        let source = r#"
            async fn process() {
                let make = || async {
                    let _ = std::fs::read_to_string("config.toml");
                };
                let _ = make();
            }
        "#;
        assert_eq!(
            check_blocking_code(source).len(),
            1,
            "blocking call in an async block nested in a sync closure must fire"
        );
    }

    #[test]
    fn test_blocking_in_sync_closure_in_sync_fn_not_flagged() {
        // Regression guard: no async context anywhere -> silent.
        let source = r#"
            fn process() {
                let read_it = || {
                    let _ = std::fs::read_to_string("config.toml");
                };
                let _ = read_it;
            }
        "#;
        assert!(check_blocking_code(source).is_empty());
    }
}
