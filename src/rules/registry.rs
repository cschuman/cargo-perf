use super::async_rules::AsyncBlockInAsyncRule;
use super::iter_rules::CollectThenIterateRule;
use super::memory_rules::{CloneInLoopRule, RegexInLoopRule};
use super::Rule;

/// Get all registered rules
pub fn all_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(AsyncBlockInAsyncRule),
        Box::new(CloneInLoopRule),
        Box::new(RegexInLoopRule),
        Box::new(CollectThenIterateRule),
    ]
}

/// Get a rule by its ID
pub fn get_rule(id: &str) -> Option<Box<dyn Rule>> {
    all_rules().into_iter().find(|r| r.id() == id)
}
