use super::visitor::VisitorState;
use super::{Diagnostic, Rule, Severity};
use crate::engine::AnalysisContext;
use syn::visit::Visit;
use syn::{Expr, ExprMethodCall};

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
            state: VisitorState::new(),
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct CollectThenIterateVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    state: VisitorState,
}

/// Iterator-producing method names. If one appears in the receiver chain of a
/// `.collect()`, that `collect` is almost certainly `Iterator::collect` rather
/// than a domain method that merely shares the name.
const ITERATOR_ADAPTERS: &[&str] = &[
    "iter",
    "into_iter",
    "iter_mut",
    "map",
    "filter",
    "filter_map",
    "flat_map",
    "flatten",
    "chars",
    "bytes",
    "enumerate",
    "zip",
    "chain",
    "cloned",
    "copied",
    "rev",
    "take",
    "take_while",
    "skip",
    "skip_while",
    "step_by",
    "scan",
    "peekable",
    "keys",
    "values",
    "drain",
    "lines",
    "split",
    "split_whitespace",
];

/// True if `collect_call` carries a syntactic signal that it is
/// `Iterator::collect` and not a domain method coincidentally named `collect`:
/// either a turbofish (`collect::<Vec<_>>()`) or a recognized iterator adapter
/// somewhere in its receiver chain. Without such a signal we stay silent — a
/// syn-only linter cannot tell `query_builder.collect()` (a domain call) from
/// `it.map(..).collect()` (a real materialization), and firing on the former is
/// the D18 false positive.
fn collect_is_iterator(collect_call: &ExprMethodCall) -> bool {
    if collect_call.turbofish.is_some() {
        return true;
    }
    let mut receiver = &*collect_call.receiver;
    while let Expr::MethodCall(inner) = receiver {
        if ITERATOR_ADAPTERS.contains(&inner.method.to_string().as_str()) {
            return true;
        }
        receiver = &inner.receiver;
    }
    false
}

impl<'ast> Visit<'ast> for CollectThenIterateVisitor<'_> {
    fn visit_expr(&mut self, node: &'ast syn::Expr) {
        if self.state.should_bail() {
            return;
        }
        self.state.enter_expr();
        syn::visit::visit_expr(self, node);
        self.state.exit_expr();
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        let method_name = node.method.to_string();

        // Check if this is an iter/into_iter call
        if method_name == "iter" || method_name == "into_iter" {
            // Check if the receiver is a .collect() call
            if let syn::Expr::MethodCall(inner) = &*node.receiver {
                // Only fire when the `.collect()` is provably `Iterator::collect`
                // (turbofish or an upstream iterator adapter). A domain method
                // named `collect` on a non-iterator — e.g. `QueryBuilder::collect`
                // returning a struct with its own `.iter()` — must stay silent (D18).
                if inner.method == "collect" && collect_is_iterator(inner) {
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
                        // No autofix: deleting the `.collect().iter()` byte range can
                        // change the resulting type/borrow and produce non-compiling
                        // code even on true positives. Advisory-only (D18).
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
        assert!(diagnostics[0]
            .suggestion
            .as_ref()
            .unwrap()
            .contains("Remove"));
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

    #[test]
    fn test_no_autofix_emitted_for_collect_iter() {
        // D18: the byte-splice autofix was removed — deleting `.collect().iter()`
        // can change the resulting type/borrow and break compilation. The
        // diagnostic still fires (turbofish signals a real Iterator::collect) but
        // is advisory-only.
        let source = r#"fn test() {
    let _: i32 = vec![1, 2, 3].iter().collect::<Vec<_>>().iter().sum();
}"#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(
            diagnostics[0].fix.is_none(),
            "autofix must be dropped (advisory-only)"
        );
    }

    #[test]
    fn test_no_autofix_emitted_for_collect_into_iter() {
        let source = r#"fn f() { vec![1].iter().collect::<Vec<_>>().into_iter().sum::<i32>(); }"#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].fix.is_none(), "autofix must be dropped");
    }

    #[test]
    fn test_no_detection_for_domain_collect_without_iterator_signal() {
        // D18: `QueryBuilder::collect()` is a domain method returning a `ResultSet`
        // with its own `.iter()`; there is no iterator materialization to fuse and
        // no turbofish / upstream adapter, so it must stay silent.
        let source = r#"
            struct QueryBuilder;
            struct ResultSet { rows: Vec<i32> }
            impl QueryBuilder {
                fn collect(&self) -> ResultSet { ResultSet { rows: vec![1, 2, 3] } }
            }
            impl ResultSet {
                fn iter(&self) -> std::slice::Iter<'_, i32> { self.rows.iter() }
            }
            fn run(q: &QueryBuilder) -> i32 {
                q.collect().iter().copied().sum()
            }
        "#;
        assert!(
            check_code(source).is_empty(),
            "domain collect() with no iterator signal must not fire"
        );
    }

    #[test]
    fn test_detects_collect_iter_via_upstream_adapter_without_turbofish() {
        // No turbofish, but the `.collect()` receiver chain contains real iterator
        // adapters (`iter`, `map`), so it is genuinely `Iterator::collect`.
        let source = r#"
            fn test(items: &[i32]) -> i32 {
                items.iter().map(|x| x + 1).collect().iter().copied().sum()
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 1);
    }
}
