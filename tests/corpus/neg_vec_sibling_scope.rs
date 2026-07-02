// perf-guard: vec-no-capacity
// Negative (D38): two same-named `out` bindings in sibling scopes. Scope-1 pushes
// once outside a loop; scope-2 uses `Vec::with_capacity` and grows in a loop. The
// first scope's stale entry must not leak across the block boundary onto the second.
fn two_scopes(a: &[i32], b: &[i32]) -> (Vec<i32>, Vec<i32>) {
    let first = {
        let mut out = Vec::new();
        out.push(a[0]);
        out
    };
    let second = {
        let mut out = Vec::with_capacity(b.len());
        for x in b {
            out.push(*x);
        }
        out
    };
    (first, second)
}
