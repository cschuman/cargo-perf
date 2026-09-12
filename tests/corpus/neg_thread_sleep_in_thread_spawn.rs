// perf-guard: async-block-in-async
// Negative near-miss: the same `std::thread::sleep` as the positive fixture, but
// inside the SYNC closure handed to `std::thread::spawn`, which moves it onto a
// dedicated OS thread. Nothing on the async runtime is blocked, so a sync
// closure body is not an async context even when the enclosing fn is async.
async fn backoff() {
    let handle = std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(50));
    });
    let _ = handle.join();
}
