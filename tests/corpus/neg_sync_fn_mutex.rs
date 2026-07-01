// Negative: a synchronous function holding a lock (no await anywhere) is fine.
fn sync_use(m: &std::sync::Mutex<i32>) {
    let mut g = m.lock().unwrap();
    *g += 1;
}
