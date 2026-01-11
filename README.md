# cargo-perf

**Static analysis for async correctness and runtime performance in Rust.**

[![Crates.io](https://img.shields.io/crates/v/cargo-perf.svg)](https://crates.io/crates/cargo-perf)
[![CI](https://github.com/cschuman/cargo-perf/actions/workflows/ci.yml/badge.svg)](https://github.com/cschuman/cargo-perf/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates/l/cargo-perf.svg)](LICENSE)

## The Problem

These bugs compile fine and ship to production:

```rust
// Blocks the async runtime — causes timeouts under load
async fn read_config() -> Config {
    let data = std::fs::read_to_string("config.toml").unwrap();
    toml::from_str(&data).unwrap()
}

// Deadlock — holds lock across yield point
async fn update(mutex: &tokio::sync::Mutex<Data>) {
    let guard = mutex.lock().await;
    some_async_op().await; // guard still held across await
}

// 737x slower — regex compilation in hot loop
for line in lines {
    if Regex::new(r"\d+").unwrap().is_match(line) { ... }
}
```

cargo-perf catches all of these.

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
cargo perf fix --dry-run            # Preview auto-fixes
cargo perf fix                      # Apply auto-fixes
```

## Rules

### Errors (High Confidence)

| Rule | What it catches |
|------|-----------------|
| `async-block-in-async` | `std::fs`, `thread::sleep`, blocking I/O in async functions |
| `lock-across-await` | MutexGuard/RwLockGuard held across `.await` (deadlock risk) |
| `n-plus-one-query` | Database queries inside loops (SQLx, Diesel, SeaORM) |

### Warnings (Medium Confidence)

| Rule | What it catches | Impact |
|------|-----------------|--------|
| `unbounded-channel` | `mpsc::channel()`, `unbounded_channel()` | Memory exhaustion |
| `unbounded-spawn` | `tokio::spawn` in loops | Resource exhaustion |
| `regex-in-loop` | `Regex::new()` inside loops | 737x slower |
| `clone-in-hot-loop` | `.clone()` on heap types in loops | 48x slower |
| `collect-then-iterate` | `.collect().iter()` | 2.3x slower |
| `vec-no-capacity` | `Vec::new()` + push in loop | 1.8x slower |
| `hashmap-no-capacity` | `HashMap::new()` + insert in loop | Repeated rehashing |
| `string-no-capacity` | `String::new()` + push_str in loop | Repeated realloc |
| `format-in-loop` | `format!()` inside loops | Allocates each iteration |
| `string-concat-loop` | String `+` in loops | Use `push_str()` |
| `mutex-in-loop` | Lock acquired inside loop | Acquire once outside |

## CI Integration

### GitHub Action

Use the official GitHub Action for the easiest setup:

```yaml
# .github/workflows/perf.yml
name: Performance Analysis

on: [push, pull_request]

jobs:
  cargo-perf:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      security-events: write  # For SARIF upload

    steps:
      - uses: actions/checkout@v4
      - uses: cschuman/cargo-perf@v1
        with:
          path: '.'
          fail-on-error: 'true'
          sarif: 'true'  # Enables GitHub Code Scanning integration
```

#### Action Inputs

| Input | Default | Description |
|-------|---------|-------------|
| `path` | `.` | Path to analyze |
| `fail-on-error` | `true` | Fail if errors found |
| `fail-on-warning` | `false` | Fail if warnings found |
| `sarif` | `true` | Upload results to GitHub Code Scanning |
| `version` | `latest` | cargo-perf version to install |

### Manual Setup

```yaml
# .github/workflows/ci.yml
- name: Performance lint
  run: |
    cargo install cargo-perf
    cargo perf --strict --fail-on error
```

For a complete workflow with SARIF integration for GitHub Code Scanning, see [examples/github-workflow.yml](examples/github-workflow.yml).

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

## IDE Integration

cargo-perf includes an LSP server for real-time diagnostics in your editor.

### Installation

```bash
cargo install cargo-perf --features lsp
```

### VS Code

See [editors/vscode/](editors/vscode/) for the extension.

### Neovim

```lua
require('lspconfig.configs').cargo_perf = {
  default_config = {
    cmd = { 'cargo-perf', 'lsp' },
    filetypes = { 'rust' },
    root_dir = require('lspconfig.util').root_pattern('Cargo.toml'),
  },
}
require('lspconfig').cargo_perf.setup({})
```

### Other Editors

See [editors/README.md](editors/README.md) for Emacs, Helix, Zed, and generic LSP setup.

## Custom Rules (Plugin System)

Extend cargo-perf with your own rules:

```rust
use cargo_perf::plugin::{PluginRegistry, analyze_with_plugins};
use cargo_perf::rules::{Rule, Diagnostic, Severity};

struct MyCustomRule;

impl Rule for MyCustomRule {
    fn id(&self) -> &'static str { "my-rule" }
    fn name(&self) -> &'static str { "My Rule" }
    fn description(&self) -> &'static str { "Detects my anti-pattern" }
    fn default_severity(&self) -> Severity { Severity::Warning }

    fn check(&self, ctx: &AnalysisContext) -> Vec<Diagnostic> {
        // Your detection logic
        Vec::new()
    }
}

let mut registry = PluginRegistry::new();
registry.add_rule(Box::new(MyCustomRule));
let diagnostics = analyze_with_plugins(path, &config, &registry)?;
```

See [examples/custom_rule.rs](examples/custom_rule.rs) for a complete example.

## License

MIT OR Apache-2.0
