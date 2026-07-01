// Negative (guards Fix 3): `io::Write::write(buf)` takes an argument and is NOT a
// lock acquisition. `mutex-in-loop` must not fire on it.
use std::io::Write;

fn write_all<W: Write>(mut w: W, chunks: &[&[u8]]) {
    for chunk in chunks {
        let _ = w.write(chunk).unwrap();
    }
}
