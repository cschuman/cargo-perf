use super::{Diagnostic, Rule, Severity};
use crate::engine::AnalysisContext;
use syn::visit::Visit;
use syn::{Expr, ExprCall, ExprMethodCall, ExprPath};

/// Detects .clone() calls inside loops on heap-allocated types
pub struct CloneInLoopRule;

impl Rule for CloneInLoopRule {
    fn id(&self) -> &'static str {
        "clone-in-hot-loop"
    }

    fn name(&self) -> &'static str {
        "Clone in Hot Loop"
    }

    fn description(&self) -> &'static str {
        "Detects .clone() calls on heap types inside loops"
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check(&self, ctx: &AnalysisContext) -> Vec<Diagnostic> {
        let mut visitor = CloneInLoopVisitor {
            ctx,
            diagnostics: Vec::new(),
            loop_depth: 0,
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct CloneInLoopVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    loop_depth: usize,
}

impl<'ast> Visit<'ast> for CloneInLoopVisitor<'_> {
    fn visit_expr_for_loop(&mut self, node: &'ast syn::ExprForLoop) {
        self.loop_depth += 1;
        syn::visit::visit_expr_for_loop(self, node);
        self.loop_depth -= 1;
    }

    fn visit_expr_while(&mut self, node: &'ast syn::ExprWhile) {
        self.loop_depth += 1;
        syn::visit::visit_expr_while(self, node);
        self.loop_depth -= 1;
    }

    fn visit_expr_loop(&mut self, node: &'ast syn::ExprLoop) {
        self.loop_depth += 1;
        syn::visit::visit_expr_loop(self, node);
        self.loop_depth -= 1;
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if self.loop_depth > 0 && node.method == "clone" {
            let span = node.method.span();
            let line = span.start().line;
            let column = span.start().column;

            self.diagnostics.push(Diagnostic {
                rule_id: "clone-in-hot-loop",
                severity: Severity::Warning,
                message: "`.clone()` called inside loop; consider borrowing or moving the clone outside".to_string(),
                file_path: self.ctx.file_path.to_path_buf(),
                line,
                column,
                end_line: None,
                end_column: None,
                suggestion: Some("Use a reference or move the clone outside the loop".to_string()),
                fix: None,
            });
        }
        syn::visit::visit_expr_method_call(self, node);
    }
}

/// Detects Regex::new() calls inside loops
pub struct RegexInLoopRule;

impl Rule for RegexInLoopRule {
    fn id(&self) -> &'static str {
        "regex-in-loop"
    }

    fn name(&self) -> &'static str {
        "Regex Compilation in Loop"
    }

    fn description(&self) -> &'static str {
        "Detects Regex::new() inside loops; use lazy_static or once_cell instead"
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check(&self, ctx: &AnalysisContext) -> Vec<Diagnostic> {
        let mut visitor = RegexInLoopVisitor {
            ctx,
            diagnostics: Vec::new(),
            loop_depth: 0,
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct RegexInLoopVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    loop_depth: usize,
}

impl<'ast> Visit<'ast> for RegexInLoopVisitor<'_> {
    fn visit_expr_for_loop(&mut self, node: &'ast syn::ExprForLoop) {
        self.loop_depth += 1;
        syn::visit::visit_expr_for_loop(self, node);
        self.loop_depth -= 1;
    }

    fn visit_expr_while(&mut self, node: &'ast syn::ExprWhile) {
        self.loop_depth += 1;
        syn::visit::visit_expr_while(self, node);
        self.loop_depth -= 1;
    }

    fn visit_expr_loop(&mut self, node: &'ast syn::ExprLoop) {
        self.loop_depth += 1;
        syn::visit::visit_expr_loop(self, node);
        self.loop_depth -= 1;
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if self.loop_depth > 0 {
            if let Expr::Path(ExprPath { path, .. }) = &*node.func {
                let path_str = path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::");

                if path_str.contains("Regex") && path_str.contains("new") {
                    let span = path.segments.last().map(|s| s.ident.span())
                        .unwrap_or_else(proc_macro2::Span::call_site);
                    let line = span.start().line;
                    let column = span.start().column;

                    self.diagnostics.push(Diagnostic {
                        rule_id: "regex-in-loop",
                        severity: Severity::Warning,
                        message: "`Regex::new()` called inside loop; compile regex once outside".to_string(),
                        file_path: self.ctx.file_path.to_path_buf(),
                        line,
                        column,
                        end_line: None,
                        end_column: None,
                        suggestion: Some("Use `lazy_static!` or `once_cell::Lazy` to compile the regex once".to_string()),
                        fix: None,
                    });
                }
            }
        }
        syn::visit::visit_expr_call(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::AnalysisContext;
    use crate::Config;
    use std::path::Path;

    fn check_clone_rule(source: &str) -> Vec<Diagnostic> {
        let ast = syn::parse_file(source).expect("Failed to parse test code");
        let config = Config::default();
        let ctx = AnalysisContext::new(Path::new("test.rs"), source, &ast, &config);
        CloneInLoopRule.check(&ctx)
    }

    fn check_regex_rule(source: &str) -> Vec<Diagnostic> {
        let ast = syn::parse_file(source).expect("Failed to parse test code");
        let config = Config::default();
        let ctx = AnalysisContext::new(Path::new("test.rs"), source, &ast, &config);
        RegexInLoopRule.check(&ctx)
    }

    // Clone in loop tests
    #[test]
    fn test_detects_clone_in_for_loop() {
        let source = r#"
            fn test(items: &[String]) {
                for item in items {
                    let owned = item.clone();
                    println!("{}", owned);
                }
            }
        "#;
        let diagnostics = check_clone_rule(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("clone"));
    }

    #[test]
    fn test_detects_clone_in_while_loop() {
        let source = r#"
            fn test(data: &String) {
                let mut i = 0;
                while i < 10 {
                    let copy = data.clone();
                    i += 1;
                }
            }
        "#;
        let diagnostics = check_clone_rule(source);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_detects_clone_in_loop_loop() {
        let source = r#"
            fn test(data: &String) {
                loop {
                    let copy = data.clone();
                    break;
                }
            }
        "#;
        let diagnostics = check_clone_rule(source);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_detects_clone_in_nested_loops() {
        let source = r#"
            fn test(matrix: &[Vec<String>]) {
                for row in matrix {
                    for cell in row {
                        let copy = cell.clone();
                    }
                }
            }
        "#;
        let diagnostics = check_clone_rule(source);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_no_detection_clone_outside_loop() {
        let source = r#"
            fn test(data: &String) {
                let copy = data.clone();  // Outside loop - OK
                for i in 0..10 {
                    println!("{}", copy);
                }
            }
        "#;
        let diagnostics = check_clone_rule(source);
        assert!(diagnostics.is_empty());
    }

    // Regex in loop tests
    #[test]
    fn test_detects_regex_new_in_for_loop() {
        let source = r#"
            fn test(inputs: &[&str]) {
                for input in inputs {
                    let re = regex::Regex::new(r"\d+").unwrap();
                    if re.is_match(input) {}
                }
            }
        "#;
        let diagnostics = check_regex_rule(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Regex::new"));
    }

    #[test]
    fn test_detects_regex_new_in_while_loop() {
        let source = r#"
            fn test() {
                let mut i = 0;
                while i < 10 {
                    let re = Regex::new(r"test").unwrap();
                    i += 1;
                }
            }
        "#;
        let diagnostics = check_regex_rule(source);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_no_detection_regex_outside_loop() {
        let source = r#"
            fn test(inputs: &[&str]) {
                let re = regex::Regex::new(r"\d+").unwrap();  // Outside loop - OK
                for input in inputs {
                    if re.is_match(input) {}
                }
            }
        "#;
        let diagnostics = check_regex_rule(source);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_loop_depth_resets_after_loop() {
        let source = r#"
            fn test() {
                for i in 0..10 {
                    // Inside loop
                }
                // Outside loop - clone should be OK
                let s = String::from("test");
                let copy = s.clone();
            }
        "#;
        let diagnostics = check_clone_rule(source);
        assert!(diagnostics.is_empty());
    }
}
