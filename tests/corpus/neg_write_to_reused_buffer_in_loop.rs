// perf-guard: format-in-loop
// Negative near-miss: `write!` in a loop looks almost identical to `format!` —
// same argument syntax, same position inside the loop — but it appends into an
// existing buffer instead of allocating a new String per iteration. This is the
// fix the rule's own suggestion points at, so flagging it would punish the cure.
use std::fmt::Write;

fn render(n: usize) -> String {
    let mut buf = String::with_capacity(n * 8);
    let mut i = 0;
    while i < n {
        let _ = write!(buf, "row {}", i);
        i += 1;
    }
    buf
}
