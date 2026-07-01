// perf-guard: collect-then-iterate
// Negative: collecting and then taking .len() is not a collect-then-iterate
// (there is no second iteration pass to fuse away).
fn count(items: &[i32]) -> usize {
    items.iter().map(|x| x + 1).collect::<Vec<_>>().len()
}
