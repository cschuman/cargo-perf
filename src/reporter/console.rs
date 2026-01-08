use crate::rules::{Diagnostic, Severity};
use colored::Colorize;

pub fn report(diagnostics: &[Diagnostic]) {
    if diagnostics.is_empty() {
        println!("{}", "No performance issues found.".green());
        return;
    }

    let error_count = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    let warning_count = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .count();
    let info_count = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Info)
        .count();

    for diagnostic in diagnostics {
        print_diagnostic(diagnostic);
    }

    println!();
    print!("Found ");
    if error_count > 0 {
        print!("{}", format!("{} error(s)", error_count).red());
    }
    if warning_count > 0 {
        if error_count > 0 {
            print!(", ");
        }
        print!("{}", format!("{} warning(s)", warning_count).yellow());
    }
    if info_count > 0 {
        if error_count > 0 || warning_count > 0 {
            print!(", ");
        }
        print!("{}", format!("{} info(s)", info_count).blue());
    }
    println!();
}

fn print_diagnostic(d: &Diagnostic) {
    let severity_str = match d.severity {
        Severity::Error => "error".red().bold(),
        Severity::Warning => "warning".yellow().bold(),
        Severity::Info => "info".blue().bold(),
    };

    let rule_id = format!("[{}]", d.rule_id).dimmed();

    println!("{}{} {} {}", severity_str, ":".bold(), d.message, rule_id,);

    println!(
        "  {} {}:{}:{}",
        "-->".blue(),
        d.file_path.display(),
        d.line,
        d.column,
    );

    if let Some(suggestion) = &d.suggestion {
        println!("  {} {}", "help:".cyan(), suggestion);
    }

    println!();
}

/// Format a diagnostic as a plain text string (no colors) for testing.
#[cfg(test)]
fn format_diagnostic_plain(d: &Diagnostic) -> String {
    let severity = match d.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    };

    let mut result = format!(
        "{}: {} [{}]\n  --> {}:{}:{}\n",
        severity,
        d.message,
        d.rule_id,
        d.file_path.display(),
        d.line,
        d.column
    );

    if let Some(suggestion) = &d.suggestion {
        result.push_str(&format!("  help: {}\n", suggestion));
    }

    result
}

/// Count diagnostics by severity.
pub fn count_by_severity(diagnostics: &[Diagnostic]) -> (usize, usize, usize) {
    let errors = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    let warnings = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .count();
    let infos = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Info)
        .count();
    (errors, warnings, infos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_diagnostic(
        rule_id: &'static str,
        severity: Severity,
        suggestion: Option<&str>,
    ) -> Diagnostic {
        Diagnostic {
            rule_id,
            message: format!("Test message for {}", rule_id),
            severity,
            file_path: PathBuf::from("test.rs"),
            line: 10,
            column: 5,
            end_line: None,
            end_column: None,
            suggestion: suggestion.map(|s| s.to_string()),
            fix: None,
        }
    }

    #[test]
    fn test_count_by_severity() {
        let diagnostics = vec![
            make_diagnostic("e1", Severity::Error, None),
            make_diagnostic("e2", Severity::Error, None),
            make_diagnostic("w1", Severity::Warning, None),
            make_diagnostic("i1", Severity::Info, None),
            make_diagnostic("i2", Severity::Info, None),
            make_diagnostic("i3", Severity::Info, None),
        ];

        let (errors, warnings, infos) = count_by_severity(&diagnostics);
        assert_eq!(errors, 2);
        assert_eq!(warnings, 1);
        assert_eq!(infos, 3);
    }

    #[test]
    fn test_count_empty() {
        let (errors, warnings, infos) = count_by_severity(&[]);
        assert_eq!(errors, 0);
        assert_eq!(warnings, 0);
        assert_eq!(infos, 0);
    }

    #[test]
    fn test_format_diagnostic_error() {
        let diag = make_diagnostic("test-rule", Severity::Error, None);
        let result = format_diagnostic_plain(&diag);

        assert!(result.contains("error:"));
        assert!(result.contains("[test-rule]"));
        assert!(result.contains("test.rs:10:5"));
    }

    #[test]
    fn test_format_diagnostic_warning() {
        let diag = make_diagnostic("warn-rule", Severity::Warning, None);
        let result = format_diagnostic_plain(&diag);

        assert!(result.contains("warning:"));
        assert!(result.contains("[warn-rule]"));
    }

    #[test]
    fn test_format_diagnostic_info() {
        let diag = make_diagnostic("info-rule", Severity::Info, None);
        let result = format_diagnostic_plain(&diag);

        assert!(result.contains("info:"));
        assert!(result.contains("[info-rule]"));
    }

    #[test]
    fn test_format_diagnostic_with_suggestion() {
        let diag = make_diagnostic("rule", Severity::Warning, Some("Try this instead"));
        let result = format_diagnostic_plain(&diag);

        assert!(result.contains("help: Try this instead"));
    }

    #[test]
    fn test_format_diagnostic_without_suggestion() {
        let diag = make_diagnostic("rule", Severity::Warning, None);
        let result = format_diagnostic_plain(&diag);

        assert!(!result.contains("help:"));
    }
}
