pub mod async_rules;
pub mod iter_rules;
pub mod memory_rules;
pub mod registry;

use crate::engine::AnalysisContext;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Severity levels for diagnostics
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    #[default]
    Info,
    Warning,
    Error,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Info => write!(f, "info"),
            Severity::Warning => write!(f, "warning"),
            Severity::Error => write!(f, "error"),
        }
    }
}

impl std::str::FromStr for Severity {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "info" => Ok(Severity::Info),
            "warning" | "warn" => Ok(Severity::Warning),
            "error" | "deny" => Ok(Severity::Error),
            _ => Err(format!("Unknown severity: {}", s)),
        }
    }
}

impl clap::ValueEnum for Severity {
    fn value_variants<'a>() -> &'a [Self] {
        &[Severity::Info, Severity::Warning, Severity::Error]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        match self {
            Severity::Info => Some(clap::builder::PossibleValue::new("info")),
            Severity::Warning => Some(clap::builder::PossibleValue::new("warning")),
            Severity::Error => Some(clap::builder::PossibleValue::new("error")),
        }
    }
}

/// A diagnostic reported by a rule
#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub rule_id: &'static str,
    pub severity: Severity,
    pub message: String,
    pub file_path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub end_line: Option<usize>,
    pub end_column: Option<usize>,
    pub suggestion: Option<String>,
    pub fix: Option<Fix>,
}

/// An auto-fix for a diagnostic
#[derive(Debug, Clone, Serialize)]
pub struct Fix {
    pub description: String,
    pub replacements: Vec<Replacement>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Replacement {
    pub file_path: PathBuf,
    pub start_byte: usize,
    pub end_byte: usize,
    pub new_text: String,
}

/// The Rule trait - implement this to add new checks
pub trait Rule: Send + Sync {
    /// Unique identifier for this rule (e.g., "async-block-in-async")
    fn id(&self) -> &'static str;

    /// Human-readable name
    fn name(&self) -> &'static str;

    /// Description of what this rule checks
    fn description(&self) -> &'static str;

    /// Default severity level
    fn default_severity(&self) -> Severity;

    /// Run the check and return diagnostics
    fn check(&self, ctx: &AnalysisContext) -> Vec<Diagnostic>;
}
