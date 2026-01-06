# Changelog

All notable changes to cargo-perf will be documented in this file.

## [0.3.0] - 2025-01-05

### Added
- New critical rule `lock-across-await`: Detects MutexGuard/RwLockGuard held across `.await` points (causes deadlocks)
- CI/CD pipeline (`.github/workflows/ci.yml`) with tests, clippy, fmt, MSRV check, dogfooding, and security audit
- `CONTRIBUTING.md` with guidelines for adding new rules
- Shared `VisitorState` for consistent loop tracking and recursion limits

### Security
- **TOCTOU fix**: File operations now use file descriptors to prevent race conditions
- **Recursion depth limits**: All AST visitors now bail out at depth 256 to prevent stack overflow

### Fixed
- Suppression span calculation for structs and modules (removed +10/+1000 hacks)

### Changed
- Total rules: 9 (up from 8)
- Total tests: 69 (up from 59)
- README rewritten to honestly position as async-focused complement to clippy

## [0.2.0] - 2025-01-05

### Added
- 4 new performance rules:
  - `vec-no-capacity`: Detects `Vec::new()` + push in loop without `with_capacity`
  - `format-in-loop`: Detects `format!()` allocations inside loops
  - `string-concat-loop`: Detects String `+` operator in loops
  - `mutex-in-loop`: Detects `Mutex::lock()` inside loops
- Parallel file analysis using rayon for faster analysis of large codebases
- Inline suppression support:
  - `#[allow(cargo_perf::rule_id)]` attribute-based suppression
  - `// cargo-perf-ignore: rule_id` comment-based suppression

### Changed
- Total rules: 8 (up from 4)
- Total tests: 59 (up from 45)

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
