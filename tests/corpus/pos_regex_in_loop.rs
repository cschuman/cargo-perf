// Positive: compiling a regex inside the loop instead of hoisting it out.
fn check(lines: &[String]) {
    for line in lines {
        let re = regex::Regex::new(r"\d+").unwrap(); // perf-expect: regex-in-loop
        let _ = re.is_match(line);
    }
}
