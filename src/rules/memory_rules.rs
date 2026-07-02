use super::resolve::ImportOracle;
use super::visitor::VisitorState;
use super::{Diagnostic, Rule, Severity};
use crate::engine::AnalysisContext;
use std::collections::HashSet;
use syn::visit::Visit;
use syn::{Expr, ExprCall, ExprMethodCall, ExprPath, FnArg, ItemFn, Lit, Local, Pat, Type};

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
            imports: ImportOracle::from_file(ctx.ast),
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
    /// In-file import/return-type oracle: lets a `let x = make_shared();` binding
    /// be recognised as holding an Arc/Rc when `make_shared` is a same-file
    /// Arc/Rc factory function (D9).
    imports: ImportOracle,
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

/// True if an expression is a numeric / bool / char / byte literal. Such a
/// literal always has a `Copy` primitive type (some integer, float, `bool`,
/// `char`, or `u8`) whether or not a suffix or annotation is present — so a
/// binding initialized directly from one is `Copy`, and its `.clone()` is a
/// no-op copy rather than a heap allocation.
fn expr_is_copy_literal(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Lit(el)
            if matches!(
                el.lit,
                Lit::Int(_) | Lit::Float(_) | Lit::Bool(_) | Lit::Char(_) | Lit::Byte(_)
            )
    )
}

/// The identifier bound by a by-reference loop pattern `&x` (or `&mut x`), if
/// any. `for &x in it` moves `x` out of a shared reference, which the borrow
/// checker only accepts when the referent is `Copy`; so any name bound this way
/// is guaranteed `Copy` and cloning it is a no-op.
fn by_ref_binding_name(pat: &Pat) -> Option<String> {
    if let Pat::Reference(r) = pat {
        return binding_name(&r.pat);
    }
    None
}

/// Strip a leading `&`/`&mut` from `expr`, then return its bare single-segment
/// name — e.g. `&s` -> `s`. Used to inspect the argument of `Clone::clone(&x)`.
fn unref_simple_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Reference(r) => unref_simple_name(&r.expr),
        _ => simple_receiver_name(expr),
    }
}

impl CloneInLoopVisitor<'_> {
    /// True if `expr` is `<name>.clone()` where `name` already holds an Arc/Rc.
    /// The result is itself an Arc/Rc handle (a refcount bump), so a binding
    /// initialized from it should be tracked as Arc/Rc too.
    fn expr_is_arc_clone(&self, expr: &Expr) -> bool {
        if let Expr::MethodCall(m) = expr {
            if m.method == "clone" && m.args.is_empty() {
                if let Some(name) = simple_receiver_name(&m.receiver) {
                    return self.arc_rc_names.contains(&name);
                }
            }
        }
        false
    }

    /// True if `expr` is a call to a same-file free function whose declared
    /// return type is `Arc`/`Rc` (D9): `let cfg = make_shared();` yields an Arc
    /// exactly as `Arc::new(..)` does, so cloning `cfg` in a loop is a refcount
    /// bump, not a deep copy. Only bare single-segment calls are matched — an
    /// associated call like `String::new()` is a two-segment path and never
    /// misresolves to a free fn of the same leaf name. Associated Arc factories
    /// (`Foo::shared()`) remain a known gap: the oracle records free fns only.
    fn init_is_local_arc_factory(&self, expr: &Expr) -> bool {
        let Expr::Call(call) = expr else { return false };
        simple_receiver_name(&call.func)
            .is_some_and(|name| self.imports.local_fn_return_mentions_arc_rc(&name))
    }

    /// Emit the `clone-in-hot-loop` diagnostic at `span`. Shared by the
    /// method-call (`x.clone()`) and UFCS (`Clone::clone(&x)`) detection paths.
    fn emit_clone(&mut self, span: proc_macro2::Span) {
        self.diagnostics.push(Diagnostic {
            rule_id: "clone-in-hot-loop",
            severity: Severity::Warning,
            message:
                "`.clone()` called inside loop; consider borrowing or moving the clone outside"
                    .to_string(),
            file_path: self.ctx.file_path.to_path_buf(),
            line: span.start().line,
            column: span.start().column,
            end_line: None,
            end_column: None,
            suggestion: Some("Use a reference or move the clone outside the loop".to_string()),
            fix: None,
        });
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
            let mut is_copy = false;
            let mut is_arc = false;
            // `let x: i32 = ..` records a Copy binding via its annotation.
            if let Pat::Type(pt) = &node.pat {
                if type_is_copy_primitive(&pt.ty) {
                    is_copy = true;
                }
            }
            if let Some(init) = &node.init {
                // An Arc/Rc ctor (`Arc::new(..)`) or a `.clone()` of an existing
                // Arc/Rc handle both yield an Arc/Rc, so cloning the binding is a
                // refcount bump. A bare Copy literal (`let x = 42u64;`) yields a
                // Copy primitive with no explicit annotation.
                if expr_is_arc_rc_ctor(&init.expr)
                    || self.expr_is_arc_clone(&init.expr)
                    || self.init_is_local_arc_factory(&init.expr)
                {
                    is_arc = true;
                } else if expr_is_copy_literal(&init.expr) {
                    is_copy = true;
                }
            }
            // Rebinding the same name (shadowing) must overwrite the prior
            // classification: `let data = Arc::new(..)` then `let data = s.to_string()`
            // leaves `data` an owned String, so a later `data.clone()` is a real heap
            // clone and must not be suppressed by the stale Arc/Copy record.
            if is_arc {
                self.arc_rc_names.insert(name.clone());
            } else {
                self.arc_rc_names.remove(&name);
            }
            if is_copy {
                self.copy_names.insert(name);
            } else {
                self.copy_names.remove(&name);
            }
        }
        syn::visit::visit_local(self, node);
    }

    fn visit_expr_for_loop(&mut self, node: &'ast syn::ExprForLoop) {
        if self.state.should_bail() {
            return;
        }
        // Collect loop-variable names that are guaranteed `Copy`:
        //  - `for x in <range>`: `x` is always an integer or char.
        //  - `for x in [copy-literals]`: `x` is the array element type, and an
        //    array of Copy literals has a Copy element type.
        //  - `for &x in it`: binding by value out of a shared reference only
        //    compiles when the referent is Copy, so `x` is Copy.
        // Cloning any of these in the body is a no-op copy, not a heap clone.
        let mut copy_vars: Vec<String> = Vec::new();
        let iter_yields_copy = match &*node.expr {
            Expr::Range(_) => true,
            Expr::Array(arr) => arr.elems.iter().all(expr_is_copy_literal),
            _ => false,
        };
        if iter_yields_copy {
            if let Some(name) = binding_name(&node.pat) {
                copy_vars.push(name);
            }
        }
        if let Some(name) = by_ref_binding_name(&node.pat) {
            copy_vars.push(name);
        }
        for name in &copy_vars {
            self.copy_names.insert(name.clone());
        }
        self.state.enter_loop();
        syn::visit::visit_expr_for_loop(self, node);
        self.state.exit_loop();
        for name in &copy_vars {
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
                self.emit_clone(node.method.span());
            }
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        // UFCS clone: `Clone::clone(&x)` / `<T as Clone>::clone(&x)` is the exact
        // desugaring of `x.clone()` and allocates identically, but parses as a
        // Call, not a MethodCall. Require both a trailing `clone` segment and a
        // `Clone` segment so that `Arc::clone(..)` / `Rc::clone(..)` (which name
        // the type, not the trait) are never matched here.
        if self.state.in_loop() {
            if let Expr::Path(ExprPath { path, .. }) = &*node.func {
                let is_ufcs_clone = path.segments.last().is_some_and(|s| s.ident == "clone")
                    && path.segments.iter().any(|s| s.ident == "Clone");
                if is_ufcs_clone {
                    // Suppress refcount / no-op clones by inspecting the single
                    // argument (`Clone::clone(&x)`); an Arc/Rc or Copy `x` is cheap.
                    let skip = node.args.first().and_then(unref_simple_name).is_some_and(|name| {
                        self.arc_rc_names.contains(&name) || self.copy_names.contains(&name)
                    });
                    if !skip {
                        if let Some(seg) = path.segments.last() {
                            self.emit_clone(seg.ident.span());
                        }
                    }
                }
            }
        }
        syn::visit::visit_expr_call(self, node);
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

    // ========================================================================
    // Batch 2: Copy/Arc tracking-form coverage + UFCS clone (D11-D15)
    // ========================================================================

    #[test]
    fn test_arc_handle_from_method_clone_not_flagged() {
        // D11: `handle` is derived from an existing Arc via `.clone()` (a method
        // call, not an `Arc::new`/`Arc::clone` ctor). It is still an Arc, so
        // cloning it in the loop is only a refcount bump.
        let source = r#"
            use std::sync::Arc;
            fn run(orig: Arc<String>) {
                let handle = orig.clone();
                let mut n = 0usize;
                for _ in 0..50 {
                    let c = handle.clone();
                    n += c.len();
                }
                let _ = n;
            }
        "#;
        assert!(
            check_clone_rule(source).is_empty(),
            "Arc handle derived via .clone() must stay silent: {:?}",
            check_clone_rule(source)
        );
    }

    #[test]
    fn test_copy_literal_suffix_local_clone_not_flagged() {
        // D12: `let x = 42u64;` binds a Copy u64 by literal suffix (no explicit
        // annotation). `x.clone()` is a no-op copy.
        let source = r#"
            fn run() -> u64 {
                let x = 42u64;
                let mut total = 0u64;
                for _ in 0..1000 {
                    total = total.wrapping_add(x.clone());
                }
                total
            }
        "#;
        assert!(
            check_clone_rule(source).is_empty(),
            "Copy local from literal suffix must stay silent: {:?}",
            check_clone_rule(source)
        );
    }

    #[test]
    fn test_copy_array_literal_loop_var_clone_not_flagged() {
        // D13: `b` is a Copy u8 loop variable from iterating an array literal.
        let source = r#"
            fn run() -> u32 {
                let mut sum = 0u32;
                for b in [10u8, 20, 30, 40] {
                    sum += b.clone() as u32;
                }
                sum
            }
        "#;
        assert!(
            check_clone_rule(source).is_empty(),
            "Copy array-literal loop var must stay silent: {:?}",
            check_clone_rule(source)
        );
    }

    #[test]
    fn test_copy_by_ref_pattern_loop_var_clone_not_flagged() {
        // D14: `for &c in bytes.iter()` binds `c` by value; this only compiles
        // when the element is Copy (you cannot move out of a shared reference),
        // so `c.clone()` is guaranteed a no-op copy.
        let source = r#"
            fn checksum(bytes: &[u8]) -> u32 {
                let mut acc = 0u32;
                for &c in bytes.iter() {
                    acc = acc.wrapping_add(c.clone() as u32);
                }
                acc
            }
        "#;
        assert!(
            check_clone_rule(source).is_empty(),
            "Copy by-ref-pattern loop var must stay silent: {:?}",
            check_clone_rule(source)
        );
    }

    #[test]
    fn test_ufcs_heap_clone_in_loop_flagged() {
        // D15: a genuine heap String clone written via UFCS `Clone::clone(&s)`
        // has identical allocation cost to `s.clone()` and must fire.
        let source = r#"
            fn run(s: String) -> usize {
                let mut total = 0;
                for _ in 0..1000 {
                    let owned = Clone::clone(&s);
                    total += owned.len();
                }
                total
            }
        "#;
        assert_eq!(
            check_clone_rule(source).len(),
            1,
            "UFCS heap clone in loop must fire: {:?}",
            check_clone_rule(source)
        );
    }

    #[test]
    fn test_ufcs_arc_clone_in_loop_not_flagged() {
        // Guard the other direction: `Clone::clone(&arc)` where `arc` is an Arc
        // is a refcount bump, not a heap clone, and must stay silent.
        let source = r#"
            use std::sync::Arc;
            fn run(arc: Arc<String>) -> usize {
                let mut total = 0;
                for _ in 0..1000 {
                    let owned = Clone::clone(&arc);
                    total += owned.len();
                }
                total
            }
        "#;
        assert!(
            check_clone_rule(source).is_empty(),
            "UFCS Arc refcount clone must stay silent: {:?}",
            check_clone_rule(source)
        );
    }

    // ========================================================================
    // Batch 3: clone-tracker shadow hygiene (D16, D17)
    // ========================================================================

    #[test]
    fn test_arc_shadowed_by_string_clone_flagged() {
        // D16: `data` starts as an Arc, then is shadowed by an owned String. The
        // loop clones the String (a real heap clone) and must fire — the stale
        // Arc entry must clear on rebind.
        let source = r#"
            use std::sync::Arc;
            fn run(seed: &str) -> usize {
                let data = Arc::new(vec![1u8, 2, 3]);
                let _ = data.len();
                let data = seed.to_string();
                let mut total = 0;
                for _ in 0..1000 {
                    let owned = data.clone();
                    total += owned.len();
                }
                total
            }
        "#;
        assert_eq!(
            check_clone_rule(source).len(),
            1,
            "String clone after Arc shadow must fire: {:?}",
            check_clone_rule(source)
        );
    }

    #[test]
    fn test_copy_shadowed_by_string_clone_flagged() {
        // D17: `key` is annotated Copy `u32`, then shadowed by an owned String.
        // The heap clone of the String in the loop must fire.
        let source = r#"
            fn run(raw: &str) -> usize {
                let key: u32 = 0;
                let _ = key;
                let key = raw.to_string();
                let mut total = 0;
                for _ in 0..1000 {
                    let owned = key.clone();
                    total += owned.len();
                }
                total
            }
        "#;
        assert_eq!(
            check_clone_rule(source).len(),
            1,
            "String clone after Copy shadow must fire: {:?}",
            check_clone_rule(source)
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

    // ========================================================================
    // Batch 9 (D9): Arc/Rc factory-function initializers
    // ========================================================================

    #[test]
    fn test_clone_of_local_arc_factory_binding_not_flagged() {
        // D9: `cfg` is initialized from a same-file `fn make_shared() -> Arc<Config>`.
        // It holds an Arc, so cloning it in the loop is a refcount bump, not a heap
        // clone — the oracle must recognise the factory return type and suppress it.
        let source = r#"
            use std::sync::Arc;
            struct Config;
            fn make_shared() -> Arc<Config> { Arc::new(Config) }
            fn run() {
                let cfg = make_shared();
                for _ in 0..100 {
                    let _c = cfg.clone();
                }
            }
        "#;
        assert!(
            check_clone_rule(source).is_empty(),
            "clone of an Arc from a local factory fn must not be flagged: {:?}",
            check_clone_rule(source)
        );
    }

    #[test]
    fn test_clone_of_local_rc_factory_binding_not_flagged() {
        let source = r#"
            use std::rc::Rc;
            fn shared_node() -> Rc<Vec<u8>> { Rc::new(vec![]) }
            fn run() {
                let node = shared_node();
                for _ in 0..10 {
                    let _c = node.clone();
                }
            }
        "#;
        assert!(check_clone_rule(source).is_empty());
    }

    #[test]
    fn test_clone_of_non_arc_factory_binding_still_flagged() {
        // Contrast: `make_owned` returns a `Vec<u8>`, so `data.clone()` is a real
        // heap clone and must still fire. Proves the factory suppression is gated on
        // the return type, not merely on "initialized from any call".
        let source = r#"
            fn make_owned() -> Vec<u8> { vec![1, 2, 3] }
            fn run() {
                let data = make_owned();
                for _ in 0..10 {
                    let _c = data.clone();
                }
            }
        "#;
        assert_eq!(
            check_clone_rule(source).len(),
            1,
            "clone of a Vec from a non-Arc factory must still fire"
        );
    }

    #[test]
    fn test_arc_factory_rebind_to_owned_reflags() {
        // Shadowing must overwrite the factory-derived Arc record: after `let cfg =
        // s.to_string();`, `cfg` is an owned String and its clone must fire again.
        let source = r#"
            use std::sync::Arc;
            struct Config;
            fn make_shared() -> Arc<Config> { Arc::new(Config) }
            fn run(s: &str) {
                let cfg = make_shared();
                let cfg = s.to_string();
                for _ in 0..10 {
                    let _c = cfg.clone();
                }
            }
        "#;
        assert_eq!(
            check_clone_rule(source).len(),
            1,
            "rebinding the factory Arc name to an owned String must re-flag its clone"
        );
    }
}
