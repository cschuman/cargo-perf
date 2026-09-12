// KNOWN GAP (precision, not scored): `unbounded-spawn` fires on a user-defined
// free `fn spawn` called in a loop. The rule accepts a bare `spawn(..)` as a
// runtime spawn via `path_str == spawn_fn` ("bare `spawn` after `use`") and is
// the only loop rule with no `ImportOracle` shadow gate, so a locally-defined
// item of the same name is not recognised as shadowing the runtime function.
//
// Every sibling rule already takes this precaution — `check_unbounded_channel`,
// `check_blocking_path_call` and `RegexInLoopVisitor` all call
// `imports.is_local_item(leading)` first. Confirmed on 2026-09-11: this file
// reports `unbounded-spawn` at the `spawn(*j)` call below.
//
// When `UnboundedSpawnVisitor` gains an `ImportOracle` and gates the bare-name
// branch on `!imports.is_local_item(leading)`, move this file to
// `tests/corpus/` as a `// perf-guard: unbounded-spawn` negative fixture.
fn spawn(job: u32) -> u32 {
    job
}

fn run(jobs: &[u32]) {
    for j in jobs {
        let _ = spawn(*j);
    }
}
