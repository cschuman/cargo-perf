// KNOWN GAP (recall, not scored) — rule: async-block-in-async.
// D5 stopped the async-blocking visitor from descending into SYNC closures as
// async context, because the dominant shape is offload — `spawn_blocking(|| ..)`,
// `thread::spawn(|| ..)`, `rayon` — where blocking inside the sync closure is
// correct. The accepted cost of that fix: a sync closure that is invoked INLINE
// (bound to a local and called in place, still on the async worker) also stops
// being treated as async context, so the blocking `thread::sleep` below is
// currently missed. This is a residual FALSE NEGATIVE.
//
// Promote to `pos_inline_blocking_sync_closure.rs` with a
// `// perf-expect: async-block-in-async` marker once the visitor can distinguish
// a closure that is offloaded (spawn_blocking/thread::spawn/rayon) from one that
// is invoked inline within the async body.
async fn handle() {
    let blocking = || {
        std::thread::sleep(std::time::Duration::from_secs(1));
    };
    blocking(); // invoked inline on the async worker — should fire, currently silent
}
