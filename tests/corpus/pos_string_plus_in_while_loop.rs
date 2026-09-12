// Positive: `+` on a String inside a loop allocates a new String each pass and
// copies everything accumulated so far — the classic quadratic append.
fn join(parts: &[String]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < parts.len() {
        out = out + &parts[i].to_string(); // perf-expect: string-concat-loop
        i += 1;
    }
    out
}
