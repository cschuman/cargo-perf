// Positive (D22): a synchronous `std::sync::Mutex` guard is acquired inside a
// `match` arm and held live across `handle().await` in that same arm. The guard
// lives to the end of the arm block — past the await — a genuine deadlock risk.
// The analyzer must recurse into match-arm bodies to track guards declared there.
use std::sync::Mutex;

async fn on_event(m: &Mutex<i32>, ev: u8) {
    match ev {
        0 => {
            let guard = m.lock().unwrap();
            let _v = *guard;
            handle().await; // perf-expect: lock-across-await
        }
        _ => {}
    }
}

async fn handle() {}
