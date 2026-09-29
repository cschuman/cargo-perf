// perf-guard: regex-in-loop
// Negative near-miss: `RegexSet::new` merely *contains* the substring "Regex"
// and sits in the same crate, one `::` segment from the real target. The rule
// matches `Regex` on exact segment boundaries, so a substring match here would
// be a false positive on a different type.
fn scan(patterns: &[String]) {
    for _p in patterns {
        let _set = regex::RegexSet::new(patterns).unwrap();
    }
}
