// perf-guard: collect-then-iterate
// Negative (D18): `collect()` here is a domain method on a QueryBuilder that
// returns a ResultSet with its own `.iter()` — not `Iterator::collect`. With no
// turbofish and no upstream iterator adapter in the chain, there is no
// intermediate collection to fuse away, so the rule must stay silent.
struct QueryBuilder;
struct ResultSet {
    rows: Vec<i32>,
}
impl QueryBuilder {
    fn collect(&self) -> ResultSet {
        ResultSet { rows: vec![1, 2, 3] }
    }
}
impl ResultSet {
    fn iter(&self) -> std::slice::Iter<'_, i32> {
        self.rows.iter()
    }
}
fn run(q: &QueryBuilder) -> i32 {
    q.collect().iter().copied().sum()
}
