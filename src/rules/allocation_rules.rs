//! Rules for detecting allocation anti-patterns.

use super::visitor::VisitorState;
use super::{Diagnostic, Fix, Replacement, Rule, Severity, MAX_FIX_TEXT_SIZE};
use crate::engine::AnalysisContext;
use std::collections::HashSet;
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Expr, ExprCall, ExprMethodCall, ExprPath, FnArg, ItemFn, Local, Pat, Type};

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
            vec_vars: std::collections::HashMap::new(),
            state: VisitorState::new(),
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct VecNoCapacityVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    /// Maps variable name to declaration location (line, column)
    vec_vars: std::collections::HashMap<String, (usize, usize)>,
    state: VisitorState,
}

impl<'ast> Visit<'ast> for VecNoCapacityVisitor<'_> {
    fn visit_local(&mut self, node: &'ast syn::Local) {
        // A `let` binding classifies the name: `let v = Vec::new()` tracks it, but
        // rebinding the same name to anything else (e.g. a shadow
        // `let v = Vec::with_capacity(n)`) must CLEAR the stale entry, or the new
        // binding inherits the old one's "needs capacity" status and is falsely flagged.
        if let syn::Pat::Ident(pat_ident) = &node.pat {
            let name = pat_ident.ident.to_string();
            if node.init.as_ref().is_some_and(|init| is_vec_new(&init.expr)) {
                let span = pat_ident.ident.span();
                self.vec_vars
                    .insert(name, (span.start().line, span.start().column));
            } else {
                self.vec_vars.remove(&name);
            }
        }
        syn::visit::visit_local(self, node);
    }

    fn visit_expr_assign(&mut self, node: &'ast syn::ExprAssign) {
        // Reassignment (`v = Vec::with_capacity(n)`) is a rebind too: it replaces the
        // backing allocation, so the prior `Vec::new()` classification no longer holds.
        if let Expr::Path(ExprPath {
            path, qself: None, ..
        }) = &*node.left
        {
            if let Some(ident) = path.get_ident() {
                let name = ident.to_string();
                if is_vec_new(&node.right) {
                    let span = ident.span();
                    self.vec_vars
                        .insert(name, (span.start().line, span.start().column));
                } else {
                    self.vec_vars.remove(&name);
                }
            }
        }
        syn::visit::visit_expr_assign(self, node);
    }

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

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if self.state.in_loop() && node.method == "push" {
            // Check if receiver is a tracked Vec variable
            if let Expr::Path(ExprPath { path, .. }) = &*node.receiver {
                if let Some(ident) = path.get_ident() {
                    let var_name = ident.to_string();
                    if let Some(&(decl_line, decl_column)) = self.vec_vars.get(&var_name) {
                        // Report at declaration location (where fix would be applied)
                        self.diagnostics.push(Diagnostic {
                            rule_id: "vec-no-capacity",
                            severity: Severity::Warning,
                            message: format!(
                                "`{}` created with `Vec::new()` then pushed to in loop; use `Vec::with_capacity()` instead",
                                ident
                            ),
                            file_path: self.ctx.file_path.to_path_buf(),
                            line: decl_line,
                            column: decl_column,
                            end_line: None,
                            end_column: None,
                            suggestion: Some("Pre-allocate with `Vec::with_capacity(expected_size)`".to_string()),
                            fix: None,
                        });

                        // Remove from tracking to avoid duplicate warnings
                        self.vec_vars.remove(&var_name);
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
                // Only match Vec::new, not bare "new" (which could be any type)
                path_str.ends_with("Vec::new")
            } else {
                false
            }
        }
        // Removed: MethodCall branch that matched any .new() call
        _ => false,
    }
}

/// Detects HashMap::new() followed by insert in a loop without using with_capacity
pub struct HashMapNoCapacityRule;

impl Rule for HashMapNoCapacityRule {
    fn id(&self) -> &'static str {
        "hashmap-no-capacity"
    }

    fn name(&self) -> &'static str {
        "HashMap Without Capacity"
    }

    fn description(&self) -> &'static str {
        "Detects HashMap::new() followed by insert in loop; use with_capacity instead"
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check(&self, ctx: &AnalysisContext) -> Vec<Diagnostic> {
        let mut visitor = HashMapNoCapacityVisitor {
            ctx,
            diagnostics: Vec::new(),
            map_vars: std::collections::HashMap::new(),
            state: VisitorState::new(),
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct HashMapNoCapacityVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    /// Maps variable name to declaration location (line, column)
    map_vars: std::collections::HashMap<String, (usize, usize)>,
    state: VisitorState,
}

impl<'ast> Visit<'ast> for HashMapNoCapacityVisitor<'_> {
    fn visit_local(&mut self, node: &'ast syn::Local) {
        // Track `let x = HashMap::new()`; a rebind to anything else clears the entry
        // so a shadow `let x = HashMap::with_capacity(n)` is not falsely flagged.
        if let syn::Pat::Ident(pat_ident) = &node.pat {
            let name = pat_ident.ident.to_string();
            if node.init.as_ref().is_some_and(|init| is_hashmap_new(&init.expr)) {
                let span = pat_ident.ident.span();
                self.map_vars
                    .insert(name, (span.start().line, span.start().column));
            } else {
                self.map_vars.remove(&name);
            }
        }
        syn::visit::visit_local(self, node);
    }

    fn visit_expr_assign(&mut self, node: &'ast syn::ExprAssign) {
        if let Expr::Path(ExprPath {
            path, qself: None, ..
        }) = &*node.left
        {
            if let Some(ident) = path.get_ident() {
                let name = ident.to_string();
                if is_hashmap_new(&node.right) {
                    let span = ident.span();
                    self.map_vars
                        .insert(name, (span.start().line, span.start().column));
                } else {
                    self.map_vars.remove(&name);
                }
            }
        }
        syn::visit::visit_expr_assign(self, node);
    }

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

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if self.state.in_loop() && node.method == "insert" {
            // Check if receiver is a tracked HashMap variable
            if let Expr::Path(ExprPath { path, .. }) = &*node.receiver {
                if let Some(ident) = path.get_ident() {
                    let var_name = ident.to_string();
                    if let Some(&(decl_line, decl_column)) = self.map_vars.get(&var_name) {
                        // Report at declaration location (where fix would be applied)
                        self.diagnostics.push(Diagnostic {
                            rule_id: "hashmap-no-capacity",
                            severity: Severity::Warning,
                            message: format!(
                                "`{}` created with `HashMap::new()` then inserted to in loop; use `HashMap::with_capacity()` instead",
                                ident
                            ),
                            file_path: self.ctx.file_path.to_path_buf(),
                            line: decl_line,
                            column: decl_column,
                            end_line: None,
                            end_column: None,
                            suggestion: Some("Pre-allocate with `HashMap::with_capacity(expected_size)`".to_string()),
                            fix: None,
                        });

                        // Remove from tracking to avoid duplicate warnings
                        self.map_vars.remove(&var_name);
                    }
                }
            }
        }
        syn::visit::visit_expr_method_call(self, node);
    }
}

/// Check if an expression is HashMap::new()
fn is_hashmap_new(expr: &Expr) -> bool {
    match expr {
        Expr::Call(ExprCall { func, .. }) => {
            if let Expr::Path(ExprPath { path, .. }) = &**func {
                let path_str: String = path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::");
                // Only match HashMap::new, not bare "new" (which could be any type)
                path_str.ends_with("HashMap::new")
            } else {
                false
            }
        }
        // Removed: MethodCall branch that matched any .new() call
        _ => false,
    }
}

/// Detects String::new() followed by push_str in a loop without using with_capacity
pub struct StringNoCapacityRule;

impl Rule for StringNoCapacityRule {
    fn id(&self) -> &'static str {
        "string-no-capacity"
    }

    fn name(&self) -> &'static str {
        "String Without Capacity"
    }

    fn description(&self) -> &'static str {
        "Detects String::new() followed by push_str in loop; use with_capacity instead"
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check(&self, ctx: &AnalysisContext) -> Vec<Diagnostic> {
        let mut visitor = StringNoCapacityVisitor {
            ctx,
            diagnostics: Vec::new(),
            string_vars: std::collections::HashMap::new(),
            state: VisitorState::new(),
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct StringNoCapacityVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    /// Maps variable name to declaration location (line, column)
    string_vars: std::collections::HashMap<String, (usize, usize)>,
    state: VisitorState,
}

impl<'ast> Visit<'ast> for StringNoCapacityVisitor<'_> {
    fn visit_local(&mut self, node: &'ast syn::Local) {
        // Track `let x = String::new()`; a rebind to anything else clears the entry
        // so a shadow `let x = String::with_capacity(n)` is not falsely flagged.
        if let syn::Pat::Ident(pat_ident) = &node.pat {
            let name = pat_ident.ident.to_string();
            if node.init.as_ref().is_some_and(|init| is_string_new(&init.expr)) {
                let span = pat_ident.ident.span();
                self.string_vars
                    .insert(name, (span.start().line, span.start().column));
            } else {
                self.string_vars.remove(&name);
            }
        }
        syn::visit::visit_local(self, node);
    }

    fn visit_expr_assign(&mut self, node: &'ast syn::ExprAssign) {
        if let Expr::Path(ExprPath {
            path, qself: None, ..
        }) = &*node.left
        {
            if let Some(ident) = path.get_ident() {
                let name = ident.to_string();
                if is_string_new(&node.right) {
                    let span = ident.span();
                    self.string_vars
                        .insert(name, (span.start().line, span.start().column));
                } else {
                    self.string_vars.remove(&name);
                }
            }
        }
        syn::visit::visit_expr_assign(self, node);
    }

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

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        // Check for push_str, push, or write_str which grow the string
        if self.state.in_loop()
            && (node.method == "push_str" || node.method == "push" || node.method == "write_str")
        {
            // Check if receiver is a tracked String variable
            if let Expr::Path(ExprPath { path, .. }) = &*node.receiver {
                if let Some(ident) = path.get_ident() {
                    let var_name = ident.to_string();
                    if let Some(&(decl_line, decl_column)) = self.string_vars.get(&var_name) {
                        // Report at declaration location (where fix would be applied)
                        self.diagnostics.push(Diagnostic {
                            rule_id: "string-no-capacity",
                            severity: Severity::Warning,
                            message: format!(
                                "`{}` created with `String::new()` then appended to in loop; use `String::with_capacity()` instead",
                                ident
                            ),
                            file_path: self.ctx.file_path.to_path_buf(),
                            line: decl_line,
                            column: decl_column,
                            end_line: None,
                            end_column: None,
                            suggestion: Some("Pre-allocate with `String::with_capacity(expected_size)`".to_string()),
                            fix: None,
                        });

                        // Remove from tracking to avoid duplicate warnings
                        self.string_vars.remove(&var_name);
                    }
                }
            }
        }
        syn::visit::visit_expr_method_call(self, node);
    }
}

/// Check if an expression is String::new()
fn is_string_new(expr: &Expr) -> bool {
    match expr {
        Expr::Call(ExprCall { func, .. }) => {
            if let Expr::Path(ExprPath { path, .. }) = &**func {
                let path_str: String = path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::");
                // Only match String::new
                path_str.ends_with("String::new")
            } else {
                false
            }
        }
        // Removed: MethodCall branch that matched any .new() call
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
            state: VisitorState::new(),
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct FormatInLoopVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    state: VisitorState,
}

impl<'ast> Visit<'ast> for FormatInLoopVisitor<'_> {
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

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        if self.state.in_loop() {
            let macro_name = node
                .path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default();

            if macro_name == "format" {
                let span = node
                    .path
                    .segments
                    .last()
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
            state: VisitorState::new(),
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct StringConcatVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    state: VisitorState,
}

impl<'ast> Visit<'ast> for StringConcatVisitor<'_> {
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

    fn visit_expr_binary(&mut self, node: &'ast syn::ExprBinary) {
        if self.state.in_loop() {
            // Check for + or += with strings
            match &node.op {
                syn::BinOp::Add(_) | syn::BinOp::AddAssign(_)
                    if is_likely_string_expr(&node.left) || is_likely_string_expr(&node.right) =>
                {
                    let span = match &node.op {
                        syn::BinOp::Add(t) => t.span,
                        syn::BinOp::AddAssign(t) => t.spans[0],
                        _ => proc_macro2::Span::call_site(),
                    };
                    let line = span.start().line;
                    let column = span.start().column;

                    // Try to generate fix for += case
                    let fix = self.generate_string_concat_fix(node);

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
                        fix,
                    });
                }
                _ => {}
            }
        }
        syn::visit::visit_expr_binary(self, node);
    }
}

impl StringConcatVisitor<'_> {
    /// Generate a fix for string concatenation patterns.
    ///
    /// For `s += expr`, generates `s.push_str(expr)`
    fn generate_string_concat_fix(&self, node: &syn::ExprBinary) -> Option<Fix> {
        // Only handle += for now (AddAssign)
        // The + case (Add) is trickier because it's usually part of an assignment
        if !matches!(&node.op, syn::BinOp::AddAssign(_)) {
            return None;
        }

        // Get the variable name from left side
        let var_name = match &*node.left {
            Expr::Path(path) => path.path.get_ident()?.to_string(),
            _ => return None,
        };

        // Get the right-hand expression as source text
        let rhs_span = node.right.span();
        let (rhs_start, rhs_end) = self.ctx.span_to_byte_range(rhs_span)?;

        // Skip fix generation for very large expressions
        let rhs_size = rhs_end.saturating_sub(rhs_start);
        if rhs_size > MAX_FIX_TEXT_SIZE {
            return None;
        }

        let rhs_text = self.ctx.source.get(rhs_start..rhs_end)?;

        // Get the full expression span for replacement
        let full_span = node.span();
        let (start, end) = self.ctx.span_to_byte_range(full_span)?;

        // Generate the replacement: var.push_str(rhs)
        let new_text = format!("{}.push_str({})", var_name, rhs_text);

        Some(Fix {
            description: format!(
                "Replace `{} += ...` with `{}.push_str(...)`",
                var_name, var_name
            ),
            replacements: vec![Replacement {
                file_path: self.ctx.file_path.to_path_buf(),
                start_byte: start,
                end_byte: end,
                new_text,
            }],
        })
    }
}

/// Check if an expression is definitely NOT a string (e.g., integer/float literals)
fn is_definitely_numeric(expr: &Expr) -> bool {
    match expr {
        Expr::Lit(lit) => matches!(
            &lit.lit,
            syn::Lit::Int(_)
                | syn::Lit::Float(_)
                | syn::Lit::Byte(_)
                | syn::Lit::Bool(_)
                | syn::Lit::Char(_)
        ),
        Expr::Reference(r) => is_definitely_numeric(&r.expr),
        Expr::Paren(p) => is_definitely_numeric(&p.expr),
        Expr::Unary(u) => is_definitely_numeric(&u.expr), // -1, !x etc
        _ => false,
    }
}

/// Check if an expression is definitely a string operation
fn is_definitely_string(expr: &Expr) -> bool {
    match expr {
        // High confidence: string literals
        Expr::Lit(lit) => matches!(&lit.lit, syn::Lit::Str(_)),

        // High confidence: methods that produce strings
        Expr::MethodCall(call) => {
            let method = call.method.to_string();
            matches!(method.as_str(), "to_string" | "to_owned" | "format")
        }

        // High confidence: format! macro
        Expr::Macro(m) => m
            .mac
            .path
            .segments
            .last()
            .map(|s| s.ident == "format")
            .unwrap_or(false),

        // Check references
        Expr::Reference(r) => is_definitely_string(&r.expr),

        // For binary expressions like `s + "text"`, check recursively
        Expr::Binary(bin) => {
            matches!(&bin.op, syn::BinOp::Add(_))
                && (is_definitely_string(&bin.left) || is_definitely_string(&bin.right))
        }

        _ => false,
    }
}

/// Heuristic to detect if an expression is likely a String concatenation.
///
/// Returns true if:
/// - Either side is definitely a string (string literal, .to_string(), format!)
/// - AND neither side is definitely numeric (integer literal, etc.)
///
/// This avoids false positives on `i + 1` while catching `s + "text"` and `s + word.to_string()`
fn is_likely_string_expr(expr: &Expr) -> bool {
    // Never flag if expression is numeric
    if is_definitely_numeric(expr) {
        return false;
    }

    // Flag if expression is definitely a string
    is_definitely_string(expr)
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
            state: VisitorState::new(),
            lock_names: HashSet::new(),
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct MutexLockVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    state: VisitorState,
    /// Names of bindings known to hold a `Mutex`/`RwLock` (function-scoped).
    /// Ambiguous `.read()`/`.write()`/`.lock()` only count as lock contention on
    /// one of these; on any other receiver they are ordinary domain methods.
    lock_names: HashSet<String>,
}

/// True if a type mentions `Mutex` or `RwLock` anywhere (through references and
/// generic wrappers like `Arc<Mutex<T>>`).
fn type_mentions_lock(ty: &Type) -> bool {
    match ty {
        Type::Path(tp) => tp.path.segments.iter().any(|s| {
            let n = s.ident.to_string();
            n == "Mutex"
                || n == "RwLock"
                || matches!(&s.arguments, syn::PathArguments::AngleBracketed(a)
                    if a.args.iter().any(|arg| matches!(arg, syn::GenericArgument::Type(t) if type_mentions_lock(t))))
        }),
        Type::Reference(r) => type_mentions_lock(&r.elem),
        Type::Paren(p) => type_mentions_lock(&p.elem),
        _ => false,
    }
}

/// True if an expression constructs a lock, e.g. `Mutex::new(..)`,
/// `RwLock::new(..)`, or a wrapped `Arc::new(Mutex::new(..))`.
fn expr_is_lock_ctor(expr: &Expr) -> bool {
    let Expr::Call(call) = expr else {
        return false;
    };
    let Expr::Path(ExprPath { path, .. }) = &*call.func else {
        return false;
    };
    let mentions_lock = path
        .segments
        .iter()
        .any(|s| s.ident == "Mutex" || s.ident == "RwLock");
    let last_is_ctor = path
        .segments
        .last()
        .is_some_and(|s| matches!(s.ident.to_string().as_str(), "new" | "from" | "default"));
    if mentions_lock && last_is_ctor {
        return true;
    }
    // `Arc::new(Mutex::new(..))` / `Rc::new(RwLock::new(..))`
    let is_wrapper = path.segments.iter().any(|s| s.ident == "Arc" || s.ident == "Rc")
        && path.segments.last().is_some_and(|s| s.ident == "new");
    if is_wrapper {
        if let Some(first) = call.args.first() {
            return expr_is_lock_ctor(first);
        }
    }
    false
}

/// The single-segment identifier of a receiver expression, if it is a bare name.
fn mutex_simple_receiver_name(expr: &Expr) -> Option<String> {
    if let Expr::Path(ExprPath {
        path, qself: None, ..
    }) = expr
    {
        if path.segments.len() == 1 {
            return Some(path.segments[0].ident.to_string());
        }
    }
    None
}

/// The bound identifier of a `let` pattern (`let x` / `let x: T`).
fn mutex_binding_name(pat: &Pat) -> Option<String> {
    match pat {
        Pat::Ident(id) => Some(id.ident.to_string()),
        Pat::Type(pt) => mutex_binding_name(&pt.pat),
        _ => None,
    }
}

impl<'ast> Visit<'ast> for MutexLockVisitor<'_> {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        if self.state.should_bail() {
            return;
        }
        // Scope lock bindings to this function so a name reused elsewhere with a
        // non-lock type is not wrongly flagged.
        let saved = std::mem::take(&mut self.lock_names);
        for input in &node.sig.inputs {
            if let FnArg::Typed(pt) = input {
                if type_mentions_lock(&pt.ty) {
                    if let Some(name) = mutex_binding_name(&pt.pat) {
                        self.lock_names.insert(name);
                    }
                }
            }
        }
        syn::visit::visit_item_fn(self, node);
        self.lock_names = saved;
    }

    fn visit_local(&mut self, node: &'ast Local) {
        // A lock ctor binding (`let g = Mutex::new(..)`) records the name; rebinding
        // that name to a non-lock value (a shadow, or a sibling-scope reuse of the
        // same name) must clear the stale entry so an ordinary `.read()`/`.lock()`
        // domain method on the new binding is not mistaken for lock contention.
        if let Some(name) = mutex_binding_name(&node.pat) {
            // Two channels mark a local as a lock: (1) an explicit type annotation
            // that mentions a lock (`let m: Arc<Mutex<_>> = shared.clone();`), where
            // the initializer is not a ctor, and (2) an initializer that constructs a
            // lock (`let m = Mutex::new(..)`). Either suffices.
            let annotated_lock =
                matches!(&node.pat, Pat::Type(pt) if type_mentions_lock(&pt.ty));
            let ctor_lock = node
                .init
                .as_ref()
                .is_some_and(|init| expr_is_lock_ctor(&init.expr));
            if annotated_lock || ctor_lock {
                self.lock_names.insert(name);
            } else {
                self.lock_names.remove(&name);
            }
        }
        syn::visit::visit_local(self, node);
    }

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

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if self.state.in_loop() {
            let method = node.method.to_string();
            // `lock`/`try_lock` are unambiguous lock acquisitions. `read`/`write`
            // (and their `try_` variants) are lock acquisitions ONLY when nullary:
            // `RwLock::read()` takes no args, whereas `io::Read::read(buf)` /
            // `io::Write::write(buf)` take a buffer argument and must not be flagged.
            let is_lock_acquire = matches!(method.as_str(), "lock" | "try_lock")
                || (matches!(method.as_str(), "read" | "try_read" | "write" | "try_write")
                    && node.args.is_empty());
            // Only flag when the receiver is a binding known to hold a lock; a
            // bare `.read()`/`.write()`/`.lock()` on any other value is an
            // ordinary domain method, not lock contention.
            let receiver_is_lock = mutex_simple_receiver_name(&node.receiver)
                .is_some_and(|name| self.lock_names.contains(&name));
            if is_lock_acquire && receiver_is_lock {
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
                    suggestion: Some(
                        "Acquire the lock before the loop to reduce lock contention".to_string(),
                    ),
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

    fn check_hashmap_capacity(source: &str) -> Vec<Diagnostic> {
        let ast = syn::parse_file(source).expect("Failed to parse");
        let config = Config::default();
        let ctx = AnalysisContext::new(Path::new("test.rs"), source, &ast, &config);
        HashMapNoCapacityRule.check(&ctx)
    }

    fn check_string_capacity(source: &str) -> Vec<Diagnostic> {
        let ast = syn::parse_file(source).expect("Failed to parse");
        let config = Config::default();
        let ctx = AnalysisContext::new(Path::new("test.rs"), source, &ast, &config);
        StringNoCapacityRule.check(&ctx)
    }

    // ========================================================================
    // Batch 3: scope/shadow map hygiene (D25, D34, D36, D37, D38)
    // ========================================================================

    #[test]
    fn test_vec_reassigned_to_with_capacity_not_flagged() {
        // D36: `buf` is reassigned to `Vec::with_capacity(n)` before the loop, so
        // every push targets the pre-sized vec. The stale `Vec::new()` binding
        // must be cleared on reassignment.
        let source = r#"
            fn build(n: usize) -> Vec<u32> {
                let mut buf = Vec::new();
                buf = Vec::with_capacity(n);
                for i in 0..n {
                    buf.push(i as u32);
                }
                buf
            }
        "#;
        assert!(
            check_vec_capacity(source).is_empty(),
            "reassigned-to-with_capacity vec must stay silent: {:?}",
            check_vec_capacity(source)
        );
    }

    #[test]
    fn test_vec_shadowed_by_with_capacity_not_flagged() {
        // D37: the outer `v` = Vec::new() is pushed once outside any loop; a
        // shadowed inner `v` = Vec::with_capacity is the one grown in the loop.
        let source = r#"
            fn render(items: &[String]) -> Vec<String> {
                let mut v = Vec::new();
                v.push(String::from("banner"));
                drop(v);
                {
                    let mut v = Vec::with_capacity(items.len());
                    for it in items {
                        v.push(it.clone());
                    }
                    v
                }
            }
        "#;
        assert!(
            check_vec_capacity(source).is_empty(),
            "shadowed with_capacity vec must stay silent: {:?}",
            check_vec_capacity(source)
        );
    }

    #[test]
    fn test_vec_sibling_scope_name_collision_not_flagged() {
        // D38: two same-named `out` bindings in sibling scopes; scope-1 pushes
        // outside a loop, scope-2 uses with_capacity and grows in a loop.
        let source = r#"
            fn two_scopes(a: &[i32], b: &[i32]) -> (Vec<i32>, Vec<i32>) {
                let first = {
                    let mut out = Vec::new();
                    out.push(a[0]);
                    out
                };
                let second = {
                    let mut out = Vec::with_capacity(b.len());
                    for x in b {
                        out.push(*x);
                    }
                    out
                };
                (first, second)
            }
        "#;
        assert!(
            check_vec_capacity(source).is_empty(),
            "sibling-scope vec name collision must stay silent: {:?}",
            check_vec_capacity(source)
        );
    }

    #[test]
    fn test_vec_new_grown_in_loop_still_flagged() {
        // Guard: the genuine antipattern (Vec::new grown in a loop) must still
        // fire after the shadow/reassign hygiene changes.
        let source = r#"
            fn build(n: usize) -> Vec<u32> {
                let mut buf = Vec::new();
                for i in 0..n {
                    buf.push(i as u32);
                }
                buf
            }
        "#;
        assert_eq!(
            check_vec_capacity(source).len(),
            1,
            "genuine Vec::new-in-loop must still fire: {:?}",
            check_vec_capacity(source)
        );
    }

    #[test]
    fn test_string_shadowed_by_with_capacity_not_flagged() {
        // D34: `line` = String::new() is immediately shadowed by
        // `line` = String::with_capacity(1024), which is the one grown in the loop.
        let source = r#"
            fn build_csv(rows: &[Vec<String>]) -> String {
                let line = String::new();
                let _ = &line;
                let mut line = String::with_capacity(1024);
                for cell in rows.iter().flatten() {
                    line.push_str(cell);
                    line.push(',');
                }
                line
            }
        "#;
        assert!(
            check_string_capacity(source).is_empty(),
            "shadowed with_capacity string must stay silent: {:?}",
            check_string_capacity(source)
        );
    }

    #[test]
    fn test_mutex_sibling_scope_name_collision_not_flagged() {
        // D25: sibling blocks both bind `g`; the first is a real Mutex locked
        // outside a loop, the second is a non-lock `Grid` whose nullary `read()`
        // is called in a loop. The stale lock name must clear on rebind.
        let source = r#"
            use std::sync::Mutex;
            struct Grid;
            impl Grid {
                fn read(&self) -> i32 { 7 }
            }
            fn phased() -> i32 {
                let mut out = 0;
                {
                    let g = Mutex::new(0i32);
                    out += *g.lock().unwrap();
                }
                {
                    let g = Grid;
                    for _ in 0..5 {
                        out += g.read();
                    }
                }
                out
            }
        "#;
        assert!(
            check_mutex_loop(source).is_empty(),
            "sibling-scope mutex name collision must stay silent: {:?}",
            check_mutex_loop(source)
        );
    }

    #[test]
    fn test_hashmap_shadowed_by_with_capacity_not_flagged() {
        // Same defect class as D34/D37 applied to HashMap: a shadowed
        // `HashMap::with_capacity` binding is the one inserted-to in the loop.
        let source = r#"
            use std::collections::HashMap;
            fn build(n: usize) -> HashMap<u32, u32> {
                let map = HashMap::new();
                let _ = &map;
                let mut map = HashMap::with_capacity(n);
                for i in 0..n as u32 {
                    map.insert(i, i);
                }
                map
            }
        "#;
        assert!(
            check_hashmap_capacity(source).is_empty(),
            "shadowed with_capacity hashmap must stay silent: {:?}",
            check_hashmap_capacity(source)
        );
    }

    // String capacity tests
    #[test]
    fn test_string_new_push_str_in_loop() {
        let source = r#"
            fn test() {
                let mut s = String::new();
                for word in ["hello", "world"] {
                    s.push_str(word);
                }
            }
        "#;
        let diagnostics = check_string_capacity(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("String::new()"));
        assert!(diagnostics[0].message.contains("with_capacity"));
    }

    #[test]
    fn test_string_new_push_char_in_loop() {
        let source = r#"
            fn test() {
                let mut s = String::new();
                for c in "hello".chars() {
                    s.push(c);
                }
            }
        "#;
        let diagnostics = check_string_capacity(source);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_string_with_capacity_no_warning() {
        let source = r#"
            fn test() {
                let mut s = String::with_capacity(100);
                for word in ["hello", "world"] {
                    s.push_str(word);
                }
            }
        "#;
        let diagnostics = check_string_capacity(source);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_string_push_str_outside_loop_ok() {
        let source = r#"
            fn test() {
                let mut s = String::new();
                s.push_str("hello");
                s.push_str(" world");
            }
        "#;
        let diagnostics = check_string_capacity(source);
        assert!(diagnostics.is_empty());
    }

    // HashMap capacity tests
    #[test]
    fn test_hashmap_new_insert_in_loop() {
        let source = r#"
            use std::collections::HashMap;
            fn test() {
                let mut map = HashMap::new();
                for i in 0..100 {
                    map.insert(i, i * 2);
                }
            }
        "#;
        let diagnostics = check_hashmap_capacity(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("HashMap::new()"));
        assert!(diagnostics[0].message.contains("with_capacity"));
    }

    #[test]
    fn test_hashmap_with_capacity_no_warning() {
        let source = r#"
            use std::collections::HashMap;
            fn test() {
                let mut map = HashMap::with_capacity(100);
                for i in 0..100 {
                    map.insert(i, i * 2);
                }
            }
        "#;
        let diagnostics = check_hashmap_capacity(source);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_hashmap_insert_outside_loop_ok() {
        let source = r#"
            use std::collections::HashMap;
            fn test() {
                let mut map = HashMap::new();
                map.insert(1, "one");
                map.insert(2, "two");
            }
        "#;
        let diagnostics = check_hashmap_capacity(source);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_hashmap_fully_qualified_path() {
        let source = r#"
            fn test() {
                let mut map = std::collections::HashMap::new();
                for i in 0..100 {
                    map.insert(i, i);
                }
            }
        "#;
        let diagnostics = check_hashmap_capacity(source);
        assert_eq!(diagnostics.len(), 1);
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
    fn test_string_plus_in_loop_with_literal() {
        let source = r#"
            fn test() {
                let mut s = String::new();
                for _ in 0..10 {
                    s = s + "text";
                }
            }
        "#;
        let diagnostics = check_string_concat(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("concatenation"));
    }

    #[test]
    fn test_string_plus_with_to_string() {
        let source = r#"
            fn test() {
                let mut s = String::new();
                for i in 0..10 {
                    s = s + &i.to_string();
                }
            }
        "#;
        let diagnostics = check_string_concat(source);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_no_false_positive_integer_plus() {
        // This should NOT be flagged as string concat
        let source = r#"
            fn test() {
                let mut sum = 0;
                for i in 0..10 {
                    sum += i;
                    let x = sum + 1;
                }
            }
        "#;
        let diagnostics = check_string_concat(source);
        assert!(diagnostics.is_empty(), "Should not flag integer operations");
    }

    #[test]
    fn test_no_false_positive_index_arithmetic() {
        // This should NOT be flagged - common pattern in parsers
        let source = r#"
            fn test(s: &str) {
                for (idx, _) in s.char_indices() {
                    let next = idx + 1;
                    let prev = idx - 1;
                }
            }
        "#;
        let diagnostics = check_string_concat(source);
        assert!(diagnostics.is_empty(), "Should not flag index arithmetic");
    }

    #[test]
    fn test_string_concat_fix_plus_assign() {
        // Test fix for s += "text" pattern
        let source = r#"fn test() {
    let mut s = String::new();
    for _ in 0..3 {
        s += "x";
    }
}"#;
        let diagnostics = check_string_concat(source);
        assert_eq!(diagnostics.len(), 1);

        // Check that fix is generated for += case
        let fix = diagnostics[0]
            .fix
            .as_ref()
            .expect("Should have a fix for +=");
        assert_eq!(fix.replacements.len(), 1);

        // Apply fix and verify
        let replacement = &fix.replacements[0];
        let mut result = source.to_string();
        result.replace_range(
            replacement.start_byte..replacement.end_byte,
            &replacement.new_text,
        );

        assert!(
            result.contains("s.push_str(\"x\")"),
            "Fix should convert to push_str: {}",
            result
        );
        assert!(!result.contains("s +="), "Fix should remove +=: {}", result);
    }

    // Mutex tests
    #[test]
    fn test_custom_nullary_read_not_flagged() {
        // A nullary `.read()` on a non-lock type is a domain method, not a lock.
        let source = r#"
            struct Sensor;
            impl Sensor { fn read(&self) -> u32 { 0 } }
            fn poll(sensor: &Sensor) {
                for _ in 0..10 { let _ = sensor.read(); }
            }
        "#;
        assert!(
            check_mutex_loop(source).is_empty(),
            "custom nullary read() must not be flagged as lock contention"
        );
    }

    #[test]
    fn test_lock_from_local_ctor_binding_flagged() {
        // A lock created via a local `Mutex::new(..)` binding is tracked.
        let source = r#"
            fn run() {
                let m = std::sync::Mutex::new(0);
                for _ in 0..10 { let _g = m.lock().unwrap(); }
            }
        "#;
        assert_eq!(
            check_mutex_loop(source).len(),
            1,
            "lock() on a tracked Mutex::new binding must be flagged"
        );
    }

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
    fn test_type_annotated_lock_local_flagged() {
        // D23: a local whose TYPE annotation is a lock (`m: Arc<Mutex<u64>>`) but
        // whose initializer is `.clone()` (not a ctor) still holds a lock. Locking
        // it every iteration is the antipattern; the local-type channel must track it.
        let source = r#"
            use std::sync::{Arc, Mutex};
            fn run(shared: &Arc<Mutex<u64>>) {
                let m: Arc<Mutex<u64>> = shared.clone();
                let mut i = 0;
                while i < 50 {
                    *m.lock().unwrap() += 1;
                    i += 1;
                }
            }
        "#;
        assert_eq!(
            check_mutex_loop(source).len(),
            1,
            "type-annotated lock local must fire: {:?}",
            check_mutex_loop(source)
        );
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

    #[test]
    fn test_rwlock_write_no_args_flagged() {
        // RwLock::write() is nullary -> still a lock acquisition.
        let source = r#"
            fn test(data: &std::sync::RwLock<Vec<i32>>) {
                for i in 0..10 {
                    let mut guard = data.write().unwrap();
                }
            }
        "#;
        let diagnostics = check_mutex_loop(source);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_no_false_positive_io_write_in_loop() {
        // io::Write::write(buf) takes an argument -> NOT a lock acquisition.
        let source = r#"
            use std::io::Write;
            fn test(w: &mut Vec<u8>) {
                for i in 0..10u8 {
                    w.write(&[i]).unwrap();
                }
            }
        "#;
        let diagnostics = check_mutex_loop(source);
        assert!(
            diagnostics.is_empty(),
            "io::Write::write(buf) must not be flagged as lock contention"
        );
    }

    #[test]
    fn test_no_false_positive_io_read_in_loop() {
        // io::Read::read(buf) takes an argument -> NOT a lock acquisition.
        let source = r#"
            use std::io::Read;
            fn test<R: Read>(r: &mut R, buf: &mut [u8]) {
                for _ in 0..10 {
                    let _n = r.read(buf).unwrap();
                }
            }
        "#;
        let diagnostics = check_mutex_loop(source);
        assert!(
            diagnostics.is_empty(),
            "io::Read::read(buf) must not be flagged as lock contention"
        );
    }

    // Test for false positive prevention - custom types with .new() should NOT be flagged
    #[test]
    fn test_no_false_positive_custom_new() {
        // Custom type with new() should NOT trigger Vec rule
        let source = r#"
            struct MyType;
            impl MyType { fn new() -> Self { MyType } }
            fn test() {
                let mut x = MyType::new();
                for i in 0..10 {
                    // Do something with x
                }
            }
        "#;
        let diagnostics = check_vec_capacity(source);
        assert!(diagnostics.is_empty(), "Should not flag custom types");
    }

    #[test]
    fn test_no_false_positive_builder_new() {
        // Builder pattern with new() should NOT be flagged
        let source = r#"
            struct Builder;
            impl Builder {
                fn new() -> Self { Builder }
                fn add(&mut self, _: i32) {}
            }
            fn test() {
                let mut builder = Builder::new();
                for i in 0..10 {
                    builder.add(i);
                }
            }
        "#;
        let diagnostics = check_hashmap_capacity(source);
        assert!(diagnostics.is_empty(), "Should not flag Builder::new()");
        let diagnostics = check_string_capacity(source);
        assert!(diagnostics.is_empty(), "Should not flag Builder::new()");
    }

    // Test for while loop variations
    #[test]
    fn test_vec_in_while_loop() {
        let source = r#"
            fn test() {
                let mut v = Vec::new();
                let mut i = 0;
                while i < 100 {
                    v.push(i);
                    i += 1;
                }
            }
        "#;
        let diagnostics = check_vec_capacity(source);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_hashmap_in_while_loop() {
        let source = r#"
            use std::collections::HashMap;
            fn test() {
                let mut map = HashMap::new();
                let mut i = 0;
                while i < 100 {
                    map.insert(i, i * 2);
                    i += 1;
                }
            }
        "#;
        let diagnostics = check_hashmap_capacity(source);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_string_in_loop_loop() {
        let source = r#"
            fn test() {
                let mut s = String::new();
                loop {
                    s.push_str("x");
                    if s.len() > 10 { break; }
                }
            }
        "#;
        let diagnostics = check_string_capacity(source);
        assert_eq!(diagnostics.len(), 1);
    }
}
