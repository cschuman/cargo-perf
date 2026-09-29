// Positive: `try_lock` inside a `while` loop is still per-iteration lock
// traffic; the contended atomic is paid on every pass even when it fails.
use std::sync::Mutex;

fn drain(m: &Mutex<Vec<u32>>, n: usize) {
    let mut i = 0;
    while i < n {
        if let Ok(mut g) = m.try_lock() { // perf-expect: mutex-in-loop
            g.pop();
        }
        i += 1;
    }
}
