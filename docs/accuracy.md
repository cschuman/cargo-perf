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
  fixtures: 80   |   floors: precision >= 1.00, recall >= 1.00
  ------------------------------------------------------------
  rule                        TP  FP  FN   prec   recall
  async-block-in-async         7   0   0   1.00   1.00
  clone-in-hot-loop            4   0   0   1.00   1.00
  collect-then-iterate         1   0   0   1.00   1.00
  format-in-loop               1   0   0   1.00   1.00
  hashmap-no-capacity          1   0   0   1.00   1.00
  lock-across-await            4   0   0   1.00   1.00
  mutex-in-loop                3   0   0   1.00   1.00
  n-plus-one-query             1   0   0   1.00   1.00
  regex-in-loop                1   0   0   1.00   1.00
  string-concat-loop           1   0   0   1.00   1.00
  string-no-capacity           1   0   0   1.00   1.00
  unbounded-channel            1   0   0   1.00   1.00
  unbounded-spawn              1   0   0   1.00   1.00
  vec-no-capacity              1   0   0   1.00   1.00
  ------------------------------------------------------------
  OVERALL                     28   0   0   1.00   1.00
```

The floors (`MIN_PRECISION` / `MIN_RECALL` in `tests/accuracy.rs`) are a one-way
**ratchet**: committed values CI enforces on every PR — **per rule, not just in
aggregate**, so one rule's false positives can't hide behind the average. After
the adversarial hunt below and its remediation, the corpus scores a clean
1.00 / 1.00, and the floor is now set to match: on this corpus the tool must be
*perfect*, and a reintroduced false positive or a dropped true positive fails
the build. The bar only moves up — lowering it is a reviewable regression, and
the honest release valve for a case we can't yet get right is `known_gaps/`
(below), never a fractional floor.

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
   metric stays honest instead of being gamed. Name it `gap_<what>.rs` and open
   with a header comment stating whether it's a **precision** (false positive) or
   **recall** (false negative) gap, which rule it concerns, the *syntactic*
   reason the current analysis misses it, and the concrete condition under which
   it should be promoted to a scored fixture. See the existing entries for the
   shape. This directory is the difference between "we're honestly at 1.00 on
   what we claim to cover" and "we buried the misses in the average."

## The adversarial hunt

The corpus did not appear fully formed — it was *attacked* into existence. A
repeatable adversarial FP/FN hunt drives the tool against a large, deliberately
tricky set of look-alikes and near-misses (user types named `Command`/`fs`;
builder chains that resemble ORM calls; `Arc`/`Rc` clones sourced from fields,
factories, and aliases; guards dropped before `.await`; blocking work correctly
offloaded to `spawn_blocking`), and every claimed defect is **independently
reproduced against the built binary** before it counts. The most recent pass
confirmed 38 defects (27 false positives, 11 false negatives) across all rule
families and drove a 10-batch, test-first remediation.

The hunt is a *discovery* process; the scorecard above is its *permanent
record*. The loop is deliberate and closed:

1. **Hunt** — generate adversarial inputs and run them through the release
   binary, looking for a diagnostic that shouldn't fire (FP) or a missing one
   that should (FN).
2. **Reproduce** — confirm the defect independently, reduced to the smallest
   fixture that still exhibits it.
3. **Pin it** — that fixture lands in `tests/corpus/` (with a `perf-expect` /
   `perf-guard` marker) if the fix is in reach, or in `known_gaps/` with a
   promotion note if it is a genuine current limitation.
4. **Fix, test-first** — the fixture is RED, then a focused change makes it
   GREEN, then the whole scorecard must still hold at the floor.

Because step 3 makes every confirmed defect a permanent fixture, a bug found
once can never silently return: the same input that first exposed it now runs on
every `cargo test`, and the ratcheted floor turns any recurrence into a build
failure. That is what "repeatable harness" means here — not a script you re-run
and eyeball, but a discovery process whose every finding is welded into CI.

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
