// perf-guard: format-in-loop
// Negative: a single format! outside a loop is fine.
fn label() -> String {
    format!("value = {}", 42)
}
