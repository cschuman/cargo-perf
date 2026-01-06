use super::{Diagnostic, Rule, Severity};
use crate::engine::AnalysisContext;
use syn::visit::Visit;
use syn::{Expr, ExprCall, ExprMethodCall, ExprPath, ItemFn};

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
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct AsyncBlockingVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    in_async_fn: bool,
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
        let was_async = self.in_async_fn;
        self.in_async_fn = node.sig.asyncness.is_some();
        syn::visit::visit_item_fn(self, node);
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

    fn check_code(source: &str) -> Vec<Diagnostic> {
        let ast = syn::parse_file(source).expect("Failed to parse test code");
        let config = Config::default();
        let ctx = AnalysisContext::new(Path::new("test.rs"), source, &ast, &config);
        AsyncBlockInAsyncRule.check(&ctx)
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
