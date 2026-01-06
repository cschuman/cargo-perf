use anyhow::Result;
use syn::parse_file as syn_parse_file;

/// Parse a Rust source file into an AST
pub fn parse_file(source: &str) -> Result<syn::File> {
    syn_parse_file(source).map_err(|e| anyhow::anyhow!("Parse error: {}", e))
}
