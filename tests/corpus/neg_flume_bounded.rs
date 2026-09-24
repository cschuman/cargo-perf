// perf-guard: unbounded-channel
// Negative near-miss: `flume::bounded(32)` is the same crate and the same
// module as the flagged `flume::unbounded()`, differing only in the constructor
// name. A bounded constructor provides the backpressure this rule asks for, so
// matching on the crate name alone would be a false positive.
fn wire() {
    let (_tx, _rx) = flume::bounded(32);
}
