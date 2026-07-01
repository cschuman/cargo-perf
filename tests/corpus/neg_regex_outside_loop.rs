// perf-guard: regex-in-loop
// Negative: compiling a Regex once, outside any loop, is correct.
fn build() {
    let re = regex::Regex::new(r"\d+").unwrap();
    let _ = re;
}
