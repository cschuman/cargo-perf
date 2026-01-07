//! Auto-fix functionality for applying suggested code changes.
//!
//! This module provides safe, atomic file modifications with proper
//! path validation to prevent directory traversal attacks.

use crate::rules::{Diagnostic, Replacement};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use tempfile::NamedTempFile;
use thiserror::Error;

/// Errors that can occur during fix application
#[derive(Debug, Error)]
pub enum FixError {
    #[error("path traversal attempt: {path} is outside base directory {base}")]
    PathTraversal { path: String, base: String },

    #[error("invalid byte offset {offset} for file of length {len} in {path}")]
    InvalidOffset {
        path: String,
        offset: usize,
        len: usize,
    },

    #[error("byte offset {offset} is not on a UTF-8 character boundary in {path}")]
    InvalidUtf8Boundary { path: String, offset: usize },

    #[error("start_byte {start} is greater than end_byte {end} in {path}")]
    InvalidRange {
        path: String,
        start: usize,
        end: usize,
    },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Validate that a file path is within the allowed base directory.
///
/// This prevents path traversal attacks where a malicious diagnostic
/// could attempt to write to files outside the project directory.
fn validate_path(path: &Path, base_dir: &Path) -> Result<std::path::PathBuf, FixError> {
    // Canonicalize the base directory
    let canonical_base = base_dir.canonicalize().map_err(FixError::Io)?;

    // For the target path, we need to handle both existing and non-existing files
    let canonical_path = if path.exists() {
        path.canonicalize().map_err(FixError::Io)?
    } else {
        // If file doesn't exist, canonicalize the parent and append the filename
        let parent = path.parent().ok_or_else(|| FixError::PathTraversal {
            path: path.display().to_string(),
            base: canonical_base.display().to_string(),
        })?;
        let filename = path.file_name().ok_or_else(|| FixError::PathTraversal {
            path: path.display().to_string(),
            base: canonical_base.display().to_string(),
        })?;
        let canonical_parent = parent.canonicalize().map_err(FixError::Io)?;
        canonical_parent.join(filename)
    };

    // Verify the path is within the base directory
    if !canonical_path.starts_with(&canonical_base) {
        return Err(FixError::PathTraversal {
            path: canonical_path.display().to_string(),
            base: canonical_base.display().to_string(),
        });
    }

    Ok(canonical_path)
}

/// Validate byte offsets for a replacement operation.
fn validate_offsets(
    replacement: &Replacement,
    content: &str,
    path: &Path,
) -> Result<(), FixError> {
    let path_str = path.display().to_string();
    let len = content.len();

    // Check bounds
    if replacement.start_byte > len {
        return Err(FixError::InvalidOffset {
            path: path_str,
            offset: replacement.start_byte,
            len,
        });
    }

    if replacement.end_byte > len {
        return Err(FixError::InvalidOffset {
            path: path_str,
            offset: replacement.end_byte,
            len,
        });
    }

    // Check ordering
    if replacement.start_byte > replacement.end_byte {
        return Err(FixError::InvalidRange {
            path: path_str,
            start: replacement.start_byte,
            end: replacement.end_byte,
        });
    }

    // Check UTF-8 boundaries
    if !content.is_char_boundary(replacement.start_byte) {
        return Err(FixError::InvalidUtf8Boundary {
            path: path_str,
            offset: replacement.start_byte,
        });
    }

    if !content.is_char_boundary(replacement.end_byte) {
        return Err(FixError::InvalidUtf8Boundary {
            path: path_str,
            offset: replacement.end_byte,
        });
    }

    Ok(())
}

/// Apply auto-fixes from diagnostics with safety checks.
///
/// # Arguments
/// * `diagnostics` - The diagnostics containing fix information
/// * `base_dir` - The base directory that all fixes must be within
///
/// # Safety
/// This function validates that:
/// - All file paths are within `base_dir` (prevents path traversal)
/// - All byte offsets are valid and on UTF-8 boundaries
/// - Multiple fixes to the same file are applied correctly (in reverse order)
///
/// Writes are performed atomically using temporary files.
pub fn apply_fixes(diagnostics: &[Diagnostic], base_dir: &Path) -> Result<usize, FixError> {
    // Group replacements by file path
    let mut by_file: HashMap<&Path, Vec<&Replacement>> = HashMap::new();

    for diagnostic in diagnostics {
        if let Some(fix) = &diagnostic.fix {
            for replacement in &fix.replacements {
                by_file
                    .entry(replacement.file_path.as_path())
                    .or_default()
                    .push(replacement);
            }
        }
    }

    let mut fixed = 0;

    for (path, mut replacements) in by_file {
        // Validate path is within base directory
        let validated_path = validate_path(path, base_dir)?;

        // Read file content
        let content = std::fs::read_to_string(&validated_path)?;

        // Validate all offsets before applying any
        for replacement in &replacements {
            validate_offsets(replacement, &content, path)?;
        }

        // Sort replacements by start_byte descending
        // This ensures later offsets are applied first, preventing offset drift
        replacements.sort_by_key(|r| std::cmp::Reverse(r.start_byte));

        // Apply all replacements
        let mut result = content;
        for replacement in &replacements {
            result.replace_range(
                replacement.start_byte..replacement.end_byte,
                &replacement.new_text,
            );
            fixed += 1;
        }

        // Write atomically: write to temp file, then rename
        // This prevents corrupted files if the process is interrupted
        // SECURITY: Use tempfile crate to create secure temp file with random name
        // This prevents symlink attacks on predictable temp file paths
        let parent = validated_path.parent().unwrap_or(Path::new("."));
        let mut temp_file = NamedTempFile::new_in(parent)?;

        // Write content to temp file
        temp_file.write_all(result.as_bytes())?;
        temp_file.flush()?;

        // Atomically rename temp file to target
        // persist() ensures the file isn't deleted when dropped
        temp_file
            .persist(&validated_path)
            .map_err(|e| FixError::Io(e.error))?;
    }

    Ok(fixed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn test_path_traversal_rejected() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        // Create a path outside the base directory
        let evil_path = PathBuf::from("/etc/passwd");

        let result = validate_path(&evil_path, base);
        assert!(result.is_err());

        if let Err(FixError::PathTraversal { .. }) = result {
            // Expected
        } else {
            panic!("Expected PathTraversal error");
        }
    }

    #[test]
    fn test_valid_path_accepted() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        // Create a file inside the base directory
        let valid_path = base.join("test.rs");
        std::fs::write(&valid_path, "test").unwrap();

        let result = validate_path(&valid_path, base);
        assert!(result.is_ok());
    }

    #[test]
    fn test_invalid_offset_rejected() {
        let content = "hello";
        let replacement = Replacement {
            file_path: PathBuf::from("test.rs"),
            start_byte: 0,
            end_byte: 100, // Way past end of content
            new_text: "world".to_string(),
        };

        let result = validate_offsets(&replacement, content, Path::new("test.rs"));
        assert!(matches!(result, Err(FixError::InvalidOffset { .. })));
    }

    #[test]
    fn test_invalid_range_rejected() {
        let content = "hello";
        let replacement = Replacement {
            file_path: PathBuf::from("test.rs"),
            start_byte: 3,
            end_byte: 1, // start > end
            new_text: "world".to_string(),
        };

        let result = validate_offsets(&replacement, content, Path::new("test.rs"));
        assert!(matches!(result, Err(FixError::InvalidRange { .. })));
    }

    #[test]
    fn test_utf8_boundary_rejected() {
        let content = "héllo"; // 'é' is 2 bytes
        let replacement = Replacement {
            file_path: PathBuf::from("test.rs"),
            start_byte: 2, // Middle of 'é'
            end_byte: 3,
            new_text: "a".to_string(),
        };

        let result = validate_offsets(&replacement, content, Path::new("test.rs"));
        assert!(matches!(result, Err(FixError::InvalidUtf8Boundary { .. })));
    }
}
