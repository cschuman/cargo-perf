//! Example: Creating a custom rule with cargo-perf's plugin system.
//!
//! This example shows how to create a custom performance rule that can be
//! used alongside cargo-perf's built-in rules.
//!
//! Run with: cargo run --example custom_rule -- path/to/analyze

use cargo_perf::engine::AnalysisContext;
use cargo_perf::plugin::{analyze_with_plugins, PluginRegistry};
use cargo_perf::rules::{Diagnostic, Rule, Severity};
use cargo_perf::Config;
use std::path::Path;
use syn::visit::Visit;
use syn::{Expr, ExprMethodCall};

/// Custom rule: Detects `.unwrap()` calls that could panic.
///
/// This is a simple example rule that flags any use of `.unwrap()`,
/// suggesting `.expect()` or proper error handling instead.
pub struct NoUnwrapRule;

impl Rule for NoUnwrapRule {
    fn id(&self) -> &'static str {
        "no-unwrap"
    }

    fn name(&self) -> &'static str {
        "No Unwrap"
    }

    fn description(&self) -> &'static str {
        "Detects .unwrap() calls that could panic in production"
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check(&self, ctx: &AnalysisContext) -> Vec<Diagnostic> {
        let mut visitor = UnwrapVisitor {
            ctx,
            diagnostics: Vec::new(),
        };
        syn::visit::visit_file(&mut visitor, ctx.ast);
        visitor.diagnostics
    }
}

struct UnwrapVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
}

impl<'ast> Visit<'ast> for UnwrapVisitor<'_> {
    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if node.method == "unwrap" {
            let span = node.method.span();
            let line = span.start().line;
            let column = span.start().column;

            self.diagnostics.push(Diagnostic {
                rule_id: "no-unwrap",
                severity: Severity::Warning,
                message: "`.unwrap()` can panic; use `.expect()` with a message or handle the error"
                    .to_string(),
                file_path: self.ctx.file_path.to_path_buf(),
                line,
                column,
                end_line: None,
                end_column: None,
                suggestion: Some(
                    "Use `.expect(\"descriptive message\")` or proper error handling".to_string(),
                ),
                fix: None,
            });
        }

        // Continue visiting child expressions
        if let Expr::MethodCall(inner) = &*node.receiver {
            self.visit_expr_method_call(inner);
        }
        syn::visit::visit_expr_method_call(self, node);
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).map(|s| s.as_str()).unwrap_or(".");

    // Create a plugin registry with built-in rules + our custom rule
    let mut registry = PluginRegistry::new();

    // Add our custom rule
    registry.add_rule(Box::new(NoUnwrapRule));

    // Load configuration
    let config = Config::load_or_default(Path::new(path))?;

    // Run analysis with our custom registry
    let diagnostics = analyze_with_plugins(Path::new(path), &config, &registry)?;

    // Print results
    if diagnostics.is_empty() {
        println!("No issues found!");
    } else {
        println!("Found {} issue(s):\n", diagnostics.len());
        for diag in &diagnostics {
            println!(
                "  [{:?}] {} at {}:{}",
                diag.severity,
                diag.message,
                diag.file_path.display(),
                diag.line
            );
            if let Some(suggestion) = &diag.suggestion {
                println!("    Suggestion: {}", suggestion);
            }
            println!();
        }
    }

    Ok(())
}
