// Negative: `.clone()` on a `Copy` loop variable (an integer from a range) is a
// no-op copy, not a heap clone. clippy::clone_on_copy already covers this; it is
// not a performance anti-pattern.
fn sum() -> i32 {
    let mut total = 0i32;
    for n in 0..10i32 {
        total += n.clone();
    }
    total
}
