// Negative: awaiting with no lock held is ordinary async code.
async fn fine() {
    let x = compute().await;
    consume(x);
}

async fn compute() -> i32 {
    0
}

fn consume(_: i32) {}
