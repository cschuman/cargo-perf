# Rule test corpus strengthening — 2026-09-11

Branch: `test-corpus-2026-09-11`. Test fixtures only; no analyser rule logic was
changed.

## What this adds

The corpus already scored a perfect 1.00 / 1.00, but the harness's own doc
comment names the weakness that score was hiding:

> every rule currently rests on a thin corpus (often a single TP), so recall is
> effectively quantized to {0.00, 1.00} … the fix for thin-rule brittleness is to
> GROW the corpus, never to lower the bar.

Ten of the fourteen rules had exactly one positive fixture. A single TP means a
rule can only be fully working or fully broken as far as the scorecard can see —
any partial regression that still fires on the one blessed shape passes CI.

This change adds **28 scored fixtures — one true positive and one near-miss
negative for every one of the 14 rules** — plus **3 unscored `known_gaps/`
entries** documenting real defects the new fixtures exposed.

Every rule now has at least two positives and at least two guards.

### Scored fixtures added (28)

| Rule | New positive | New near-miss negative |
|---|---|---|
| async-block-in-async | `pos_thread_sleep_in_async.rs` | `neg_thread_sleep_in_thread_spawn.rs` |
| unbounded-channel | `pos_crossbeam_unbounded.rs` | `neg_flume_bounded.rs` |
| unbounded-spawn | `pos_spawn_in_while_loop.rs` | `neg_spawn_on_domain_receiver.rs` |
| lock-across-await | `pos_guard_across_await_in_loop.rs` | `neg_guard_scoped_block_before_await.rs` |
| n-plus-one-query | `pos_diesel_load_in_loop.rs` | `neg_query_outside_loop.rs` |
| collect-then-iterate | `pos_collect_into_iter.rs` | `neg_collect_then_sort.rs` |
| regex-in-loop | `pos_regex_in_loop_body.rs` | `neg_regexset_in_loop.rs` |
| clone-in-hot-loop | `pos_clone_in_while_loop.rs` | `neg_arc_ufcs_type_clone.rs` |
| mutex-in-loop | `pos_try_lock_in_while_loop.rs` | `neg_guard_held_across_loop.rs` |
| format-in-loop | `pos_format_in_while_loop.rs` | `neg_write_to_reused_buffer_in_loop.rs` |
| string-concat-loop | `pos_string_plus_in_while_loop.rs` | `neg_numeric_accumulate_in_loop.rs` |
| vec-no-capacity | `pos_vec_push_in_while_loop.rs` | `neg_vec_push_outside_loop.rs` |
| hashmap-no-capacity | `pos_hashmap_insert_in_while_loop.rs` | `neg_hashmap_get_in_loop.rs` |
| string-no-capacity | `pos_string_push_in_while_loop.rs` | `neg_string_push_str_outside_loop.rs` |

The negatives are the valuable half, and each was chosen to sit as close to its
positive as possible rather than being trivially different — a near-miss only
tests something if a plausible implementation would get it wrong:

* `neg_flume_bounded.rs` — same crate, same module, one constructor name apart
  from the flagged `flume::unbounded()`.
* `neg_write_to_reused_buffer_in_loop.rs` — `write!` vs `format!` in the same
  loop position with the same argument syntax; flagging it would punish the
  exact fix the rule's suggestion text recommends.
* `neg_arc_ufcs_type_clone.rs` — `Arc::clone(&x)` parses as a Call with a
  trailing `clone` segment, structurally identical to the UFCS `Clone::clone(&x)`
  the rule does flag; only the qualifier distinguishes them.
* `neg_query_outside_loop.rs` — every ORM signal the N+1 rule keys on is present
  in the file; only the hoist out of the loop differs.
* `neg_guard_held_across_loop.rs` / `neg_guard_scoped_block_before_await.rs` —
  both are the remediated form of their own rule's positive.
* `neg_thread_sleep_in_thread_spawn.rs` — the same blocking call as the positive,
  moved into a sync closure handed to an offloader.

## Verification

Run on branch `test-corpus-2026-09-11`, commit of this change. Verbatim
`cargo test` output (6 suites):

```
     Running unittests src/lib.rs (target/debug/deps/cargo_perf-53f22a1636f8d31e)
running 299 tests
test result: ok. 299 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
     Running unittests src/main.rs (target/debug/deps/cargo_perf-d2a65ef26d6e04bd)
running 3 tests
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/accuracy.rs (target/debug/deps/accuracy-5f5a47f48da17019)
running 2 tests
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
     Running tests/cli.rs (target/debug/deps/cli-c3a246ba57735097)
running 26 tests
test result: ok. 26 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.45s
     Running tests/integration.rs (target/debug/deps/integration-19425057f237bfc3)
running 10 tests
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
   Doc-tests cargo_perf
running 18 tests
test result: ok. 2 passed; 0 failed; 16 ignored; 0 measured; 0 filtered out; finished in 0.11s
```

**Totals: 342 passed, 0 failed, 16 ignored.** Identical to the pre-change
baseline on `main` (342 / 0 / 16) — the fixture count is data consumed by the
two `accuracy.rs` tests, not a test-case count, so adding fixtures does not move
the reported test total. The growth shows up in the scorecard instead.

`cargo build`:

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.17s
```

### Scorecard, before and after

Before (81 scored fixtures):

```
  OVERALL                     29   0   0   1.00   1.00
```

After (109 scored fixtures):

```
cargo-perf accuracy scorecard
  fixtures: 109   |   floors: precision >= 1.00, recall >= 1.00
  ------------------------------------------------------------
  rule                        TP  FP  FN   prec   recall
  async-block-in-async         8   0   0   1.00   1.00
  clone-in-hot-loop            5   0   0   1.00   1.00
  collect-then-iterate         2   0   0   1.00   1.00
  format-in-loop               2   0   0   1.00   1.00
  hashmap-no-capacity          2   0   0   1.00   1.00
  lock-across-await            6   0   0   1.00   1.00
  mutex-in-loop                4   0   0   1.00   1.00
  n-plus-one-query             2   0   0   1.00   1.00
  regex-in-loop                2   0   0   1.00   1.00
  string-concat-loop           2   0   0   1.00   1.00
  string-no-capacity           2   0   0   1.00   1.00
  unbounded-channel            2   0   0   1.00   1.00
  unbounded-spawn              2   0   0   1.00   1.00
  vec-no-capacity              2   0   0   1.00   1.00
  ------------------------------------------------------------
  OVERALL                     43   0   0   1.00   1.00
```

No rule sits on a single TP any more. The 1.00 / 1.00 floor is preserved and is
now resting on 34% more evidence.

## Rule bugs the new fixtures exposed

Three genuine defects turned up while probing near-misses. **No rule logic was
changed** — per the task's standing rule, each is documented and parked in
`tests/corpus/known_gaps/` (tracked, unscored) with an explicit promotion path,
so the suite stays green *and* honest. Each was confirmed by observing the actual
diagnostic, not inferred from reading the code.

### Bug 1 — `unbounded-spawn` fires on a user-defined `fn spawn` (false positive)

`tests/corpus/known_gaps/gap_unbounded_spawn_user_fn.rs`

A locally-defined free `fn spawn` called inside a loop is reported as unbounded
task spawning. `UnboundedSpawnVisitor::check_spawn_call` accepts a bare
`spawn(..)` as a runtime spawn (`path_str == spawn_fn`, commented "bare `spawn`
after `use`") and is **the only loop rule with no `ImportOracle` shadow gate**.
Its siblings all take the precaution first: `check_unbounded_channel`,
`check_blocking_path_call` and `RegexInLoopVisitor` each call
`imports.is_local_item(leading)` before matching.

Observed: `FALSE POSITIVE: p1_user_spawn.rs reported 'unbounded-spawn' at line 5`.

Fix shape: give `UnboundedSpawnVisitor` an `ImportOracle` and gate the bare-name
branch on `!imports.is_local_item(leading)`. Then promote the gap file to a
`// perf-guard: unbounded-spawn` negative.

### Bug 2 — capacity rules match any type whose name *ends in* the std type's name (false positive, 3 rules)

`tests/corpus/known_gaps/gap_capacity_rules_vec_suffix_type.rs`

`is_vec_new` tests `path_str.ends_with("Vec::new")` with **no `::`-segment
boundary check**, so `BitVec::new`, `SmallVec::new` and `IndexVec::new` all match
`Vec::new`. `is_hashmap_new` and `is_string_new` are the same shape, so
`MyHashMap::new` and `MyString::new` match too. All three rules are affected.

This is worse than a spurious warning: the diagnostic tells the author to call
`Vec::with_capacity()` on a type that may not have such a constructor, so acting
on the advice produces code that does not compile. `SmallVec` in particular is a
common dependency, which makes this reachable in ordinary codebases.

The correct pattern already exists twice in this repo:
`AsyncBlockingVisitor::path_ends_with_boundary` and the segment-equality check in
`RegexInLoopVisitor` (added, per its comment, for exactly this class of bug —
`RegexCacheKey::new` merely containing "Regex").

Observed:

```
FALSE POSITIVE: p2_smallvec.rs reported 'vec-no-capacity' at line 8
FALSE POSITIVE: p4_siblings.rs reported 'hashmap-no-capacity' at line 7
FALSE POSITIVE: p4_siblings.rs reported 'string-no-capacity' at line 8
```

Fix shape: have the three `is_*_new` helpers compare the final two path segments,
or reuse a shared boundary-aware helper. Then promote the gap file to a negative
guarding all three rules.

### Bug 3 — `lock-across-await` never analyzes `impl` methods (false negative)

`tests/corpus/known_gaps/gap_lock_across_await_impl_method.rs`

`LockAcrossAwaitVisitor` implements `visit_item_fn` but **not
`visit_impl_item_fn`**, so only free async functions are analyzed. A synchronous
`MutexGuard` held across `.await` inside an async method of an `impl` block is
missed entirely — while the byte-identical body in a free fn is reported as an
`Error` (`tests/corpus/pos_std_mutex_across_await.rs`).

This is the same blind spot `async-block-in-async` already closed as D3/D4;
`AsyncBlockingVisitor` grew a `visit_impl_item_fn` for precisely this reason, and
its comment notes blocking calls in impl methods were "systematically missed".
`lock-across-await` never received the matching fix. Since async Rust
overwhelmingly lives in impl methods, this is likely the single largest recall
gap in the tool, and it sits on its highest-severity rule.

Observed: the fixture reports nothing at all.

Fix shape: add a `visit_impl_item_fn` mirroring `visit_item_fn`, calling
`analyze_block` on async methods. Then promote the gap file to a positive with a
`// perf-expect: lock-across-await` marker.

## Notes

* Idiom followed exactly as established: inline `// perf-expect:` markers on the
  offending line, a leading `// perf-guard: <rule-id>` line on negatives, one
  scenario per file, and a comment explaining *why* the fixture is a positive or
  why the near-miss must stay silent.
* Fixtures are parsed (`syn`), not compiled, so they reference `sqlx`, `diesel`,
  `tokio`, `regex`, `flume` and `crossbeam_channel` without those being
  dependencies — consistent with the existing corpus.
* No dependency, formatting, or naming changes.
