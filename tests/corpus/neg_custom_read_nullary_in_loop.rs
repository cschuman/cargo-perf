// perf-guard: mutex-in-loop
// Negative: a nullary `.read()` on a custom (non-lock) type is a domain method,
// not lock contention. mutex-in-loop must only fire on real lock guards.
struct Sensor;

impl Sensor {
    fn read(&self) -> u32 {
        0
    }
}

fn poll(sensor: &Sensor) -> Vec<u32> {
    let mut out = Vec::with_capacity(10);
    for _ in 0..10 {
        out.push(sensor.read());
    }
    out
}
