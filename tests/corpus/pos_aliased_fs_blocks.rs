// Positive (D7): `use std::fs as sfs;` then `sfs::read_to_string(..)` in an
// async fn is a genuine blocking call reached through an alias. The import
// oracle canonicalizes `sfs` back to `std::fs`, so the true positive must
// survive — an alias is not a way to hide a blocking call from the linter.
use std::fs as sfs;

async fn run() -> String {
    sfs::read_to_string("config.toml").unwrap() // perf-expect: async-block-in-async
}
