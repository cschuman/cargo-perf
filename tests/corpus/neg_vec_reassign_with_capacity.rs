// perf-guard: vec-no-capacity
// Negative (D36): `buf` is reassigned to `Vec::with_capacity(n)` before the hot
// loop, so every push targets the pre-sized vec. Reassignment must clear the stale
// `Vec::new()` classification, or the name-keyed tracker falsely flags the sized vec.
fn build(n: usize) -> Vec<u32> {
    let mut buf = Vec::new();
    buf = Vec::with_capacity(n);
    for i in 0..n {
        buf.push(i as u32);
    }
    buf
}
