// perf-guard: n-plus-one-query
// Negative near-miss: the very same SQLx `fetch_all(pool)` chain as the positive
// fixture, hoisted OUT of the loop so it runs once and the loop merely walks the
// rows it returned. That hoist is the fix this rule recommends, so the rule must
// stay silent even though every ORM signal it keys on is present in the file.
use sqlx::PgPool;

async fn load_all(pool: &PgPool, ids: &[i32]) {
    let rows = sqlx::query("SELECT 1").bind(ids).fetch_all(pool).await;
    for _row in &rows {
        let _ = 1;
    }
}
