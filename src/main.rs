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
    /// Explain a specific rule in detail
    Explain {
        /// Rule ID to explain (e.g., "regex-in-loop")
        rule_id: String,
    },
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
        Some(Commands::Explain { rule_id }) => run_explain(&rule_id),
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

    // Offer to create .taplo.toml for IDE schema support
    let taplo_path = path.join(".taplo.toml");
    if !taplo_path.exists() {
        let taplo_config = r#"# Taplo configuration for cargo-perf.toml schema validation
# Provides autocompletion in VS Code (Even Better TOML) and Neovim

[[rule]]
include = ["cargo-perf.toml"]

[rule.schema]
url = "https://raw.githubusercontent.com/cschuman/cargo-perf/main/cargo-perf.schema.json"
"#;
        std::fs::write(&taplo_path, taplo_config)?;
        println!("Created {} (IDE schema support)", taplo_path.display());
    }

    println!("\nTip: Install 'Even Better TOML' (VS Code) or 'taplo' for autocompletion.");
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
    println!("\nUse `cargo perf explain <rule-id>` for detailed information.");
    Ok(())
}

fn run_explain(rule_id: &str) -> Result<()> {
    use cargo_perf::rules::registry;
    use colored::Colorize;

    // Find the rule
    let rule = registry::all_rules()
        .into_iter()
        .find(|r| r.id() == rule_id);

    let rule = match rule {
        Some(r) => r,
        None => {
            eprintln!("{} Unknown rule: {}", "error:".red().bold(), rule_id);
            eprintln!("\nAvailable rules:");
            for r in registry::all_rules() {
                eprintln!("  {}", r.id());
            }
            anyhow::bail!("Unknown rule: {}", rule_id);
        }
    };

    // Print header
    println!("{}", rule.name().bold().underline());
    println!("Rule ID: {}", rule.id().cyan());
    println!("Severity: {:?}", rule.default_severity());
    println!();
    println!("{}", rule.description());
    println!();

    // Print detailed explanation based on rule ID
    print_rule_explanation(rule.id());

    Ok(())
}

fn print_rule_explanation(rule_id: &str) {
    use colored::Colorize;

    match rule_id {
        "async-block-in-async" => {
            println!("{}", "Why it matters:".yellow().bold());
            println!("  Blocking calls in async functions block the entire async runtime thread.");
            println!("  This can cause all other async tasks to stall, destroying concurrency.");
            println!();
            println!("{}", "Bad:".red().bold());
            println!("  async fn fetch_data() {{");
            println!("      let data = std::fs::read_to_string(\"file.txt\"); // BLOCKS!");
            println!("      std::thread::sleep(Duration::from_secs(1));      // BLOCKS!");
            println!("  }}");
            println!();
            println!("{}", "Good:".green().bold());
            println!("  async fn fetch_data() {{");
            println!("      let data = tokio::fs::read_to_string(\"file.txt\").await;");
            println!("      tokio::time::sleep(Duration::from_secs(1)).await;");
            println!("  }}");
            println!();
            println!("{}", "Performance impact:".yellow().bold());
            println!("  Can reduce async throughput by 10-100x depending on workload.");
        }

        "lock-across-await" => {
            println!("{}", "Why it matters:".yellow().bold());
            println!("  Holding a MutexGuard across an .await point can cause deadlocks.");
            println!("  The task may be suspended while holding the lock, blocking other tasks.");
            println!();
            println!("{}", "Bad:".red().bold());
            println!("  async fn update() {{");
            println!("      let guard = mutex.lock().unwrap();");
            println!("      do_async_work().await;  // DEADLOCK RISK!");
            println!("      *guard = new_value;");
            println!("  }}");
            println!();
            println!("{}", "Good:".green().bold());
            println!("  async fn update() {{");
            println!("      {{");
            println!("          let guard = mutex.lock().unwrap();");
            println!("          *guard = new_value;");
            println!("      }} // guard dropped before await");
            println!("      do_async_work().await;");
            println!("  }}");
            println!();
            println!("{}", "Performance impact:".yellow().bold());
            println!("  Can cause complete system hangs in production.");
        }

        "n-plus-one-query" => {
            println!("{}", "Why it matters:".yellow().bold());
            println!("  Executing database queries inside loops causes N+1 query problems.");
            println!("  For N items, you execute N+1 queries instead of 1-2 batch queries.");
            println!();
            println!("{}", "Bad:".red().bold());
            println!("  for user_id in user_ids {{");
            println!("      let user = sqlx::query!(\"SELECT * FROM users WHERE id = ?\", user_id)");
            println!("          .fetch_one(&pool).await?;");
            println!("  }}");
            println!();
            println!("{}", "Good:".green().bold());
            println!("  let users = sqlx::query!(\"SELECT * FROM users WHERE id IN (?))\", &user_ids)");
            println!("      .fetch_all(&pool).await?;");
            println!();
            println!("{}", "Performance impact:".yellow().bold());
            println!("  100 items = 101 queries vs 1 query. Can be 50-100x slower.");
        }

        "regex-in-loop" => {
            println!("{}", "Why it matters:".yellow().bold());
            println!("  Regex::new() compiles the regex pattern on every call.");
            println!("  Compilation is expensive and should be done once.");
            println!();
            println!("{}", "Bad:".red().bold());
            println!("  for line in lines {{");
            println!("      let re = Regex::new(r\"\\d+\").unwrap();");
            println!("      if re.is_match(line) {{ ... }}");
            println!("  }}");
            println!();
            println!("{}", "Good:".green().bold());
            println!("  static RE: LazyLock<Regex> = LazyLock::new(|| {{");
            println!("      Regex::new(r\"\\d+\").unwrap()");
            println!("  }});");
            println!("  for line in lines {{");
            println!("      if RE.is_match(line) {{ ... }}");
            println!("  }}");
            println!();
            println!("{}", "Performance impact:".yellow().bold());
            println!("  Benchmark: {} faster with pre-compiled regex.", "737x".green().bold());
        }

        "clone-in-hot-loop" => {
            println!("{}", "Why it matters:".yellow().bold());
            println!("  Cloning heap-allocated types (String, Vec, etc.) in loops");
            println!("  causes repeated memory allocations and copies.");
            println!();
            println!("{}", "Bad:".red().bold());
            println!("  for item in items {{");
            println!("      let owned = expensive_string.clone();");
            println!("      process(owned);");
            println!("  }}");
            println!();
            println!("{}", "Good:".green().bold());
            println!("  for item in items {{");
            println!("      process(&expensive_string);  // borrow instead");
            println!("  }}");
            println!("  // Or clone once before the loop if ownership needed");
            println!();
            println!("{}", "Performance impact:".yellow().bold());
            println!("  Benchmark: {} faster avoiding clone in loop.", "48x".green().bold());
        }

        "collect-then-iterate" => {
            println!("{}", "Why it matters:".yellow().bold());
            println!("  Calling .collect() followed by .iter() creates an unnecessary");
            println!("  intermediate collection. Continue the iterator chain instead.");
            println!();
            println!("{}", "Bad:".red().bold());
            println!("  items.iter()");
            println!("      .filter(|x| x.is_valid())");
            println!("      .collect::<Vec<_>>()");
            println!("      .iter()  // unnecessary!");
            println!("      .map(|x| x.process())");
            println!();
            println!("{}", "Good:".green().bold());
            println!("  items.iter()");
            println!("      .filter(|x| x.is_valid())");
            println!("      .map(|x| x.process())");
            println!();
            println!("{}", "Performance impact:".yellow().bold());
            println!("  Benchmark: {} faster without intermediate collection.", "2.3x".green().bold());
            println!();
            println!("{}", "Auto-fix available:".cyan().bold());
            println!("  This rule supports automatic fixing via `cargo perf fix`.");
        }

        "vec-no-capacity" => {
            println!("{}", "Why it matters:".yellow().bold());
            println!("  Vec::new() starts with zero capacity. Each push beyond capacity");
            println!("  triggers reallocation. Pre-allocating avoids repeated allocations.");
            println!();
            println!("{}", "Bad:".red().bold());
            println!("  let mut results = Vec::new();");
            println!("  for i in 0..1000 {{");
            println!("      results.push(compute(i));");
            println!("  }}");
            println!();
            println!("{}", "Good:".green().bold());
            println!("  let mut results = Vec::with_capacity(1000);");
            println!("  for i in 0..1000 {{");
            println!("      results.push(compute(i));");
            println!("  }}");
            println!();
            println!("{}", "Performance impact:".yellow().bold());
            println!("  Benchmark: {} faster with pre-allocated capacity.", "1.8x".green().bold());
        }

        "format-in-loop" => {
            println!("{}", "Why it matters:".yellow().bold());
            println!("  format!() allocates a new String on every call.");
            println!("  In loops, this causes repeated heap allocations.");
            println!();
            println!("{}", "Bad:".red().bold());
            println!("  for item in items {{");
            println!("      let msg = format!(\"Processing: {{}}\", item);");
            println!("      log(msg);");
            println!("  }}");
            println!();
            println!("{}", "Good:".green().bold());
            println!("  let mut buf = String::new();");
            println!("  for item in items {{");
            println!("      buf.clear();");
            println!("      write!(&mut buf, \"Processing: {{}}\", item)?;");
            println!("      log(&buf);");
            println!("  }}");
        }

        "string-concat-loop" => {
            println!("{}", "Why it matters:".yellow().bold());
            println!("  The + operator on Strings allocates a new String each time.");
            println!("  Use push_str() to append in place without allocation.");
            println!();
            println!("{}", "Bad:".red().bold());
            println!("  let mut result = String::new();");
            println!("  for word in words {{");
            println!("      result = result + word;  // allocates each time!");
            println!("  }}");
            println!();
            println!("{}", "Good:".green().bold());
            println!("  let mut result = String::new();");
            println!("  for word in words {{");
            println!("      result.push_str(word);  // appends in place");
            println!("  }}");
            println!();
            println!("{}", "Auto-fix available:".cyan().bold());
            println!("  This rule supports automatic fixing via `cargo perf fix`.");
        }

        "mutex-in-loop" => {
            println!("{}", "Why it matters:".yellow().bold());
            println!("  Acquiring a lock inside a loop causes repeated lock/unlock overhead.");
            println!("  Acquire once before the loop when possible.");
            println!();
            println!("{}", "Bad:".red().bold());
            println!("  for item in items {{");
            println!("      let mut guard = data.lock().unwrap();");
            println!("      guard.push(item);");
            println!("  }}");
            println!();
            println!("{}", "Good:".green().bold());
            println!("  let mut guard = data.lock().unwrap();");
            println!("  for item in items {{");
            println!("      guard.push(item);");
            println!("  }}");
        }

        "unbounded-channel" => {
            println!("{}", "Why it matters:".yellow().bold());
            println!("  Unbounded channels can grow without limit, exhausting memory");
            println!("  if producers outpace consumers.");
            println!();
            println!("{}", "Bad:".red().bold());
            println!("  let (tx, rx) = std::sync::mpsc::channel();  // unbounded!");
            println!("  let (tx, rx) = tokio::sync::mpsc::unbounded_channel();");
            println!();
            println!("{}", "Good:".green().bold());
            println!("  let (tx, rx) = std::sync::mpsc::sync_channel(100);  // bounded");
            println!("  let (tx, rx) = tokio::sync::mpsc::channel(100);");
            println!();
            println!("{}", "Performance impact:".yellow().bold());
            println!("  Prevents OOM crashes under load.");
        }

        "unbounded-spawn" => {
            println!("{}", "Why it matters:".yellow().bold());
            println!("  Spawning tasks in a loop without limits can exhaust memory");
            println!("  and overwhelm the runtime with too many concurrent tasks.");
            println!();
            println!("{}", "Bad:".red().bold());
            println!("  for url in urls {{");
            println!("      tokio::spawn(fetch(url));  // thousands of concurrent tasks!");
            println!("  }}");
            println!();
            println!("{}", "Good:".green().bold());
            println!("  use futures::stream::StreamExt;");
            println!("  futures::stream::iter(urls)");
            println!("      .map(|url| fetch(url))");
            println!("      .buffer_unordered(10)  // max 10 concurrent");
            println!("      .collect::<Vec<_>>().await;");
        }

        _ => {
            println!("No detailed explanation available for this rule.");
            println!("Run `cargo perf rules` to see all available rules.");
        }
    }

    println!();
    println!("{}", "Suppression:".yellow().bold());
    println!("  // cargo-perf-ignore: {}", rule_id);
    println!("  #[allow(cargo_perf::{})]", rule_id);
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
