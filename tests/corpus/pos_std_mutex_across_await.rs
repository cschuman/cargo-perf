// Positive: a synchronous std::sync::Mutex guard held across .await (deadlock risk).
async fn bad(m: &std::sync::Mutex<i32>) {
    let g = m.lock().unwrap();
    other().await; // perf-expect: lock-across-await
    let _ = g;
}

async fn other() {}
