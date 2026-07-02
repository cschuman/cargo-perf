// perf-guard: clone-in-hot-loop
// Negative (D13): `b` is a Copy u8 loop variable produced by iterating an array
// literal of Copy literals; `b.clone()` is a trivial bitwise copy.
fn run() -> u32 {
    let mut sum = 0u32;
    for b in [10u8, 20, 30, 40] {
        sum += b.clone() as u32;
    }
    sum
}
