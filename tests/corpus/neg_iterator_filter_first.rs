// perf-guard: n-plus-one-query
// Negative (D27): a pure iterator pipeline `.filter(..).collect().first()` in a
// loop. `filter` is a query-builder verb only when an ORM is imported; here it is
// `Iterator::filter` and `first` is a slice method, so nothing is a database query.
fn pick_evens(rows: &[Vec<i32>]) -> Vec<i32> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        if let Some(&v) = row.iter().filter(|n| *n % 2 == 0).collect::<Vec<_>>().first() {
            out.push(v);
        }
    }
    out
}
