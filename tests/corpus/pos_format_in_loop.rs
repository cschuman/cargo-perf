// Positive: allocating a formatted String on every iteration.
fn build(names: &[String]) {
    for name in names {
        let s = format!("hello {}", name); // perf-expect: format-in-loop
        consume(s);
    }
}

fn consume(_: String) {}
