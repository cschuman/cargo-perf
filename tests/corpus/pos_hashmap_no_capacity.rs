// Positive: HashMap::new() then insert in a known-size loop rehashes repeatedly.
use std::collections::HashMap;

fn build() -> HashMap<i32, i32> {
    let mut map = HashMap::new(); // perf-expect: hashmap-no-capacity
    for i in 0..100 {
        map.insert(i, i * 2);
    }
    map
}
