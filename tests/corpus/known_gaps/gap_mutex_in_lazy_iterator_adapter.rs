// KNOWN GAP (recall, not scored) — rule: mutex-in-loop.
// D24 taught the lock visitor to treat `for_each`/`try_for_each` closures as
// loop context (their bodies run once per element, so a `.lock()` inside is
// per-iteration contention). The LAZY adapters — `map`/`filter`/`filter_map`/
// `fold` — are not yet wrapped, because they don't drive iteration on their own
// (a bare `.map(..)` without a consuming terminal is inert). Here the `.map`
// closure is consumed by `.collect()`, so the lock IS taken per element, but the
// visitor's loop-depth is only raised for `for`/`while`/`loop`/`for_each`, so the
// contention below is currently missed.
//
// Promote to `pos_mutex_in_lazy_adapter.rs` with a `// perf-expect: mutex-in-loop`
// marker once the visitor tracks a consuming terminal (collect/sum/count/…) and
// raises loop context for the lazy adapter closures feeding it.
use std::sync::Mutex;
fn accumulate(items: &[i32], counter: &Mutex<i32>) -> Vec<i32> {
    items
        .iter()
        .map(|&x| {
            let mut g = counter.lock().unwrap();
            *g += x;
            *g
        })
        .collect()
}
