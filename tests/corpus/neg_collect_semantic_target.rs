// perf-guard: collect-then-iterate
// Negative: each intermediate collection changes the result, so it cannot be
// fused away. HashSet/BTreeSet deduplicate, BTreeMap sorts by key and keeps
// the last value per key, and Result short-circuits on the first error.
use std::collections::{BTreeMap, BTreeSet, HashSet};

fn dedup(items: &[i32]) -> Vec<i32> {
    items.iter().copied().collect::<HashSet<_>>().into_iter().collect()
}

fn sorted_unique(items: &[i32]) -> Vec<i32> {
    items.iter().copied().collect::<BTreeSet<_>>().iter().copied().collect()
}

fn last_per_key(pairs: &[(u8, i32)]) -> Vec<i32> {
    pairs.iter().copied().collect::<BTreeMap<_, _>>().into_values().collect()
}

fn parse_all(raw: &[&str]) -> Vec<i32> {
    raw.iter()
        .map(|s| s.parse::<i32>())
        .collect::<Result<Vec<_>, _>>()
        .into_iter()
        .flatten()
        .collect()
}
