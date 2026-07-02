// perf-guard: n-plus-one-query
// Negative (D29): `v.all(&ctx)` on an in-memory validator. `ctx` merely contains
// the substring "tx"; the connection-arg check is now exact, so a context argument
// no longer corroborates a SeaORM query. `all` is a plain bool-returning method.
struct Validator;
impl Validator {
    fn all(&self, _ctx: &Ctx) -> bool {
        true
    }
}
struct Ctx;

fn validate(items: &[Validator]) -> usize {
    let ctx = Ctx;
    let mut ok = 0;
    for v in items {
        if v.all(&ctx) {
            ok += 1;
        }
    }
    ok
}
