//! Shared file analysis logic used by both Engine and Plugin.
//!
//! This module provides TOCTOU-safe file reading and rule execution
//! that can be reused across different analysis paths.

use crate::discovery::MAX_FILE_SIZE;
use crate::engine::context::AnalysisContext;
use crate::engine::parser;
use crate::error::{Error, Result};
use crate::rules::{Diagnostic, Rule};
use crate::suppression::SuppressionExtractor;
use crate::Config;
use std::any::Any;
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Extract a human-readable message from a panic payload.
///
/// Panic payloads can be String, &str, or other types. This function
/// attempts to extract a useful message from common panic payload types.
fn extract_panic_message(payload: &Box<dyn Any + Send>) -> String {
    // Try to extract &str
    if let Some(s) = payload.downcast_ref::<&str>() {
        return (*s).to_string();
    }

    // Try to extract String
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }

    // Fallback for unknown types
    "(unknown panic payload)".to_string()
}

/// Read a file with TOCTOU-safe handling.
///
/// Opens the file, validates metadata from the file descriptor,
/// and returns the file content. This prevents race conditions
/// where the file could be replaced between check and read.
pub fn read_file_secure(file_path: &Path) -> Result<String> {
    // SECURITY: Use file descriptor to prevent TOCTOU attacks
    // Open file once, verify via fd metadata, then read from same fd
    let mut file = File::open(file_path).map_err(|e| Error::io(file_path, e))?;

    // Get metadata from the open file descriptor (not the path)
    // This prevents race conditions where path is replaced after open
    let metadata = file.metadata().map_err(|e| Error::io(file_path, e))?;

    // Verify it's still a regular file via the fd
    if !metadata.is_file() {
        return Err(Error::io(
            file_path,
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a regular file"),
        ));
    }

    // Check file size via fd metadata
    if metadata.len() > MAX_FILE_SIZE {
        return Err(Error::io(
            file_path,
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "file too large: {} bytes (max: {} bytes)",
                    metadata.len(),
                    MAX_FILE_SIZE
                ),
            ),
        ));
    }

    // Read from the same file descriptor
    let mut source = String::with_capacity(metadata.len() as usize);
    file.read_to_string(&mut source)
        .map_err(|e| Error::io(file_path, e))?;

    Ok(source)
}

/// Analyze a single file with a given set of rules.
///
/// This is the shared analysis logic used by both `Engine` and `analyze_with_plugins`.
/// It handles:
/// - TOCTOU-safe file reading
/// - Parsing with syn
/// - Suppression extraction
/// - Rule execution with panic catching
/// - Diagnostic filtering
///
/// # Arguments
///
/// * `file_path` - Path to the Rust source file
/// * `config` - Configuration for rule severity
/// * `rules` - Iterator of rules to execute
///
/// # Returns
///
/// A vector of diagnostics found in the file.
pub fn analyze_file_with_rules<'a, I>(
    file_path: &Path,
    config: &Config,
    rules: I,
) -> Result<Vec<Diagnostic>>
where
    I: Iterator<Item = &'a dyn Rule>,
{
    // Read file with TOCTOU-safe handling
    let source = read_file_secure(file_path)?;

    // Parse the source
    let ast = parser::parse_file(&source).map_err(|e| Error::parse(file_path, e.to_string()))?;

    // Create analysis context
    let ctx = AnalysisContext::new(file_path, &source, &ast, config);

    // Extract suppressions for this file
    let suppressions = SuppressionExtractor::new(&source, &ast);

    // Run rules and collect diagnostics
    // cargo-perf-ignore: vec-no-capacity
    let mut diagnostics = Vec::new();

    for rule in rules {
        // Check if rule is enabled via config
        if config
            .rule_severity(rule.id(), rule.default_severity())
            .is_none()
        {
            continue;
        }

        // Catch panics in rule execution to prevent one bad rule from crashing analysis
        let rule_diagnostics =
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rule.check(&ctx))) {
                Ok(diags) => diags,
                Err(panic_payload) => {
                    // Extract panic message from the payload
                    let panic_msg = extract_panic_message(&panic_payload);
                    eprintln!(
                        "Warning: Rule '{}' panicked while analyzing {}: {}",
                        rule.id(),
                        file_path.display(),
                        panic_msg
                    );
                    continue;
                }
            };

        // Filter out suppressed diagnostics
        for diag in rule_diagnostics {
            if !suppressions.is_suppressed(diag.rule_id, diag.line) {
                diagnostics.push(diag);
            }
        }
    }

    Ok(diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_read_file_secure_success() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("test.rs");
        std::fs::write(&file_path, "fn main() {}").unwrap();

        let content = read_file_secure(&file_path).unwrap();
        assert_eq!(content, "fn main() {}");
    }

    #[test]
    fn test_read_file_secure_not_found() {
        let result = read_file_secure(Path::new("/nonexistent/file.rs"));
        assert!(result.is_err());
    }

    #[test]
    fn test_read_file_secure_directory() {
        let tmp = TempDir::new().unwrap();
        let result = read_file_secure(tmp.path());
        assert!(result.is_err());
    }
}
