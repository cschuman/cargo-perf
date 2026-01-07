//! File discovery utilities for cargo-perf.
//!
//! This module provides shared file discovery functionality used by both
//! the engine and plugin systems.

use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Maximum file size to analyze (10 MB).
///
/// Files larger than this are skipped to prevent memory exhaustion attacks.
pub const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// Options for file discovery.
#[derive(Clone, Debug, Default)]
pub struct DiscoveryOptions {
    /// Whether to check file size limits.
    pub check_file_size: bool,
    /// Whether to perform TOCTOU-safe metadata checks.
    pub security_checks: bool,
}

impl DiscoveryOptions {
    /// Create options with all security checks enabled (recommended for engine).
    pub fn secure() -> Self {
        Self {
            check_file_size: true,
            security_checks: true,
        }
    }

    /// Create options without security checks (faster, for trusted contexts).
    pub fn fast() -> Self {
        Self {
            check_file_size: false,
            security_checks: false,
        }
    }
}

/// Discover all Rust files at the given path.
///
/// This function walks the directory tree, filtering out:
/// - `target` directories (Cargo build output)
/// - Hidden directories (starting with `.`)
/// - Common non-source directories (node_modules, vendor, etc.)
/// - Files that are too large (when `options.check_file_size` is true)
/// - Symlinks (when `options.security_checks` is true)
///
/// # Arguments
///
/// * `path` - The directory to search.
/// * `options` - Discovery options controlling security checks.
///
/// # Returns
///
/// A vector of paths to Rust source files.
pub fn discover_rust_files(path: &Path, options: &DiscoveryOptions) -> Vec<PathBuf> {
    // cargo-perf-ignore: vec-no-capacity
    let mut files = Vec::new();

    // SECURITY: Disable symlink following within the tree to prevent attacks
    // Note: We allow the root path to be a symlink (common for /tmp on macOS)
    for entry in WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !is_excluded_dir(e))
        .filter_map(|e| e.ok())
    {
        // Skip non-files (directories, symlinks, etc.)
        if !entry.file_type().is_file() {
            continue;
        }

        let file_path = entry.path();

        // Skip non-Rust files
        if !file_path.extension().is_some_and(|ext| ext == "rs") {
            continue;
        }

        if options.security_checks {
            // SECURITY: Double-check it's a regular file via metadata
            // This catches TOCTOU edge cases
            match std::fs::symlink_metadata(file_path) {
                Ok(meta) if meta.is_file() => {
                    // Check file size limit if enabled
                    if options.check_file_size && meta.len() > MAX_FILE_SIZE {
                        eprintln!(
                            "Warning: Skipping {} (file too large: {} bytes, max: {} bytes)",
                            file_path.display(),
                            meta.len(),
                            MAX_FILE_SIZE
                        );
                        continue;
                    }
                    files.push(file_path.to_path_buf());
                }
                Ok(_) => {
                    // Not a regular file (could be symlink), skip silently
                    continue;
                }
                Err(e) => {
                    eprintln!(
                        "Warning: Cannot read metadata for {}: {}",
                        file_path.display(),
                        e
                    );
                    continue;
                }
            }
        } else {
            // Fast path: trust the entry without additional checks
            files.push(file_path.to_path_buf());
        }
    }

    files
}

/// Check if a directory entry should be excluded from traversal.
///
/// This excludes:
/// - `target` directories (Cargo build output)
/// - Hidden directories (starting with `.`)
/// - Common dependency/build directories
///
/// Note: The root directory (depth 0) is never excluded, even if it starts with `.`.
pub fn is_excluded_dir(entry: &walkdir::DirEntry) -> bool {
    if !entry.file_type().is_dir() {
        return false;
    }

    // Never exclude the root directory (allows temp dirs like .tmpXXX)
    if entry.depth() == 0 {
        return false;
    }

    let name = entry.file_name().to_string_lossy();

    // Exclude target directory (cross-platform)
    if name == "target" {
        return true;
    }

    // Exclude hidden directories
    if name.starts_with('.') {
        return true;
    }

    // Exclude common non-source directories
    matches!(
        name.as_ref(),
        "node_modules" | "vendor" | "third_party" | "build" | "dist" | "out"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_excludes_target_directory() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        std::fs::create_dir(&target_dir).unwrap();
        std::fs::write(target_dir.join("test.rs"), "fn main() {}").unwrap();

        let files = discover_rust_files(temp_dir.path(), &DiscoveryOptions::fast());
        assert!(files.is_empty());
    }

    #[test]
    fn test_excludes_hidden_directories() {
        let temp_dir = TempDir::new().unwrap();
        let hidden_dir = temp_dir.path().join(".hidden");
        std::fs::create_dir(&hidden_dir).unwrap();
        std::fs::write(hidden_dir.join("test.rs"), "fn main() {}").unwrap();

        let files = discover_rust_files(temp_dir.path(), &DiscoveryOptions::fast());
        assert!(files.is_empty());
    }

    #[test]
    fn test_finds_rust_files() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src");
        std::fs::create_dir(&src_dir).unwrap();
        std::fs::write(src_dir.join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(src_dir.join("lib.rs"), "pub fn foo() {}").unwrap();
        std::fs::write(src_dir.join("not_rust.txt"), "text file").unwrap();

        let files = discover_rust_files(temp_dir.path(), &DiscoveryOptions::fast());
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| f.extension().unwrap() == "rs"));
    }
}
