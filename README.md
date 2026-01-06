# cargo-perf

**Catch async and allocation anti-patterns that clippy misses.**

cargo-perf is a specialized static analysis tool for Rust that focuses on two areas clippy doesn't cover well:

1. **Async correctness** — blocking calls in async functions, lock guards held across `.await`
2. **Loop allocations** — repeated allocations inside hot loops that kill performance

## Why Not Just Use Clippy?

Clippy is excellent for general code quality. cargo-perf is laser-focused on **performance anti-patterns** in async code and loops.

| What you're looking for | Use |
|------------------------|-----|
| General lints, style, correctness | `cargo clippy` |
| Blocking calls in async functions | `cargo perf` |
| Lock guards held across await points | `cargo perf` |
| Allocations inside loops | `cargo perf` |

**Use both together:** `cargo clippy && cargo perf`

## The Problem

These bugs ship to production constantly because they compile fine and often work:

```rust
// BUG: Blocks the async runtime - clippy won't catch this
async fn read_config() -> Config {
    let data = std::fs::read_to_string("config.toml").unwrap(); // WRONG
    toml::from_str(&data).unwrap()
}

// BUG: Deadlock waiting to happen - clippy won't catch this
async fn update_data(mutex: &tokio::sync::Mutex<Data>) {
    let guard = mutex.lock().await;
    some_async_operation().await; // DEADLOCK: guard still held!
}

// SLOW: Compiles regex 1000 times - clippy has limited coverage
for line in lines {
    if Regex::new(r"\d+").unwrap().is_match(line) { ... }
}
```

cargo-perf catches all of these.

## Installation

```bash
cargo install cargo-perf
```

## Quick Start

```bash
# Run analysis
cargo perf

# Strict mode (high-confidence rules only, recommended for CI)
cargo perf --strict

# Fail CI on errors
cargo perf --strict --fail-on error

# JSON/SARIF output for tooling
cargo perf --format json
cargo perf --format sarif
```

## Rules

### Async Rules (Errors) — High Confidence

These indicate real bugs that will cause issues in production:

| Rule | Description |
|------|-------------|
| `async-block-in-async` | Blocking std calls (fs, thread::sleep, stdin) in async functions |
| `lock-across-await` | MutexGuard/RwLockGuard held across `.await` — causes deadlocks |

### Loop Rules (Warnings) — Medium Confidence

These indicate performance issues. May have false positives in cold paths:

| Rule | Description |
|------|-------------|
| `clone-in-hot-loop` | `.clone()` on heap types inside loops |
| `regex-in-loop` | `Regex::new()` inside loops — compile once outside |
| `format-in-loop` | `format!()` inside loops — allocates each iteration |
| `string-concat-loop` | String `+` operator in loops — use `push_str()` |
| `vec-no-capacity` | `Vec::new()` + push in loop — use `with_capacity()` |
| `mutex-in-loop` | `Mutex::lock()` inside loops — acquire once outside |

### Iterator Rules (Warnings)

| Rule | Description |
|------|-------------|
| `collect-then-iterate` | `.collect().iter()` — remove intermediate allocation |

## CI Integration

### GitHub Actions (Recommended)

```yaml
- name: Performance lint
  run: |
    cargo install cargo-perf
    cargo perf --strict --fail-on error
```

### With SARIF (for GitHub Code Scanning)

```yaml
- name: Run cargo-perf
  run: |
    cargo install cargo-perf
    cargo perf --format sarif > results.sarif

- name: Upload SARIF
  uses: github/codeql-action/upload-sarif@v2
  with:
    sarif_file: results.sarif
```

## Suppressing Warnings

When a warning is intentional (e.g., clone in a cold path), suppress it:

### Comment-based (single line)

```rust
// cargo-perf-ignore: clone-in-hot-loop
let owned = data.clone(); // suppressed
```

### Attribute-based (function/module scope)

```rust
#[allow(cargo_perf::clone_in_hot_loop)]
fn cold_path() {
    // all clone-in-hot-loop warnings suppressed here
}
```

## Configuration

Create `cargo-perf.toml` to customize severity:

```toml
[rules]
async-block-in-async = "deny"   # error (default)
clone-in-hot-loop = "warn"      # warning (default)
regex-in-loop = "allow"         # disabled
```

## Benchmarks

The anti-patterns cargo-perf detects have measurable costs (Apple M1 Pro, 1000 iterations):

| Anti-pattern | Impact |
|--------------|--------|
| `Regex::new()` in loop | **737x slower** (28ms vs 38µs) |
| `clone()` in loop | **48x slower** (19µs vs 0.4µs) |
| `collect().iter()` | **2.3x slower** (77ns vs 33ns) |
| `Vec::new()` without capacity | **1.8x slower** (758ns vs 430ns) |
| Blocking in async | Blocks entire runtime thread |
| Lock across await | **Deadlock** (infinite wait) |

See [benchmarks/](benchmarks/) for reproducible measurements.

## Comparison: cargo-perf vs clippy

| | clippy | cargo-perf |
|---|--------|------------|
| **Focus** | General code quality | Async + allocation performance |
| **Rules** | ~700 | 9 (focused) |
| **Blocking in async** | Limited | Comprehensive |
| **Lock across await** | No | Yes |
| **Loop allocations** | Partial | Comprehensive |
| **False positive rate** | Low | Low (especially `--strict`) |
| **Use case** | Every project | Async projects, hot paths |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on adding rules.

## License

MIT OR Apache-2.0
