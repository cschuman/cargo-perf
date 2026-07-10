// perf-guard: lock-across-await
// Negative (guards Fix 4): the guard is explicitly dropped before the await, so
// nothing is held across the yield point. `lock-across-await` must not fire.
async fn ok(m: &tokio::sync::Mutex<i32>) {
    let g = m.lock().await;
    *g += 1;
    drop(g);
    other().await;
}

async fn other() {}
