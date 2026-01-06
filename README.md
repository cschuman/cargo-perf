# cargo-perf

Static analysis for async Rust performance anti-patterns. Complements `clippy` with checks specific to async code and common loop-related performance issues.

## What This Tool Does

cargo-perf catches performance issues that clippy doesn't:
- **Async-specific**: Blocking calls in async functions, lock guards held across await points
- **Loop patterns**: Allocations inside loops (clone, format!, regex compilation)
- **Common mistakes**: collect-then-iterate, missing Vec capacity hints

This is **not** a replacement for clippy - use both together.

## Installation

```bash
cargo install --path .
```

## Quick Start

```bash
# Analyze current directory
cargo perf

# Analyze specific path
cargo perf check src/

# JSON output for CI
cargo perf --format json

# SARIF output for GitHub code scanning
cargo perf --format sarif
```

## Rules (9 total)

### Async Rules (Errors)

| Rule | Description |
|------|-------------|
| `async-block-in-async` | Blocking std calls (fs, thread::sleep) in async functions |
| `lock-across-await` | MutexGuard/RwLockGuard held across `.await` - causes deadlocks |

### Loop Rules (Warnings)

| Rule | Description |
|------|-------------|
| `clone-in-hot-loop` | `.clone()` on heap types inside loops |
| `regex-in-loop` | `Regex::new()` inside loops - compile once outside |
| `format-in-loop` | `format!()` inside loops - allocates each iteration |
| `string-concat-loop` | String `+` operator in loops - use `push_str()` |
| `vec-no-capacity` | `Vec::new()` + push in loop - use `with_capacity()` |
| `mutex-in-loop` | `Mutex::lock()` inside loops - acquire once outside |

### Iterator Rules (Warnings)

| Rule | Description |
|------|-------------|
| `collect-then-iterate` | `.collect().iter()` - remove intermediate allocation |

## Suppressing Warnings

### Attribute-based

```rust
#[allow(cargo_perf::clone_in_hot_loop)]
fn my_function() {
    // clone warnings suppressed here
}
```

### Comment-based

```rust
// cargo-perf-ignore: clone-in-hot-loop
let x = data.clone(); // this line suppressed

// cargo-perf-ignore
let y = data.clone(); // all rules suppressed
```

## Configuration

Create `cargo-perf.toml`:

```toml
[rules]
async-block-in-async = "deny"   # error
clone-in-hot-loop = "warn"      # warning
regex-in-loop = "allow"         # ignore

[output]
format = "console"  # console, json, sarif
```

## CI Integration

### GitHub Actions

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

### Fail on Warnings

```bash
cargo perf --fail-on warning
```

## Comparison with Clippy

| Category | clippy | cargo-perf |
|----------|--------|------------|
| General perf lints | Many (~50+) | Few (9) |
| Async-specific | Limited | Focus area |
| Lock-across-await | No | Yes |
| Loop allocations | Some | Comprehensive |

**Use both**: `cargo clippy && cargo perf`

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on adding rules.

## License

MIT OR Apache-2.0
