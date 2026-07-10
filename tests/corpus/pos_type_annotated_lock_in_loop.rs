// Positive (D23): `m` is a lock holder by its TYPE annotation (`Arc<Mutex<u64>>`),
// even though its initializer is a `.clone()` rather than a `Mutex::new(..)` ctor.
// Locking it on every while-loop iteration is real contention — the exact
// "acquire the lock once outside the loop" antipattern — so it must fire.
use std::sync::{Arc, Mutex};

fn run(shared: &Arc<Mutex<u64>>) {
    let m: Arc<Mutex<u64>> = shared.clone();
    let mut i = 0;
    while i < 50 {
        *m.lock().unwrap() += 1; // perf-expect: mutex-in-loop
        i += 1;
    }
}
