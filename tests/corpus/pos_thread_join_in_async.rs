// Positive: `std::thread::JoinHandle::join()` parks the calling thread until
// the spawned thread finishes. Inside an async fn that is a runtime worker, so
// every other task on it stalls. `std::thread::sleep` blocks the same way.
async fn wait_for_worker() -> u64 {
    let handle = std::thread::spawn(|| (1..=1_000u64).sum::<u64>());
    handle.join().unwrap_or(0) // perf-expect: async-block-in-async
}

async fn wait_inline() -> u64 {
    std::thread::spawn(|| 42u64).join().unwrap_or(0) // perf-expect: async-block-in-async
}

async fn pause() {
    std::thread::sleep(std::time::Duration::from_millis(10)); // perf-expect: async-block-in-async
}
