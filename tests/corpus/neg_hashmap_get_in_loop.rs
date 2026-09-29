// perf-guard: hashmap-no-capacity
// Negative near-miss: a `HashMap::new()` that IS touched inside a loop, but only
// read. Lookups never grow the table, so there is nothing to pre-allocate; the
// rule must key on the growing operation, not on the map merely appearing in a
// loop body.
use std::collections::HashMap;

fn lookup(keys: &[u32]) -> usize {
    let mut map = HashMap::new();
    map.insert(1u32, 10usize);
    let mut hits = 0;
    for k in keys {
        if let Some(v) = map.get(k) {
            hits += v;
        }
    }
    hits
}
