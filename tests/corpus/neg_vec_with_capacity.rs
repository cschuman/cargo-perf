// perf-guard: vec-no-capacity
// Negative: pre-sizing with Vec::with_capacity is exactly the recommended fix.
fn build() -> Vec<i32> {
    let mut v = Vec::with_capacity(10);
    for i in 0..10 {
        v.push(i);
    }
    v
}
