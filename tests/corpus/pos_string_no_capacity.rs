// Positive: String::new() then push_str in a loop reallocates as it grows.
fn build(words: &[&str]) -> String {
    let mut s = String::new(); // perf-expect: string-no-capacity
    for word in words {
        s.push_str(word);
    }
    s
}
