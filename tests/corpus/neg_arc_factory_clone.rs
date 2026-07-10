// perf-guard: clone-in-hot-loop
// Negative (D9): `cfg` is initialized from a same-file factory function that
// returns `Arc<Config>`. It therefore holds an Arc, and `.clone()` inside the
// loop is a cheap reference-count bump — not a deep heap clone. The in-file
// oracle recognises the factory's Arc return type, so the rule must stay silent.
use std::sync::Arc;

struct Config {
    _data: Vec<u8>,
}

fn make_shared() -> Arc<Config> {
    Arc::new(Config { _data: vec![1, 2, 3] })
}

fn run() -> usize {
    let cfg = make_shared();
    let mut total = 0;
    for _ in 0..100 {
        let handle = cfg.clone();
        total += handle._data.len();
    }
    total
}
