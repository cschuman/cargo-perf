// Negative (guards Fix 3): `io::Read::read(&mut buf)` takes a buffer argument and
// is NOT a lock acquisition. `mutex-in-loop` must not fire on it.
use std::io::Read;

fn read_all<R: Read>(mut r: R) {
    let mut buf = [0u8; 1024];
    loop {
        let n = r.read(&mut buf).unwrap();
        if n == 0 {
            break;
        }
    }
}
