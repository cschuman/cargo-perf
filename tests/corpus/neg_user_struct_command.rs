// perf-guard: async-block-in-async
// Negative (D1): a locally-defined `struct Command` is not std::process::Command.
// Its `.output()` is a plain accessor with no runtime-blocking cost, even though
// the receiver chain `Command::new(..).output()` mentions the name `Command`.
struct Command {
    label: String,
}

impl Command {
    fn new(label: &str) -> Self {
        Command {
            label: label.to_string(),
        }
    }
    fn output(&self) -> usize {
        self.label.len()
    }
}

async fn run() -> usize {
    Command::new("x").output()
}
