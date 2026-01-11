use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::Severity;

/// Maximum config file size (1 MB) - prevents memory exhaustion from malformed files
const MAX_CONFIG_SIZE: u64 = 1024 * 1024;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            format: default_format(),
            color: default_color(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub orm: Option<String>,
}

impl Config {
    /// Load config from cargo-perf.toml in the given path, or return default
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the project directory containing cargo-perf.toml
    ///
    /// # Errors
    ///
    /// Returns an error if the path doesn't exist or if the config file
    /// exists but cannot be parsed.
    pub fn load_or_default(path: &Path) -> anyhow::Result<Self> {
        // Validate path exists
        if !path.exists() {
            anyhow::bail!("Path does not exist: {}", path.display());
        }

        // If path is a file, use its parent directory for config lookup
        let dir_path = if path.is_file() {
            path.parent().unwrap_or(path)
        } else {
            path
        };

        let config_path = dir_path.join("cargo-perf.toml");
        if config_path.exists() {
            // Check file size before reading to prevent memory exhaustion
            let metadata = std::fs::metadata(&config_path)?;
            if metadata.len() > MAX_CONFIG_SIZE {
                anyhow::bail!(
                    "Config file too large ({} bytes, max {} bytes): {}",
                    metadata.len(),
                    MAX_CONFIG_SIZE,
                    config_path.display()
                );
            }

            let content = std::fs::read_to_string(&config_path)?;
            let config: Config = toml::from_str(&content)?;

            // Validate rule IDs against known rules
            Self::validate_rule_ids(&config);

            Ok(config)
        } else {
            Ok(Config::default())
        }
    }

    /// Validate that configured rule IDs exist, warning about unknown ones.
    fn validate_rule_ids(config: &Config) {
        use crate::rules::registry;

        for rule_id in config.rules.keys() {
            if !registry::has_rule(rule_id) {
                eprintln!(
                    "Warning: Unknown rule '{}' in cargo-perf.toml (will be ignored)",
                    rule_id
                );
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.rules.is_empty());
        // Both Config::default() and serde deserialization use the same defaults
        assert_eq!(config.output.format, "console");
        assert_eq!(config.output.color, "auto");
    }

    #[test]
    fn test_rule_severity_default() {
        let config = Config::default();
        // Should return default when not configured
        assert_eq!(
            config.rule_severity("unknown-rule", Severity::Warning),
            Some(Severity::Warning)
        );
    }

    #[test]
    fn test_rule_severity_deny() {
        let mut config = Config::default();
        config
            .rules
            .insert("test-rule".to_string(), RuleSeverity::Deny);
        assert_eq!(
            config.rule_severity("test-rule", Severity::Warning),
            Some(Severity::Error)
        );
    }

    #[test]
    fn test_rule_severity_allow() {
        let mut config = Config::default();
        config
            .rules
            .insert("test-rule".to_string(), RuleSeverity::Allow);
        assert_eq!(config.rule_severity("test-rule", Severity::Warning), None);
    }

    #[test]
    fn test_load_or_default_nonexistent_path() {
        let result = Config::load_or_default(Path::new("/nonexistent/path"));
        assert!(result.is_err());
    }

    #[test]
    fn test_load_or_default_no_config_file() {
        let tmp = TempDir::new().unwrap();
        let config = Config::load_or_default(tmp.path()).unwrap();
        assert!(config.rules.is_empty());
    }

    #[test]
    fn test_load_or_default_with_config_file() {
        let tmp = TempDir::new().unwrap();
        let config_content = r#"
[rules]
async-block-in-async = "deny"
clone-in-hot-loop = "allow"
"#;
        std::fs::write(tmp.path().join("cargo-perf.toml"), config_content).unwrap();

        let config = Config::load_or_default(tmp.path()).unwrap();
        assert_eq!(
            config.rule_severity("async-block-in-async", Severity::Warning),
            Some(Severity::Error)
        );
        assert_eq!(
            config.rule_severity("clone-in-hot-loop", Severity::Warning),
            None
        );
    }

    #[test]
    fn test_load_or_default_with_file_path() {
        let tmp = TempDir::new().unwrap();
        let config_content = r#"
[rules]
test-rule = "warn"
"#;
        std::fs::write(tmp.path().join("cargo-perf.toml"), config_content).unwrap();
        let file_path = tmp.path().join("some_file.rs");
        std::fs::write(&file_path, "").unwrap();

        // Should find config from parent directory when given a file
        let config = Config::load_or_default(&file_path).unwrap();
        assert_eq!(
            config.rule_severity("test-rule", Severity::Error),
            Some(Severity::Warning)
        );
    }

    #[test]
    fn test_load_invalid_config() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("cargo-perf.toml"), "invalid { toml").unwrap();
        let result = Config::load_or_default(tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_rule_severity_from_conversion() {
        assert_eq!(
            Option::<Severity>::from(RuleSeverity::Deny),
            Some(Severity::Error)
        );
        assert_eq!(
            Option::<Severity>::from(RuleSeverity::Warn),
            Some(Severity::Warning)
        );
        assert_eq!(Option::<Severity>::from(RuleSeverity::Allow), None);
    }
}
