// Positive (D21): a synchronous `std::sync::Mutex` guard is acquired inside an
// `if` body and held live across `network_call().await` in that same body. When
// `flag` is true the guard lives until the end of the `if` block — past the await —
// so it can deadlock the async runtime. Wrapping it in `if` is semantically
// irrelevant; the analyzer must recurse into the control-flow body to see it.
use std::sync::Mutex;

async fn conditional(m: &Mutex<i32>, flag: bool) {
    if flag {
        let guard = m.lock().unwrap();
        let _v = *guard;
        network_call().await; // perf-expect: lock-across-await
    }
}

async fn network_call() {}
