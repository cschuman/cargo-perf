//! Rule for detecting lock guards held across await points.
//!
//! Holding a `MutexGuard` (or similar) across `.await` can cause deadlocks
//! in async code because the guard isn't released while waiting.

use super::visitor::VisitorState;
use super::{Diagnostic, Rule, Severity};
use crate::engine::AnalysisContext;
use std::collections::HashMap;
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Expr, ItemFn, Pat, Stmt};

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

    /// Analyze a block of statements for lock-across-await issues
    fn analyze_block(&mut self, stmts: &[Stmt], in_async: bool) {
        if !in_async {
            return;
        }

        // Track guards by their declaration line
        let mut active_guards: HashMap<String, usize> = HashMap::new();

        for stmt in stmts {
            match stmt {
                Stmt::Local(local) => {
                    // Check if this is a lock guard assignment
                    if let Some(init) = &local.init {
                        if Self::get_lock_method(&init.expr).is_some() {
                            if let Some(var_name) = Self::extract_var_name(&local.pat) {
                                let line = local.span().start().line;
                                active_guards.insert(var_name, line);
                            }
                        }
                    }
                }
                Stmt::Expr(expr, _) => {
                    // Check for await points while guards are held
                    if !active_guards.is_empty() {
                        self.check_await_in_expr(expr, &active_guards);
                    }

                    // Check if this is a block that might drop guards
                    if let Expr::Block(block) = expr {
                        self.analyze_block(&block.block.stmts, in_async);
                    }
                }
                Stmt::Macro(_) => {
                    // Macros can contain anything, skip for now
                }
                _ => {}
            }
        }
    }

    /// Check for await points in an expression (but not the lock acquisition await)
    fn check_await_in_expr(&mut self, expr: &Expr, guards: &HashMap<String, usize>) {
        match expr {
            Expr::Await(await_expr) => {
                // Don't flag the await that's part of lock acquisition
                if Self::get_lock_method(&await_expr.base).is_none() {
                    let guard_names: Vec<_> = guards.keys().cloned().collect();
                    let span = await_expr.await_token.span;
                    let line = span.start().line;
                    let column = span.start().column;

                    self.diagnostics.push(Diagnostic {
                        rule_id: "lock-across-await",
                        severity: Severity::Error,
                        message: format!(
                            "Lock guard{} `{}` held across `.await` point; this can cause deadlocks",
                            if guard_names.len() > 1 { "s" } else { "" },
                            guard_names.join("`, `")
                        ),
                        file_path: self.ctx.file_path.to_path_buf(),
                        line,
                        column,
                        end_line: None,
                        end_column: None,
                        suggestion: Some(
                            "Drop the guard before awaiting, or restructure to avoid holding locks across await points. \
                            Consider using a scope block: `{ let guard = lock.lock(); /* use guard */ }` before the await."
                                .to_string(),
                        ),
                        fix: None,
                    });
                }
            }
            Expr::Block(block) => {
                // Analyze nested block - guards from outer scope are still active
                for stmt in &block.block.stmts {
                    if let Stmt::Expr(e, _) = stmt {
                        self.check_await_in_expr(e, guards);
                    }
                }
            }
            Expr::If(if_expr) => {
                self.check_await_in_expr(&if_expr.cond, guards);
                for stmt in &if_expr.then_branch.stmts {
                    if let Stmt::Expr(e, _) = stmt {
                        self.check_await_in_expr(e, guards);
                    }
                }
                if let Some((_, else_branch)) = &if_expr.else_branch {
                    self.check_await_in_expr(else_branch, guards);
                }
            }
            Expr::Match(match_expr) => {
                self.check_await_in_expr(&match_expr.expr, guards);
                for arm in &match_expr.arms {
                    self.check_await_in_expr(&arm.body, guards);
                }
            }
            Expr::MethodCall(call) => {
                self.check_await_in_expr(&call.receiver, guards);
                for arg in &call.args {
                    self.check_await_in_expr(arg, guards);
                }
            }
            Expr::Call(call) => {
                self.check_await_in_expr(&call.func, guards);
                for arg in &call.args {
                    self.check_await_in_expr(arg, guards);
                }
            }
            _ => {}
        }
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
            self.analyze_block(&node.block.stmts, true);
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
    fn test_detects_mutex_lock_across_await() {
        let source = r#"
            async fn bad(mutex: &tokio::sync::Mutex<i32>) {
                let guard = mutex.lock().await;
                some_async_fn().await;
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("guard"));
        assert!(diagnostics[0].message.contains("deadlock"));
    }

    #[test]
    fn test_detects_std_mutex_lock_across_await() {
        let source = r#"
            async fn bad(mutex: &std::sync::Mutex<i32>) {
                let guard = mutex.lock().unwrap();
                some_async_fn().await;
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 1);
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
}
