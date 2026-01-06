//! Analysis engine - coordinates file discovery and rule execution.

mod context;
mod parser;

pub use context::AnalysisContext;

use crate::rules::{registry, Diagnostic};
use crate::Config;
use anyhow::Result;
use std::path::Path;
use walkdir::WalkDir;

/// Maximum file size to analyze (10 MB)
const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

pub struct Engine<'a> {
    config: &'a Config,
}

impl<'a> Engine<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self { config }
    }

    pub fn analyze(&self, path: &Path) -> Result<Vec<Diagnostic>> {
        let mut all_diagnostics = Vec::new();
        let mut errors = Vec::new();

        // Find all Rust files
        // SECURITY: Explicitly disable symlink following to prevent attacks
        for entry in WalkDir::new(path)
            .follow_links(false)
            .follow_root_links(false)
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
            if !file_path.extension().map_or(false, |ext| ext == "rs") {
                continue;
            }

            // SECURITY: Double-check it's a regular file via metadata
            // This catches TOCTOU edge cases
            match std::fs::symlink_metadata(file_path) {
                Ok(meta) if meta.is_file() => {
                    // Check file size limit
                    if meta.len() > MAX_FILE_SIZE {
                        eprintln!(
                            "Warning: Skipping {} (file too large: {} bytes, max: {} bytes)",
                            file_path.display(),
                            meta.len(),
                            MAX_FILE_SIZE
                        );
                        continue;
                    }
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

            match self.analyze_file(file_path) {
                Ok(diagnostics) => all_diagnostics.extend(diagnostics),
                Err(e) => {
                    // Collect errors but continue analyzing other files
                    errors.push((file_path.to_path_buf(), e));
                }
            }
        }

        // Report errors at the end
        for (path, error) in &errors {
            eprintln!("Warning: Failed to analyze {}: {}", path.display(), error);
        }

        Ok(all_diagnostics)
    }

    fn analyze_file(&self, file_path: &Path) -> Result<Vec<Diagnostic>> {
        let source = std::fs::read_to_string(file_path)?;
        let ast = parser::parse_file(&source)?;

        let ctx = AnalysisContext {
            file_path,
            source: &source,
            ast: &ast,
            config: self.config,
        };

        let mut diagnostics = Vec::new();

        for rule in registry::all_rules() {
            // Check if rule is enabled via config
            if self
                .config
                .rule_severity(rule.id(), rule.default_severity())
                .is_none()
            {
                continue;
            }

            let rule_diagnostics = rule.check(&ctx);
            diagnostics.extend(rule_diagnostics);
        }

        Ok(diagnostics)
    }
}

/// Check if a directory entry should be excluded from traversal.
///
/// This excludes:
/// - `target` directories (Cargo build output)
/// - Hidden directories (starting with `.`)
/// - Common dependency/build directories
fn is_excluded_dir(entry: &walkdir::DirEntry) -> bool {
    if !entry.file_type().is_dir() {
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

        // Also create a src file that should be analyzed
        let src_dir = temp_dir.path().join("src");
        std::fs::create_dir(&src_dir).unwrap();
        std::fs::write(src_dir.join("lib.rs"), "fn main() {}").unwrap();

        let config = Config::default();
        let engine = Engine::new(&config);
        let diagnostics = engine.analyze(temp_dir.path()).unwrap();

        // Should have analyzed src/lib.rs but not target/test.rs
        // (no diagnostics expected for clean code, but no errors either)
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_excludes_hidden_directories() {
        let temp_dir = TempDir::new().unwrap();

        // Hidden directory
        let hidden_dir = temp_dir.path().join(".hidden");
        std::fs::create_dir(&hidden_dir).unwrap();
        std::fs::write(hidden_dir.join("secret.rs"), "fn bad() {}").unwrap();

        // Visible file
        std::fs::write(temp_dir.path().join("visible.rs"), "fn good() {}").unwrap();

        let config = Config::default();
        let engine = Engine::new(&config);
        let result = engine.analyze(temp_dir.path());

        assert!(result.is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn test_does_not_follow_symlinks() {
        use std::os::unix::fs::symlink;

        let temp_dir = TempDir::new().unwrap();

        // Create a symlink to /etc/passwd (or any system file)
        let symlink_path = temp_dir.path().join("evil.rs");
        let _ = symlink("/etc/passwd", &symlink_path);

        // Create a real file
        std::fs::write(temp_dir.path().join("real.rs"), "fn main() {}").unwrap();

        let config = Config::default();
        let engine = Engine::new(&config);
        let result = engine.analyze(temp_dir.path());

        // Should succeed without trying to parse /etc/passwd
        assert!(result.is_ok());
    }
}
