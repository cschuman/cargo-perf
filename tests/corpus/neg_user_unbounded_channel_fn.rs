// perf-guard: unbounded-channel
// Negative (D35): a user-defined `fn unbounded_channel` shadows the tokio
// primitive of the same name; calling it has nothing to do with channels.
fn unbounded_channel() -> u8 {
    0
}

fn run() {
    let _q = unbounded_channel();
}
