use super::resolve::ImportOracle;
use super::visitor::VisitorState;
use super::{Diagnostic, Rule, Severity};
use crate::engine::AnalysisContext;
use std::collections::HashSet;
use syn::visit::Visit;
use syn::{Expr, ExprCall, ExprMethodCall, ExprPath, FnArg, ItemFn, Local, Pat, Type};

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
        "Detects .clone() calls inside loops (Arc/Rc reference-count clones are excluded)"
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check(&self, ctx: &AnalysisContext) -> Vec<Diagnostic> {
        let mut visitor = CloneInLoopVisitor {
            ctx,
            diagnostics: Vec::new(),
            state: VisitorState::new(),
            arc_rc_names: HashSet::new(),
            copy_names: HashSet::new(),
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct CloneInLoopVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    state: VisitorState,
    /// Names of bindings known to hold an `Arc`/`Rc` (function-scoped). Cloning
    /// these only bumps a reference count, so it must not be flagged.
    arc_rc_names: HashSet<String>,
    /// Names of bindings known to hold a `Copy` primitive (function-scoped).
    /// Cloning these is a no-op copy, already covered by clippy::clone_on_copy.
    copy_names: HashSet<String>,
}

/// Known `Copy` primitive type names. Cloning a value of one of these is a
/// trivial bitwise copy, not a heap allocation.
const COPY_PRIMITIVES: &[&str] = &[
    "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize", "f32",
    "f64", "bool", "char",
];

/// True if a type is (a reference to) a known `Copy` primitive.
fn type_is_copy_primitive(ty: &Type) -> bool {
    match ty {
        Type::Path(tp) => tp
            .path
            .segments
            .last()
            .is_some_and(|s| COPY_PRIMITIVES.contains(&s.ident.to_string().as_str())),
        Type::Reference(r) => type_is_copy_primitive(&r.elem),
        Type::Paren(p) => type_is_copy_primitive(&p.elem),
        _ => false,
    }
}

/// True if a type is (a reference to) `Arc<..>` or `Rc<..>`.
fn type_is_arc_rc(ty: &Type) -> bool {
    match ty {
        Type::Path(tp) => tp
            .path
            .segments
            .last()
            .is_some_and(|s| s.ident == "Arc" || s.ident == "Rc"),
        Type::Reference(r) => type_is_arc_rc(&r.elem),
        Type::Paren(p) => type_is_arc_rc(&p.elem),
        _ => false,
    }
}

/// True if an expression is an `Arc`/`Rc` constructor call, e.g. `Arc::new(..)`,
/// `Rc::clone(..)`, `std::sync::Arc::from(..)`.
fn expr_is_arc_rc_ctor(expr: &Expr) -> bool {
    let Expr::Call(call) = expr else { return false };
    let Expr::Path(ExprPath { path, .. }) = &*call.func else {
        return false;
    };
    let has_arc_rc = path
        .segments
        .iter()
        .any(|s| s.ident == "Arc" || s.ident == "Rc");
    let is_ctor = path.segments.last().is_some_and(|s| {
        matches!(
            s.ident.to_string().as_str(),
            "new" | "clone" | "from" | "downgrade" | "default" | "new_cyclic"
        )
    });
    has_arc_rc && is_ctor
}

/// The single-segment identifier of a `receiver` expression, if it is a bare name.
fn simple_receiver_name(expr: &Expr) -> Option<String> {
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

/// Extract the bound identifier from a `let` pattern (handles `let x` and `let x: T`).
fn binding_name(pat: &Pat) -> Option<String> {
    match pat {
        Pat::Ident(id) => Some(id.ident.to_string()),
        Pat::Type(pt) => binding_name(&pt.pat),
        _ => None,
    }
}

impl<'ast> Visit<'ast> for CloneInLoopVisitor<'_> {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        if self.state.should_bail() {
            return;
        }
        // Scope Arc/Rc and Copy bindings to this function so a name reused with a
        // different type elsewhere is not wrongly suppressed.
        let saved_arc = std::mem::take(&mut self.arc_rc_names);
        let saved_copy = std::mem::take(&mut self.copy_names);
        for input in &node.sig.inputs {
            if let FnArg::Typed(pt) = input {
                if let Some(name) = binding_name(&pt.pat) {
                    if type_is_arc_rc(&pt.ty) {
                        self.arc_rc_names.insert(name.clone());
                    }
                    if type_is_copy_primitive(&pt.ty) {
                        self.copy_names.insert(name);
                    }
                }
            }
        }
        syn::visit::visit_item_fn(self, node);
        self.arc_rc_names = saved_arc;
        self.copy_names = saved_copy;
    }

    fn visit_local(&mut self, node: &'ast Local) {
        if let Some(name) = binding_name(&node.pat) {
            // `let x: i32 = ..` records a Copy binding.
            if let Pat::Type(pt) = &node.pat {
                if type_is_copy_primitive(&pt.ty) {
                    self.copy_names.insert(name.clone());
                }
            }
            if let Some(init) = &node.init {
                if expr_is_arc_rc_ctor(&init.expr) {
                    self.arc_rc_names.insert(name);
                }
            }
        }
        syn::visit::visit_local(self, node);
    }

    fn visit_expr_for_loop(&mut self, node: &'ast syn::ExprForLoop) {
        if self.state.should_bail() {
            return;
        }
        // A `for x in <range>` variable is always an integer or char (Copy), so
        // `x.clone()` in the body is a no-op copy, not a heap clone.
        let range_var = matches!(&*node.expr, Expr::Range(_))
            .then(|| binding_name(&node.pat))
            .flatten();
        if let Some(name) = &range_var {
            self.copy_names.insert(name.clone());
        }
        self.state.enter_loop();
        syn::visit::visit_expr_for_loop(self, node);
        self.state.exit_loop();
        if let Some(name) = &range_var {
            self.copy_names.remove(name);
        }
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
        if self.state.in_loop() && node.method == "clone" {
            // Skip Arc/Rc reference-count clones and Copy-primitive clones
            // (both are cheap; the latter is already clippy::clone_on_copy).
            let skip = simple_receiver_name(&node.receiver).is_some_and(|name| {
                self.arc_rc_names.contains(&name) || self.copy_names.contains(&name)
            });

            if !skip {
                let span = node.method.span();
                let line = span.start().line;
                let column = span.start().column;

                self.diagnostics.push(Diagnostic {
                    rule_id: "clone-in-hot-loop",
                    severity: Severity::Warning,
                    message:
                        "`.clone()` called inside loop; consider borrowing or moving the clone outside"
                            .to_string(),
                    file_path: self.ctx.file_path.to_path_buf(),
                    line,
                    column,
                    end_line: None,
                    end_column: None,
                    suggestion: Some(
                        "Use a reference or move the clone outside the loop".to_string(),
                    ),
                    fix: None,
                });
            }
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
            state: VisitorState::new(),
            imports: ImportOracle::from_file(ctx.ast),
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct RegexInLoopVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    state: VisitorState,
    imports: ImportOracle,
}

impl<'ast> Visit<'ast> for RegexInLoopVisitor<'_> {
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
                // Match `Regex::new` on exact `::`-segment boundaries, not as a
                // substring: a user `RegexCacheKey::new` merely *contains*
                // "Regex" and has no compilation cost. The regex crate's type
                // is spelled exactly `Regex` (`Regex::new`, `regex::Regex::new`,
                // `fancy_regex::Regex::new`, …), and the constructor is `new`.
                let has_regex_segment = path.segments.iter().any(|s| s.ident == "Regex");
                let ends_with_new = path
                    .segments
                    .last()
                    .is_some_and(|s| s.ident == "new");
                // A locally-defined `struct Regex` shadows the crate type.
                let shadowed = self.imports.is_local_item("Regex");

                if has_regex_segment && ends_with_new && !shadowed {
                    let span = path
                        .segments
                        .last()
                        .map(|s| s.ident.span())
                        .unwrap_or_else(proc_macro2::Span::call_site);
                    let line = span.start().line;
                    let column = span.start().column;

                    self.diagnostics.push(Diagnostic {
                        rule_id: "regex-in-loop",
                        severity: Severity::Warning,
                        message: "`Regex::new()` called inside loop; compile regex once outside"
                            .to_string(),
                        file_path: self.ctx.file_path.to_path_buf(),
                        line,
                        column,
                        end_line: None,
                        end_column: None,
                        suggestion: Some(
                            "Use `lazy_static!` or `once_cell::Lazy` to compile the regex once"
                                .to_string(),
                        ),
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

    // Copy-primitive clones must NOT be flagged (clippy::clone_on_copy covers them).
    #[test]
    fn test_copy_range_loop_var_clone_not_flagged() {
        let source = r#"
            fn sum() -> i32 {
                let mut total = 0i32;
                for n in 0..10i32 { total += n.clone(); }
                total
            }
        "#;
        assert!(
            check_clone_rule(source).is_empty(),
            "clone of a Copy range loop variable must not be flagged"
        );
    }

    #[test]
    fn test_copy_param_clone_not_flagged() {
        let source = r#"
            fn run(n: usize) -> usize {
                let mut total = 0;
                for _ in 0..5 { total += n.clone(); }
                total
            }
        "#;
        assert!(
            check_clone_rule(source).is_empty(),
            "clone of a Copy-typed parameter must not be flagged"
        );
    }

    #[test]
    fn test_heap_clone_still_flagged_alongside_copy() {
        // A String clone in the same loop must still fire even when Copy tracking is active.
        let source = r#"
            fn run(name: &str) {
                for n in 0..5i32 {
                    let _ = n.clone();
                    let _ = name.to_string().clone();
                }
            }
        "#;
        assert_eq!(
            check_clone_rule(source).len(),
            1,
            "the heap clone must still be flagged, the Copy clone must not"
        );
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
    fn test_regex_cache_key_not_flagged() {
        // D33: a custom type whose name merely *contains* "Regex" is not the
        // regex crate; constructing it in a loop has no compilation cost.
        let source = r#"
            struct RegexCacheKey { id: u32 }
            impl RegexCacheKey { fn new(id: u32) -> Self { RegexCacheKey { id } } }
            fn test() {
                for id in 0..10 {
                    let _k = RegexCacheKey::new(id);
                }
            }
        "#;
        let diagnostics = check_regex_rule(source);
        assert!(
            diagnostics.is_empty(),
            "RegexCacheKey::new flagged as regex compile: {diagnostics:?}"
        );
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

    // --- Arc/Rc reference-count clones must NOT be flagged (they are a cheap, correct idiom) ---

    #[test]
    fn test_no_flag_arc_param_clone() {
        let source = r#"
            use std::sync::Arc;
            fn test(shared: Arc<Vec<u8>>) {
                for _ in 0..10 {
                    let _c = shared.clone(); // Arc refcount bump, not a deep clone
                }
            }
        "#;
        let diagnostics = check_clone_rule(source);
        assert!(
            diagnostics.is_empty(),
            "Arc parameter clone in loop must not be flagged"
        );
    }

    #[test]
    fn test_no_flag_arc_new_binding_clone() {
        let source = r#"
            use std::sync::Arc;
            fn test() {
                let shared = Arc::new(vec![1u8, 2, 3]);
                for _ in 0..10 {
                    let _c = shared.clone();
                }
            }
        "#;
        let diagnostics = check_clone_rule(source);
        assert!(
            diagnostics.is_empty(),
            "clone of an Arc::new binding must not be flagged"
        );
    }

    #[test]
    fn test_no_flag_rc_param_clone() {
        let source = r#"
            use std::rc::Rc;
            fn test(shared: Rc<String>) {
                for _ in 0..10 {
                    let _c = shared.clone();
                }
            }
        "#;
        let diagnostics = check_clone_rule(source);
        assert!(diagnostics.is_empty(), "Rc clone must not be flagged");
    }

    #[test]
    fn test_no_flag_qualified_arc_clone() {
        // Arc::clone(&x) is an idiomatic explicit refcount bump; it is a call, not a
        // method receiver, so it must never be flagged.
        let source = r#"
            use std::sync::Arc;
            fn test(shared: Arc<Vec<u8>>) {
                for _ in 0..10 {
                    let _c = Arc::clone(&shared);
                }
            }
        "#;
        let diagnostics = check_clone_rule(source);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_string_clone_still_flagged_over_suppression_guard() {
        // Guards against over-suppression: a real heap clone must still fire.
        let source = r#"
            fn test(s: &String) {
                for _ in 0..10 {
                    let _c = s.clone();
                }
            }
        "#;
        let diagnostics = check_clone_rule(source);
        assert_eq!(
            diagnostics.len(),
            1,
            "String clone in loop must still be flagged"
        );
    }

    #[test]
    fn test_arc_binding_does_not_leak_across_functions() {
        // `shared` is an Arc in f1 but a String in f2 -> f2's clone must still fire.
        let source = r#"
            use std::sync::Arc;
            fn f1(shared: Arc<Vec<u8>>) {
                for _ in 0..10 { let _c = shared.clone(); }
            }
            fn f2(shared: &String) {
                for _ in 0..10 { let _c = shared.clone(); }
            }
        "#;
        let diagnostics = check_clone_rule(source);
        assert_eq!(
            diagnostics.len(),
            1,
            "only f2's String clone should be flagged"
        );
    }
}
