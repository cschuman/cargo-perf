# Changelog

All notable changes to cargo-perf will be documented in this file.

## [0.5.3] - 2025-01-08

### Added
- **`cargo perf explain <rule>`**: New command shows detailed rule documentation
  - Why the pattern is problematic
  - Bad vs Good code examples
  - Performance impact with benchmark data
  - Suppression syntax
- **`--timing` flag**: Shows performance breakdown for analysis phases
  - File discovery, parsing, analysis, and reporting times
- **JSON Schema for cargo-perf.toml**: IDE autocompletion and validation
  - Schema URL: `https://raw.githubusercontent.com/cschuman/cargo-perf/main/cargo-perf.schema.json`
  - `cargo perf init` now creates both config and `.taplo.toml` for schema support
  - Works with VS Code (Even Better TOML), Neovim, and other Taplo-compatible editors

## [0.5.2] - 2025-01-07

### Fixed
- Release workflow Windows builds now use bash shell for consistent behavior

## [0.5.1] - 2025-01-07

### Added
- **GitHub Releases workflow**: Pre-built binaries for 6 targets on every release
  - Linux: x86_64-gnu, x86_64-musl, aarch64-gnu
  - macOS: x86_64, aarch64 (Apple Silicon)
  - Windows: x86_64-msvc
- SHA256 checksums for all release artifacts
- **LSP code actions**: Quick-fix support for rules with auto-fix capability
  - Diagnostics with fixes now show as lightbulb actions in IDE
  - Works with `collect-then-iterate` rule (more coming)

## [0.5.0] - 2025-01-06

### Added
- **Plugin system**: Create custom rules via `PluginRegistry` and `define_rule!` macro
- **LSP server**: IDE integration with real-time diagnostics (`--features lsp`)
- **N+1 query detection**: New `n-plus-one-query` rule for sqlx, diesel, and SeaORM
- **Unbounded spawn detection**: New `unbounded-spawn` rule for task spawning in loops
- **Unbounded channel detection**: New `unbounded-channel` rule for memory exhaustion risks
- Shared file discovery module with security options

### Fixed
- **`vec-no-capacity` UX**: Now reports at `Vec::new()` declaration (where fix applies) instead of `.push()` call
- **False positives**: Database rules no longer flag `Vec::first()`, `HashMap::insert()`, etc.
- **False positives**: `unbounded-spawn` validates receiver is actually an async runtime type
- **Security**: LSP server validates paths and file sizes to prevent traversal/DoS
- **Security**: Fix module uses `tempfile` crate properly for atomic writes (TOCTOU fix)
- **Clippy clean**: All clippy warnings resolved, Entry API used for HashMap operations
- Static methods for recursive helpers (no unnecessary `&self`)

### Changed
- `add_rule()` now has `try_add_rule()` non-panicking variant
- Total rules: 12 (up from 9)
- Total tests: 122 (up from 74)

### Dog-fooding
cargo-perf now passes all its own checks. Running `cargo-perf check ./src` reports zero issues.
The dogfooding process identified and fixed the `vec-no-capacity` diagnostic location issue.

## [0.4.0] - 2025-01-05

### Added
- **`--strict` mode**: Only runs high-confidence rules (`async-block-in-async`, `lock-across-await`) - recommended for CI
- **Benchmarks**: Real performance measurements for all anti-patterns (see `benchmarks/`)
- Auto-fix infrastructure for `collect-then-iterate` and `string-concat-loop` rules (internal, not exposed)

### Fixed
- **False positive fix**: `string-concat-loop` no longer flags integer arithmetic (`i + 1`, `sum += 1`)
- Improved `is_likely_string_expr` heuristic to require clear string evidence

### Changed
- **Removed `fix` subcommand** from CLI (will return when more rules support auto-fix)
- README completely rewritten with strong clippy positioning and real benchmark data
- Total tests: 74 unit + 10 integration (up from 69)

### Documentation
- Clear "Why Not Just Use Clippy?" section
- CI integration examples (GitHub Actions, SARIF)
- Suppression syntax documentation
- Benchmark methodology and results

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
