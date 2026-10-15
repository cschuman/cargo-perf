// Positive: `Vec::new()` starts at zero capacity, so pushing in a loop pays a
// realloc-and-copy every time the buffer doubles.
fn collect_ids(n: usize) -> Vec<usize> {
    let mut out = Vec::new(); // perf-expect: vec-no-capacity
    let mut i = 0;
    while i < n {
        out.push(i);
        i += 1;
    }
    out
}
