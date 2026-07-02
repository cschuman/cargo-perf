// perf-guard: n-plus-one-query
// Negative (D28): `Grid::select(col)` returns an in-memory `Vec`; `.first()` on it
// is a slice method. `select` is a query-builder verb only when an ORM is imported,
// so this UI-style accessor must not be flagged as a Diesel N+1.
struct Grid;
impl Grid {
    fn select(&self, _col: usize) -> Vec<i32> {
        vec![1, 2, 3]
    }
}

fn header_values(grids: &[Grid]) -> Vec<i32> {
    let mut headers = Vec::with_capacity(grids.len());
    for g in grids {
        if let Some(&h) = g.select(0).first() {
            headers.push(h);
        }
    }
    headers
}
