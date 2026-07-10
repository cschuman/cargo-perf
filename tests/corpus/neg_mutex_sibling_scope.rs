// perf-guard: mutex-in-loop
// Negative (D25): sibling blocks both bind `g`. The first is a real Mutex, locked
// once outside a loop; the second is a plain `Grid` whose nullary `read()` is called
// in a loop. The stale lock name must clear on rebind so the domain `read()` on the
// second `g` is not mistaken for lock contention.
use std::sync::Mutex;

struct Grid;
impl Grid {
    fn read(&self) -> i32 {
        7
    }
}

fn phased() -> i32 {
    let mut out = 0;
    {
        let g = Mutex::new(0i32);
        out += *g.lock().unwrap();
    }
    {
        let g = Grid;
        for _ in 0..5 {
            out += g.read();
        }
    }
    out
}
