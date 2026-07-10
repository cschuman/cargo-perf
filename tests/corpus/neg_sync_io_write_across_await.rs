// perf-guard: lock-across-await
// Negative (D20): `buf.write(data)` on a `Vec<u8>` is `std::io::Write::write` —
// the bound `n` is a byte count, not a lock guard. `write` called WITH a buffer
// argument is I/O; only a nullary `RwLock::write()` is a guard. Holding a usize
// across `commit().await` is benign, so nothing must fire.
use std::io::Write;

async fn dump(buf: &mut Vec<u8>, data: &[u8]) {
    let n = buf.write(data).unwrap();
    commit().await;
    let _ = n;
}

async fn commit() {}
