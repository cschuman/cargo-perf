// Positive: `crossbeam_channel::unbounded()` has no backpressure, so a producer
// outrunning its consumer grows the queue until memory is exhausted.
fn wire() {
    let (_tx, _rx) = crossbeam_channel::unbounded(); // perf-expect: unbounded-channel
}
