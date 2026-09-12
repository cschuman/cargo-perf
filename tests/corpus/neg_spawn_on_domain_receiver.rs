// perf-guard: unbounded-spawn
// Negative near-miss: `.spawn()` in a loop, but on an ordinary domain builder
// rather than an async runtime handle. The receiver check only trusts known
// async spawn types (`JoinSet`, `LocalSet`, `TaskPool`) and async-sounding field
// names; a bare `.spawn()` on anything else is a method-name collision, not
// unbounded task creation.
struct Builder;

impl Builder {
    fn default() -> Self {
        Builder
    }
    fn spawn(&mut self) {}
}

fn launch(cmds: &[String]) {
    for _c in cmds {
        let mut process = Builder::default();
        process.spawn();
    }
}
