//! cargo-perf: Preventive performance analysis for Rust
//!
//! Catch performance anti-patterns before they reach production.

pub mod config;
pub mod engine;
pub mod error;
pub mod fix;
pub mod reporter;
pub mod rules;
pub mod suppression;

pub use config::Config;
pub use engine::{AnalysisContext, Engine};
pub use error::{Error, Result};
pub use fix::FixError;
pub use rules::{Diagnostic, Rule, Severity};

/// Run analysis on a project directory
pub fn analyze(path: &std::path::Path, config: &Config) -> Result<Vec<Diagnostic>> {
    let engine = Engine::new(config);
    engine.analyze(path)
}
