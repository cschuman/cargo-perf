// Positive (D24): a `Mutex` locked inside a `for_each` closure is re-acquired
// once per element — identical contention to locking inside a `for`/`while`
// loop, even though there is no syntactic loop keyword. `for_each`/`try_for_each`
// eagerly drive the iterator, so the closure body is a loop body.
use std::sync::Mutex;

fn tally(counter: &Mutex<i32>, items: &[i32]) {
    items.iter().for_each(|x| {
        *counter.lock().unwrap() += *x; // perf-expect: mutex-in-loop
    });
}
