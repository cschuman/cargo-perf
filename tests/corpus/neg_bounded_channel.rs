// perf-guard: unbounded-channel
// Negative: a bounded sync_channel provides backpressure and must not be flagged.
fn make() {
    let (tx, rx) = std::sync::mpsc::sync_channel::<i32>(100);
    let _ = (tx, rx);
}
