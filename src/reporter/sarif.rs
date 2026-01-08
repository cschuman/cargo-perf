use crate::rules::{Diagnostic, Severity};
use anyhow::Result;
use serde::Serialize;

/// SARIF (Static Analysis Results Interchange Format) output for GitHub integration
pub fn report(diagnostics: &[Diagnostic]) -> Result<()> {
    let sarif = SarifReport::from_diagnostics(diagnostics);
    let json = serde_json::to_string_pretty(&sarif)?;
    println!("{}", json);
    Ok(())
}

#[derive(Serialize)]
struct SarifReport {
    #[serde(rename = "$schema")]
    schema: &'static str,
    version: &'static str,
    runs: Vec<SarifRun>,
}

#[derive(Serialize)]
struct SarifRun {
    tool: SarifTool,
    results: Vec<SarifResult>,
}

#[derive(Serialize)]
struct SarifTool {
    driver: SarifDriver,
}

#[derive(Serialize)]
struct SarifDriver {
    name: &'static str,
    version: &'static str,
    #[serde(rename = "informationUri")]
    information_uri: &'static str,
    rules: Vec<SarifRule>,
}

#[derive(Serialize)]
struct SarifRule {
    id: String,
    name: String,
    #[serde(rename = "shortDescription")]
    short_description: SarifMessage,
}

#[derive(Serialize)]
struct SarifResult {
    #[serde(rename = "ruleId")]
    rule_id: String,
    level: &'static str,
    message: SarifMessage,
    locations: Vec<SarifLocation>,
}

#[derive(Serialize)]
struct SarifMessage {
    text: String,
}

#[derive(Serialize)]
struct SarifLocation {
    #[serde(rename = "physicalLocation")]
    physical_location: SarifPhysicalLocation,
}

#[derive(Serialize)]
struct SarifPhysicalLocation {
    #[serde(rename = "artifactLocation")]
    artifact_location: SarifArtifactLocation,
    region: SarifRegion,
}

#[derive(Serialize)]
struct SarifArtifactLocation {
    uri: String,
}

#[derive(Serialize)]
struct SarifRegion {
    #[serde(rename = "startLine")]
    start_line: usize,
    #[serde(rename = "startColumn")]
    start_column: usize,
}

impl SarifReport {
    fn from_diagnostics(diagnostics: &[Diagnostic]) -> Self {
        use crate::rules::registry;

        // Collect unique rule IDs first (no cloning in loop)
        let seen_rules: std::collections::HashSet<&str> =
            diagnostics.iter().map(|d| d.rule_id).collect();

        // Build rules list outside the loop using registry for descriptions
        let rules: Vec<SarifRule> = seen_rules
            .into_iter()
            .map(|rule_id| {
                let description = registry::get_rule(rule_id)
                    .map(|r| r.description())
                    .unwrap_or(rule_id);
                SarifRule {
                    id: rule_id.to_string(),
                    name: rule_id.to_string(),
                    short_description: SarifMessage {
                        text: description.to_string(),
                    },
                }
            })
            .collect();

        let results: Vec<SarifResult> = diagnostics
            .iter()
            .map(|d| SarifResult {
                rule_id: d.rule_id.to_string(),
                level: match d.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                    Severity::Info => "note",
                },
                message: SarifMessage {
                    text: d.message.clone(),
                },
                locations: vec![SarifLocation {
                    physical_location: SarifPhysicalLocation {
                        artifact_location: SarifArtifactLocation {
                            uri: d.file_path.to_string_lossy().to_string(),
                        },
                        region: SarifRegion {
                            start_line: d.line,
                            start_column: d.column,
                        },
                    },
                }],
            })
            .collect();

        SarifReport {
            schema: "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json",
            version: "2.1.0",
            runs: vec![SarifRun {
                tool: SarifTool {
                    driver: SarifDriver {
                        name: "cargo-perf",
                        version: env!("CARGO_PKG_VERSION"),
                        information_uri: "https://github.com/cschuman/cargo-perf",
                        rules,
                    },
                },
                results,
            }],
        }
    }
}

/// Format diagnostics as SARIF JSON string without printing.
pub fn format(diagnostics: &[Diagnostic]) -> Result<String> {
    let sarif = SarifReport::from_diagnostics(diagnostics);
    Ok(serde_json::to_string_pretty(&sarif)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_diagnostic(rule_id: &'static str, severity: Severity) -> Diagnostic {
        Diagnostic {
            rule_id,
            message: format!("Test message for {}", rule_id),
            severity,
            file_path: PathBuf::from("test.rs"),
            line: 10,
            column: 5,
            end_line: None,
            end_column: None,
            suggestion: None,
            fix: None,
        }
    }

    #[test]
    fn test_sarif_schema_version() {
        let result = format(&[]).unwrap();
        assert!(result.contains("sarif-schema-2.1.0.json"));
        assert!(result.contains(r#""version": "2.1.0""#));
    }

    #[test]
    fn test_sarif_tool_info() {
        let result = format(&[]).unwrap();
        assert!(result.contains(r#""name": "cargo-perf""#));
        assert!(result.contains("github.com/cschuman/cargo-perf"));
    }

    #[test]
    fn test_sarif_severity_mapping() {
        let error = test_diagnostic("rule-e", Severity::Error);
        let warning = test_diagnostic("rule-w", Severity::Warning);
        let info = test_diagnostic("rule-i", Severity::Info);

        let result = format(&[error, warning, info]).unwrap();

        assert!(result.contains(r#""level": "error""#));
        assert!(result.contains(r#""level": "warning""#));
        assert!(result.contains(r#""level": "note""#));
    }

    #[test]
    fn test_sarif_location_info() {
        let diag = Diagnostic {
            rule_id: "test-rule",
            message: "Test".to_string(),
            severity: Severity::Warning,
            file_path: PathBuf::from("/path/to/file.rs"),
            line: 42,
            column: 8,
            end_line: None,
            end_column: None,
            suggestion: None,
            fix: None,
        };

        let result = format(&[diag]).unwrap();

        assert!(result.contains("/path/to/file.rs"));
        assert!(result.contains(r#""startLine": 42"#));
        assert!(result.contains(r#""startColumn": 8"#));
    }

    #[test]
    fn test_sarif_unique_rules() {
        // Two diagnostics with same rule should only create one rule entry
        let diag1 = test_diagnostic("same-rule", Severity::Warning);
        let diag2 = test_diagnostic("same-rule", Severity::Warning);

        let result = format(&[diag1, diag2]).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

        let rules = &parsed["runs"][0]["tool"]["driver"]["rules"];
        assert_eq!(rules.as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_sarif_valid_json() {
        let diag = test_diagnostic("test-rule", Severity::Error);
        let result = format(&[diag]).unwrap();

        // Should parse as valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed.is_object());
        assert!(parsed.get("$schema").is_some());
        assert!(parsed.get("runs").is_some());
    }
}
