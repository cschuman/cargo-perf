// perf-guard: regex-in-loop
// Negative (D33): a custom type whose name merely *contains* "Regex" is not the
// regex crate. Constructing `RegexCacheKey::new` in a loop compiles no pattern,
// so matching "Regex" as a substring rather than an exact segment is wrong.
struct RegexCacheKey {
    id: u32,
}

impl RegexCacheKey {
    fn new(id: u32) -> Self {
        RegexCacheKey { id }
    }
}

fn build() {
    for id in 0..10 {
        let _k = RegexCacheKey::new(id);
    }
}
