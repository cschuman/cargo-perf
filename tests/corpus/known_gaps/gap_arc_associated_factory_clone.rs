// KNOWN GAP (precision, not scored) — rule: clone-in-hot-loop.
// D9 taught the ImportOracle to record the return types of FREE functions, so a
// local bound from a factory that returns `Arc<T>`/`Rc<T>` is recognized as a
// cheap-to-clone handle and its in-loop `.clone()` is suppressed. Associated /
// inherent-method factories — `Registry::shared()`, `Foo::instance()` — are not
// recorded (the oracle keys on bare fn names, not `Type::method` paths), so the
// handle below reads as an unknown type and its refcount-bump clone is still
// flagged as an allocation. This is a residual FALSE POSITIVE.
//
// Promote to `neg_arc_associated_factory_clone.rs` with a
// `// perf-guard: clone-in-hot-loop` marker once the oracle records inherent-impl
// method return types alongside free-fn returns.
use std::sync::Arc;
struct Registry;
impl Registry {
    fn shared() -> Arc<Registry> {
        Arc::new(Registry)
    }
}
fn refresh() {
    let reg = Registry::shared();
    for _ in 0..100 {
        let _held = reg.clone(); // cheap Arc refcount bump, wrongly flagged
    }
}
