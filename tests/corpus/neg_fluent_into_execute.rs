// perf-guard: n-plus-one-query
// Negative (D32): `c.into_runner().into().execute()` — a hand-rolled fluent command
// builder. `into` is the ubiquitous stdlib `Into::into`; it is a query-builder verb
// only when an ORM is imported, so this plain builder chain is not an N+1 query.
struct Cmd;
impl Cmd {
    fn into_runner(self) -> Cmd {
        self
    }
    fn execute(self) -> u32 {
        0
    }
}

fn run(items: Vec<Cmd>) -> u32 {
    let mut acc = 0;
    for c in items {
        acc += c.into_runner().into().execute();
    }
    acc
}
