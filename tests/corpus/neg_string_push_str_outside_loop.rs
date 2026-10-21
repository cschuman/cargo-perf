// perf-guard: string-no-capacity
// Negative near-miss: a `String::new()` appended to twice with no loop around
// it. Without iteration there is no growth curve to pre-allocate against, so the
// rule must stay loop-scoped rather than firing on `String::new()` plus any
// later `push_str`.
fn greet(name: &str) -> String {
    let mut out = String::new();
    out.push_str("hello, ");
    out.push_str(name);
    out
}
