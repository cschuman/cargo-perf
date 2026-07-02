// Positive (D5): the enclosing function is synchronous, but the blocking
// `std::fs::read_to_string` sits inside an `async { .. }` block handed to
// `tokio::spawn`. That block is polled by a runtime worker, so the blocking
// call stalls the executor exactly as it would inside an async fn. The async
// block — not the fn signature — establishes the async context.
fn spawn_config_load() {
    tokio::spawn(async {
        let _ = std::fs::read_to_string("config.toml"); // perf-expect: async-block-in-async
    });
}
