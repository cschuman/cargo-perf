// Positive: synchronous blocking I/O inside an async fn stalls the runtime.
async fn load() -> String {
    std::fs::read_to_string("config.toml").unwrap() // perf-expect: async-block-in-async
}
