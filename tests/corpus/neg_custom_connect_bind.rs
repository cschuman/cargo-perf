// Negative: `.connect()` / `.bind()` on custom types (a pool, a query builder)
// are not `std::net` blocking calls.
async fn setup() {
    let pool = Pool;
    let _conn = pool.connect().await;

    let q = Query.bind(5);
    let _ = q;
}

struct Pool;
impl Pool {
    async fn connect(&self) -> u32 {
        0
    }
}

struct Query;
impl Query {
    fn bind(self, _v: i32) -> Self {
        self
    }
}
