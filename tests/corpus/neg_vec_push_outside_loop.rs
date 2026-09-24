// perf-guard: vec-no-capacity
// Negative near-miss: a `Vec::new()` pushed to a fixed, small number of times
// with no loop around it. The push count is a compile-time constant, so there is
// no growth-by-iteration to pre-allocate against and the rule stays loop-scoped.
fn pair(a: u32, b: u32) -> Vec<u32> {
    let mut out = Vec::new();
    out.push(a);
    out.push(b);
    out
}
