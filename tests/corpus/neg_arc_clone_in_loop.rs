// perf-guard: clone-in-hot-loop
// Negative (guards Fix 2): cloning an Arc is a cheap refcount bump, NOT a
// heap-copy. `clone-in-hot-loop` must not fire here.
use std::sync::Arc;

fn process(shared: Arc<Vec<u8>>) {
    for _ in 0..10 {
        let cloned = shared.clone();
        consume(cloned);
    }
}

fn consume(_: Arc<Vec<u8>>) {}
