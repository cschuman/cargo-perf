//! Rust source code parser for cargo-perf.
//!
//! This module provides a thin wrapper around the `syn` crate's parser,
//! standardizing error handling for use throughout cargo-perf.
//!
//! # Panic safety
//!
//! cargo-perf runs against **untrusted** third-party source: any `.rs` file in a
//! dependency tree, a downloaded crate, or a fuzzing corpus. A panic in the
//! parser must therefore be contained and turned into an ordinary parse error,
//! never allowed to unwind into (or abort) the caller. [`parse_file`] catches
//! panics originating in the underlying parser for this reason.

use std::any::Any;
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
/// Returns a `ParseError` if the source cannot be parsed (e.g., syntax errors),
/// or if the underlying parser panics on adversarial input.
///
/// # Memory Usage
///
/// The `syn` parser builds a complete in-memory AST, which can be substantial
/// for large files. For files larger than [`crate::discovery::MAX_FILE_SIZE`],
/// the discovery module will skip analysis to prevent excessive memory usage.
///
/// # Panic safety
///
/// A panic inside the parser is caught and returned as a `ParseError` rather
/// than propagating. Note that a *stack overflow* from pathologically deep
/// nesting aborts the process and cannot be caught here; that class of input is
/// bounded only by [`crate::discovery::MAX_FILE_SIZE`].
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
    parse_with(source, syn_parse_file)
}

/// Run `parse` against `source`, converting both parse errors and any panic in
/// `parse` into a [`ParseError`].
///
/// Factored out from [`parse_file`] so the panic-containment behaviour is
/// directly testable with an injected parser.
fn parse_with<F>(source: &str, parse: F) -> Result<syn::File, ParseError>
where
    F: FnOnce(&str) -> Result<syn::File, syn::Error>,
{
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| parse(source)));
    match result {
        Ok(parsed) => parsed.map_err(ParseError),
        Err(payload) => Err(ParseError(syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("parser panicked: {}", panic_message(&*payload)),
        ))),
    }
}

/// Extract a human-readable message from a caught panic payload.
fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "(unknown panic payload)".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_with_passes_through_ok() {
        let ast = parse_with("fn main() {}", syn_parse_file).expect("valid source parses");
        assert_eq!(ast.items.len(), 1);
    }

    #[test]
    fn test_parse_with_passes_through_syn_error() {
        // A genuine syntax error must surface as a ParseError, not a panic.
        let result = parse_with("fn (", syn_parse_file);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_with_converts_panic_to_error() {
        // The whole point of the wrapper: a panicking parser becomes a ParseError.
        let result = parse_with("anything", |_| panic!("boom in the parser"));
        let err = result.expect_err("a panicking parser must yield an error, not unwind");
        let msg = err.to_string();
        assert!(
            msg.contains("panicked"),
            "message should note the panic: {msg}"
        );
        assert!(
            msg.contains("boom in the parser"),
            "message should carry the payload: {msg}"
        );
    }

    #[test]
    fn test_parse_with_converts_string_panic_payload() {
        let result = parse_with("anything", |_| panic!("{}", String::from("owned payload")));
        let err = result.expect_err("panic must be caught");
        assert!(err.to_string().contains("owned payload"));
    }

    #[test]
    fn test_parse_file_valid() {
        let ast = parse_file("fn main() {}").expect("valid source parses");
        assert_eq!(ast.items.len(), 1);
    }

    /// A battery of adversarial-but-bounded inputs must never panic the process:
    /// `parse_file` may return `Ok` or `Err`, but must always *return*.
    ///
    /// Inputs are deliberately kept away from the stack-overflow regime (which
    /// aborts and cannot be caught); deep nesting is capped well below that.
    #[test]
    fn test_parse_file_robust_against_adversarial_input() {
        // Nesting is kept modest on purpose: deep recursion overflows the stack
        // (which aborts and is uncatchable — see the doc comment on `parse_file`),
        // and test threads have a small default stack. Adversarial nesting depth
        // is instead explored by the cargo-fuzz harness under `fuzz/`.
        let nested_parens = format!("fn f() {{ {}(){} }}", "(".repeat(50), ")".repeat(50));
        let nested_brackets = format!("const X: [u8; 0] = {}{};", "[".repeat(50), "]".repeat(50));
        let long_ident = format!("fn {}() {{}}", "a".repeat(100_000));
        let cases: &[&str] = &[
            "",
            "\0\0\0",
            "fn",
            "fn f(",
            "«»",
            "🦀🦀🦀",
            "let x = \"unterminated",
            "let x = r#\"unterminated raw",
            "//! only a doc comment",
            "\u{202e}\u{202d}",       // bidi control characters
            "fn f() { 0x }",          // malformed literal
            "fn f() { 1e999999999 }", // absurd exponent
            "impl",
            "mod m { mod m { mod m {",
            &nested_parens,
            &nested_brackets,
            &long_ident,
        ];
        for case in cases {
            // The assertion is simply that this returns rather than panicking.
            let _ = parse_file(case);
        }
    }
}
