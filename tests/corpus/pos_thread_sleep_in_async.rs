// Positive: `std::thread::sleep` parks the entire runtime worker thread instead
// of yielding the task, so every other future scheduled on that worker stalls
// with it. Inside an async fn this must fire.
async fn backoff() {
    std::thread::sleep(std::time::Duration::from_millis(50)); // perf-expect: async-block-in-async
}
