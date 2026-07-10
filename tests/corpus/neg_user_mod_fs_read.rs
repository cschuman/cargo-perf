// perf-guard: async-block-in-async
// Negative (D6): a local `mod fs` with a free `read` fn is not std::fs::read,
// so calling `fs::read(..)` in an async fn does not block the runtime.
mod fs {
    pub fn read(_path: &str) -> u8 {
        0
    }
}

async fn run() {
    let _ = fs::read("x");
}
