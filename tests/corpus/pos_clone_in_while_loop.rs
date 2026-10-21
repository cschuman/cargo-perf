// Positive: a `String` deep-copied on every `while` iteration only to read its
// length and drop it. A borrow reads the same value, so the whole
// allocation-and-memcpy is redundant work.
fn total_len(src: &String, n: usize) -> usize {
    let mut total = 0;
    let mut i = 0;
    while i < n {
        let copy = src.clone(); // perf-expect: clone-in-hot-loop
        total += copy.len();
        i += 1;
    }
    total
}
