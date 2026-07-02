// perf-guard: lock-across-await
// Negative (D19): `reader.read(&mut buf).await` is idiomatic tokio AsyncReadExt
// I/O — the bound `n` is a byte count, not a lock guard. `read` called WITH a
// buffer argument is I/O; only a nullary `RwLock::read()` is a guard. Holding a
// usize across `flush().await` is benign, so nothing must fire.
use tokio::io::AsyncReadExt;

async fn copy_stream<R: AsyncReadExt + Unpin>(reader: &mut R) {
    let mut buf = [0u8; 1024];
    let n = reader.read(&mut buf).await.unwrap();
    flush().await;
    let _ = n;
}

async fn flush() {}
