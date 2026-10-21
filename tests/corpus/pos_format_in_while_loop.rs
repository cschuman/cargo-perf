// Positive: `format!` inside a `while` loop allocates a fresh String on every
// iteration and throws it away at the end of the pass.
fn render(n: usize) {
    let mut i = 0;
    while i < n {
        let line = format!("row {}", i); // perf-expect: format-in-loop
        drop(line);
        i += 1;
    }
}
