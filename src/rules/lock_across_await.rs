//! Rule for detecting lock guards held across await points.
//!
//! Holding a `MutexGuard` (or similar) across `.await` can cause deadlocks
//! in async code because the guard isn't released while waiting.

use super::visitor::VisitorState;
use super::{Diagnostic, Rule, Severity};
use crate::engine::AnalysisContext;
use std::collections::HashMap;
use syn::visit::Visit;
use syn::{Expr, ExprPath, ItemFn, Pat, Stmt};

/// Detects lock guards held across await points
pub struct LockAcrossAwaitRule;

impl Rule for LockAcrossAwaitRule {
    fn id(&self) -> &'static str {
        "lock-across-await"
    }

    fn name(&self) -> &'static str {
        "Lock Held Across Await"
    }

    fn description(&self) -> &'static str {
        "Detects MutexGuard/RwLockGuard held across .await points, which can cause deadlocks"
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, ctx: &AnalysisContext) -> Vec<Diagnostic> {
        let mut visitor = LockAcrossAwaitVisitor {
            ctx,
            diagnostics: Vec::new(),
            state: VisitorState::new(),
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct LockAcrossAwaitVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    state: VisitorState,
}

/// Methods known to return lock guards
const LOCK_METHODS: &[&str] = &["lock", "try_lock", "read", "try_read", "write", "try_write"];

impl LockAcrossAwaitVisitor<'_> {
    /// Extract variable name from a pattern
    fn extract_var_name(pat: &Pat) -> Option<String> {
        match pat {
            Pat::Ident(ident) => Some(ident.ident.to_string()),
            Pat::Type(pat_type) => Self::extract_var_name(&pat_type.pat),
            _ => None,
        }
    }

    /// Check if an expression is a lock acquisition (returns the lock method name)
    fn get_lock_method(expr: &Expr) -> Option<&str> {
        match expr {
            Expr::MethodCall(call) => {
                let method = call.method.to_string();
                for &lock_method in LOCK_METHODS {
                    if method == lock_method {
                        // `read`/`write` (and their `try_` forms) are lock
                        // acquisitions ONLY when nullary: `RwLock::read()` /
                        // `RwLock::write()` take no arguments, whereas
                        // `io::Read::read(buf)` / `io::Write::write(buf)` take a
                        // buffer and are ordinary async/sync I/O, not a guard.
                        // `lock`/`try_lock` are unambiguous and stay unconditional.
                        let is_read_write = matches!(
                            lock_method,
                            "read" | "try_read" | "write" | "try_write"
                        );
                        if is_read_write && !call.args.is_empty() {
                            return None;
                        }
                        return Some(lock_method);
                    }
                }
                // Check for .unwrap(), .expect(), .ok(), ? on a lock
                if matches!(method.as_str(), "unwrap" | "expect" | "ok") {
                    return Self::get_lock_method(&call.receiver);
                }
                None
            }
            // Handle .await on async locks
            Expr::Await(await_expr) => Self::get_lock_method(&await_expr.base),
            // Handle try operator: mutex.lock()?
            Expr::Try(try_expr) => Self::get_lock_method(&try_expr.expr),
            _ => None,
        }
    }

    /// Whether a lock acquisition is asynchronous (acquired via `.await`, as with
    /// `tokio::sync::Mutex`). Async guards held across `.await` merely serialize
    /// tasks; synchronous guards (std/parking_lot) risk deadlocking the runtime.
    fn is_async_lock_acquisition(expr: &Expr) -> bool {
        match expr {
            Expr::Await(_) => true,
            Expr::MethodCall(call)
                if matches!(call.method.to_string().as_str(), "unwrap" | "expect" | "ok") =>
            {
                Self::is_async_lock_acquisition(&call.receiver)
            }
            Expr::Try(try_expr) => Self::is_async_lock_acquisition(&try_expr.expr),
            _ => false,
        }
    }

    /// Analyze a block of statements for lock-across-await issues.
    ///
    /// `outer_guards` holds guards from enclosing scopes that are still active.
    /// A local `active` map is threaded through the block so that `drop(guard)`
    /// releases a guard for the remainder of the block, and guards declared in a
    /// nested block are scoped to that block.
    fn analyze_block(
        &mut self,
        stmts: &[Stmt],
        in_async: bool,
        outer_guards: &HashMap<String, bool>,
    ) {
        if !in_async {
            return;
        }

        // Maps guard name -> is_async (acquired via `.await`, e.g. a tokio lock).
        let mut active: HashMap<String, bool> = outer_guards.clone();

        for stmt in stmts {
            match stmt {
                Stmt::Local(local) => {
                    if let Some(init) = &local.init {
                        // Awaits in the initializer happen while current guards are held.
                        self.find_awaits(&init.expr, &active);
                        if let Some((_, diverge)) = &init.diverge {
                            self.find_awaits(diverge, &active);
                        }
                        // If this binds a lock guard, start tracking it (recording
                        // whether it is an async lock acquired via `.await`).
                        if Self::get_lock_method(&init.expr).is_some() {
                            if let Some(var_name) = Self::extract_var_name(&local.pat) {
                                let is_async = Self::is_async_lock_acquisition(&init.expr);
                                active.insert(var_name, is_async);
                            }
                        }
                    }
                }
                Stmt::Expr(expr, _) => {
                    // `drop(guard)` / `std::mem::drop(guard)` releases the guard.
                    if let Some(name) = Self::dropped_guard_name(expr) {
                        active.remove(&name);
                        continue;
                    }
                    // Recurse into nested blocks and control-flow bodies so guards
                    // declared inside them are scoped and tracked correctly.
                    self.analyze_flow_expr(expr, &active);
                }
                Stmt::Macro(_) => {
                    // Macro bodies are opaque to us; skip (documented limitation).
                }
                _ => {}
            }
        }
    }

    /// Analyze an expression appearing in statement position (or as a control-flow
    /// branch / match-arm body).
    ///
    /// The prior implementation only recursed into plain `{ }` blocks, so a
    /// `let guard = m.lock()` declared *inside* an `if`/`match`/`while`/`for`/`loop`
    /// body was never registered and a subsequent `.await` in that same body was
    /// missed (D21, D22). Here every control-flow body is recursed into via
    /// `analyze_block` with the currently-active guards as the outer scope, while
    /// awaits in the parts evaluated *before* a body — an `if`/`while` condition, a
    /// `match` scrutinee or arm guard, a `for` iterand — are checked against the
    /// active guards directly. Awaits are attributed exactly once: conditions go
    /// through `find_awaits`, bodies through `analyze_block`, with no overlap.
    fn analyze_flow_expr(&mut self, expr: &Expr, active: &HashMap<String, bool>) {
        match expr {
            Expr::Block(b) => self.analyze_block(&b.block.stmts, true, active),
            Expr::Unsafe(u) => self.analyze_block(&u.block.stmts, true, active),
            Expr::If(ei) => {
                self.find_awaits(&ei.cond, active);
                self.analyze_block(&ei.then_branch.stmts, true, active);
                if let Some((_, else_expr)) = &ei.else_branch {
                    self.analyze_flow_expr(else_expr, active);
                }
            }
            Expr::Match(em) => {
                self.find_awaits(&em.expr, active);
                for arm in &em.arms {
                    if let Some((_, guard_expr)) = &arm.guard {
                        self.find_awaits(guard_expr, active);
                    }
                    self.analyze_flow_expr(&arm.body, active);
                }
            }
            Expr::While(ew) => {
                self.find_awaits(&ew.cond, active);
                self.analyze_block(&ew.body.stmts, true, active);
            }
            Expr::ForLoop(ef) => {
                self.find_awaits(&ef.expr, active);
                self.analyze_block(&ef.body.stmts, true, active);
            }
            Expr::Loop(el) => {
                self.analyze_block(&el.body.stmts, true, active);
            }
            // Leaf / non-control-flow expression: attribute any awaits within it to
            // the active guards. `find_awaits` early-returns when none are held.
            _ => self.find_awaits(expr, active),
        }
    }

    /// Report every `.await` in `expr` (that is not itself a lock acquisition) as
    /// holding the currently-active guards across an await point. Delegates to a
    /// dedicated visitor so every expression form is covered (`?`, loops, `if`,
    /// `match`, assignments, ...), but does not descend into nested async blocks
    /// or closures, which are separate futures/scopes.
    fn find_awaits(&mut self, expr: &Expr, guards: &HashMap<String, bool>) {
        if guards.is_empty() {
            return;
        }
        let mut finder = AwaitFinder {
            sites: Vec::new(),
            depth: 0,
        };
        finder.visit_expr(expr);
        for span in finder.sites {
            self.push_lock_await_diagnostic(span, guards);
        }
    }

    /// Detect `drop(x)` / `std::mem::drop(x)` / `mem::drop(x)` and return `x`.
    fn dropped_guard_name(expr: &Expr) -> Option<String> {
        let Expr::Call(call) = expr else { return None };
        let Expr::Path(ExprPath { path, .. }) = &*call.func else {
            return None;
        };
        if path.segments.last()?.ident != "drop" || call.args.len() != 1 {
            return None;
        }
        if let Some(Expr::Path(ExprPath {
            path: arg_path,
            qself: None,
            ..
        })) = call.args.first()
        {
            if arg_path.segments.len() == 1 {
                return Some(arg_path.segments[0].ident.to_string());
            }
        }
        None
    }

    /// Emit a lock-across-await diagnostic for an await at `span`.
    ///
    /// A synchronous guard (std/parking_lot) held across `.await` can deadlock the
    /// runtime and is reported as an `Error`. If every held guard is asynchronous
    /// (e.g. `tokio::sync::Mutex`), it is correct-by-design but serializes tasks,
    /// so it is reported as a `Warning`.
    fn push_lock_await_diagnostic(
        &mut self,
        span: proc_macro2::Span,
        guards: &HashMap<String, bool>,
    ) {
        let mut guard_names: Vec<_> = guards.keys().cloned().collect();
        guard_names.sort();
        let plural = if guard_names.len() > 1 { "s" } else { "" };
        let names = guard_names.join("`, `");
        let line = span.start().line;
        let column = span.start().column;

        // Any synchronous guard among those held makes this a deadlock risk.
        let has_sync_guard = guards.values().any(|&is_async| !is_async);

        let (severity, message, suggestion) = if has_sync_guard {
            (
                Severity::Error,
                format!(
                    "Synchronous lock guard{plural} `{names}` held across `.await` point; \
                    this can deadlock the async runtime"
                ),
                "Drop the guard before awaiting, or narrow its scope: \
                `{ let guard = lock.lock(); /* use guard */ }` before the await. \
                If you must hold a lock across await, use an async lock (tokio::sync::Mutex)."
                    .to_string(),
            )
        } else {
            (
                Severity::Warning,
                format!(
                    "Async lock guard{plural} `{names}` held across `.await` point; \
                    this is safe but serializes tasks and can throttle throughput in hot paths"
                ),
                "This is safe with an async lock. If contention matters, drop the guard \
                before awaiting or shorten the critical section."
                    .to_string(),
            )
        };

        self.diagnostics.push(Diagnostic {
            rule_id: "lock-across-await",
            severity,
            message,
            file_path: self.ctx.file_path.to_path_buf(),
            line,
            column,
            end_line: None,
            end_column: None,
            suggestion: Some(suggestion),
            fix: None,
        });
    }
}

/// Maximum recursion depth for the await sub-visitor (defense against pathological input).
const AWAIT_FINDER_MAX_DEPTH: usize = 256;

/// Collects `.await` sites within an expression subtree while a lock guard is held,
/// without descending into nested async blocks or closures.
struct AwaitFinder {
    sites: Vec<proc_macro2::Span>,
    depth: usize,
}

impl<'ast> Visit<'ast> for AwaitFinder {
    fn visit_expr(&mut self, node: &'ast Expr) {
        if self.depth >= AWAIT_FINDER_MAX_DEPTH {
            return;
        }
        self.depth += 1;
        syn::visit::visit_expr(self, node);
        self.depth -= 1;
    }

    fn visit_expr_await(&mut self, node: &'ast syn::ExprAwait) {
        // The await that acquires a lock (e.g. `m.lock().await`) is not itself a
        // held-across point.
        if LockAcrossAwaitVisitor::get_lock_method(&node.base).is_none() {
            self.sites.push(node.await_token.span);
        }
        // Continue into the base to catch further nested awaits.
        self.visit_expr(&node.base);
    }

    fn visit_expr_async(&mut self, _node: &'ast syn::ExprAsync) {
        // Separate future: its awaits do not hold the current guard.
    }

    fn visit_expr_closure(&mut self, _node: &'ast syn::ExprClosure) {
        // Separate scope: skip.
    }

    fn visit_item(&mut self, _node: &'ast syn::Item) {
        // Nested item definitions have their own scope.
    }
}

impl<'ast> Visit<'ast> for LockAcrossAwaitVisitor<'_> {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        if self.state.should_bail() {
            return;
        }
        self.state.enter_expr();

        let is_async = node.sig.asyncness.is_some();
        if is_async {
            // Start with no outer guards at the function level
            self.analyze_block(&node.block.stmts, true, &HashMap::new());
        }

        // Continue visiting nested functions
        syn::visit::visit_item_fn(self, node);
        self.state.exit_expr();
    }

    fn visit_expr(&mut self, node: &'ast Expr) {
        if self.state.should_bail() {
            return;
        }
        self.state.enter_expr();
        syn::visit::visit_expr(self, node);
        self.state.exit_expr();
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
        LockAcrossAwaitRule.check(&ctx)
    }

    #[test]
    fn test_tokio_mutex_across_await_is_warning_not_deadlock() {
        // Holding a tokio (async) Mutex guard across .await is CORRECT by design
        // (that is the whole point of an async mutex). It is not a deadlock; at most
        // it serializes tasks. Report it as a Warning, not an Error, and do not call
        // it a deadlock. This is the opposite of clippy::await_holding_lock.
        let source = r#"
            async fn bad(mutex: &tokio::sync::Mutex<i32>) {
                let guard = mutex.lock().await;
                some_async_fn().await;
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].severity,
            Severity::Warning,
            "tokio async guard across await is a Warning, not an Error"
        );
        assert!(
            !diagnostics[0].message.contains("deadlock"),
            "must not describe an async lock as a deadlock"
        );
    }

    #[test]
    fn test_std_mutex_lock_across_await_is_error_deadlock() {
        // A std::sync::Mutex guard is a synchronous lock; holding it across .await
        // can deadlock the runtime. This stays an Error.
        let source = r#"
            async fn bad(mutex: &std::sync::Mutex<i32>) {
                let guard = mutex.lock().unwrap();
                some_async_fn().await;
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert!(diagnostics[0].message.contains("deadlock"));
    }

    #[test]
    fn test_mixed_sync_and_async_guard_is_error() {
        // If a synchronous guard is among those held, the await is an Error.
        let source = r#"
            async fn bad(a: &std::sync::Mutex<i32>, b: &tokio::sync::Mutex<i32>) {
                let ga = a.lock().unwrap();
                let gb = b.lock().await;
                some_async_fn().await;
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, Severity::Error);
    }

    #[test]
    fn test_detects_rwlock_read_across_await() {
        let source = r#"
            async fn bad(lock: &tokio::sync::RwLock<i32>) {
                let guard = lock.read().await;
                other_async().await;
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_detects_rwlock_write_across_await() {
        let source = r#"
            async fn bad(lock: &tokio::sync::RwLock<i32>) {
                let mut guard = lock.write().await;
                other_async().await;
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_no_detection_in_sync_function() {
        let source = r#"
            fn sync_fn(mutex: &std::sync::Mutex<i32>) {
                let guard = mutex.lock().unwrap();
                *guard += 1;
            }
        "#;
        let diagnostics = check_code(source);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_no_detection_when_no_await_after_lock() {
        let source = r#"
            async fn good(mutex: &tokio::sync::Mutex<i32>) {
                let guard = mutex.lock().await;
                *guard += 1;
            }
        "#;
        let diagnostics = check_code(source);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_detects_multiple_guards() {
        let source = r#"
            async fn bad(m1: &tokio::sync::Mutex<i32>, m2: &tokio::sync::Mutex<i32>) {
                let g1 = m1.lock().await;
                let g2 = m2.lock().await;
                some_async().await;
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_no_detection_when_guard_scoped() {
        // Guards dropped before await should NOT trigger warning
        let source = r#"
            async fn good(mutex: &tokio::sync::Mutex<i32>) {
                {
                    let guard = mutex.lock().await;
                    *guard += 1;
                } // guard dropped here
                some_async().await; // This is safe
            }
        "#;
        let diagnostics = check_code(source);
        assert!(
            diagnostics.is_empty(),
            "Should not warn when guard is scoped before await"
        );
    }

    #[test]
    fn test_detects_guard_in_outer_scope_with_await_in_inner() {
        // Guard in outer scope, await in inner block - should warn
        let source = r#"
            async fn bad(mutex: &tokio::sync::Mutex<i32>) {
                let guard = mutex.lock().await;
                {
                    some_async().await; // Guard still held from outer scope
                }
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(
            diagnostics.len(),
            1,
            "Should warn when outer guard spans inner await"
        );
    }

    // --- Detection-coverage fixes (H1/H6) and drop() false-positive fix ---

    #[test]
    fn test_detects_await_via_try_operator() {
        // `other().await?` desugars to Try(Await(..)); the guard is held across it.
        let source = r#"
            async fn bad(m: &std::sync::Mutex<i32>) -> Result<(), ()> {
                let g = m.lock().unwrap();
                other().await?;
                let _ = g;
                Ok(())
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(
            diagnostics.len(),
            1,
            "guard held across `foo().await?` must be detected"
        );
    }

    #[test]
    fn test_detects_await_inside_for_loop_body() {
        let source = r#"
            async fn bad(m: &std::sync::Mutex<i32>) {
                let g = m.lock().unwrap();
                for _ in 0..10 {
                    other().await; // guard held across await inside loop
                }
                let _ = g;
            }
        "#;
        let diagnostics = check_code(source);
        assert!(
            !diagnostics.is_empty(),
            "guard held across an await inside a loop body must be detected"
        );
    }

    #[test]
    fn test_detects_await_inside_while_loop_body() {
        let source = r#"
            async fn bad(m: &std::sync::Mutex<i32>) {
                let g = m.lock().unwrap();
                while cond() {
                    other().await;
                }
                let _ = g;
            }
        "#;
        let diagnostics = check_code(source);
        assert!(!diagnostics.is_empty());
    }

    #[test]
    fn test_detects_await_in_let_initializer() {
        // `let v = fetch().await;` while a guard is held.
        let source = r#"
            async fn bad(m: &std::sync::Mutex<i32>) {
                let g = m.lock().unwrap();
                let _v = fetch().await;
                let _ = g;
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_no_detection_when_guard_dropped_before_await() {
        let source = r#"
            async fn good(m: &std::sync::Mutex<i32>) {
                let g = m.lock().unwrap();
                let _v = *g;
                drop(g);
                other().await; // safe: guard already dropped
            }
        "#;
        let diagnostics = check_code(source);
        assert!(
            diagnostics.is_empty(),
            "dropping the guard before awaiting must suppress the diagnostic"
        );
    }

    #[test]
    fn test_no_detection_when_guard_mem_dropped_before_await() {
        let source = r#"
            async fn good(m: &std::sync::Mutex<i32>) {
                let g = m.lock().unwrap();
                std::mem::drop(g);
                other().await;
            }
        "#;
        let diagnostics = check_code(source);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_does_not_descend_into_spawned_async_block() {
        // The await lives in a future spawned onto another task; the guard is not
        // held across it, so this must NOT be flagged.
        let source = r#"
            async fn ok(m: &std::sync::Mutex<i32>) {
                let g = m.lock().unwrap();
                tokio::spawn(async move { other().await; });
                let _ = g;
            }
        "#;
        let diagnostics = check_code(source);
        assert!(
            diagnostics.is_empty(),
            "await inside a spawned async block must not be attributed to the outer guard"
        );
    }

    // --- read/write name-collision with async I/O (D19, D20) ---

    #[test]
    fn test_async_io_read_not_treated_as_lock_guard() {
        // D19: `reader.read(&mut buf).await` is idiomatic tokio AsyncReadExt I/O; the
        // bound `n` is a byte count, not a lock guard. `read` with an argument is I/O,
        // not `RwLock::read()` (which is nullary), so nothing must be flagged.
        let source = r#"
            use tokio::io::AsyncReadExt;
            async fn copy_stream<R: AsyncReadExt + Unpin>(reader: &mut R) {
                let mut buf = [0u8; 1024];
                let n = reader.read(&mut buf).await.unwrap();
                flush().await;
                let _ = n;
            }
            async fn flush() {}
        "#;
        let diagnostics = check_code(source);
        assert!(
            diagnostics.is_empty(),
            "async I/O read (with a buffer arg) must not be treated as a lock guard: {:?}",
            diagnostics
        );
    }

    #[test]
    fn test_sync_io_write_not_treated_as_lock_guard() {
        // D20: `buf.write(data)` is `std::io::Write` on a Vec<u8>; the bound `n` is a
        // byte count, not a lock guard. `write` with an argument is I/O, not
        // `RwLock::write()` (which is nullary).
        let source = r#"
            use std::io::Write;
            async fn dump(buf: &mut Vec<u8>, data: &[u8]) {
                let n = buf.write(data).unwrap();
                commit().await;
                let _ = n;
            }
            async fn commit() {}
        "#;
        let diagnostics = check_code(source);
        assert!(
            diagnostics.is_empty(),
            "sync I/O write (with a buffer arg) must not be treated as a lock guard: {:?}",
            diagnostics
        );
    }

    #[test]
    fn test_nullary_rwlock_read_still_detected() {
        // Guard: the genuine RwLock case (nullary `read()`) must still fire after the
        // arity discriminator is added.
        let source = r#"
            async fn bad(lock: &tokio::sync::RwLock<i32>) {
                let guard = lock.read().await;
                other().await;
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(
            diagnostics.len(),
            1,
            "nullary RwLock::read().await guard must still fire: {:?}",
            diagnostics
        );
    }

    // --- Guards declared INSIDE control-flow bodies (D21, D22) ---

    #[test]
    fn test_detects_guard_declared_inside_if_body() {
        // D21: std guard acquired and held across an await, both inside an `if` body.
        let source = r#"
            async fn conditional(m: &std::sync::Mutex<i32>, flag: bool) {
                if flag {
                    let guard = m.lock().unwrap();
                    let _v = *guard;
                    network_call().await;
                }
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(
            diagnostics.len(),
            1,
            "std guard held across await inside an if-body must fire: {:?}",
            diagnostics
        );
        assert_eq!(diagnostics[0].severity, Severity::Error);
    }

    #[test]
    fn test_detects_guard_declared_inside_match_arm() {
        // D22: std guard acquired and held across an await, both inside a match arm.
        let source = r#"
            async fn on_event(m: &std::sync::Mutex<i32>, ev: u8) {
                match ev {
                    0 => {
                        let guard = m.lock().unwrap();
                        let _v = *guard;
                        handle().await;
                    }
                    _ => {}
                }
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(
            diagnostics.len(),
            1,
            "std guard held across await inside a match arm must fire: {:?}",
            diagnostics
        );
        assert_eq!(diagnostics[0].severity, Severity::Error);
    }

    #[test]
    fn test_detects_guard_declared_inside_else_branch() {
        // The `else` arm of an if must be recursed into as well.
        let source = r#"
            async fn conditional(m: &std::sync::Mutex<i32>, flag: bool) {
                if flag {
                } else {
                    let guard = m.lock().unwrap();
                    let _v = *guard;
                    network_call().await;
                }
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(
            diagnostics.len(),
            1,
            "std guard held across await inside an else-body must fire: {:?}",
            diagnostics
        );
    }

    #[test]
    fn test_detects_guard_declared_inside_while_body() {
        // A guard acquired inside a loop body and held across an await in the same
        // iteration is a deadlock risk just as at function level.
        let source = r#"
            async fn looper(m: &std::sync::Mutex<i32>) {
                while cond() {
                    let guard = m.lock().unwrap();
                    let _v = *guard;
                    other().await;
                }
            }
        "#;
        let diagnostics = check_code(source);
        assert!(
            !diagnostics.is_empty(),
            "std guard declared inside a while-body held across await must fire"
        );
    }

    #[test]
    fn test_no_detection_when_guard_dropped_inside_if_body() {
        // Recursing into the if-body must not break drop handling: a guard dropped
        // before the await in the same body stays silent.
        let source = r#"
            async fn ok(m: &std::sync::Mutex<i32>, flag: bool) {
                if flag {
                    let guard = m.lock().unwrap();
                    let _v = *guard;
                    drop(guard);
                    other().await;
                }
            }
        "#;
        let diagnostics = check_code(source);
        assert!(
            diagnostics.is_empty(),
            "dropping a guard inside an if-body before the await must suppress: {:?}",
            diagnostics
        );
    }
}
