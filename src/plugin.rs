//! Plugin system for extending cargo-perf with custom rules.
//!
//! This module provides the infrastructure for creating custom performance rules
//! that can be integrated with cargo-perf.
//!
//! # Creating a Custom Rule
//!
//! To create a custom rule, implement the [`Rule`] trait:
//!
//! ```rust,ignore
//! use cargo_perf::rules::{Rule, Diagnostic, Severity};
//! use cargo_perf::engine::AnalysisContext;
//!
//! pub struct MyCustomRule;
//!
//! impl Rule for MyCustomRule {
//!     fn id(&self) -> &'static str { "my-custom-rule" }
//!     fn name(&self) -> &'static str { "My Custom Rule" }
//!     fn description(&self) -> &'static str { "Detects my custom anti-pattern" }
//!     fn default_severity(&self) -> Severity { Severity::Warning }
//!
//!     fn check(&self, ctx: &AnalysisContext) -> Vec<Diagnostic> {
//!         // Your detection logic here
//!         Vec::new()
//!     }
//! }
//! ```
//!
//! # Creating a Custom Binary
//!
//! To use custom rules, create a new binary that combines built-in and custom rules:
//!
//! ```rust,ignore
//! use cargo_perf::plugin::{PluginRegistry, run_with_plugins};
//!
//! fn main() {
//!     let mut registry = PluginRegistry::new();
//!
//!     // Add all built-in rules
//!     registry.add_builtin_rules();
//!
//!     // Add your custom rules
//!     registry.add_rule(Box::new(MyCustomRule));
//!     registry.add_rule(Box::new(AnotherCustomRule));
//!
//!     // Run with the combined rule set
//!     run_with_plugins(registry);
//! }
//! ```
//!
//! # Configuration
//!
//! Custom rules can be configured in `cargo-perf.toml` just like built-in rules:
//!
//! ```toml
//! [rules]
//! my-custom-rule = "warn"
//! another-custom-rule = "deny"
//! ```

use crate::discovery::{discover_rust_files, DiscoveryOptions, MAX_FILE_SIZE};
use crate::engine::AnalysisContext;
use crate::rules::{Diagnostic, Rule};
use crate::suppression::SuppressionExtractor;
use crate::Config;
use rayon::prelude::*;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;

/// A registry for managing both built-in and custom rules.
///
/// # Example
///
/// ```rust,ignore
/// use cargo_perf::plugin::PluginRegistry;
///
/// let mut registry = PluginRegistry::new();
/// registry.add_builtin_rules();
/// registry.add_rule(Box::new(MyCustomRule));
/// ```
pub struct PluginRegistry {
    rules: Vec<Box<dyn Rule>>,
    rule_index: HashMap<String, usize>,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginRegistry {
    /// Create an empty plugin registry.
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            rule_index: HashMap::new(),
        }
    }

    /// Add all built-in rules to the registry.
    ///
    /// This creates new instances of each built-in rule and adds them to the registry.
    pub fn add_builtin_rules(&mut self) {
        use crate::rules::allocation_rules::{
            FormatInLoopRule, MutexLockInLoopRule, StringConcatLoopRule, VecNoCapacityRule,
        };
        use crate::rules::async_rules::{
            AsyncBlockInAsyncRule, UnboundedChannelRule, UnboundedSpawnRule,
        };
        use crate::rules::database_rules::NPlusOneQueryRule;
        use crate::rules::iter_rules::CollectThenIterateRule;
        use crate::rules::lock_across_await::LockAcrossAwaitRule;
        use crate::rules::memory_rules::{CloneInLoopRule, RegexInLoopRule};

        // Create new instances of each built-in rule
        let builtin_rules: Vec<Box<dyn Rule>> = vec![
            Box::new(AsyncBlockInAsyncRule),
            Box::new(LockAcrossAwaitRule),
            Box::new(UnboundedChannelRule),
            Box::new(UnboundedSpawnRule),
            Box::new(NPlusOneQueryRule),
            Box::new(CloneInLoopRule),
            Box::new(RegexInLoopRule),
            Box::new(CollectThenIterateRule),
            Box::new(VecNoCapacityRule),
            Box::new(FormatInLoopRule),
            Box::new(StringConcatLoopRule),
            Box::new(MutexLockInLoopRule),
        ];

        for rule in builtin_rules {
            let id = rule.id().to_string();
            if let Entry::Vacant(entry) = self.rule_index.entry(id) {
                let idx = self.rules.len();
                entry.insert(idx);
                self.rules.push(rule);
            }
        }
    }

    /// Add a custom rule to the registry.
    ///
    /// # Panics
    ///
    /// Panics if a rule with the same ID already exists. Use [`try_add_rule`] for
    /// a non-panicking version or [`add_or_replace_rule`] to replace existing rules.
    ///
    /// [`try_add_rule`]: Self::try_add_rule
    /// [`add_or_replace_rule`]: Self::add_or_replace_rule
    pub fn add_rule(&mut self, rule: Box<dyn Rule>) {
        let id = rule.id().to_string();
        if self.try_add_rule(rule).is_err() {
            panic!("Rule with ID '{}' already exists", id);
        }
    }

    /// Try to add a custom rule to the registry.
    ///
    /// Returns an error if a rule with the same ID already exists.
    /// Use [`add_or_replace_rule`] to replace existing rules without error.
    ///
    /// [`add_or_replace_rule`]: Self::add_or_replace_rule
    ///
    /// # Errors
    ///
    /// Returns `Err` with the rejected rule if a rule with the same ID already exists.
    pub fn try_add_rule(&mut self, rule: Box<dyn Rule>) -> Result<(), Box<dyn Rule>> {
        let id = rule.id().to_string();
        if self.rule_index.contains_key(&id) {
            return Err(rule);
        }
        let idx = self.rules.len();
        self.rule_index.insert(id, idx);
        self.rules.push(rule);
        Ok(())
    }

    /// Add a custom rule, replacing any existing rule with the same ID.
    pub fn add_or_replace_rule(&mut self, rule: Box<dyn Rule>) {
        let id = rule.id().to_string();
        if let Some(&idx) = self.rule_index.get(&id) {
            self.rules[idx] = rule;
        } else {
            let idx = self.rules.len();
            self.rule_index.insert(id, idx);
            self.rules.push(rule);
        }
    }

    /// Get all registered rules.
    pub fn rules(&self) -> &[Box<dyn Rule>] {
        &self.rules
    }

    /// Get a rule by its ID.
    pub fn get_rule(&self, id: &str) -> Option<&dyn Rule> {
        self.rule_index.get(id).map(|&idx| self.rules[idx].as_ref())
    }

    /// Check if a rule with the given ID exists.
    pub fn has_rule(&self, id: &str) -> bool {
        self.rule_index.contains_key(id)
    }

    /// Get all rule IDs.
    pub fn rule_ids(&self) -> impl Iterator<Item = &str> {
        self.rule_index.keys().map(|s| s.as_str())
    }

    /// Run all rules on the given analysis context.
    pub fn check_all(&self, ctx: &AnalysisContext) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for rule in &self.rules {
            diagnostics.extend(rule.check(ctx));
        }
        diagnostics
    }

    /// Run specific rules on the given analysis context.
    pub fn check_rules(&self, ctx: &AnalysisContext, rule_ids: &[&str]) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for id in rule_ids {
            if let Some(rule) = self.get_rule(id) {
                diagnostics.extend(rule.check(ctx));
            }
        }
        diagnostics
    }
}

/// Builder pattern for creating a plugin registry with a fluent API.
///
/// # Example
///
/// ```rust,ignore
/// use cargo_perf::plugin::PluginRegistryBuilder;
///
/// let registry = PluginRegistryBuilder::new()
///     .with_builtin_rules()
///     .with_rule(Box::new(MyCustomRule))
///     .with_rule(Box::new(AnotherRule))
///     .build();
/// ```
pub struct PluginRegistryBuilder {
    registry: PluginRegistry,
}

impl Default for PluginRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginRegistryBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            registry: PluginRegistry::new(),
        }
    }

    /// Add all built-in rules.
    pub fn with_builtin_rules(mut self) -> Self {
        self.registry.add_builtin_rules();
        self
    }

    /// Add a custom rule.
    pub fn with_rule(mut self, rule: Box<dyn Rule>) -> Self {
        self.registry.add_rule(rule);
        self
    }

    /// Build the registry.
    pub fn build(self) -> PluginRegistry {
        self.registry
    }
}

/// Analyze a path using a custom plugin registry.
///
/// This is similar to [`crate::analyze`] but uses the provided registry
/// instead of the built-in rules.
///
/// # Example
///
/// ```rust,ignore
/// use cargo_perf::plugin::{PluginRegistry, analyze_with_plugins};
/// use cargo_perf::Config;
/// use std::path::Path;
///
/// let mut registry = PluginRegistry::new();
/// registry.add_builtin_rules();
/// registry.add_rule(Box::new(MyCustomRule));
///
/// let config = Config::default();
/// let diagnostics = analyze_with_plugins(Path::new("."), &config, &registry)?;
/// ```
pub fn analyze_with_plugins(
    path: &Path,
    config: &Config,
    registry: &PluginRegistry,
) -> Result<Vec<Diagnostic>, crate::error::Error> {
    // Use secure discovery (same as Engine) to prevent symlink attacks
    let files = discover_rust_files(path, &DiscoveryOptions::secure());

    // Track errors but don't fail the entire analysis
    let errors: Mutex<Vec<(std::path::PathBuf, String)>> = Mutex::new(Vec::new());

    // Analyze files in parallel (same as Engine)
    let all_diagnostics: Vec<Diagnostic> = files
        .par_iter()
        .flat_map(|file_path| {
            match analyze_single_file_with_registry(file_path, config, registry) {
                Ok(diagnostics) => diagnostics,
                Err(e) => {
                    // Log errors but continue analyzing other files
                    if let Ok(mut errs) = errors.lock() {
                        errs.push((file_path.clone(), e));
                    }
                    Vec::new()
                }
            }
        })
        .collect();

    // Report errors at the end (same as Engine)
    if let Ok(errs) = errors.lock() {
        for (path, error) in errs.iter() {
            eprintln!("Warning: Failed to analyze {}: {}", path.display(), error);
        }
    }

    Ok(all_diagnostics)
}

/// Analyze a single file with a custom plugin registry.
/// Uses TOCTOU-safe file handling (same as Engine).
fn analyze_single_file_with_registry(
    file_path: &Path,
    config: &Config,
    registry: &PluginRegistry,
) -> Result<Vec<Diagnostic>, String> {
    // SECURITY: Use file descriptor to prevent TOCTOU attacks
    // Open file once, verify via fd metadata, then read from same fd
    let mut file = File::open(file_path).map_err(|e| e.to_string())?;

    // Get metadata from the open file descriptor (not the path)
    let metadata = file.metadata().map_err(|e| e.to_string())?;

    // Verify it's still a regular file via the fd
    if !metadata.is_file() {
        return Err("not a regular file".to_string());
    }

    // Check file size via fd metadata
    if metadata.len() > MAX_FILE_SIZE {
        return Err(format!(
            "file too large: {} bytes (max: {} bytes)",
            metadata.len(),
            MAX_FILE_SIZE
        ));
    }

    // Read from the same file descriptor
    let mut source = String::with_capacity(metadata.len() as usize);
    file.read_to_string(&mut source).map_err(|e| e.to_string())?;

    let ast = syn::parse_file(&source).map_err(|e| e.to_string())?;

    let ctx = AnalysisContext::new(file_path, &source, &ast, config);
    let suppressions = SuppressionExtractor::new(&source, &ast);

    // Run all rules from the custom registry
    let mut diagnostics = Vec::new();
    for rule in registry.rules() {
        // Check if rule is enabled in config (Some severity means enabled)
        if config
            .rule_severity(rule.id(), rule.default_severity())
            .is_some()
        {
            // Catch panics in rule execution to prevent one bad rule from crashing analysis
            // This is especially important for user-provided plugin rules
            let rule_diagnostics = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                rule.check(&ctx)
            })) {
                Ok(diags) => diags,
                Err(_) => {
                    eprintln!(
                        "Warning: Rule '{}' panicked while analyzing {}",
                        rule.id(),
                        file_path.display()
                    );
                    continue;
                }
            };

            // Filter suppressed diagnostics
            for diag in rule_diagnostics {
                if !suppressions.is_suppressed(diag.rule_id, diag.line) {
                    diagnostics.push(diag);
                }
            }
        }
    }

    Ok(diagnostics)
}

/// A helper macro for defining custom rules more concisely.
///
/// # Example
///
/// ```rust,ignore
/// use cargo_perf::define_rule;
///
/// define_rule! {
///     /// Detects usage of unwrap() in production code.
///     pub struct NoUnwrapRule {
///         id: "no-unwrap",
///         name: "No Unwrap",
///         description: "Detects .unwrap() calls that should use proper error handling",
///         severity: Warning,
///     }
///
///     fn check(&self, ctx: &AnalysisContext) -> Vec<Diagnostic> {
///         // Implementation here
///         Vec::new()
///     }
/// }
/// ```
#[macro_export]
macro_rules! define_rule {
    (
        $(#[$meta:meta])*
        pub struct $name:ident {
            id: $id:literal,
            name: $rule_name:literal,
            description: $desc:literal,
            severity: $severity:ident,
        }

        fn check(&$self:ident, $ctx:ident: &AnalysisContext) -> Vec<Diagnostic> $body:block
    ) => {
        $(#[$meta])*
        pub struct $name;

        impl $crate::rules::Rule for $name {
            fn id(&self) -> &'static str {
                $id
            }

            fn name(&self) -> &'static str {
                $rule_name
            }

            fn description(&self) -> &'static str {
                $desc
            }

            fn default_severity(&self) -> $crate::rules::Severity {
                $crate::rules::Severity::$severity
            }

            fn check(&$self, $ctx: &$crate::engine::AnalysisContext) -> Vec<$crate::rules::Diagnostic> $body
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Severity;

    struct TestRule;

    impl Rule for TestRule {
        fn id(&self) -> &'static str {
            "test-rule"
        }

        fn name(&self) -> &'static str {
            "Test Rule"
        }

        fn description(&self) -> &'static str {
            "A test rule"
        }

        fn default_severity(&self) -> Severity {
            Severity::Warning
        }

        fn check(&self, _ctx: &AnalysisContext) -> Vec<Diagnostic> {
            Vec::new()
        }
    }

    #[test]
    fn test_registry_add_rule() {
        let mut registry = PluginRegistry::new();
        registry.add_rule(Box::new(TestRule));

        assert!(registry.has_rule("test-rule"));
        assert!(!registry.has_rule("nonexistent"));
    }

    #[test]
    fn test_registry_get_rule() {
        let mut registry = PluginRegistry::new();
        registry.add_rule(Box::new(TestRule));

        let rule = registry.get_rule("test-rule");
        assert!(rule.is_some());
        assert_eq!(rule.unwrap().id(), "test-rule");
    }

    #[test]
    #[should_panic(expected = "already exists")]
    fn test_registry_duplicate_rule_panics() {
        let mut registry = PluginRegistry::new();
        registry.add_rule(Box::new(TestRule));
        registry.add_rule(Box::new(TestRule)); // Should panic
    }

    #[test]
    fn test_try_add_rule_returns_err_on_duplicate() {
        let mut registry = PluginRegistry::new();
        assert!(registry.try_add_rule(Box::new(TestRule)).is_ok());
        assert!(registry.try_add_rule(Box::new(TestRule)).is_err());
        // Original rule should still be there
        assert!(registry.has_rule("test-rule"));
    }

    #[test]
    fn test_registry_add_or_replace() {
        let mut registry = PluginRegistry::new();
        registry.add_rule(Box::new(TestRule));
        registry.add_or_replace_rule(Box::new(TestRule)); // Should not panic

        assert!(registry.has_rule("test-rule"));
    }

    #[test]
    fn test_builder_pattern() {
        let registry = PluginRegistryBuilder::new()
            .with_rule(Box::new(TestRule))
            .build();

        assert!(registry.has_rule("test-rule"));
    }

    #[test]
    fn test_rule_ids() {
        let mut registry = PluginRegistry::new();
        registry.add_rule(Box::new(TestRule));

        let ids: Vec<_> = registry.rule_ids().collect();
        assert!(ids.contains(&"test-rule"));
    }
}
