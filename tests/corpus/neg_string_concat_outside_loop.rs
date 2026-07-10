// perf-guard: string-concat-loop
// Negative: a couple of `+=` appends outside a loop are not a hot-loop concat.
fn build(a: &str, b: &str) -> String {
    let mut s = String::with_capacity(16);
    s += a;
    s += b;
    s
}
