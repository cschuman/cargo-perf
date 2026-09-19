// Positive: prepending with `+` inside a loop. `parts[i].to_string() + &out`
// allocates a new String from the part and then copies the entire accumulated
// `out` into it, discarding the old buffer. Every pass re-copies everything
// built so far, so the total work is quadratic in the output length.
fn join_reversed(parts: &[String]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < parts.len() {
        out = parts[i].to_string() + &out; // perf-expect: string-concat-loop
        i += 1;
    }
    out
}
