// Positive: re-acquiring a lock every iteration instead of once outside.
use std::sync::RwLock;

fn writer(lock: &RwLock<i32>) {
    for _ in 0..10 {
        let mut g = lock.write().unwrap(); // perf-expect: mutex-in-loop
        *g += 1;
    }
}
