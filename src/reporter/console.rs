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
