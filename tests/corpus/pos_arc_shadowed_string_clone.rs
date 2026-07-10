// Positive (D16): `data` starts as an Arc (cloning it is a cheap refcount bump),
// but is then shadowed by an owned String. The loop clones the String — a real
// per-iteration heap allocation — so the stale Arc classification must NOT suppress
// it. Shadowing must clear the Arc/Rc record keyed by the reused name.
use std::sync::Arc;

fn run(seed: &str) -> usize {
    let data = Arc::new(vec![1u8, 2, 3]);
    let _ = data.len();
    let data = seed.to_string();
    let mut total = 0;
    for _ in 0..1000 {
        let owned = data.clone(); // perf-expect: clone-in-hot-loop
        total += owned.len();
    }
    total
}
