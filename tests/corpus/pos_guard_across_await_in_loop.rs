// Positive: the synchronous guard is acquired once before the loop and stays
// held across the `.await` on every iteration, so the runtime can deadlock the
// moment another task needs the same mutex.
async fn pump(m: &std::sync::Mutex<u32>) {
    let g = m.lock().unwrap();
    for _ in 0..3 {
        flush().await; // perf-expect: lock-across-await
    }
    let _ = g;
}

async fn flush() {}
