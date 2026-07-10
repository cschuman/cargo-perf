// Positive: collecting into a Vec only to immediately iterate it again wastes
// an allocation.
fn build(items: &[i32]) -> Vec<i32> {
    items.iter().map(|x| x * 2).collect::<Vec<_>>().iter().map(|x| x + 1).collect() // perf-expect: collect-then-iterate
}
