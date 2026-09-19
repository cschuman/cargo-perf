// perf-guard: async-block-in-async
// Negative near-miss: the same `std::thread::sleep` as the positive fixture, but
// inside the SYNC closure handed to `std::thread::spawn`, which moves it onto a
// dedicated OS thread. The handle is returned rather than joined here, so the
// async fn never waits on the thread and nothing on the async runtime is
// blocked. A sync closure body is not an async context even when the enclosing
// fn is async.
async fn backoff() -> std::thread::JoinHandle<()> {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(50));
    })
}
