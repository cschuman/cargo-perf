// Positive: `std::process::Command::output()` really does block the async
// runtime. The receiver chain (`Command::new(..).output()`) corroborates it, so
// the true positive must survive receiver-gating.
async fn run() {
    let _ = std::process::Command::new("ls").output(); // perf-expect: async-block-in-async
}
