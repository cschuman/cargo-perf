// perf-guard: collect-then-iterate
// Negative near-miss: the same `.collect::<Vec<_>>()` as the positive fixture,
// but followed by `.sort()` instead of a second iteration. Sorting *requires*
// the materialized Vec, so there is no intermediate allocation to fuse away.
fn sorted(items: &[i32]) -> Vec<i32> {
    let mut out = items.iter().map(|x| x * 2).collect::<Vec<_>>();
    out.sort();
    out
}
