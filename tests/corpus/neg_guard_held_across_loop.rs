// perf-guard: mutex-in-loop
// Negative near-miss: the lock is acquired ONCE before the loop and only the
// guard is used inside it — the exact remedy this rule suggests. Only the
// acquisition site is contention, so uses of an already-held guard within the
// loop body must stay silent.
use std::sync::Mutex;

fn bump(m: &Mutex<Vec<u32>>, items: &[u32]) {
    let mut g = m.lock().unwrap();
    for x in items {
        g.push(*x);
    }
}
