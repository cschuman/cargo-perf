// Positive: `String::new()` has zero capacity, so `push` in a loop reallocates
// and copies the buffer as it grows.
fn mask(n: usize) -> String {
    let mut out = String::new(); // perf-expect: string-no-capacity
    let mut i = 0;
    while i < n {
        out.push('*');
        i += 1;
    }
    out
}
