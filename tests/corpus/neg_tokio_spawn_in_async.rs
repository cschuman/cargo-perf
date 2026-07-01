// Negative: `tokio::spawn` is the idiomatic way to start a task, not a blocking
// call. It must never be reported as `async-block-in-async`.
async fn run() {
    tokio::spawn(async {
        work().await;
    });
}

async fn work() {}
