// Positive: a `String` deep-copied on every `while` iteration. The value never
// changes, so the whole allocation-and-memcpy is redundant work.
fn repeat(src: &String, n: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        out.push(src.clone()); // perf-expect: clone-in-hot-loop
        i += 1;
    }
    out
}
