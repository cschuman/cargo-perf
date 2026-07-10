// perf-guard: n-plus-one-query
// Negative: a custom `.load()` with no database connection argument and no ORM
// import is not an N+1 query.
struct Cache;

impl Cache {
    fn load(&self, _key: u32) -> u32 {
        0
    }
}

fn warm(cache: &Cache) {
    for key in 0..5 {
        let _ = cache.load(key);
    }
}
