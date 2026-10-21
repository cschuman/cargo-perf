// Positive: `HashMap::new()` starts empty, so inserting in a loop rehashes the
// whole table every time the load factor is exceeded.
use std::collections::HashMap;

fn index(keys: &[u32]) -> HashMap<u32, usize> {
    let mut map = HashMap::new(); // perf-expect: hashmap-no-capacity
    let mut i = 0;
    while i < keys.len() {
        map.insert(keys[i], i);
        i += 1;
    }
    map
}
