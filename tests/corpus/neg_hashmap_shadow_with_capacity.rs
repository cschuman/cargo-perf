// perf-guard: hashmap-no-capacity
// Negative (same defect class as D34/D37, applied to HashMap): a shadowed
// `HashMap::with_capacity` binding is the one inserted-to in the loop; the earlier
// `HashMap::new()` binding is never grown. The rebind must clear the stale entry.
use std::collections::HashMap;

fn build(n: usize) -> HashMap<u32, u32> {
    let map = HashMap::new();
    let _ = &map;
    let mut map = HashMap::with_capacity(n);
    for i in 0..n as u32 {
        map.insert(i, i);
    }
    map
}
