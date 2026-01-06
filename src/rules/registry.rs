//! Rule registry - static collection of all available performance rules.
//!
//! Rules are initialized once using `LazyLock` and reused across all analysis runs.

use super::allocation_rules::{
    FormatInLoopRule, MutexLockInLoopRule, StringConcatLoopRule, VecNoCapacityRule,
};
use super::async_rules::AsyncBlockInAsyncRule;
use super::iter_rules::CollectThenIterateRule;
use super::lock_across_await::LockAcrossAwaitRule;
use super::memory_rules::{CloneInLoopRule, RegexInLoopRule};
use super::Rule;
use std::collections::HashMap;
use std::sync::LazyLock;

/// Static registry of all rules, initialized once on first access.
static RULES: LazyLock<Vec<Box<dyn Rule>>> = LazyLock::new(|| {
    vec![
        // Async rules
        Box::new(AsyncBlockInAsyncRule),
        Box::new(LockAcrossAwaitRule),
        // Memory rules
        Box::new(CloneInLoopRule),
        Box::new(RegexInLoopRule),
        // Iterator rules
        Box::new(CollectThenIterateRule),
        // Allocation rules
        Box::new(VecNoCapacityRule),
        Box::new(FormatInLoopRule),
        Box::new(StringConcatLoopRule),
        Box::new(MutexLockInLoopRule),
    ]
});

/// Index for O(1) rule lookup by ID.
static RULE_INDEX: LazyLock<HashMap<&'static str, usize>> = LazyLock::new(|| {
    RULES
        .iter()
        .enumerate()
        .map(|(idx, rule)| (rule.id(), idx))
        .collect()
});

/// Get all registered rules.
///
/// This returns a reference to the static rule list, avoiding allocation.
#[inline]
pub fn all_rules() -> &'static [Box<dyn Rule>] {
    &RULES
}

/// Get a rule by its ID in O(1) time.
///
/// Returns `None` if no rule with the given ID exists.
#[inline]
pub fn get_rule(id: &str) -> Option<&'static dyn Rule> {
    RULE_INDEX.get(id).map(|&idx| RULES[idx].as_ref())
}

/// Get all rule IDs.
pub fn rule_ids() -> impl Iterator<Item = &'static str> {
    RULES.iter().map(|r| r.id())
}

/// Check if a rule ID exists.
#[inline]
pub fn has_rule(id: &str) -> bool {
    RULE_INDEX.contains_key(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_rules_returns_same_slice() {
        let rules1 = all_rules();
        let rules2 = all_rules();
        // Should be the exact same memory location
        assert!(std::ptr::eq(rules1.as_ptr(), rules2.as_ptr()));
    }

    #[test]
    fn test_get_rule_by_id() {
        let rule = get_rule("async-block-in-async");
        assert!(rule.is_some());
        assert_eq!(rule.unwrap().id(), "async-block-in-async");
    }

    #[test]
    fn test_get_rule_unknown_id() {
        let rule = get_rule("nonexistent-rule");
        assert!(rule.is_none());
    }

    #[test]
    fn test_has_rule() {
        assert!(has_rule("clone-in-hot-loop"));
        assert!(!has_rule("fake-rule"));
    }

    #[test]
    fn test_rule_ids() {
        let ids: Vec<_> = rule_ids().collect();
        assert!(ids.contains(&"async-block-in-async"));
        assert!(ids.contains(&"lock-across-await"));
        assert!(ids.contains(&"clone-in-hot-loop"));
        assert!(ids.contains(&"regex-in-loop"));
        assert!(ids.contains(&"collect-then-iterate"));
        assert!(ids.contains(&"vec-no-capacity"));
        assert!(ids.contains(&"format-in-loop"));
        assert!(ids.contains(&"string-concat-loop"));
        assert!(ids.contains(&"mutex-in-loop"));
    }
}
