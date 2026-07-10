// KNOWN GAP (precision, not scored) — rule: clone-in-hot-loop.
// D10 taught the clone visitor that a method-call clone of a struct field whose
// declared type is `Arc<T>`/`Rc<T>` — `self.data.clone()` — is a cheap refcount
// bump and suppressed it. The universal-function-call form of the same clone —
// `Clone::clone(&self.data)` — routes through a different expression shape
// (`Expr::Call` with a `Clone::clone` path, not `Expr::MethodCall`), which the
// field-type check does not inspect, so it is still flagged. This is a residual
// FALSE POSITIVE for a semantically identical clone.
//
// Promote to `neg_ufcs_arc_field_clone.rs` with a `// perf-guard: clone-in-hot-loop`
// marker once the UFCS `Clone::clone(&<arc-field>)` form is normalized to the same
// field-type suppression path as method-call clones.
use std::sync::Arc;
struct Holder {
    data: Arc<Vec<u8>>,
}
impl Holder {
    fn fan_out(&self) {
        for _ in 0..100 {
            let _copy = Clone::clone(&self.data); // cheap Arc bump, wrongly flagged
        }
    }
}
