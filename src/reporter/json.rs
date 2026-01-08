use crate::rules::Diagnostic;
use anyhow::Result;

pub fn report(diagnostics: &[Diagnostic]) -> Result<()> {
    let json = serde_json::to_string_pretty(diagnostics)?;
    println!("{}", json);
    Ok(())
}

/// Format diagnostics as JSON string without printing.
pub fn format(diagnostics: &[Diagnostic]) -> Result<String> {
    Ok(serde_json::to_string_pretty(diagnostics)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Severity;
    use std::path::PathBuf;

    fn test_diagnostic() -> Diagnostic {
        Diagnostic {
            rule_id: "test-rule",
            message: "Test message".to_string(),
            severity: Severity::Warning,
            file_path: PathBuf::from("test.rs"),
            line: 10,
            column: 5,
            end_line: None,
            end_column: None,
            suggestion: Some("Test suggestion".to_string()),
            fix: None,
        }
    }

    #[test]
    fn test_format_empty_diagnostics() {
        let result = format(&[]).unwrap();
        assert_eq!(result, "[]");
    }

    #[test]
    fn test_format_single_diagnostic() {
        let diag = test_diagnostic();
        let result = format(&[diag]).unwrap();

        assert!(result.contains(r#""rule_id": "test-rule""#));
        assert!(result.contains(r#""message": "Test message""#));
        assert!(result.contains(r#""severity": "warning""#));
        assert!(result.contains(r#""line": 10"#));
        assert!(result.contains(r#""column": 5"#));
        assert!(result.contains(r#""suggestion": "Test suggestion""#));
    }

    #[test]
    fn test_format_multiple_diagnostics() {
        let diag1 = Diagnostic {
            rule_id: "rule-a",
            message: "First".to_string(),
            severity: Severity::Error,
            file_path: PathBuf::from("a.rs"),
            line: 1,
            column: 1,
            end_line: None,
            end_column: None,
            suggestion: None,
            fix: None,
        };
        let diag2 = Diagnostic {
            rule_id: "rule-b",
            message: "Second".to_string(),
            severity: Severity::Info,
            file_path: PathBuf::from("b.rs"),
            line: 2,
            column: 2,
            end_line: None,
            end_column: None,
            suggestion: None,
            fix: None,
        };

        let result = format(&[diag1, diag2]).unwrap();

        assert!(result.contains(r#""rule_id": "rule-a""#));
        assert!(result.contains(r#""rule_id": "rule-b""#));
        assert!(result.contains(r#""severity": "error""#));
        assert!(result.contains(r#""severity": "info""#));
    }

    #[test]
    fn test_format_is_valid_json() {
        let diag = test_diagnostic();
        let result = format(&[diag]).unwrap();

        // Should parse back as valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 1);
    }
}
