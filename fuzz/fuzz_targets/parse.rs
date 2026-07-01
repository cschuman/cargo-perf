//! Fuzz target: the parser must never panic or abort on arbitrary input.
//!
//! `parse_file` catches panics originating in the underlying `syn` parser and
//! converts them into an ordinary `ParseError`, so any *crash* the fuzzer finds
//! here is a real robustness bug (a panic path that escaped the guard, or an
//! abort such as a stack overflow that we need to bound).
#![no_main]

use cargo_perf::engine::parser::parse_file;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // `syn` operates on `&str`; invalid UTF-8 is not an interesting parser input.
    if let Ok(source) = std::str::from_utf8(data) {
        let _ = parse_file(source);
    }
});
