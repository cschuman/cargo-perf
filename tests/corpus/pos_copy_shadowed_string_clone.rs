// Positive (D17): `key` is annotated Copy `u32` (cloning it is a no-op bitwise
// copy), but is then shadowed by an owned String. The loop clones the String — a
// real heap allocation each iteration — so the stale Copy classification must NOT
// suppress it. Shadowing must clear the Copy record keyed by the reused name.
fn run(raw: &str) -> usize {
    let key: u32 = 0;
    let _ = key;
    let key = raw.to_string();
    let mut total = 0;
    for _ in 0..1000 {
        let owned = key.clone(); // perf-expect: clone-in-hot-loop
        total += owned.len();
    }
    total
}
