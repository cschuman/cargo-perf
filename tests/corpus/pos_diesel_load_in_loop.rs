// Positive: a Diesel `.load(conn)` executed once per iteration is the N+1
// pattern; the `conn` executor in the first argument slot corroborates that this
// really is a database round trip rather than a same-named domain method.
use diesel::prelude::*;

fn load_all(conn: &mut PgConnection, ids: &[i32]) {
    for id in ids {
        let _ = posts::table.filter(posts::user_id.eq(id)).load::<Post>(conn); // perf-expect: n-plus-one-query
    }
}
