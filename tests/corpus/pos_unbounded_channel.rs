// Positive: an unbounded mpsc channel has no backpressure and can exhaust memory.
fn make() {
    let (tx, rx) = std::sync::mpsc::channel::<i32>(); // perf-expect: unbounded-channel
    let _ = (tx, rx);
}
