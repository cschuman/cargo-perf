// Positive (as a Warning): a tokio async guard held across .await serializes tasks.
// The rule still fires here; only the severity differs from the sync case.
async fn bad(m: &tokio::sync::Mutex<i32>) {
    let g = m.lock().await;
    other().await; // perf-expect: lock-across-await
    let _ = g;
}

async fn other() {}
