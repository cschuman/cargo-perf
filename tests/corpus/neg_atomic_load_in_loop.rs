// perf-guard: n-plus-one-query
// Negative: `AtomicUsize::load` in a loop is an atomic read, not an N+1 query.
// `load` is a ubiquitous method name; flagging it unconditionally is wrong.
use std::sync::atomic::{AtomicUsize, Ordering};

fn total(counter: &AtomicUsize) -> usize {
    let mut sum = 0;
    for _ in 0..10 {
        sum += counter.load(Ordering::Relaxed);
    }
    sum
}
