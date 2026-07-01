// perf-guard: string-no-capacity
// Negative: String::with_capacity then push_str is the recommended form.
fn build(words: &[&str]) -> String {
    let mut s = String::with_capacity(64);
    for word in words {
        s.push_str(word);
    }
    s
}
