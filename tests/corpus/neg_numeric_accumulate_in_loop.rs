// perf-guard: string-concat-loop
// Negative near-miss: `+=` in a loop over a slice of Strings — the shape that
// trips naive matchers — but the operands are numeric and the result is a count.
// The rule requires positive evidence that a side really is a string (literal,
// `to_string()`, `format!`), so arithmetic accumulation must stay silent.
fn total_len(parts: &[String]) -> usize {
    let mut total = 0;
    for p in parts {
        total += p.len();
    }
    total
}
