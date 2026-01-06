# cargo-perf

**Catch async bugs and performance anti-patterns that clippy misses.**

[![Crates.io](https://img.shields.io/crates/v/cargo-perf.svg)](https://crates.io/crates/cargo-perf)
[![CI](https://github.com/cschuman/cargo-perf/actions/workflows/ci.yml/badge.svg)](https://github.com/cschuman/cargo-perf/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates/l/cargo-perf.svg)](LICENSE)

## What clippy doesn't catch

These bugs compile fine, pass clippy, and ship to production:

```rust
// BUG: Blocks the async runtime — clippy won't catch this
async fn read_config() -> Config {
    let data = std::fs::read_to_string("config.toml").unwrap();
    toml::from_str(&data).unwrap()
}

// BUG: Deadlock — clippy won't catch this
async fn update(mutex: &tokio::sync::Mutex<Data>) {
    let guard = mutex.lock().await;
    some_async_op().await; // guard still held across await!
}

// SLOW: 737x slower — clippy's coverage is limited
for line in lines {
    if Regex::new(r"\d+").unwrap().is_match(line) { ... }
}
```

**cargo-perf catches all of these.**

## How it's different from clippy

| | clippy | cargo-perf |
|---|--------|------------|
| **Purpose** | General code quality (style, correctness, complexity) | Async correctness + loop performance |
| **Blocking calls in async** | No | Yes |
| **Lock guards across await** | No | Yes |
| **Loop allocation patterns** | Limited | Comprehensive |
| **Scope** | 700+ lints | 9 focused rules |

**Use both:** `cargo clippy && cargo perf`

## Installation

```bash
cargo install cargo-perf
```

## Usage

```bash
cargo perf                          # Analyze current directory
cargo perf --strict                 # High-confidence rules only (CI recommended)
cargo perf --strict --fail-on error # Fail CI on issues
cargo perf --format sarif           # For GitHub Code Scanning
```

## Rules

### Errors (High Confidence)

| Rule | What it catches |
|------|-----------------|
| `async-block-in-async` | `std::fs`, `thread::sleep`, blocking I/O in async functions |
| `lock-across-await` | MutexGuard/RwLockGuard held across `.await` (deadlock risk) |

### Warnings (Medium Confidence)

| Rule | What it catches | Impact |
|------|-----------------|--------|
| `regex-in-loop` | `Regex::new()` inside loops | 737x slower |
| `clone-in-hot-loop` | `.clone()` on heap types in loops | 48x slower |
| `collect-then-iterate` | `.collect().iter()` | 2.3x slower |
| `vec-no-capacity` | `Vec::new()` + push in loop | 1.8x slower |
| `format-in-loop` | `format!()` inside loops | Allocates each iteration |
| `string-concat-loop` | String `+` in loops | Use `push_str()` |
| `mutex-in-loop` | Lock acquired inside loop | Acquire once outside |

## CI Integration

```yaml
# .github/workflows/ci.yml
- name: Performance lint
  run: |
    cargo install cargo-perf
    cargo perf --strict --fail-on error
```

## Suppressing warnings

```rust
// cargo-perf-ignore: clone-in-hot-loop
let owned = data.clone(); // intentional in cold path
```

Or for a whole function:

```rust
#[allow(cargo_perf::clone_in_hot_loop)]
fn cold_path() { ... }
```

## Benchmarks

Real measurements (Apple M1 Pro, 1000 iterations):

| Anti-pattern | Impact |
|--------------|--------|
| `Regex::new()` in loop | **737x slower** |
| `clone()` in loop | **48x slower** |
| `collect().iter()` | **2.3x slower** |
| Blocking in async | Blocks runtime thread |
| Lock across await | **Deadlock** |

See [benchmarks/](benchmarks/) for methodology.

## License

MIT OR Apache-2.0
