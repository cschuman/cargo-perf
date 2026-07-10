// Positive: building a string with `+=` in a loop; use push_str / with_capacity.
fn build() -> String {
    let mut s = String::with_capacity(8);
    for _ in 0..3 {
        s += "x"; // perf-expect: string-concat-loop
    }
    s
}
