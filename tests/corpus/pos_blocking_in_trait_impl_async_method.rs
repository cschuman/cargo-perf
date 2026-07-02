// Positive (D4): a synchronous `std::process::Command::..output()` inside an async
// method of a trait impl blocks the executor thread until the child exits. Trait
// and inherent impl async methods are async functions too, so this must fire.
trait Runner {
    async fn go(&self);
}

struct Shell;

impl Runner for Shell {
    async fn go(&self) {
        let _ = std::process::Command::new("ls").arg("-la").output(); // perf-expect: async-block-in-async
    }
}
