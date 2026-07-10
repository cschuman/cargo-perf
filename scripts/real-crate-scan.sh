#!/usr/bin/env bash
#
# real-crate-scan.sh — run cargo-perf against real, published crates and assert
# it never crashes on real-world code.
#
# This complements two other guarantees:
#   * tests/accuracy.rs  — precision/recall on a *labeled* corpus (correctness).
#   * fuzz/              — parser/rule robustness on *adversarial* input.
#
# Here we check the middle ground: cargo-perf must run to completion on large,
# real codebases without panicking or aborting. Findings are expected and fine;
# a crash (panic=101, abort=134, segv=139) is a failure.
#
# Usage:
#   scripts/real-crate-scan.sh            # full local set
#   scripts/real-crate-scan.sh --fast     # small subset (used by scheduled CI)
#
set -euo pipefail

FAST=0
if [[ "${1:-}" == "--fast" ]]; then
  FAST=1
fi

# Shallow-cloned real crates. The fast subset is intentionally small and quick;
# the full set casts a wider net for local pre-release scans.
REPOS_FAST=(
  "https://github.com/dtolnay/anyhow"
  "https://github.com/bitflags/bitflags"
)
REPOS_FULL=(
  "https://github.com/dtolnay/anyhow"
  "https://github.com/bitflags/bitflags"
  "https://github.com/BurntSushi/byteorder"
  "https://github.com/dtolnay/itoa"
  "https://github.com/serde-rs/serde"
  "https://github.com/rust-lang/log"
)

if [[ $FAST -eq 1 ]]; then
  REPOS=("${REPOS_FAST[@]}")
else
  REPOS=("${REPOS_FULL[@]}")
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/target/release/cargo-perf"

if [[ ! -x "$BIN" ]]; then
  echo "Building release binary..."
  (cd "$ROOT" && cargo build --release)
fi

WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT

failures=0
scanned=0

for repo in "${REPOS[@]}"; do
  name="$(basename "$repo")"
  dest="$WORKDIR/$name"
  echo "==> cloning $name"
  if ! git clone --depth 1 --quiet "$repo" "$dest"; then
    echo "    WARN: clone failed (network?), skipping $name"
    continue
  fi

  target="$dest/src"
  [[ -d "$target" ]] || target="$dest"

  echo "==> scanning $name"
  set +e
  "$BIN" check "$target"
  code=$?
  set -e
  scanned=$((scanned + 1))

  # cargo-perf without --fail-on exits 0 even when it reports findings; any other
  # exit code means it crashed or errored on real code.
  case $code in
    0)
      echo "    ok ($name): no crash"
      ;;
    101|134|139)
      echo "    CRASH ($name): cargo-perf exited $code on real code"
      failures=$((failures + 1))
      ;;
    *)
      echo "    ERROR ($name): unexpected exit code $code"
      failures=$((failures + 1))
      ;;
  esac
done

echo
echo "real-crate scan: scanned=$scanned failures=$failures"
if [[ $failures -gt 0 ]]; then
  echo "FAILED: cargo-perf crashed or errored on real-world code."
  exit 1
fi
if [[ $scanned -eq 0 ]]; then
  echo "WARNING: no crates were scanned (all clones failed)."
  exit 0
fi
echo "PASSED: cargo-perf ran cleanly on $scanned real crates."
