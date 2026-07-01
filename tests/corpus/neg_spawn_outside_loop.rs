// perf-guard: unbounded-spawn
// Negative: a single tokio::spawn outside a loop is idiomatic.
async fn run() {
    tokio::spawn(process());
}

async fn process() {}
