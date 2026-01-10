//! Rust source code parser for cargo-perf.
//!
//! This module provides a thin wrapper around the `syn` crate's parser,
//! standardizing error handling for use throughout cargo-perf.

use syn::parse_file as syn_parse_file;

/// Parse error wrapper for syn parse errors.
#[derive(Debug)]
pub struct ParseError(pub syn::Error);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParseError {}

/// Parse a Rust source file into an AST.
///
/// Returns a `ParseError` if the source cannot be parsed (e.g., syntax errors).
///
/// # Memory Usage
///
/// The `syn` parser builds a complete in-memory AST, which can be substantial
/// for large files. For files larger than [`crate::discovery::MAX_FILE_SIZE`],
/// the discovery module will skip analysis to prevent excessive memory usage.
///
/// # Example
///
/// ```ignore
/// use cargo_perf::engine::parser::parse_file;
///
/// let source = "fn main() { println!(\"Hello\"); }";
/// let ast = parse_file(source).expect("valid Rust code");
/// ```
pub fn parse_file(source: &str) -> Result<syn::File, ParseError> {
    syn_parse_file(source).map_err(ParseError)
}
