// perf-guard: lock-across-await
// Negative near-miss: the guard is confined to an inner block that closes before
// the `.await`, so it is already dropped at the yield point. Guards declared in
// a nested block must stay scoped to that block and must not leak into the
// statements that follow it — narrowing the critical section this way is exactly
// the fix the rule recommends.
async fn tick(m: &std::sync::Mutex<u32>) {
    {
        let g = m.lock().unwrap();
        let _ = *g;
    }
    flush().await;
}

async fn flush() {}
