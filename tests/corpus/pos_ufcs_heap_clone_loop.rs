// Positive (D15): a genuine heap clone of a String written via UFCS
// `Clone::clone(&s)` allocates identically to `s.clone()` on every iteration,
// but parses as a Call rather than a MethodCall. Using UFCS is not a way to
// hide a hot-loop heap clone from the linter, so it must still fire.
fn run(s: String) -> usize {
    let mut total = 0;
    for _ in 0..1000 {
        let owned = Clone::clone(&s); // perf-expect: clone-in-hot-loop
        total += owned.len();
    }
    total
}
