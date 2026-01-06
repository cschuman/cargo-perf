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
        // Collect unique rules
        let mut rules: Vec<SarifRule> = Vec::new();
        let mut seen_rules = std::collections::HashSet::new();

        for d in diagnostics {
            if seen_rules.insert(d.rule_id) {
                rules.push(SarifRule {
                    id: d.rule_id.to_string(),
                    name: d.rule_id.to_string(),
                    short_description: SarifMessage {
                        text: d.message.clone(),
                    },
                });
            }
        }

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
                        information_uri: "https://github.com/YOUR_USERNAME/cargo-perf",
                        rules,
                    },
                },
                results,
            }],
        }
    }
}
