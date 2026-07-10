// perf-guard: clone-in-hot-loop
// Negative (D10): `self.state` is an `Arc<Inner>` struct field. Cloning it inside
// the loop is a cheap reference-count bump, not a deep heap clone. The receiver is
// a field access rather than a bare local, so binding-based tracking misses it —
// the in-file oracle recognises the field's Arc type and keeps the rule silent.
use std::sync::Arc;

struct Inner {
    _payload: Vec<u8>,
}

struct Service {
    state: Arc<Inner>,
}

impl Service {
    fn run(&self) -> usize {
        let mut total = 0;
        for _ in 0..100 {
            let handle = self.state.clone();
            total += handle._payload.len();
        }
        total
    }
}
