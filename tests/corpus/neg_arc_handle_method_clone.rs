// perf-guard: clone-in-hot-loop
// Negative (D11): `handle` is derived from an existing Arc via `.clone()` (a
// method call, not an `Arc::new`/`Arc::clone` ctor). It is still an Arc, so
// cloning it in the loop is only a refcount bump and must stay silent.
use std::sync::Arc;

fn run(orig: Arc<String>) -> usize {
    let handle = orig.clone();
    let mut n = 0usize;
    for _ in 0..50 {
        let c = handle.clone();
        n += c.len();
    }
    n
}
