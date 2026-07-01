//! Fuzz target: the full analysis pipeline (parse + every rule) must never
//! panic or abort on arbitrary input.
//!
//! Rules are invoked here **directly**, bypassing the per-rule `catch_unwind`
//! guard in `analyze_file_with_rules`, so that a panic inside any rule surfaces
//! as a fuzzer crash instead of being swallowed. The production analyzer keeps
//! the guard; this target exists to hunt down the panics it would otherwise
//! hide.
#![no_main]

use cargo_perf::engine::parser::parse_file;
use cargo_perf::engine::AnalysisContext;
use cargo_perf::rules::registry;
use cargo_perf::Config;
use libfuzzer_sys::fuzz_target;
use std::path::Path;

fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(ast) = parse_file(source) else {
        return;
    };

    let config = Config::default();
    let ctx = AnalysisContext::new(Path::new("fuzz.rs"), source, &ast, &config);
    for rule in registry::all_rules() {
        let _ = rule.check(&ctx);
    }
});
