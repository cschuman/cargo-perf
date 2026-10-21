// Positive: a VecDeque yields exactly what was collected, in order, so
// collecting into one only to `.into_iter()` it again is a throwaway buffer.
use std::collections::VecDeque;

fn shifted(items: &[i32]) -> Vec<i32> {
    items.iter().map(|x| x + 1).collect::<VecDeque<_>>().into_iter().filter(|x| *x > 0).collect() // perf-expect: collect-then-iterate
}
