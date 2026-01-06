mod context;
mod parser;

pub use context::AnalysisContext;

use crate::rules::{registry, Diagnostic};
use crate::Config;
use anyhow::Result;
use std::path::Path;
use walkdir::WalkDir;

pub struct Engine<'a> {
    config: &'a Config,
}

impl<'a> Engine<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self { config }
    }

    pub fn analyze(&self, path: &Path) -> Result<Vec<Diagnostic>> {
        let mut all_diagnostics = Vec::new();

        // Find all Rust files
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path().extension().map_or(false, |ext| ext == "rs")
                    && !e.path().to_string_lossy().contains("/target/")
            })
        {
            let file_path = entry.path();
            match self.analyze_file(file_path) {
                Ok(diagnostics) => all_diagnostics.extend(diagnostics),
                Err(e) => {
                    eprintln!("Warning: Failed to analyze {}: {}", file_path.display(), e);
                }
            }
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
            if self.config.rule_severity(rule.id(), rule.default_severity()).is_none() {
                continue;
            }

            let rule_diagnostics = rule.check(&ctx);
            diagnostics.extend(rule_diagnostics);
        }

        Ok(diagnostics)
    }
}
