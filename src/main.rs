use anyhow::Result;
use cargo_perf::{analyze, Config};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

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

    /// Strict mode: only run high-confidence rules (async-block-in-async, lock-across-await)
    #[arg(long)]
    strict: bool,

    /// Show timing information for performance debugging
    #[arg(long)]
    timing: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Run performance analysis (default)
    Check {
        /// Path to analyze
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Strict mode: only run high-confidence rules
        #[arg(long)]
        strict: bool,

        /// Show timing information
        #[arg(long)]
        timing: bool,
    },
    /// Apply auto-fixes for detected issues
    Fix {
        /// Path to analyze and fix
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Only show what would be fixed without making changes
        #[arg(long)]
        dry_run: bool,

        /// Specific rules to apply fixes for (comma-separated)
        #[arg(long)]
        rules: Option<String>,
    },
    /// Initialize cargo-perf.toml config
    Init,
    /// List available rules
    Rules,
    /// Start LSP server for IDE integration (requires 'lsp' feature)
    #[cfg(feature = "lsp")]
    Lsp,
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
        Some(Commands::Check { path, strict, timing }) => {
            let strict_mode = strict || cli.strict;
            let show_timing = timing || cli.timing;
            run_check(
                &path,
                &config,
                cli.format,
                cli.min_severity,
                cli.fail_on,
                strict_mode,
                show_timing,
            )
        }
        None => {
            // Default to check with cli.path
            run_check(
                &cli.path,
                &config,
                cli.format,
                cli.min_severity,
                cli.fail_on,
                cli.strict,
                cli.timing,
            )
        }
        Some(Commands::Fix {
            path,
            dry_run,
            rules,
        }) => run_fix(&path, &config, dry_run, rules.as_deref()),
        Some(Commands::Init) => run_init(&cli.path),
        Some(Commands::Rules) => run_list_rules(),
        #[cfg(feature = "lsp")]
        Some(Commands::Lsp) => run_lsp(),
    }
}

#[cfg(feature = "lsp")]
fn run_lsp() -> Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(cargo_perf::lsp::run_server());
    Ok(())
}

/// High-confidence rules for strict mode
const STRICT_RULES: &[&str] = &["async-block-in-async", "lock-across-await"];

fn run_check(
    path: &Path,
    config: &Config,
    format: OutputFormat,
    min_severity: cargo_perf::Severity,
    fail_on: Option<cargo_perf::Severity>,
    strict: bool,
    show_timing: bool,
) -> Result<()> {
    let start = Instant::now();
    let diagnostics = analyze(path, config)?;
    let analysis_time = start.elapsed();

    // Filter by minimum severity and strict mode
    let diagnostics: Vec<_> = diagnostics
        .into_iter()
        .filter(|d| d.severity >= min_severity)
        .filter(|d| !strict || STRICT_RULES.contains(&d.rule_id))
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

    // Show timing information
    if show_timing {
        use colored::Colorize;
        eprintln!();
        eprintln!("{}", "Timing:".bold());
        eprintln!("  Analysis time: {:?}", analysis_time);
        eprintln!("  Diagnostics:   {}", diagnostics.len());
    }

    // Check fail condition
    if let Some(fail_severity) = fail_on {
        if diagnostics.iter().any(|d| d.severity >= fail_severity) {
            anyhow::bail!(
                "Found {} diagnostic(s) at or above {:?} severity",
                diagnostics
                    .iter()
                    .filter(|d| d.severity >= fail_severity)
                    .count(),
                fail_severity
            );
        }
    }

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

fn run_fix(path: &Path, config: &Config, dry_run: bool, rules_filter: Option<&str>) -> Result<()> {
    use cargo_perf::fix::apply_fixes;
    use colored::Colorize;

    // Run analysis
    let diagnostics = analyze(path, config)?;

    // Filter by rules if specified
    let diagnostics: Vec<_> = if let Some(filter) = rules_filter {
        let allowed_rules: Vec<&str> = filter.split(',').map(|s| s.trim()).collect();
        diagnostics
            .into_iter()
            .filter(|d| allowed_rules.contains(&d.rule_id))
            .collect()
    } else {
        diagnostics
    };

    // Count fixable diagnostics
    let fixable: Vec<_> = diagnostics.iter().filter(|d| d.fix.is_some()).collect();
    let total_fixes: usize = fixable
        .iter()
        .filter_map(|d| d.fix.as_ref())
        .map(|f| f.replacements.len())
        .sum();

    if fixable.is_empty() {
        println!(
            "{}",
            "No auto-fixes available for detected issues.".yellow()
        );
        if !diagnostics.is_empty() {
            println!(
                "\nFound {} issue(s), but none have auto-fix support yet.",
                diagnostics.len()
            );
        }
        return Ok(());
    }

    println!(
        "Found {} fixable issue(s) with {} replacement(s):\n",
        fixable.len(),
        total_fixes
    );

    // Show what will be fixed
    for diagnostic in &fixable {
        if let Some(fix) = &diagnostic.fix {
            println!(
                "  {} {}:{} - {}",
                diagnostic.rule_id.cyan(),
                diagnostic.file_path.display(),
                diagnostic.line,
                fix.description
            );
        }
    }

    if dry_run {
        println!("\n{}", "Dry run - no changes made.".yellow());
        return Ok(());
    }

    // Apply fixes
    let base_dir = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    match apply_fixes(&diagnostics, &base_dir) {
        Ok(count) => {
            println!("\n{}", format!("Applied {} fix(es).", count).green());
            println!("Run `cargo perf check` to verify remaining issues.");
        }
        Err(e) => {
            anyhow::bail!("Failed to apply fixes: {}", e);
        }
    }

    Ok(())
}
