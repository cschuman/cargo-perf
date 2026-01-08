//! # cargo-perf
//!
//! Static analysis for async Rust performance anti-patterns.
//!
//! cargo-perf complements `clippy` with checks specific to async code and
//! common loop-related performance issues that clippy doesn't catch.
//!
//! ## Quick Start
//!
//! ```no_run
//! use cargo_perf::{analyze, Config};
//! use std::path::Path;
//!
//! let config = Config::default();
//! let diagnostics = analyze(Path::new("src/"), &config).unwrap();
//!
//! for diag in diagnostics {
//!     println!("{}: {} at {}:{}",
//!         diag.rule_id,
//!         diag.message,
//!         diag.file_path.display(),
//!         diag.line
//!     );
//! }
//! ```
//!
//! ## Rules
//!
//! cargo-perf includes 12 rules organized into categories:
//!
//! ### Async Rules (Errors)
//! - `async-block-in-async`: Blocking std calls in async functions
//! - `lock-across-await`: Lock guards held across `.await` points
//!
//! ### Database Rules (Errors)
//! - `n-plus-one-query`: Database queries inside loops (SQLx, Diesel, SeaORM)
//!
//! ### Async Warnings
//! - `unbounded-channel`: Unbounded channels that can cause memory exhaustion
//! - `unbounded-spawn`: Task spawning in loops without concurrency limits
//!
//! ### Loop Rules (Warnings)
//! - `clone-in-hot-loop`: `.clone()` on heap types inside loops
//! - `regex-in-loop`: `Regex::new()` inside loops
//! - `format-in-loop`: `format!()` inside loops
//! - `string-concat-loop`: String `+` operator in loops
//! - `vec-no-capacity`: `Vec::new()` + push in loop
//! - `mutex-in-loop`: `Mutex::lock()` inside loops
//!
//! ### Iterator Rules (Warnings)
//! - `collect-then-iterate`: `.collect().iter()` anti-pattern
//!
//! ## Extending with Custom Rules
//!
//! Use the [`plugin`] module to add custom rules:
//!
//! ```rust,ignore
//! use cargo_perf::plugin::{PluginRegistry, analyze_with_plugins};
//!
//! let mut registry = PluginRegistry::new();
//! registry.add_builtin_rules();
//! registry.add_rule(Box::new(MyCustomRule));
//!
//! let diagnostics = analyze_with_plugins(path, &config, &registry)?;
//! ```
//!
//! ## Configuration
//!
//! Rules can be configured via `cargo-perf.toml`:
//!
//! ```toml
//! [rules]
//! async-block-in-async = "deny"   # error
//! clone-in-hot-loop = "warn"      # warning
//! regex-in-loop = "allow"         # disabled
//! ```
//!
//! ## Suppression
//!
//! Suppress warnings inline:
//!
//! ```rust,ignore
//! // Attribute-based (function/item scope)
//! #[allow(cargo_perf::clone_in_hot_loop)]
//! fn my_function() { /* ... */ }
//!
//! // Comment-based (next line only)
//! // cargo-perf-ignore: clone-in-hot-loop
//! let x = data.clone();
//! ```

pub mod baseline;
pub mod config;
pub mod discovery;
pub mod engine;
pub mod error;
pub mod fix;
#[cfg(feature = "lsp")]
pub mod lsp;
pub mod plugin;
pub mod reporter;
pub mod rules;
pub mod suppression;

pub use baseline::Baseline;
pub use config::Config;
pub use engine::{AnalysisContext, Engine};
pub use error::{Error, Result};
pub use fix::FixError;
pub use plugin::{analyze_with_plugins, PluginRegistry, PluginRegistryBuilder};
pub use rules::{Diagnostic, Fix, Replacement, Rule, Severity};

/// Analyze Rust files at the given path for performance anti-patterns.
///
/// # Arguments
///
/// * `path` - A file or directory to analyze. Directories are traversed recursively.
/// * `config` - Configuration controlling rule severity and output options.
///
/// # Returns
///
/// A vector of diagnostics found during analysis. Empty if no issues detected.
///
/// # Errors
///
/// Returns an error if file I/O fails or parsing encounters invalid syntax.
///
/// # Example
///
/// ```no_run
/// use cargo_perf::{analyze, Config, Severity};
/// use std::path::Path;
///
/// let config = Config::default();
/// let diagnostics = analyze(Path::new("src/"), &config)?;
///
/// let errors: Vec<_> = diagnostics
///     .iter()
///     .filter(|d| d.severity == Severity::Error)
///     .collect();
///
/// if !errors.is_empty() {
///     eprintln!("Found {} errors", errors.len());
/// }
/// # Ok::<(), cargo_perf::Error>(())
/// ```
pub fn analyze(path: &std::path::Path, config: &Config) -> Result<Vec<Diagnostic>> {
    let engine = Engine::new(config);
    engine.analyze(path)
}
