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
