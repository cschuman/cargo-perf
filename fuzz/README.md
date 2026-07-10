# Fuzzing cargo-perf

cargo-perf runs against **untrusted** third-party Rust source (any `.rs` file in
a dependency tree, a downloaded crate, an editor buffer). The parser and every
rule must therefore be robust to hostile input: they may return an error, but
they must never crash the process.

This directory is a [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz)
harness that continuously searches for inputs that violate that guarantee. It is
a **separate, nightly-only crate** — excluded from the published package and
from the crate's stable CI — so it never affects a normal `cargo build`/`cargo
test`.

## Targets

| Target    | What it exercises                                                        |
|-----------|--------------------------------------------------------------------------|
| `parse`   | `parse_file` alone — the parser must never panic or abort.               |
| `analyze` | `parse_file` **plus every registered rule**, run without the production per-rule panic guard, so a rule panic surfaces as a crash. |

## Running

Requires a nightly toolchain and the CLI:

```bash
rustup toolchain install nightly
cargo install cargo-fuzz

# From the repository root:
cargo +nightly fuzz run parse      # fuzz the parser
cargo +nightly fuzz run analyze    # fuzz the full pipeline

# Bounded run (e.g. in scheduled CI):
cargo +nightly fuzz run analyze -- -max_total_time=300 -max_len=8192
```

Crashing inputs are written to `fuzz/artifacts/<target>/`. Reproduce with:

```bash
cargo +nightly fuzz run analyze fuzz/artifacts/analyze/crash-<hash>
```

When you find and fix a crash, add the minimized reproducer to the stable
robustness test in `src/engine/parser.rs`
(`test_parse_file_robust_against_adversarial_input`) so the fix is guarded on
stable CI without requiring nightly.

## Known uncatchable class

A *stack overflow* from pathologically deep nesting aborts the process and
cannot be caught by `catch_unwind`. Input size is bounded by
`discovery::MAX_FILE_SIZE` (10 MiB) in normal operation; the fuzzer can still
surface such inputs, which is useful signal for adding an explicit nesting-depth
bound later.
