// perf-guard: vec-no-capacity
// Negative (D37): the outer `v` = Vec::new() is pushed once outside any loop; a
// shadowed inner `v` = Vec::with_capacity is the one actually grown in the loop.
// A name-keyed tracker that never clears on shadow would blame the sized inner vec.
fn render(items: &[String]) -> Vec<usize> {
    let mut v = Vec::new();
    v.push(0);
    drop(v);
    {
        let mut v = Vec::with_capacity(items.len());
        for it in items {
            v.push(it.len());
        }
        v
    }
}
