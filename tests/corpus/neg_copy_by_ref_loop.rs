// perf-guard: clone-in-hot-loop
// Negative (D14): `for &c in bytes.iter()` binds `c` by value out of a shared
// reference, which only compiles when the element is Copy. So `c` is a Copy u8
// and `c.clone()` is a no-op copy, not a heap clone.
fn checksum(bytes: &[u8]) -> u32 {
    let mut acc = 0u32;
    for &c in bytes.iter() {
        acc = acc.wrapping_add(c.clone() as u32);
    }
    acc
}
