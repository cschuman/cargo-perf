use anyhow::Result;
use cargo_perf::{analyze, Config};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "cargo-perf")]
#[command(about = "Preventive performance analysis for Rust")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Path to analyze (defaults to current directory)
    #[arg(short, long, default_value = ".")]
    path: PathBuf,

    /// Output format
    #[arg(short, long, default_value = "console")]
    format: OutputFormat,

    /// Minimum severity to report
    #[arg(long, default_value = "info")]
    min_severity: cargo_perf::Severity,

    /// Fail if any diagnostic meets this severity
    #[arg(long)]
    fail_on: Option<cargo_perf::Severity>,

    /// Specific rules to run (comma-separated)
    #[arg(long)]
    rules: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run performance analysis (default)
    Check {
        /// Path to analyze
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Apply auto-fixes
    Fix {
        /// Path to fix
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Initialize cargo-perf.toml config
    Init,
    /// List available rules
    Rules,
}

#[derive(Clone, Copy, Default, clap::ValueEnum)]
enum OutputFormat {
    #[default]
    Console,
    Json,
    Sarif,
}

fn main() -> ExitCode {
    if let Err(e) = run() {
        eprintln!("Error: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    // Handle "cargo perf" invocation (first arg is "perf")
    let args: Vec<String> = std::env::args().collect();
    let cli = if args.get(1).map(|s| s.as_str()) == Some("perf") {
        // Re-parse skipping the "perf" argument
        Cli::parse_from(std::iter::once("cargo-perf".to_string()).chain(args.into_iter().skip(2)))
    } else {
        cli
    };

    let config = Config::load_or_default(&cli.path)?;

    match cli.command {
        Some(Commands::Check { path }) => {
            run_check(&path, &config, cli.format, cli.min_severity, cli.fail_on)
        }
        None => {
            // Default to check with cli.path
            run_check(&cli.path, &config, cli.format, cli.min_severity, cli.fail_on)
        }
        Some(Commands::Fix { path }) => {
            run_fix(&path, &config)
        }
        Some(Commands::Init) => {
            run_init(&cli.path)
        }
        Some(Commands::Rules) => {
            run_list_rules()
        }
    }
}

fn run_check(
    path: &Path,
    config: &Config,
    format: OutputFormat,
    min_severity: cargo_perf::Severity,
    fail_on: Option<cargo_perf::Severity>,
) -> Result<()> {
    let diagnostics = analyze(path, config)?;

    // Filter by minimum severity
    let diagnostics: Vec<_> = diagnostics
        .into_iter()
        .filter(|d| d.severity >= min_severity)
        .collect();

    // Report
    match format {
        OutputFormat::Console => {
            cargo_perf::reporter::console::report(&diagnostics);
        }
        OutputFormat::Json => {
            cargo_perf::reporter::json::report(&diagnostics)?;
        }
        OutputFormat::Sarif => {
            cargo_perf::reporter::sarif::report(&diagnostics)?;
        }
    }

    // Check fail condition
    if let Some(fail_severity) = fail_on {
        if diagnostics.iter().any(|d| d.severity >= fail_severity) {
            anyhow::bail!(
                "Found {} diagnostic(s) at or above {:?} severity",
                diagnostics.iter().filter(|d| d.severity >= fail_severity).count(),
                fail_severity
            );
        }
    }

    Ok(())
}

fn run_fix(path: &Path, config: &Config) -> Result<()> {
    let diagnostics = analyze(path, config)?;
    let fixed = cargo_perf::fix::apply_fixes(&diagnostics, path)?;
    println!("Applied {} fix(es)", fixed);
    Ok(())
}

fn run_init(path: &Path) -> Result<()> {
    let config_path = path.join("cargo-perf.toml");
    if config_path.exists() {
        anyhow::bail!("cargo-perf.toml already exists");
    }
    std::fs::write(&config_path, Config::default_toml())?;
    println!("Created {}", config_path.display());
    Ok(())
}

fn run_list_rules() -> Result<()> {
    use cargo_perf::rules::registry;

    println!("Available rules:\n");
    for rule in registry::all_rules() {
        println!(
            "  {:<30} [{:?}] {}",
            rule.id(),
            rule.default_severity(),
            rule.description()
        );
    }
    Ok(())
}
