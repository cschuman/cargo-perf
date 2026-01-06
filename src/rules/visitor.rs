//! Shared visitor utilities for rule implementations.
//!
//! Provides loop-tracking and recursion depth limiting to prevent stack overflow.

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
    #[inline]
    pub fn exit_loop(&mut self) {
        self.loop_depth -= 1;
        self.recursion_depth -= 1;
    }

    /// Enter a nested expression - call at start of visit_expr, etc.
    #[inline]
    pub fn enter_expr(&mut self) {
        self.recursion_depth += 1;
    }

    /// Exit a nested expression - call at end of visit_expr, etc.
    #[inline]
    pub fn exit_expr(&mut self) {
        self.recursion_depth -= 1;
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
        assert!(MAX_RECURSION_DEPTH >= 128);
        assert!(MAX_RECURSION_DEPTH <= 512);
    }
}
