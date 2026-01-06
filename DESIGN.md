# cargo-perf Design Document

> **THE** preventive performance analysis tool for Rust. Catch performance anti-patterns before production.

## Vision

Static analysis tool that identifies Rust-specific performance anti-patterns at compile time - things that runtime profilers can't catch until it's too late.

## Core Differentiators

| Feature | Clippy | cargo-perf |
|---------|--------|------------|
| Async-aware analysis | No | Yes |
| N+1 query detection | No | Yes |
| ORM integration | No | Yes (Diesel, SQLx, SeaORM) |
| Data flow (lock-across-await) | No | Yes |
| Auto-fix generation | Some | Comprehensive |
| CI/CD first | Afterthought | Native |
| SARIF output | No | Yes |
| Performance-only focus | Mixed | Laser-focused |

---

## Detection Rules

### Critical / High-Impact (Unique)

| ID | Pattern | Severity |
|----|---------|----------|
| `async-block-in-async` | Blocking calls inside `async fn` | Error |
| `lock-across-await` | MutexGuard held across `.await` | Error |
| `n-plus-one-query` | Database query inside loop body | Error |
| `unbounded-channel` | Unbounded channel without backpressure | Warning |
| `clone-in-hot-loop` | `.clone()` on heap types in loops | Warning |
| `regex-in-loop` | `Regex::new()` inside loop | Warning |
| `collect-then-iterate` | `.collect()` followed by iteration | Warning |

### Medium Impact

| ID | Pattern | Severity |
|----|---------|----------|
| `vec-no-capacity` | Vec::new() + loop push (known size) | Warning |
| `format-in-loop` | `format!()` in loops | Warning |
| `arc-when-rc` | Arc in single-threaded context | Info |
| `box-small-type` | Box on small stack-allocable types | Info |
| `repeated-serialize` | serde in loop on same schema | Warning |

### Nice-to-Have

| ID | Pattern | Severity |
|----|---------|----------|
| `par-iter-small` | `.par_iter()` on tiny collections | Info |
| `ffi-string-loop` | CString conversion in loops | Warning |
| `monomorph-explosion` | Excessive generic instantiations | Info |

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         cargo-perf CLI                          │
├─────────────────────────────────────────────────────────────────┤
│  Commands: check, fix, init                                     │
│  Flags: --rules, --severity, --format, --fix                    │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                      Analysis Engine                            │
├──────────────────┬──────────────────┬───────────────────────────┤
│   Source Parser  │  Rule Engine     │  Reporter                 │
│   (syn + quote)  │                  │                           │
├──────────────────┼──────────────────┼───────────────────────────┤
│ • Parse to AST   │ • Pattern match  │ • Console (colored)       │
│ • Preserve spans │ • Data flow      │ • JSON (CI/CD)            │
│ • Type inference │ • Severity calc  │ • SARIF (GitHub)          │
│   (limited)      │ • Fix generation │                           │
└──────────────────┴──────────────────┴───────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                        Rule Modules                             │
├─────────────┬─────────────┬─────────────┬───────────────────────┤
│   async/    │  memory/    │  database/  │  iter/                │
└─────────────┴─────────────┴─────────────┴───────────────────────┘
```

## Crate Structure

```
cargo-perf/
├── Cargo.toml
├── src/
│   ├── lib.rs               # Library entry point
│   ├── main.rs              # CLI entry point
│   ├── config.rs            # Configuration (cargo-perf.toml)
│   ├── engine/
│   │   ├── mod.rs
│   │   ├── parser.rs        # syn-based parsing
│   │   ├── visitor.rs       # AST visitor utilities
│   │   └── context.rs       # Analysis context
│   ├── rules/
│   │   ├── mod.rs           # Rule trait + registry
│   │   ├── async_rules.rs   # Async-specific rules
│   │   ├── memory_rules.rs  # Allocation patterns
│   │   ├── database_rules.rs # ORM/query rules
│   │   └── iter_rules.rs    # Iterator anti-patterns
│   ├── reporter/
│   │   ├── mod.rs
│   │   ├── console.rs       # Colored terminal output
│   │   ├── json.rs          # JSON output
│   │   └── sarif.rs         # GitHub SARIF format
│   └── fix/
│       └── mod.rs           # Auto-fix generation
└── tests/
    └── fixtures/            # Test cases per rule
```

## Core Types

```rust
/// Severity levels for diagnostics
pub enum Severity {
    Error,    // Likely performance bug, fail CI
    Warning,  // Probable issue, should fix
    Info,     // Suggestion for improvement
}

/// A diagnostic reported by a rule
pub struct Diagnostic {
    pub rule_id: &'static str,
    pub severity: Severity,
    pub message: String,
    pub file_path: PathBuf,
    pub span: Span,
    pub suggestion: Option<String>,
    pub fix: Option<Fix>,
}

/// The Rule trait - implement this to add new checks
pub trait Rule: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn default_severity(&self) -> Severity;
    fn check(&self, ctx: &AnalysisContext) -> Vec<Diagnostic>;
}

/// Context passed to rules during analysis
pub struct AnalysisContext<'a> {
    pub file_path: &'a Path,
    pub source: &'a str,
    pub ast: &'a syn::File,
    pub config: &'a Config,
}
```

## Configuration

`cargo-perf.toml` in project root:

```toml
[rules]
async-block-in-async = "deny"   # error, fail CI
clone-in-hot-loop = "warn"      # warning
vec-no-capacity = "allow"       # ignore

[output]
format = "console"              # or "json", "sarif"
color = "auto"                  # or "always", "never"

[database]
orm = "sqlx"                    # or "diesel", "sea-orm"
```

## Suppression

```rust
// Suppress single occurrence
#[allow(cargo_perf::clone_in_hot_loop)]
fn my_function() { }

// Inline suppression
let x = data.clone(); // perf:ignore[clone-in-hot-loop]
```

---

## Phased Roadmap

### v0.1.0 - MVP
- [ ] Core engine with syn parsing
- [ ] 5 high-impact rules
- [ ] Console output
- [ ] Basic CLI

### v0.2.0 - CI Ready
- [ ] JSON + SARIF output
- [ ] `--fail-on` severity filtering
- [ ] Suppression comments
- [ ] Config file support

### v0.3.0 - Auto-fix
- [ ] Fix suggestions
- [ ] `cargo perf fix` command

### v0.4.0 - Ecosystem
- [ ] More rules
- [ ] IDE integration
- [ ] Plugin system for custom rules

---

## Blocking Call Detection (async-block-in-async)

Known blocking calls to detect:
- `std::fs::*` (read, write, metadata, etc.)
- `std::thread::sleep`
- `std::net::TcpStream::connect`
- `std::io::stdin().read_line()`
- `std::process::Command::output()`
- `.lock().unwrap()` on std Mutex (not tokio)

Suggest alternatives:
- `std::fs::read` → `tokio::fs::read`
- `std::thread::sleep` → `tokio::time::sleep`
- `std::net::*` → `tokio::net::*`
