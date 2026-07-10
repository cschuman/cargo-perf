// perf-guard: hashmap-no-capacity
// Negative: HashMap::with_capacity is the recommended form.
use std::collections::HashMap;

fn build() -> HashMap<i32, i32> {
    let mut map = HashMap::with_capacity(100);
    for i in 0..100 {
        map.insert(i, i);
    }
    map
}
