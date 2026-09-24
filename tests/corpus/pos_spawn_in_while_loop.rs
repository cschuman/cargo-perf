// Positive: `tokio::spawn` inside a `while let` drain loop creates one task per
// message with no ceiling; a burst on the channel becomes a burst of tasks.
async fn drain(mut rx: tokio::sync::mpsc::Receiver<u32>) {
    while let Some(id) = rx.recv().await {
        tokio::spawn(handle(id)); // perf-expect: unbounded-spawn
    }
}

async fn handle(_id: u32) {}
