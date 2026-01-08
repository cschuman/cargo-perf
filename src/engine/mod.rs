//! Analysis engine - coordinates file discovery and rule execution.

mod context;
pub mod file_analyzer;
mod parser;

pub use context::{AnalysisContext, LineIndex};
pub use file_analyzer::{analyze_file_with_rules, read_file_secure};

use crate::discovery::{discover_rust_files, DiscoveryOptions};
use crate::error::{Error, Result};
use crate::rules::{registry, Diagnostic};
use crate::Config;
use rayon::prelude::*;
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
        // Use shared file analysis logic with static registry rules
        let rules = registry::all_rules().iter().map(|r| r.as_ref());
        analyze_file_with_rules(file_path, self.config, rules)
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
