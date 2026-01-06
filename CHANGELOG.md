# Changelog

All notable changes to cargo-perf will be documented in this file.

## [0.1.0] - 2025-01-05

### Added
- Initial release of cargo-perf
- 4 performance rules:
  - `async-block-in-async`: Detects blocking calls in async functions
  - `clone-in-hot-loop`: Detects `.clone()` inside loops
  - `regex-in-loop`: Detects `Regex::new()` inside loops
  - `collect-then-iterate`: Detects `.collect().iter()` anti-pattern
- Multiple output formats: console, JSON, SARIF
- Configuration via `cargo-perf.toml`
- GitHub Actions integration via SARIF output
- 45 unit tests

### Security
- Path traversal protection in auto-fix feature
- Symlink attack prevention in file traversal
- File size limits (10MB max) to prevent DoS

### Performance
- Zero-allocation rule registry using `LazyLock`
- O(log n) line/column lookups with `LineIndex`

### Dog-fooding
cargo-perf detected and helped fix a real performance issue in its own SARIF reporter:

```
warning: `.clone()` called inside loop [clone-in-hot-loop]
  --> src/reporter/sarif.rs:101:40
```

The loop was refactored to collect rule IDs first, then build the rules list outside the loop. This eliminated unnecessary heap allocations during SARIF report generation.

**Result**: cargo-perf now reports zero issues when run on itself.
