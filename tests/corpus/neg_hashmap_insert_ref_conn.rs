// perf-guard: n-plus-one-query
// Negative (D26): a plain `HashMap::insert(k, &conn)` in a loop, where `conn` is a
// local `u32` in the VALUE position. The N+1 connection-arg heuristic inspects only
// the first argument (the executor slot in real ORM calls), so a `conn` in the
// value slot no longer corroborates a query. No ORM is imported here.
use std::collections::HashMap;

fn build(pairs: &[(u32, u32)]) -> HashMap<u32, u32> {
    let mut map = HashMap::with_capacity(pairs.len());
    let conn = 7u32;
    for &(k, v) in pairs {
        map.insert(k, &conn);
        let _ = map.get(&k);
        let _ = v;
    }
    map
}
