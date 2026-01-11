//! Shared visitor utilities for rule implementations.
//!
//! Provides loop-tracking and recursion depth limiting to prevent stack overflow.
//!
//! # Macros
//!
//! The `impl_loop_tracking_visitor!` macro generates boilerplate Visit trait methods
//! for tracking loop depth and recursion. Use it to reduce repetitive code in rules.

/// Macro to implement loop-tracking visitor methods.
///
/// This macro generates the standard Visit trait implementations for
/// `visit_expr_for_loop`, `visit_expr_while`, `visit_expr_loop`, and `visit_expr`
/// that properly track loop and recursion depth using `VisitorState`.
///
/// # Requirements
///
/// The visitor struct must have a field named `state` of type `VisitorState`.
///
/// # Example
///
/// ```ignore
/// use syn::visit::Visit;
/// use crate::rules::visitor::{VisitorState, impl_loop_tracking_visitor};
///
/// struct MyVisitor<'a> {
///     ctx: &'a AnalysisContext<'a>,
///     diagnostics: Vec<Diagnostic>,
///     state: VisitorState,
/// }
///
/// impl_loop_tracking_visitor!(MyVisitor<'a>);
///
/// impl<'ast> Visit<'ast> for MyVisitor<'_> {
///     impl_loop_tracking_visitor!(@methods);
///
///     fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
///         // Your custom logic here
///         syn::visit::visit_expr_method_call(self, node);
///     }
/// }
/// ```
#[macro_export]
macro_rules! impl_loop_tracking_visitor {
    // Generate all loop tracking methods for use in a Visit impl block
    (@methods) => {
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
    };
}

// Re-export macro at crate level
pub use impl_loop_tracking_visitor;

/// Maximum recursion depth for AST visitors.
/// Protects against maliciously crafted deeply-nested code.
pub const MAX_RECURSION_DEPTH: usize = 256;

/// Helper for tracking loop depth and recursion depth in visitors.
///
/// Embed this in your visitor struct and use its methods to track state.
#[derive(Default)]
pub struct VisitorState {
    pub loop_depth: usize,
    pub recursion_depth: usize,
}

impl VisitorState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if we should bail out due to excessive recursion.
    #[inline]
    pub fn should_bail(&self) -> bool {
        self.recursion_depth >= MAX_RECURSION_DEPTH
    }

    /// Enter a loop - call at start of visit_expr_for_loop, etc.
    #[inline]
    pub fn enter_loop(&mut self) {
        self.loop_depth += 1;
        self.recursion_depth += 1;
    }

    /// Exit a loop - call at end of visit_expr_for_loop, etc.
    ///
    /// Uses saturating subtraction to prevent underflow if called
    /// more times than enter_loop (indicates a visitor implementation bug).
    #[inline]
    pub fn exit_loop(&mut self) {
        self.loop_depth = self.loop_depth.saturating_sub(1);
        self.recursion_depth = self.recursion_depth.saturating_sub(1);
    }

    /// Enter a nested expression - call at start of visit_expr, etc.
    #[inline]
    pub fn enter_expr(&mut self) {
        self.recursion_depth += 1;
    }

    /// Exit a nested expression - call at end of visit_expr, etc.
    ///
    /// Uses saturating subtraction to prevent underflow if called
    /// more times than enter_expr (indicates a visitor implementation bug).
    #[inline]
    pub fn exit_expr(&mut self) {
        self.recursion_depth = self.recursion_depth.saturating_sub(1);
    }

    /// Check if currently inside a loop.
    #[inline]
    pub fn in_loop(&self) -> bool {
        self.loop_depth > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visitor_state_loop_tracking() {
        let mut state = VisitorState::new();
        assert!(!state.in_loop());

        state.enter_loop();
        assert!(state.in_loop());
        assert_eq!(state.loop_depth, 1);

        state.enter_loop();
        assert_eq!(state.loop_depth, 2);

        state.exit_loop();
        assert!(state.in_loop());

        state.exit_loop();
        assert!(!state.in_loop());
    }

    #[test]
    fn test_visitor_state_recursion_limit() {
        let mut state = VisitorState::new();
        assert!(!state.should_bail());

        for _ in 0..MAX_RECURSION_DEPTH {
            state.enter_expr();
        }
        assert!(state.should_bail());

        state.exit_expr();
        assert!(!state.should_bail());
    }

    #[test]
    fn test_max_recursion_depth_is_reasonable() {
        const { assert!(MAX_RECURSION_DEPTH >= 128) };
        const { assert!(MAX_RECURSION_DEPTH <= 512) };
    }
}
