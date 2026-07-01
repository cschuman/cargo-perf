# Accuracy

A performance linter is only worth adopting if you can trust its findings. A
tool that cries wolf gets muted; a tool that misses real problems gets
uninstalled. So cargo-perf measures its own **precision** (of the things it
flags, how many are real) and **recall** (of the real problems, how many it
catches) — and enforces a floor on both in CI.

## The scorecard

Every fixture under [`tests/corpus/`](../tests/corpus) is hand-labeled ground
truth. [`tests/accuracy.rs`](../tests/accuracy.rs) runs **every** rule over each
fixture and scores the output:

- A line marked `// perf-expect: <rule-id>` must produce that diagnostic (a true
  positive). An expected-but-missing diagnostic is a **false negative**.
- Any diagnostic with no matching marker is a **false positive**.
- A fixture marked `// perf-guard: <rule-id>` is a negative case whose whole job
  is to stay silent for that rule — these directly guard against the false
  positives real linters are infamous for: `Arc::clone`/`Rc::clone` refcount
  bumps, `Copy` values cloned in loops, `io::Read`/`io::Write` and
  `AtomicUsize::load` calls in loops, custom `.output()`/`.load()` methods
  mistaken for blocking or DB calls, async guards dropped before `.await`, and
  single clones outside loops.

Markers live inline, next to the code they describe, so inserting a line in a
fixture can never desynchronize the label from the code.

**Coverage is enforced, not assumed.** A companion test
(`every_rule_has_positive_and_negative_fixtures`) fails the build unless *every*
registered rule has both a positive fixture (proving it fires) and a
`perf-guard` negative (proving it stays quiet). This is what stops the scorecard
from being vacuous — a rule with no positive case could silently regress to
never-firing and still show a perfect aggregate.

Current corpus (run `cargo test --test accuracy -- --nocapture` to reproduce):

```
cargo-perf accuracy scorecard
  fixtures: 39   |   floors: precision >= 0.90, recall >= 0.90
  ------------------------------------------------------------
  rule                        TP  FP  FN   prec   recall
  async-block-in-async         2   0   0   1.00   1.00
  clone-in-hot-loop            1   0   0   1.00   1.00
  collect-then-iterate         1   0   0   1.00   1.00
  format-in-loop               1   0   0   1.00   1.00
  hashmap-no-capacity          1   0   0   1.00   1.00
  lock-across-await            2   0   0   1.00   1.00
  mutex-in-loop                1   0   0   1.00   1.00
  n-plus-one-query             1   0   0   1.00   1.00
  regex-in-loop                1   0   0   1.00   1.00
  string-concat-loop           1   0   0   1.00   1.00
  string-no-capacity           1   0   0   1.00   1.00
  unbounded-channel            1   0   0   1.00   1.00
  unbounded-spawn              1   0   0   1.00   1.00
  vec-no-capacity              1   0   0   1.00   1.00
  ------------------------------------------------------------
  OVERALL                     16   0   0   1.00   1.00
```

The floors (`MIN_PRECISION` / `MIN_RECALL` in `tests/accuracy.rs`) are a
**ratchet**: they are committed values that CI enforces on every PR — **per
rule, not just in aggregate**, so one rule's false positives can't hide behind
the average — and are raised as the corpus grows and stabilizes. The scorecard
reports the true numbers; the floor is the safety net that fails the build on a
regression.

## Adding a case

1. Drop a focused `.rs` file in `tests/corpus/`.
2. For a **true positive**, put `// perf-expect: <rule-id>` on the exact line the
   diagnostic should point to. For a **false-positive guard**, add
   `// perf-guard: <rule-id>` (usually at the top of the file) — the file must
   stay silent for that rule.
3. Run `PERF_CORPUS_DUMP=1 cargo test --test accuracy -- --nocapture` to see the
   exact `(rule, line)` the tool emits, and align your markers.
4. A case the tool does not yet handle correctly goes under
   `tests/corpus/known_gaps/` (tracked, excluded from the scored floor) so the
   metric stays honest instead of being gamed.

## Robustness on real code

Precision/recall proves correctness on curated cases; two more layers prove the
tool survives the wild:

- [`scripts/real-crate-scan.sh`](../scripts/real-crate-scan.sh) runs cargo-perf
  against real published crates (anyhow, bitflags, serde, ...) and fails if it
  ever crashes on real-world code. A weekly
  [scheduled workflow](../.github/workflows/real-crate-scan.yml) runs the fast
  subset; run the full set locally before a release.
- [`fuzz/`](../fuzz) is a cargo-fuzz harness that hunts for parser/rule panics on
  adversarial input.

Together: **correct** on labeled cases, **quiet** on look-alikes, and **robust**
on both real and hostile input.
