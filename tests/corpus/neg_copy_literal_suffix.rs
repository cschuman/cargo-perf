// perf-guard: clone-in-hot-loop
// Negative (D12): `x` is a Copy u64 bound by a literal suffix (no explicit
// annotation). `x.clone()` is a no-op bitwise copy (clippy::clone_on_copy),
// not a heap allocation.
fn run() -> u64 {
    let x = 42u64;
    let mut total = 0u64;
    for _ in 0..1000 {
        total = total.wrapping_add(x.clone());
    }
    total
}
