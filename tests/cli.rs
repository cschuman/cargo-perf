//! CLI integration tests for cargo-perf binary.
//!
//! Tests the command-line interface behavior.

use assert_cmd::cargo::cargo_bin_cmd;
use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Get a Command for the cargo-perf binary.
fn cargo_perf() -> Command {
    cargo_bin_cmd!("cargo-perf")
}

#[test]
fn test_help_flag() {
    cargo_perf()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Preventive performance analysis"));
}

#[test]
fn test_version_flag() {
    cargo_perf()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("cargo-perf"));
}

#[test]
fn test_rules_subcommand() {
    cargo_perf()
        .arg("rules")
        .assert()
        .success()
        .stdout(predicate::str::contains("async-block-in-async"))
        .stdout(predicate::str::contains("lock-across-await"))
        .stdout(predicate::str::contains("clone-in-hot-loop"))
        .stdout(predicate::str::contains("n-plus-one-query"));
}

#[test]
fn test_explain_known_rule() {
    cargo_perf()
        .arg("explain")
        .arg("async-block-in-async")
        .assert()
        .success()
        .stdout(predicate::str::contains("Why it matters"))
        .stdout(predicate::str::contains("Blocking calls in async"));
}

#[test]
fn test_explain_unknown_rule() {
    cargo_perf()
        .arg("explain")
        .arg("nonexistent-rule")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown rule"));
}

#[test]
fn test_init_creates_config() {
    let temp = TempDir::new().unwrap();

    cargo_perf()
        .arg("init")
        .current_dir(temp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Created"));

    assert!(temp.path().join("cargo-perf.toml").exists());
}

#[test]
fn test_init_fails_if_exists() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("cargo-perf.toml"), "").unwrap();

    cargo_perf()
        .arg("init")
        .current_dir(temp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn test_check_clean_code() {
    let temp = TempDir::new().unwrap();
    fs::write(
        temp.path().join("clean.rs"),
        r#"
fn main() {
    let x = 1 + 2;
    println!("{}", x);
}
"#,
    )
    .unwrap();

    cargo_perf()
        .arg("check")
        .arg(temp.path())
        .assert()
        .success();
}

#[test]
fn test_check_finds_issues() {
    let temp = TempDir::new().unwrap();
    fs::write(
        temp.path().join("bad.rs"),
        r#"
async fn bad() {
    std::thread::sleep(std::time::Duration::from_secs(1));
}
"#,
    )
    .unwrap();

    cargo_perf()
        .arg("check")
        .arg(temp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("async-block-in-async"));
}

#[test]
fn test_check_json_output() {
    let temp = TempDir::new().unwrap();
    fs::write(
        temp.path().join("bad.rs"),
        r#"
async fn bad() {
    std::thread::sleep(std::time::Duration::from_secs(1));
}
"#,
    )
    .unwrap();

    // --format is a global option, must come before subcommand
    cargo_perf()
        .arg("--format")
        .arg("json")
        .arg("--path")
        .arg(temp.path())
        .assert()
        .success()
        // JSON output is pretty-printed
        .stdout(predicate::str::contains(
            r#""rule_id": "async-block-in-async""#,
        ));
}

#[test]
fn test_check_sarif_output() {
    let temp = TempDir::new().unwrap();
    fs::write(
        temp.path().join("bad.rs"),
        r#"
async fn bad() {
    std::thread::sleep(std::time::Duration::from_secs(1));
}
"#,
    )
    .unwrap();

    // --format is a global option, must come before subcommand
    cargo_perf()
        .arg("--format")
        .arg("sarif")
        .arg("--path")
        .arg(temp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("$schema"))
        .stdout(predicate::str::contains("sarif-schema"))
        .stdout(predicate::str::contains("ruleId"));
}

#[test]
fn test_check_strict_mode() {
    let temp = TempDir::new().unwrap();
    // Clone in loop is NOT a strict rule
    fs::write(
        temp.path().join("code.rs"),
        r#"
fn test(data: &[String]) {
    for s in data {
        let _ = s.clone();
    }
}
"#,
    )
    .unwrap();

    // Without --strict, should report clone-in-hot-loop
    cargo_perf()
        .arg("check")
        .arg(temp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("clone-in-hot-loop"));

    // With --strict, should NOT report clone-in-hot-loop
    cargo_perf()
        .arg("check")
        .arg(temp.path())
        .arg("--strict")
        .assert()
        .success()
        .stdout(predicate::str::contains("clone-in-hot-loop").not());
}

#[test]
fn test_check_timing_flag() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("code.rs"), "fn main() {}").unwrap();

    cargo_perf()
        .arg("check")
        .arg(temp.path())
        .arg("--timing")
        .assert()
        .success()
        .stderr(predicate::str::contains("Analysis time"));
}

#[test]
fn test_check_fail_on_error() {
    let temp = TempDir::new().unwrap();
    fs::write(
        temp.path().join("bad.rs"),
        r#"
async fn bad() {
    std::thread::sleep(std::time::Duration::from_secs(1));
}
"#,
    )
    .unwrap();

    // async-block-in-async is Error severity, so --fail-on=error should fail
    // --fail-on is a global option
    cargo_perf()
        .arg("--fail-on")
        .arg("error")
        .arg("--path")
        .arg(temp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("diagnostic(s) at or above"));
}

#[test]
fn test_check_nonexistent_path() {
    // Using --path with nonexistent path should fail during config load
    cargo_perf()
        .arg("--path")
        .arg("/nonexistent/path/to/project")
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not exist"));
}

#[test]
fn test_baseline_creates_file() {
    let temp = TempDir::new().unwrap();
    fs::write(
        temp.path().join("code.rs"),
        r#"
async fn bad() {
    std::thread::sleep(std::time::Duration::from_secs(1));
}
"#,
    )
    .unwrap();

    // Create baseline
    cargo_perf()
        .arg("baseline")
        .arg(temp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Baseline created"));

    // Baseline file should exist
    assert!(temp.path().join(".cargo-perf-baseline").exists());

    // Read the baseline file and verify it contains our rule
    let baseline_content = fs::read_to_string(temp.path().join(".cargo-perf-baseline")).unwrap();
    assert!(baseline_content.contains("async-block-in-async"));
}

#[test]
fn test_check_with_baseline_flag() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("code.rs"), "fn main() {}").unwrap();

    // Check with --baseline when no baseline file exists should warn
    cargo_perf()
        .arg("check")
        .arg(temp.path())
        .arg("--baseline")
        .assert()
        .success()
        .stderr(predicate::str::contains("No baseline file found"));
}

#[test]
fn test_fix_dry_run() {
    let temp = TempDir::new().unwrap();
    let code = r#"
fn test() {
    let items: Vec<i32> = vec![1, 2, 3];
    let _ = items.iter().collect::<Vec<_>>().iter().count();
}
"#;
    fs::write(temp.path().join("fix.rs"), code).unwrap();

    cargo_perf()
        .arg("fix")
        .arg(temp.path())
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry run"));

    // File should be unchanged
    let after = fs::read_to_string(temp.path().join("fix.rs")).unwrap();
    assert_eq!(code, after);
}

#[test]
fn test_default_command_is_check() {
    let temp = TempDir::new().unwrap();
    fs::write(
        temp.path().join("bad.rs"),
        r#"
async fn bad() {
    std::thread::sleep(std::time::Duration::from_secs(1));
}
"#,
    )
    .unwrap();

    // Running without subcommand should do check
    cargo_perf()
        .arg("--path")
        .arg(temp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("async-block-in-async"));
}

// Note: The "cargo perf" invocation handling is tested via actual cargo
// invocation, not by passing "perf" as first arg to the binary directly.
// The re-parsing logic in main.rs handles args from cargo's invocation path.
