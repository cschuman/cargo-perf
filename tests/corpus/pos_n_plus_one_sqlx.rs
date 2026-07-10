// Positive: a SQLx query executed once per loop iteration is the N+1 pattern.
// The `.fetch_one(pool)` connection argument corroborates the database call.
use sqlx::PgPool;

async fn load_all(pool: &PgPool, ids: &[i32]) {
    for id in ids {
        let _ = sqlx::query("SELECT 1").bind(id).fetch_one(pool).await; // perf-expect: n-plus-one-query
    }
}
