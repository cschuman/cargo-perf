//! Baseline support for ignoring known issues.
//!
//! A baseline file records the fingerprints of existing diagnostics so they
//! can be filtered out of subsequent runs. This is useful for:
//!
//! - Gradually adopting cargo-perf in an existing codebase
//! - Tracking technical debt without noise
//! - CI pipelines that should only fail on new issues
//!
//! # Fingerprinting Strategy
//!
//! Diagnostics are fingerprinted using:
//! - Rule ID
//! - Relative file path
//! - A hash of the source code around the diagnostic
//!
//! This means diagnostics remain matched even if:
//! - Lines are added/removed elsewhere in the file
//! - The file is moved (relative path changes)
//!
//! But will be treated as new if:
//! - The problematic code itself changes
//! - The rule ID changes

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::Diagnostic;

/// Default baseline filename
pub const BASELINE_FILENAME: &str = ".cargo-perf-baseline";

/// A fingerprint that uniquely identifies a diagnostic
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct Fingerprint {
    /// The rule that produced this diagnostic
    pub rule_id: String,
    /// Relative path to the file
    pub file_path: String,
    /// Hash of the source code context
    pub code_hash: u64,
}

impl Fingerprint {
    /// Create a fingerprint from a diagnostic and its source file
    pub fn from_diagnostic(diag: &Diagnostic, root: &Path) -> Option<Self> {
        let file_path = diag
            .file_path
            .strip_prefix(root)
            .unwrap_or(&diag.file_path)
            .to_string_lossy()
            .to_string();

        // Read the source line for hashing
        let code_hash = Self::hash_source_context(&diag.file_path, diag.line)?;

        Some(Fingerprint {
            rule_id: diag.rule_id.to_string(),
            file_path,
            code_hash,
        })
    }

    /// Create a fingerprint from a diagnostic using cached file content.
    ///
    /// This is more efficient when processing multiple diagnostics from
    /// the same file, as the file content is read once and reused.
    pub fn from_diagnostic_with_cache(
        diag: &Diagnostic,
        root: &Path,
        lines: &[&str],
    ) -> Option<Self> {
        let file_path = diag
            .file_path
            .strip_prefix(root)
            .unwrap_or(&diag.file_path)
            .to_string_lossy()
            .to_string();

        // Use cached lines for hashing
        let code_hash = Self::hash_source_context_with_cache(lines, diag.line)?;

        Some(Fingerprint {
            rule_id: diag.rule_id.to_string(),
            file_path,
            code_hash,
        })
    }

    /// Hash the source code around a specific line
    fn hash_source_context(file_path: &Path, line: usize) -> Option<u64> {
        let file = fs::File::open(file_path).ok()?;
        let reader = BufReader::new(file);

        // Get 3 lines of context: line-1, line, line+1
        let mut context = String::new();
        for (i, line_result) in reader.lines().enumerate() {
            let line_num = i + 1; // 1-indexed
            if line_num >= line.saturating_sub(1) && line_num <= line + 1 {
                if let Ok(text) = line_result {
                    // Normalize whitespace for stability
                    context.push_str(text.trim());
                    context.push('\n');
                }
            }
            if line_num > line + 1 {
                break;
            }
        }

        if context.is_empty() {
            return None;
        }

        // Simple hash - we use a stable algorithm
        Some(Self::stable_hash(&context))
    }

    /// A stable string hash using FNV-1a algorithm.
    /// This is guaranteed stable across Rust versions and platforms.
    fn stable_hash(s: &str) -> u64 {
        // FNV-1a constants for 64-bit
        const FNV_OFFSET: u64 = 14695981039346656037;
        const FNV_PRIME: u64 = 1099511628211;

        let mut hash = FNV_OFFSET;
        for byte in s.bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        hash
    }

    /// Hash source context with cached file content
    pub(crate) fn hash_source_context_with_cache(
        lines: &[&str],
        target_line: usize,
    ) -> Option<u64> {
        if target_line == 0 || target_line > lines.len() {
            return None;
        }

        // Get 3 lines of context: line-1, line, line+1 (1-indexed)
        let start = target_line.saturating_sub(2); // Convert to 0-indexed, go back 1
        let end = (target_line).min(lines.len()); // target_line is 1-indexed, so this is line+1 in 0-indexed

        let mut context = String::new();
        for line in &lines[start..end] {
            context.push_str(line.trim());
            context.push('\n');
        }

        if context.is_empty() {
            return None;
        }

        Some(Self::stable_hash(&context))
    }
}

/// A baseline entry with human-readable metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineEntry {
    /// The diagnostic fingerprint
    pub fingerprint: Fingerprint,
    /// Human-readable description
    pub description: String,
    /// When this entry was added
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added: Option<String>,
}

/// A collection of baselined diagnostics
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Baseline {
    /// Schema version for forward compatibility
    pub version: u32,
    /// Baselined entries
    pub entries: Vec<BaselineEntry>,

    /// Cached set of fingerprints for O(1) lookup
    #[serde(skip)]
    fingerprints: HashSet<Fingerprint>,
}

impl Baseline {
    /// Create a new empty baseline
    pub fn new() -> Self {
        Baseline {
            version: 1,
            entries: Vec::new(),
            fingerprints: HashSet::new(),
        }
    }

    /// Load a baseline from the default location
    pub fn load(root: &Path) -> std::io::Result<Self> {
        Self::load_from(root.join(BASELINE_FILENAME))
    }

    /// Load a baseline from a specific path
    pub fn load_from(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let content = fs::read_to_string(path.as_ref())?;
        let mut baseline: Baseline = serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // Build lookup cache
        baseline.fingerprints = baseline
            .entries
            .iter()
            .map(|e| e.fingerprint.clone())
            .collect();

        Ok(baseline)
    }

    /// Save the baseline to the default location
    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        self.save_to(root.join(BASELINE_FILENAME))
    }

    /// Save the baseline to a specific path
    pub fn save_to(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path.as_ref(), content)?;
        Ok(())
    }

    /// Check if a diagnostic is in the baseline
    pub fn contains(&self, diag: &Diagnostic, root: &Path) -> bool {
        if let Some(fp) = Fingerprint::from_diagnostic(diag, root) {
            self.fingerprints.contains(&fp)
        } else {
            false
        }
    }

    /// Add a diagnostic to the baseline
    pub fn add(&mut self, diag: &Diagnostic, root: &Path) {
        if let Some(fingerprint) = Fingerprint::from_diagnostic(diag, root) {
            if !self.fingerprints.contains(&fingerprint) {
                let entry = BaselineEntry {
                    fingerprint: fingerprint.clone(),
                    description: format!(
                        "{}: {} ({}:{})",
                        diag.rule_id,
                        diag.message,
                        diag.file_path.display(),
                        diag.line
                    ),
                    added: Some(chrono_lite_now()),
                };
                self.entries.push(entry);
                self.fingerprints.insert(fingerprint);
            }
        }
    }

    /// Create a baseline from a list of diagnostics
    ///
    /// This method uses file caching to avoid re-reading the same file
    /// when multiple diagnostics come from the same source file.
    pub fn from_diagnostics(diagnostics: &[Diagnostic], root: &Path) -> Self {
        let mut baseline = Baseline::new();

        // Group diagnostics by file path for efficient caching
        let mut by_file: HashMap<&PathBuf, Vec<&Diagnostic>> = HashMap::new();
        for diag in diagnostics {
            by_file.entry(&diag.file_path).or_default().push(diag);
        }

        // Process each file once, caching its content
        for (file_path, file_diagnostics) in by_file {
            // Read file once and cache lines
            let source = match fs::read_to_string(file_path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let lines: Vec<&str> = source.lines().collect();

            for diag in file_diagnostics {
                if let Some(fingerprint) =
                    Fingerprint::from_diagnostic_with_cache(diag, root, &lines)
                {
                    if !baseline.fingerprints.contains(&fingerprint) {
                        // Each diagnostic needs a unique description
                        // cargo-perf-ignore: format-in-loop
                        let description = format!(
                            "{}: {} ({}:{})",
                            diag.rule_id,
                            diag.message,
                            diag.file_path.display(),
                            diag.line
                        );
                        // Clone needed: fingerprint goes into both HashSet and entry
                        // cargo-perf-ignore: clone-in-hot-loop
                        baseline.fingerprints.insert(fingerprint.clone());
                        baseline.entries.push(BaselineEntry {
                            fingerprint,
                            description,
                            added: Some(chrono_lite_now()),
                        });
                    }
                }
            }
        }

        baseline
    }

    /// Filter diagnostics, removing those in the baseline
    ///
    /// This method uses file caching to avoid re-reading the same file
    /// when checking multiple diagnostics from the same source file.
    pub fn filter(&self, diagnostics: Vec<Diagnostic>, root: &Path) -> Vec<Diagnostic> {
        // Pre-allocate result with same capacity as input (worst case: no filtering)
        let diag_count = diagnostics.len();

        // Group diagnostics by file path for efficient caching
        let mut by_file: HashMap<PathBuf, Vec<Diagnostic>> = HashMap::new();
        for diag in diagnostics {
            // Clone needed for HashMap key; diag ownership moves to value
            // cargo-perf-ignore: clone-in-hot-loop
            let key = diag.file_path.clone();
            by_file.entry(key).or_default().push(diag);
        }

        let mut result = Vec::with_capacity(diag_count);

        // Process each file once, caching its content
        for (file_path, file_diagnostics) in by_file {
            // Read file once and cache lines
            let source = match fs::read_to_string(&file_path) {
                Ok(s) => s,
                Err(_) => {
                    // Can't read file, keep all diagnostics from it
                    result.extend(file_diagnostics);
                    continue;
                }
            };
            let lines: Vec<&str> = source.lines().collect();

            for diag in file_diagnostics {
                if let Some(fp) = Fingerprint::from_diagnostic_with_cache(&diag, root, &lines) {
                    if !self.fingerprints.contains(&fp) {
                        result.push(diag);
                    }
                } else {
                    // Can't fingerprint, keep the diagnostic
                    result.push(diag);
                }
            }
        }

        result
    }

    /// Number of entries in the baseline
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if baseline is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Simple date string without chrono dependency
fn chrono_lite_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Convert to ISO-like date string (approximate, good enough for metadata)
    let days = secs / 86400;
    let years = 1970 + days / 365;
    let remaining_days = days % 365;
    let month = remaining_days / 30 + 1;
    let day = remaining_days % 30 + 1;

    format!("{:04}-{:02}-{:02}", years, month.min(12), day.min(31))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_diagnostic(rule_id: &'static str, file: PathBuf, line: usize) -> Diagnostic {
        Diagnostic {
            rule_id,
            severity: crate::Severity::Warning,
            message: "test message".to_string(),
            file_path: file,
            line,
            column: 1,
            end_line: None,
            end_column: None,
            suggestion: None,
            fix: None,
        }
    }

    #[test]
    fn test_fingerprint_stability() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("test.rs");
        fs::write(&file, "fn main() {\n    println!(\"hello\");\n}\n").unwrap();

        let diag = create_test_diagnostic("test-rule", file.clone(), 2);

        let fp1 = Fingerprint::from_diagnostic(&diag, tmp.path()).unwrap();
        let fp2 = Fingerprint::from_diagnostic(&diag, tmp.path()).unwrap();

        assert_eq!(fp1, fp2);
    }

    #[test]
    fn test_baseline_save_load() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("test.rs");
        fs::write(&file, "let x = 1;\nlet y = x.clone();\n").unwrap();

        let diag = create_test_diagnostic("clone-in-hot-loop", file, 2);
        let baseline = Baseline::from_diagnostics(&[diag], tmp.path());

        baseline.save(tmp.path()).unwrap();
        let loaded = Baseline::load(tmp.path()).unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.entries[0].fingerprint.rule_id, "clone-in-hot-loop");
    }

    #[test]
    fn test_baseline_filtering() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("test.rs");
        fs::write(&file, "fn f() {}\nfn g() {}\n").unwrap();

        let diag1 = create_test_diagnostic("rule-a", file.clone(), 1);
        let diag2 = create_test_diagnostic("rule-b", file, 2);

        // Only baseline the first diagnostic
        let baseline = Baseline::from_diagnostics(std::slice::from_ref(&diag1), tmp.path());

        let filtered = baseline.filter(vec![diag1, diag2], tmp.path());
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].rule_id, "rule-b");
    }
}
