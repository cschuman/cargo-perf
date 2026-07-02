// Positive (D3): a synchronous `std::fs::read_to_string` inside an async method of
// an inherent impl blocks the executor thread — the canonical blocking-in-async
// antipattern. An async method is an async function regardless of the impl-block
// context, so it must fire just as a free async fn does.
struct Worker;

impl Worker {
    async fn load(&self) -> String {
        std::fs::read_to_string("config.toml").unwrap() // perf-expect: async-block-in-async
    }
}
