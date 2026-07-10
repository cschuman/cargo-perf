// Positive: cloning an owned heap value on every iteration is a hot-loop clone.
fn process(items: &[String]) {
    for item in items {
        let copy = item.clone(); // perf-expect: clone-in-hot-loop
        consume(copy);
    }
}

fn consume(_: String) {}
