// Positive: spawning a task per loop iteration without a concurrency limit can
// exhaust resources.
async fn run(ids: Vec<i32>) {
    for id in ids {
        tokio::spawn(process(id)); // perf-expect: unbounded-spawn
    }
}

async fn process(_id: i32) {}
