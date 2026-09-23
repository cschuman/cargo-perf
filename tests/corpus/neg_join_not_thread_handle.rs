// perf-guard: async-block-in-async
// Negative: `.join` names many non-blocking methods. Joining strings and
// paths, a handle that is returned rather than joined, a join on a binding
// later rebound to something else, and a join in a sync fn must all stay
// silent inside or outside an async fn.
use std::path::Path;
use std::thread::JoinHandle;

struct Batch;

impl Batch {
    fn join(&self) -> usize {
        0
    }
}

async fn render(parts: &[&str], base: &Path) -> (String, usize) {
    let line = parts.join(",");
    let full = base.join("out.txt");
    let batch = Batch;
    (format!("{line}{}", full.display()), batch.join())
}

async fn start() -> JoinHandle<u64> {
    std::thread::spawn(|| 7u64)
}

async fn rebound() -> usize {
    let handle = std::thread::spawn(|| ());
    drop(handle);
    let handle = Batch;
    handle.join()
}

fn wait_sync() -> u64 {
    let handle = std::thread::spawn(|| 7u64);
    handle.join().unwrap_or(0)
}
