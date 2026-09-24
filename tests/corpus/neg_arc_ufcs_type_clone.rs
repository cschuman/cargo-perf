// perf-guard: clone-in-hot-loop
// Negative near-miss: `Arc::clone(&x)` parses as a Call with a trailing `clone`
// segment, exactly like the UFCS `Clone::clone(&x)` the rule does flag. The
// difference is the qualifier: this one names the TYPE, so it is a refcount bump
// — and it is the idiomatic way to make a cheap handle copy explicit, which is
// precisely the code a naive `clone`-in-loop matcher punishes.
use std::sync::Arc;

fn fan_out(shared: &Arc<Vec<u8>>, n: usize) -> Vec<Arc<Vec<u8>>> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(Arc::clone(shared));
    }
    out
}
