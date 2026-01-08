use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::Severity;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub rules: HashMap<String, RuleSeverity>,

    #[serde(default)]
    pub output: OutputConfig,

    #[serde(default)]
    pub database: DatabaseConfig,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuleSeverity {
    Deny,
    Warn,
    Allow,
}

impl From<RuleSeverity> for Option<Severity> {
    fn from(rs: RuleSeverity) -> Option<Severity> {
        match rs {
            RuleSeverity::Deny => Some(Severity::Error),
            RuleSeverity::Warn => Some(Severity::Warning),
            RuleSeverity::Allow => None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutputConfig {
    #[serde(default = "default_format")]
    pub format: String,

    #[serde(default = "default_color")]
    pub color: String,
}

fn default_format() -> String {
    "console".to_string()
}

fn default_color() -> String {
    "auto".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub orm: Option<String>,
}

impl Config {
    /// Load config from cargo-perf.toml in the given path, or return default
    pub fn load_or_default(path: &Path) -> anyhow::Result<Self> {
        let config_path = path.join("cargo-perf.toml");
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let config: Config = toml::from_str(&content)?;
            Ok(config)
        } else {
            Ok(Config::default())
        }
    }

    /// Get the effective severity for a rule
    pub fn rule_severity(&self, rule_id: &str, default: Severity) -> Option<Severity> {
        match self.rules.get(rule_id) {
            Some(RuleSeverity::Allow) => None,
            Some(RuleSeverity::Warn) => Some(Severity::Warning),
            Some(RuleSeverity::Deny) => Some(Severity::Error),
            None => Some(default),
        }
    }

    /// Generate default TOML config
    pub fn default_toml() -> &'static str {
        r#"# cargo-perf configuration
# Schema: https://raw.githubusercontent.com/cschuman/cargo-perf/main/cargo-perf.schema.json
# Docs: https://github.com/cschuman/cargo-perf

[rules]
# Set rule severity: "deny" (error), "warn" (warning), "allow" (ignore)
# async-block-in-async = "deny"
# lock-across-await = "deny"
# clone-in-hot-loop = "warn"
# vec-no-capacity = "allow"

[output]
format = "console"  # "console", "json", "sarif"
color = "auto"      # "auto", "always", "never"

[database]
# orm = "sqlx"  # "sqlx", "diesel", "sea-orm"
"#
    }
}
