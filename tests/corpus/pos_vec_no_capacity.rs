// Positive: growing a Vec with push in a loop without reserving capacity.
fn collect(n: usize) -> Vec<usize> {
    let mut v = Vec::new(); // perf-expect: vec-no-capacity
    for i in 0..n {
        v.push(i);
    }
    v
}
