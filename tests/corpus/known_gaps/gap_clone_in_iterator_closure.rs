// KNOWN GAP (recall, not scored): loop-scoped rules do not yet enter
// iterator-adapter closures. `.map(|x| x.clone())` and `.for_each(..)` are the
// dominant modern-Rust looping idiom, but the visitor's loop-depth is only
// raised by `for`/`while`/`loop`, so the clone below is currently missed.
//
// When visitor.rs learns to treat recognized iterator-adapter closures
// (map/for_each/filter_map/flat_map/fold) as loop context, promote this to a
// positive fixture with a `// perf-expect: clone-in-hot-loop` marker.
fn process(items: &[String]) -> Vec<String> {
    items.iter().map(|s| s.clone()).collect()
}
