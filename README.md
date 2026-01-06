# cargo-perf

Preventive performance analysis for Rust. Catch performance anti-patterns before they reach production.

## Installation

```bash
cargo install --path .
```

## Usage

```bash
# Analyze current directory
cargo perf

# Analyze specific path
cargo perf check src/

# Output as JSON
cargo perf --format json

# Output as SARIF (for GitHub integration)
cargo perf --format sarif

# List available rules
cargo perf rules

# Initialize config file
cargo perf init
```

## Rules

| Rule | Severity | Description |
|------|----------|-------------|
| `async-block-in-async` | Error | Detects blocking std calls inside async functions |
| `clone-in-hot-loop` | Warning | Detects `.clone()` calls on heap types inside loops |
| `regex-in-loop` | Warning | Detects `Regex::new()` inside loops |
| `collect-then-iterate` | Warning | Detects `.collect()` immediately followed by `.iter()` |
| `vec-no-capacity` | Warning | Detects `Vec::new()` + push in loop without `with_capacity` |
| `format-in-loop` | Warning | Detects `format!()` allocations inside loops |
| `string-concat-loop` | Warning | Detects String `+` operator in loops |
| `mutex-in-loop` | Warning | Detects `Mutex::lock()` inside loops |

## Suppressing Warnings

### Attribute-based suppression

```rust
#[allow(cargo_perf::clone_in_hot_loop)]
fn my_function() {
    // clone warnings suppressed in this function
}
```

### Comment-based suppression

```rust
// cargo-perf-ignore: clone-in-hot-loop
let x = data.clone(); // this line is suppressed

// cargo-perf-ignore
let y = data.clone(); // all rules suppressed on this line
```

## Configuration

Create `cargo-perf.toml` in your project root:

```toml
[rules]
# Set rule severity: "deny" (error), "warn" (warning), "allow" (ignore)
async-block-in-async = "deny"
clone-in-hot-loop = "warn"
regex-in-loop = "allow"

[output]
format = "console"  # "console", "json", "sarif"
color = "auto"      # "auto", "always", "never"
```

## Dog-fooding Results

cargo-perf is tested on itself. During development, it detected a real issue in its own codebase:

```
$ cargo-perf check src/
warning: `.clone()` called inside loop; consider borrowing or moving the clone outside [clone-in-hot-loop]
  --> src/reporter/sarif.rs:101:40
  help: Use a reference or move the clone outside the loop

Found 1 warning(s)
```

The warning identified a `.clone()` call inside a loop in the SARIF reporter. This was refactored to collect data outside the loop, eliminating the unnecessary allocations. After the fix:

```
$ cargo-perf check src/
No performance issues found.
```

This demonstrates that cargo-perf catches real performance issues, even in well-reviewed code.

## CI Integration

### GitHub Actions with SARIF

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

## License

MIT OR Apache-2.0
