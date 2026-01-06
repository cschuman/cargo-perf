use super::{Diagnostic, Rule, Severity};
use crate::engine::AnalysisContext;
use syn::visit::Visit;
use syn::ExprMethodCall;

/// Detects .collect() immediately followed by iteration
pub struct CollectThenIterateRule;

impl Rule for CollectThenIterateRule {
    fn id(&self) -> &'static str {
        "collect-then-iterate"
    }

    fn name(&self) -> &'static str {
        "Collect Then Iterate"
    }

    fn description(&self) -> &'static str {
        "Detects .collect::<Vec<_>>() immediately followed by .iter()/.into_iter()"
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check(&self, ctx: &AnalysisContext) -> Vec<Diagnostic> {
        let mut visitor = CollectThenIterateVisitor {
            ctx,
            diagnostics: Vec::new(),
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct CollectThenIterateVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
}

impl<'ast> Visit<'ast> for CollectThenIterateVisitor<'_> {
    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        let method_name = node.method.to_string();

        // Check if this is an iter/into_iter call
        if method_name == "iter" || method_name == "into_iter" {
            // Check if the receiver is a .collect() call
            if let syn::Expr::MethodCall(inner) = &*node.receiver {
                if inner.method == "collect" {
                    let span = node.method.span();
                    let line = span.start().line;
                    let column = span.start().column;

                    self.diagnostics.push(Diagnostic {
                        rule_id: "collect-then-iterate",
                        severity: Severity::Warning,
                        message: "`.collect()` immediately followed by `.iter()`; remove the intermediate collection".to_string(),
                        file_path: self.ctx.file_path.to_path_buf(),
                        line,
                        column,
                        end_line: None,
                        end_column: None,
                        suggestion: Some("Remove `.collect::<Vec<_>>().iter()` and continue the iterator chain".to_string()),
                        fix: None,
                    });
                }
            }
        }

        // Also check for for-loop iteration patterns
        // (handled separately in visit_expr_for_loop if needed)

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
        CollectThenIterateRule.check(&ctx)
    }

    #[test]
    fn test_detects_collect_then_iter() {
        let source = r#"
            fn test() {
                let items = vec![1, 2, 3];
                let _: Vec<_> = items.iter().map(|x| x * 2).collect::<Vec<_>>().iter().map(|x| x + 1).collect();
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("collect"));
        assert!(diagnostics[0].message.contains("iter"));
    }

    #[test]
    fn test_detects_collect_then_into_iter() {
        let source = r#"
            fn test() {
                let items = vec![1, 2, 3];
                let _: i32 = items.iter().collect::<Vec<_>>().into_iter().sum();
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_no_detection_for_legitimate_collect() {
        let source = r#"
            fn test() {
                let items = vec![1, 2, 3];
                let collected: Vec<_> = items.iter().map(|x| x * 2).collect();
                // Using collected later for something else
                println!("{:?}", collected);
                for item in collected.iter() {
                    println!("{}", item);
                }
            }
        "#;
        let diagnostics = check_code(source);
        // No detection because collect and iter are on separate lines/statements
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_no_detection_when_collect_stored() {
        let source = r#"
            fn test() -> Vec<i32> {
                let items = vec![1, 2, 3];
                items.iter().map(|x| x * 2).collect()
            }
        "#;
        let diagnostics = check_code(source);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_detects_in_function_chain() {
        let source = r#"
            fn process(data: &[i32]) -> i32 {
                data.iter()
                    .filter(|x| **x > 0)
                    .collect::<Vec<_>>()
                    .iter()
                    .map(|x| **x * 2)
                    .sum()
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].suggestion.as_ref().unwrap().contains("Remove"));
    }

    #[test]
    fn test_detects_multiple_violations() {
        let source = r#"
            fn test() {
                let a: Vec<_> = vec![1, 2].iter().collect::<Vec<_>>().iter().collect();
                let b: Vec<_> = vec![3, 4].iter().collect::<Vec<_>>().into_iter().collect();
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn test_no_detection_for_other_methods_after_collect() {
        let source = r#"
            fn test() {
                let items = vec![1, 2, 3];
                let len = items.iter().collect::<Vec<_>>().len();
                let first = items.iter().collect::<Vec<_>>().first();
            }
        "#;
        let diagnostics = check_code(source);
        // len() and first() are not iter() or into_iter(), so no detection
        assert!(diagnostics.is_empty());
    }
}
