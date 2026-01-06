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
/// Returns a `ParseError` if the source cannot be parsed.
pub fn parse_file(source: &str) -> Result<syn::File, ParseError> {
    syn_parse_file(source).map_err(ParseError)
}
