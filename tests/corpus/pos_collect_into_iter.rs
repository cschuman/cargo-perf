// Positive: collecting into a Vec only to immediately `.into_iter()` it again
// materializes a whole intermediate collection the chain then discards. The
// `map` and `filter` could run as one lazy chain with a single final collect.
fn doubled_positive(items: &[i32]) -> Vec<i32> {
    items.iter().map(|x| x * 2).collect::<Vec<_>>().into_iter().filter(|x| *x > 0).collect() // perf-expect: collect-then-iterate
}
