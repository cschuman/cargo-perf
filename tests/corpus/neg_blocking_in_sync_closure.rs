// perf-guard: async-block-in-async
// Negative (D5): although this is an async fn, the blocking read runs inside a
// SYNC closure passed to `spawn_blocking`, which exists precisely to run
// blocking work off the async worker threads. Blocking here is correct, so the
// rule must not treat the sync closure body as an async context and must stay
// silent. (Reducing the false positives that plague naive async-blocking lints.)
async fn load_config() -> String {
    let handle = tokio::task::spawn_blocking(|| {
        std::fs::read_to_string("config.toml").unwrap_or_default()
    });
    handle.await.unwrap_or_default()
}
