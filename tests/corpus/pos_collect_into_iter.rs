// Positive: collecting into a HashSet only to immediately `.into_iter()` it
// again materializes a whole intermediate collection the chain then discards.
use std::collections::HashSet;

fn dedup(items: &[i32]) -> Vec<i32> {
    items.iter().copied().collect::<HashSet<_>>().into_iter().collect() // perf-expect: collect-then-iterate
}
