//! Integration tests for cargo-perf
//!
//! Tests the public API and CLI behavior.

use cargo_perf::{analyze, Config, Severity};
use std::path::Path;

/// Test that analysis finds expected issues in fixture file
#[test]
fn test_analyze_fixture_file() {
    let config = Config::default();
    let path = Path::new("tests/fixtures/bad_code.rs");

    let diagnostics = analyze(path, &config).expect("Analysis should succeed");

    // Should find multiple issues
    assert!(!diagnostics.is_empty(), "Should find issues in bad_code.rs");

    // Verify specific rules triggered
    let rule_ids: Vec<&str> = diagnostics.iter().map(|d| d.rule_id).collect();

    assert!(
        rule_ids.contains(&"async-block-in-async"),
        "Should detect blocking calls in async: {:?}",
        rule_ids
    );
    assert!(
        rule_ids.contains(&"clone-in-hot-loop"),
        "Should detect clone in loop: {:?}",
        rule_ids
    );
    assert!(
        rule_ids.contains(&"collect-then-iterate"),
        "Should detect collect-then-iterate: {:?}",
        rule_ids
    );
}

/// Test that analysis returns empty for clean code
#[test]
fn test_analyze_clean_code() {
    let config = Config::default();
    let source = r#"
        fn good_function() {
            let x = 1 + 2;
            println!("{}", x);
        }
    "#;

    // Write to temp file
    let temp_dir = tempfile::tempdir().expect("Create temp dir");
    let file_path = temp_dir.path().join("clean.rs");
    std::fs::write(&file_path, source).expect("Write temp file");

    let diagnostics = analyze(&file_path, &config).expect("Analysis should succeed");

    assert!(
        diagnostics.is_empty(),
        "Clean code should have no issues: {:?}",
        diagnostics
    );
}

/// Test severity filtering
#[test]
fn test_severity_levels() {
    let config = Config::default();
    let path = Path::new("tests/fixtures/bad_code.rs");

    let diagnostics = analyze(path, &config).expect("Analysis should succeed");

    // Check severity distribution
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    let warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect();

    // async-block-in-async should be Error
    assert!(
        errors.iter().any(|d| d.rule_id == "async-block-in-async"),
        "async-block-in-async should be Error severity"
    );

    // clone-in-hot-loop should be Warning
    assert!(
        warnings.iter().any(|d| d.rule_id == "clone-in-hot-loop"),
        "clone-in-hot-loop should be Warning severity"
    );
}

/// Test that diagnostics include location info
#[test]
fn test_diagnostic_locations() {
    let config = Config::default();
    let path = Path::new("tests/fixtures/bad_code.rs");

    let diagnostics = analyze(path, &config).expect("Analysis should succeed");

    for diag in &diagnostics {
        assert!(diag.line > 0, "Line number should be positive");
        // Column is usize, so always >= 0; just verify it's reasonable
        assert!(diag.column < 10000, "Column should be reasonable");
        assert!(
            diag.file_path.ends_with("bad_code.rs"),
            "File path should be correct"
        );
    }
}

/// Test suppression via comment
#[test]
fn test_comment_suppression() {
    let config = Config::default();
    let source = r#"
        fn test() {
            let data = vec!["a".to_string()];
            for item in &data {
                // cargo-perf-ignore: clone-in-hot-loop
                let owned = item.clone();
                println!("{}", owned);
            }
        }
    "#;

    let temp_dir = tempfile::tempdir().expect("Create temp dir");
    let file_path = temp_dir.path().join("suppressed.rs");
    std::fs::write(&file_path, source).expect("Write temp file");

    let diagnostics = analyze(&file_path, &config).expect("Analysis should succeed");

    let clone_issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.rule_id == "clone-in-hot-loop")
        .collect();

    assert!(
        clone_issues.is_empty(),
        "Suppressed clone should not be reported: {:?}",
        clone_issues
    );
}

/// Test suppression via attribute
#[test]
fn test_attribute_suppression() {
    let config = Config::default();
    let source = r#"
        #[allow(cargo_perf::clone_in_hot_loop)]
        fn test() {
            let data = vec!["a".to_string()];
            for item in &data {
                let owned = item.clone();
                println!("{}", owned);
            }
        }
    "#;

    let temp_dir = tempfile::tempdir().expect("Create temp dir");
    let file_path = temp_dir.path().join("attr_suppressed.rs");
    std::fs::write(&file_path, source).expect("Write temp file");

    let diagnostics = analyze(&file_path, &config).expect("Analysis should succeed");

    let clone_issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.rule_id == "clone-in-hot-loop")
        .collect();

    assert!(
        clone_issues.is_empty(),
        "Attribute-suppressed clone should not be reported: {:?}",
        clone_issues
    );
}

/// Test directory analysis finds files recursively
#[test]
fn test_directory_analysis() {
    let temp_dir = tempfile::tempdir().expect("Create temp dir");

    // Create a non-hidden project directory inside temp
    // (tempfile may create dirs starting with . which are excluded)
    let project_dir = temp_dir.path().join("project");
    std::fs::create_dir(&project_dir).expect("Create project dir");

    // Create nested structure
    let sub_dir = project_dir.join("src");
    std::fs::create_dir(&sub_dir).expect("Create subdir");

    let file1 = project_dir.join("root.rs");
    let file2 = sub_dir.join("module.rs");

    // Use patterns that reliably trigger
    std::fs::write(
        &file1,
        r#"
fn bad_root(data: &[String]) {
    for s in data {
        let _ = s.clone();
    }
}
"#,
    )
    .expect("Write file1");

    std::fs::write(
        &file2,
        r#"
fn bad_module(items: &[String]) {
    for item in items {
        let _ = item.clone();
    }
}
"#,
    )
    .expect("Write file2");

    let config = Config::default();
    let diagnostics = analyze(&project_dir, &config).expect("Analysis should succeed");

    // Should find issues in both files
    let files: std::collections::HashSet<_> = diagnostics
        .iter()
        .map(|d| d.file_path.file_name().unwrap().to_str().unwrap())
        .collect();

    assert!(
        files.contains("root.rs"),
        "Should analyze root.rs, found: {:?}",
        files
    );
    assert!(
        files.contains("module.rs"),
        "Should analyze module.rs, found: {:?}",
        files
    );
}

/// Test config can disable rules via Allow
#[test]
fn test_config_disables_rule() {
    use cargo_perf::config::RuleSeverity;
    use std::collections::HashMap;

    let mut rules = HashMap::new();
    rules.insert("clone-in-hot-loop".to_string(), RuleSeverity::Allow);

    let config = Config {
        rules,
        ..Config::default()
    };

    let source = r#"
fn test(data: &[String]) {
    for s in data {
        let _ = s.clone();
    }
}
"#;

    let temp_dir = tempfile::tempdir().expect("Create temp dir");
    let file_path = temp_dir.path().join("test.rs");
    std::fs::write(&file_path, source).expect("Write file");

    let diagnostics = analyze(&file_path, &config).expect("Analysis should succeed");

    let clone_issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.rule_id == "clone-in-hot-loop")
        .collect();

    assert!(
        clone_issues.is_empty(),
        "Rule with Allow should be disabled: {:?}",
        clone_issues
    );
}

/// Test lock-across-await detection
#[test]
fn test_lock_across_await() {
    let config = Config::default();
    let source = r#"
        async fn bad(mutex: &tokio::sync::Mutex<i32>) {
            let guard = mutex.lock().await;
            some_async_fn().await;  // guard still held!
        }
        async fn some_async_fn() {}
    "#;

    let temp_dir = tempfile::tempdir().expect("Create temp dir");
    let file_path = temp_dir.path().join("lock_await.rs");
    std::fs::write(&file_path, source).expect("Write file");

    let diagnostics = analyze(&file_path, &config).expect("Analysis should succeed");

    assert!(
        diagnostics.iter().any(|d| d.rule_id == "lock-across-await"),
        "Should detect lock held across await: {:?}",
        diagnostics
    );
}

/// Test non-Rust files are skipped
#[test]
fn test_skips_non_rust_files() {
    let temp_dir = tempfile::tempdir().expect("Create temp dir");

    // Create a .txt file with Rust-like content
    let txt_file = temp_dir.path().join("fake.txt");
    std::fs::write(
        &txt_file,
        r#"
        async fn bad() {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    "#,
    )
    .expect("Write txt file");

    let config = Config::default();
    let diagnostics = analyze(temp_dir.path(), &config).expect("Analysis should succeed");

    assert!(
        diagnostics.is_empty(),
        "Should skip non-.rs files: {:?}",
        diagnostics
    );
}
