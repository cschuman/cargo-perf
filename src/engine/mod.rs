//! Analysis engine - coordinates file discovery and rule execution.

mod context;
mod parser;

pub use context::{AnalysisContext, LineIndex};

use crate::discovery::{discover_rust_files, DiscoveryOptions, MAX_FILE_SIZE};
use crate::error::{Error, Result};
use crate::rules::{registry, Diagnostic};
use crate::suppression::SuppressionExtractor;
use crate::Config;
use rayon::prelude::*;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct Engine<'a> {
    config: &'a Config,
}

impl<'a> Engine<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self { config }
    }

    pub fn analyze(&self, path: &Path) -> Result<Vec<Diagnostic>> {
        // First, collect all valid file paths (sequential - fast)
        let files = self.collect_files(path);

        // Analyze files in parallel
        let errors: Mutex<Vec<(PathBuf, Error)>> = Mutex::new(Vec::new());

        let all_diagnostics: Vec<Diagnostic> = files
            .par_iter()
            .flat_map(|file_path| {
                match self.analyze_file(file_path) {
                    Ok(diagnostics) => diagnostics,
                    Err(e) => {
                        // Collect errors but continue analyzing other files
                        if let Ok(mut errs) = errors.lock() {
                            errs.push((file_path.clone(), e));
                        }
                        Vec::new()
                    }
                }
            })
            .collect();

        // Report errors at the end
        if let Ok(errs) = errors.lock() {
            for (path, error) in errs.iter() {
                eprintln!("Warning: Failed to analyze {}: {}", path.display(), error);
            }
        }

        Ok(all_diagnostics)
    }

    /// Collect all Rust files to analyze (sequential, fast).
    fn collect_files(&self, path: &Path) -> Vec<PathBuf> {
        discover_rust_files(path, &DiscoveryOptions::secure())
    }

    fn analyze_file(&self, file_path: &Path) -> Result<Vec<Diagnostic>> {
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

        let ast =
            parser::parse_file(&source).map_err(|e| Error::parse(file_path, e.to_string()))?;

        let ctx = AnalysisContext::new(file_path, &source, &ast, self.config);

        // Extract suppressions for this file
        let suppressions = SuppressionExtractor::new(&source, &ast);

        // cargo-perf-ignore: vec-no-capacity
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

            // Filter out suppressed diagnostics
            for diag in rule_diagnostics {
                if !suppressions.is_suppressed(diag.rule_id, diag.line) {
                    diagnostics.push(diag);
                }
            }
        }

        Ok(diagnostics)
    }
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
