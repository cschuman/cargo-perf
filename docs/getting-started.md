# Getting Started with cargo-perf

This guide takes you from zero to cargo-perf enforced in CI, including the
**adoption path for an existing codebase** that already has findings you don't
want to fix all at once.

## 1. Install

```bash
# Prebuilt binary (fastest):
cargo binstall cargo-perf

# Or from source:
cargo install cargo-perf
```

The binary installs as `cargo-perf`, so every command below is a cargo
subcommand (`cargo perf ...`).

## 2. First run

```bash
cargo perf                 # analyze the current directory
cargo perf rules           # list every rule and its default severity
cargo perf explain lock-across-await   # deep-dive a single rule
```

`cargo perf` prints findings grouped by severity (Error / Warning / Info). On a
real project you will usually see some findings immediately — that is expected,
and step 4 shows how to adopt without fixing them all first.

## 3. Configure (optional)

```bash
cargo perf init            # writes cargo-perf.toml
```

Tune severities per rule. `deny` fails CI (Error), `warn` reports (Warning),
`allow` disables:

```toml
[rules]
async-block-in-async = "deny"
lock-across-await     = "deny"
clone-in-hot-loop     = "warn"
vec-no-capacity       = "allow"
```

You can also suppress a single line or a whole function in source:

```rust
// cargo-perf-ignore: clone-in-hot-loop
let owned = data.clone(); // intentional in a cold path
```

## 4. Adopt on an existing codebase (the baseline workflow)

The mistake most teams make is turning a linter on in `--fail-on error` mode and
drowning in pre-existing findings. cargo-perf supports a **baseline** so you can
enforce *from today forward* while paying down the backlog on your own schedule.

```bash
# Record every current finding as the accepted baseline:
cargo perf baseline

# Now analysis only reports issues that are NOT in the baseline:
cargo perf check --baseline
```

`cargo perf baseline` writes a `.cargo-perf-baseline` file capturing today's
findings by fingerprint. **Commit that file** so CI can read it. From then on,
`cargo perf check --baseline` filters those out, so **CI fails only on newly
introduced problems**. As you fix backlog items, refresh the record:

```bash
cargo perf baseline --update
```

This is the ratchet: new code is held to the full standard, existing debt is
visible but non-blocking, and the baseline shrinks over time.

## 5. Enforce in CI

Minimal gate (fail on new errors only):

```yaml
# .github/workflows/perf.yml
name: Performance Analysis
on: [push, pull_request]
jobs:
  cargo-perf:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo binstall -y cargo-perf
      - run: cargo perf check --baseline --fail-on error
```

For GitHub Code Scanning integration, emit SARIF instead:

```bash
cargo perf --format sarif > cargo-perf.sarif
```

See the [README CI section](../README.md#ci-integration) for the official
GitHub Action and SARIF upload wiring.

## 6. Auto-fix what can be fixed

```bash
cargo perf fix --dry-run   # preview changes
cargo perf fix             # apply them
```

## Where to go next

- `cargo perf rules` — the full rule catalog.
- [README](../README.md) — rule reference, IDE/LSP setup, custom-rule plugins.
- [DESIGN.md](../DESIGN.md) — how cargo-perf relates to (and complements) clippy.
