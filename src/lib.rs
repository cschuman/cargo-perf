//! cargo-perf: Preventive performance analysis for Rust
//!
//! Catch performance anti-patterns before they reach production.

pub mod config;
pub mod engine;
pub mod fix;
pub mod reporter;
pub mod rules;

pub use config::Config;
pub use engine::{AnalysisContext, Engine};
pub use rules::{Diagnostic, Rule, Severity};

/// Run analysis on a project directory
pub fn analyze(path: &std::path::Path, config: &Config) -> anyhow::Result<Vec<Diagnostic>> {
    let engine = Engine::new(config);
    engine.analyze(path)
}
