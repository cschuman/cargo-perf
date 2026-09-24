// KNOWN GAP (recall, not scored): `lock-across-await` only analyzes FREE async
// functions. `LockAcrossAwaitVisitor` implements `visit_item_fn` but not
// `visit_impl_item_fn`, so a synchronous guard held across `.await` inside an
// async method of an `impl` block is missed entirely — even though the identical
// body in a free fn is reported as an Error (see
// `tests/corpus/pos_std_mutex_across_await.rs`).
//
// This is the same blind spot `async-block-in-async` already closed as D3/D4,
// where async methods in inherent and trait impls were systematically skipped;
// `AsyncBlockingVisitor` grew a `visit_impl_item_fn` for exactly this reason.
// Async code overwhelmingly lives in impl methods, so the miss is not marginal.
//
// Confirmed on 2026-09-11: this file reports nothing.
//
// When `LockAcrossAwaitVisitor` gains a `visit_impl_item_fn` that calls
// `analyze_block` on async methods, move this file to `tests/corpus/` and mark
// the await line `// perf-expect: lock-across-await`.
struct Svc;

impl Svc {
    async fn tick(&self, m: &std::sync::Mutex<i32>) {
        let g = m.lock().unwrap();
        other().await;
        let _ = g;
    }
}

async fn other() {}
