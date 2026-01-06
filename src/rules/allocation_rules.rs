//! Rules for detecting allocation anti-patterns.

use super::{Diagnostic, Rule, Severity};
use crate::engine::AnalysisContext;
use syn::visit::Visit;
use syn::{Expr, ExprCall, ExprMethodCall, ExprPath};

/// Detects Vec::new() followed by push in a loop without using with_capacity
pub struct VecNoCapacityRule;

impl Rule for VecNoCapacityRule {
    fn id(&self) -> &'static str {
        "vec-no-capacity"
    }

    fn name(&self) -> &'static str {
        "Vec Without Capacity"
    }

    fn description(&self) -> &'static str {
        "Detects Vec::new() followed by push in loop; use with_capacity instead"
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check(&self, ctx: &AnalysisContext) -> Vec<Diagnostic> {
        let mut visitor = VecNoCapacityVisitor {
            ctx,
            diagnostics: Vec::new(),
            vec_vars: std::collections::HashSet::new(),
            loop_depth: 0,
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct VecNoCapacityVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    vec_vars: std::collections::HashSet<String>,
    loop_depth: usize,
}

impl<'ast> Visit<'ast> for VecNoCapacityVisitor<'_> {
    fn visit_local(&mut self, node: &'ast syn::Local) {
        // Check for `let x = Vec::new()` pattern
        if let Some(init) = &node.init {
            if is_vec_new(&init.expr) {
                if let syn::Pat::Ident(pat_ident) = &node.pat {
                    self.vec_vars.insert(pat_ident.ident.to_string());
                }
            }
        }
        syn::visit::visit_local(self, node);
    }

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
        if self.loop_depth > 0 && node.method == "push" {
            // Check if receiver is a tracked Vec variable
            if let Expr::Path(ExprPath { path, .. }) = &*node.receiver {
                if let Some(ident) = path.get_ident() {
                    if self.vec_vars.contains(&ident.to_string()) {
                        let span = node.method.span();
                        let line = span.start().line;
                        let column = span.start().column;

                        self.diagnostics.push(Diagnostic {
                            rule_id: "vec-no-capacity",
                            severity: Severity::Warning,
                            message: format!(
                                "`{}` created with `Vec::new()` then pushed to in loop; use `Vec::with_capacity()` instead",
                                ident
                            ),
                            file_path: self.ctx.file_path.to_path_buf(),
                            line,
                            column,
                            end_line: None,
                            end_column: None,
                            suggestion: Some("Pre-allocate with `Vec::with_capacity(expected_size)`".to_string()),
                            fix: None,
                        });

                        // Remove from tracking to avoid duplicate warnings
                        self.vec_vars.remove(&ident.to_string());
                    }
                }
            }
        }
        syn::visit::visit_expr_method_call(self, node);
    }
}

/// Check if an expression is Vec::new()
fn is_vec_new(expr: &Expr) -> bool {
    match expr {
        Expr::Call(ExprCall { func, .. }) => {
            if let Expr::Path(ExprPath { path, .. }) = &**func {
                let path_str: String = path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::");
                path_str.ends_with("Vec::new") || path_str == "new"
            } else {
                false
            }
        }
        Expr::MethodCall(call) => call.method == "new",
        _ => false,
    }
}

/// Detects format!() macro calls inside loops
pub struct FormatInLoopRule;

impl Rule for FormatInLoopRule {
    fn id(&self) -> &'static str {
        "format-in-loop"
    }

    fn name(&self) -> &'static str {
        "Format in Loop"
    }

    fn description(&self) -> &'static str {
        "Detects format!() inside loops; each call allocates a new String"
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check(&self, ctx: &AnalysisContext) -> Vec<Diagnostic> {
        let mut visitor = FormatInLoopVisitor {
            ctx,
            diagnostics: Vec::new(),
            loop_depth: 0,
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct FormatInLoopVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    loop_depth: usize,
}

impl<'ast> Visit<'ast> for FormatInLoopVisitor<'_> {
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

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        if self.loop_depth > 0 {
            let macro_name = node
                .path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default();

            if macro_name == "format" {
                let span = node.path.segments.last()
                    .map(|s| s.ident.span())
                    .unwrap_or_else(proc_macro2::Span::call_site);
                let line = span.start().line;
                let column = span.start().column;

                self.diagnostics.push(Diagnostic {
                    rule_id: "format-in-loop",
                    severity: Severity::Warning,
                    message: "`format!()` called inside loop; allocates a new String each iteration".to_string(),
                    file_path: self.ctx.file_path.to_path_buf(),
                    line,
                    column,
                    end_line: None,
                    end_column: None,
                    suggestion: Some("Consider using `write!()` to a reusable buffer or moving format outside loop".to_string()),
                    fix: None,
                });
            }
        }
        syn::visit::visit_macro(self, node);
    }
}

/// Detects String concatenation with + operator inside loops
pub struct StringConcatLoopRule;

impl Rule for StringConcatLoopRule {
    fn id(&self) -> &'static str {
        "string-concat-loop"
    }

    fn name(&self) -> &'static str {
        "String Concatenation in Loop"
    }

    fn description(&self) -> &'static str {
        "Detects String + operator inside loops; use push_str() instead"
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check(&self, ctx: &AnalysisContext) -> Vec<Diagnostic> {
        let mut visitor = StringConcatVisitor {
            ctx,
            diagnostics: Vec::new(),
            loop_depth: 0,
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct StringConcatVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    loop_depth: usize,
}

impl<'ast> Visit<'ast> for StringConcatVisitor<'_> {
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

    fn visit_expr_binary(&mut self, node: &'ast syn::ExprBinary) {
        if self.loop_depth > 0 {
            // Check for + or += with strings
            match &node.op {
                syn::BinOp::Add(_) | syn::BinOp::AddAssign(_) => {
                    // Check if either operand looks like a string operation
                    if is_likely_string_expr(&node.left) || is_likely_string_expr(&node.right) {
                        let span = match &node.op {
                            syn::BinOp::Add(t) => t.span,
                            syn::BinOp::AddAssign(t) => t.spans[0],
                            _ => proc_macro2::Span::call_site(),
                        };
                        let line = span.start().line;
                        let column = span.start().column;

                        self.diagnostics.push(Diagnostic {
                            rule_id: "string-concat-loop",
                            severity: Severity::Warning,
                            message: "String concatenation with `+` inside loop; allocates new String each time".to_string(),
                            file_path: self.ctx.file_path.to_path_buf(),
                            line,
                            column,
                            end_line: None,
                            end_column: None,
                            suggestion: Some("Use `String::push_str()` or `write!()` to a buffer instead".to_string()),
                            fix: None,
                        });
                    }
                }
                _ => {}
            }
        }
        syn::visit::visit_expr_binary(self, node);
    }
}

/// Heuristic to detect if an expression is likely a String
fn is_likely_string_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Lit(lit) => matches!(&lit.lit, syn::Lit::Str(_)),
        Expr::MethodCall(call) => {
            let method = call.method.to_string();
            matches!(method.as_str(), "to_string" | "to_owned" | "into" | "format")
        }
        Expr::Macro(m) => {
            m.mac.path.segments.last()
                .map(|s| s.ident == "format")
                .unwrap_or(false)
        }
        Expr::Path(path) => {
            // Could be a String variable - we'll flag conservatively
            path.path.get_ident().is_some()
        }
        _ => false,
    }
}

/// Detects Mutex lock acquired inside loops
pub struct MutexLockInLoopRule;

impl Rule for MutexLockInLoopRule {
    fn id(&self) -> &'static str {
        "mutex-in-loop"
    }

    fn name(&self) -> &'static str {
        "Mutex Lock in Loop"
    }

    fn description(&self) -> &'static str {
        "Detects Mutex::lock() inside loops; acquire lock once outside loop"
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check(&self, ctx: &AnalysisContext) -> Vec<Diagnostic> {
        let mut visitor = MutexLockVisitor {
            ctx,
            diagnostics: Vec::new(),
            loop_depth: 0,
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct MutexLockVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    loop_depth: usize,
}

impl<'ast> Visit<'ast> for MutexLockVisitor<'_> {
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
        if self.loop_depth > 0 {
            let method = node.method.to_string();
            if method == "lock" || method == "try_lock" || method == "read" || method == "write" {
                let span = node.method.span();
                let line = span.start().line;
                let column = span.start().column;

                self.diagnostics.push(Diagnostic {
                    rule_id: "mutex-in-loop",
                    severity: Severity::Warning,
                    message: format!(
                        "`.{}()` called inside loop; consider acquiring lock once outside loop",
                        method
                    ),
                    file_path: self.ctx.file_path.to_path_buf(),
                    line,
                    column,
                    end_line: None,
                    end_column: None,
                    suggestion: Some("Acquire the lock before the loop to reduce lock contention".to_string()),
                    fix: None,
                });
            }
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

    fn check_vec_capacity(source: &str) -> Vec<Diagnostic> {
        let ast = syn::parse_file(source).expect("Failed to parse");
        let config = Config::default();
        let ctx = AnalysisContext::new(Path::new("test.rs"), source, &ast, &config);
        VecNoCapacityRule.check(&ctx)
    }

    fn check_format_loop(source: &str) -> Vec<Diagnostic> {
        let ast = syn::parse_file(source).expect("Failed to parse");
        let config = Config::default();
        let ctx = AnalysisContext::new(Path::new("test.rs"), source, &ast, &config);
        FormatInLoopRule.check(&ctx)
    }

    fn check_string_concat(source: &str) -> Vec<Diagnostic> {
        let ast = syn::parse_file(source).expect("Failed to parse");
        let config = Config::default();
        let ctx = AnalysisContext::new(Path::new("test.rs"), source, &ast, &config);
        StringConcatLoopRule.check(&ctx)
    }

    fn check_mutex_loop(source: &str) -> Vec<Diagnostic> {
        let ast = syn::parse_file(source).expect("Failed to parse");
        let config = Config::default();
        let ctx = AnalysisContext::new(Path::new("test.rs"), source, &ast, &config);
        MutexLockInLoopRule.check(&ctx)
    }

    // Vec capacity tests
    #[test]
    fn test_vec_new_push_in_loop() {
        let source = r#"
            fn test() {
                let mut v = Vec::new();
                for i in 0..100 {
                    v.push(i);
                }
            }
        "#;
        let diagnostics = check_vec_capacity(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("with_capacity"));
    }

    #[test]
    fn test_vec_with_capacity_no_warning() {
        let source = r#"
            fn test() {
                let mut v = Vec::with_capacity(100);
                for i in 0..100 {
                    v.push(i);
                }
            }
        "#;
        let diagnostics = check_vec_capacity(source);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_vec_push_outside_loop_ok() {
        let source = r#"
            fn test() {
                let mut v = Vec::new();
                v.push(1);
                v.push(2);
            }
        "#;
        let diagnostics = check_vec_capacity(source);
        assert!(diagnostics.is_empty());
    }

    // Format in loop tests
    #[test]
    fn test_format_in_for_loop() {
        let source = r#"
            fn test() {
                for i in 0..10 {
                    let s = format!("value: {}", i);
                }
            }
        "#;
        let diagnostics = check_format_loop(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("format!"));
    }

    #[test]
    fn test_format_outside_loop_ok() {
        let source = r#"
            fn test() {
                let s = format!("hello {}", "world");
                for i in 0..10 {
                    println!("{}", s);
                }
            }
        "#;
        let diagnostics = check_format_loop(source);
        assert!(diagnostics.is_empty());
    }

    // String concat tests
    #[test]
    fn test_string_plus_in_loop() {
        let source = r#"
            fn test() {
                let mut s = String::new();
                for word in &["a", "b", "c"] {
                    s = s + word;
                }
            }
        "#;
        let diagnostics = check_string_concat(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("concatenation"));
    }

    #[test]
    fn test_string_plus_assign_in_loop() {
        let source = r#"
            fn test() {
                let mut s = String::new();
                for word in &["a", "b", "c"] {
                    s += word;
                }
            }
        "#;
        let diagnostics = check_string_concat(source);
        assert_eq!(diagnostics.len(), 1);
    }

    // Mutex tests
    #[test]
    fn test_mutex_lock_in_loop() {
        let source = r#"
            fn test(data: &std::sync::Mutex<Vec<i32>>) {
                for i in 0..10 {
                    let mut guard = data.lock().unwrap();
                    guard.push(i);
                }
            }
        "#;
        let diagnostics = check_mutex_loop(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("lock"));
    }

    #[test]
    fn test_mutex_lock_outside_loop_ok() {
        let source = r#"
            fn test(data: &std::sync::Mutex<Vec<i32>>) {
                let mut guard = data.lock().unwrap();
                for i in 0..10 {
                    guard.push(i);
                }
            }
        "#;
        let diagnostics = check_mutex_loop(source);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_rwlock_in_loop() {
        let source = r#"
            fn test(data: &std::sync::RwLock<Vec<i32>>) {
                for i in 0..10 {
                    let guard = data.read().unwrap();
                }
            }
        "#;
        let diagnostics = check_mutex_loop(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("read"));
    }
}
