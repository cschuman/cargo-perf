// Positive: `Regex::new` inside a `loop` recompiles the pattern on every
// iteration, and compilation costs orders of magnitude more than matching.
fn scan(lines: &[String]) {
    let mut i = 0;
    loop {
        if i >= lines.len() {
            break;
        }
        let re = regex::Regex::new(r"\d+").unwrap(); // perf-expect: regex-in-loop
        let _ = re.is_match(&lines[i]);
        i += 1;
    }
}
