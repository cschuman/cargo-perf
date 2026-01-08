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

use crate::discovery::{discover_rust_files, DiscoveryOptions};
use crate::engine::{analyze_file_with_rules, AnalysisContext};
use crate::error::Error;
use crate::rules::{Diagnostic, Rule};
use crate::Config;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

/// A registry for managing both built-in and custom rules.
///
/// This registry reuses the static rule registry for built-in rules,
/// avoiding duplication. Custom rules are stored separately and can
/// override built-in rules with the same ID.
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
    /// Whether built-in rules from the static registry are included.
    include_builtins: bool,
    /// Custom rules (may override built-in rules).
    custom_rules: Vec<Box<dyn Rule>>,
    /// Index for O(1) lookup of custom rules by ID.
    custom_index: HashMap<String, usize>,
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
            include_builtins: false,
            custom_rules: Vec::new(),
            custom_index: HashMap::new(),
        }
    }

    /// Add all built-in rules to the registry.
    ///
    /// This references the static rule registry rather than creating new instances,
    /// avoiding memory duplication.
    pub fn add_builtin_rules(&mut self) {
        self.include_builtins = true;
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
    /// Returns an error if a rule with the same ID already exists in custom rules.
    /// Note: This allows overriding built-in rules with custom implementations.
    /// Use [`add_or_replace_rule`] to replace existing rules without error.
    ///
    /// [`add_or_replace_rule`]: Self::add_or_replace_rule
    ///
    /// # Errors
    ///
    /// Returns `Err` with the rejected rule if a custom rule with the same ID already exists.
    pub fn try_add_rule(&mut self, rule: Box<dyn Rule>) -> Result<(), Box<dyn Rule>> {
        let id = rule.id().to_string();
        if self.custom_index.contains_key(&id) {
            return Err(rule);
        }
        let idx = self.custom_rules.len();
        self.custom_index.insert(id, idx);
        self.custom_rules.push(rule);
        Ok(())
    }

    /// Add a custom rule, replacing any existing custom rule with the same ID.
    pub fn add_or_replace_rule(&mut self, rule: Box<dyn Rule>) {
        let id = rule.id().to_string();
        if let Some(&idx) = self.custom_index.get(&id) {
            self.custom_rules[idx] = rule;
        } else {
            let idx = self.custom_rules.len();
            self.custom_index.insert(id, idx);
            self.custom_rules.push(rule);
        }
    }

    /// Get all registered rules as trait object references.
    ///
    /// Returns an iterator over all rules (built-in + custom).
    /// Custom rules with the same ID as built-in rules will override them.
    pub fn rules(&self) -> Vec<&dyn Rule> {
        use crate::rules::registry;

        let mut rules: Vec<&dyn Rule> = Vec::new();

        // Add built-in rules (if enabled), skipping those overridden by custom rules
        if self.include_builtins {
            for rule in registry::all_rules() {
                if !self.custom_index.contains_key(rule.id()) {
                    rules.push(rule.as_ref());
                }
            }
        }

        // Add custom rules
        for rule in &self.custom_rules {
            rules.push(rule.as_ref());
        }

        rules
    }

    /// Get a rule by its ID.
    ///
    /// Custom rules take precedence over built-in rules.
    pub fn get_rule(&self, id: &str) -> Option<&dyn Rule> {
        use crate::rules::registry;

        // Check custom rules first (they override built-ins)
        if let Some(&idx) = self.custom_index.get(id) {
            return Some(self.custom_rules[idx].as_ref());
        }

        // Fall back to built-in rules
        if self.include_builtins {
            return registry::get_rule(id);
        }

        None
    }

    /// Check if a rule with the given ID exists.
    pub fn has_rule(&self, id: &str) -> bool {
        use crate::rules::registry;

        self.custom_index.contains_key(id)
            || (self.include_builtins && registry::has_rule(id))
    }

    /// Get all rule IDs.
    pub fn rule_ids(&self) -> Vec<&str> {
        use crate::rules::registry;

        let mut ids: Vec<&str> = Vec::new();

        // Add built-in rule IDs (if enabled), skipping overridden ones
        if self.include_builtins {
            for id in registry::rule_ids() {
                if !self.custom_index.contains_key(id) {
                    ids.push(id);
                }
            }
        }

        // Add custom rule IDs
        for id in self.custom_index.keys() {
            ids.push(id.as_str());
        }

        ids
    }

    /// Run all rules on the given analysis context.
    pub fn check_all(&self, ctx: &AnalysisContext) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for rule in self.rules() {
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
) -> Result<Vec<Diagnostic>, Error> {
    // Use secure discovery (same as Engine) to prevent symlink attacks
    let files = discover_rust_files(path, &DiscoveryOptions::secure());

    // Track errors but don't fail the entire analysis
    let errors: Mutex<Vec<(std::path::PathBuf, Error)>> = Mutex::new(Vec::new());

    // Analyze files in parallel using shared file analysis logic
    let all_diagnostics: Vec<Diagnostic> = files
        .par_iter()
        .flat_map(|file_path| {
            // Use shared analysis function with plugin registry rules
            let rules = registry.rules().into_iter();
            match analyze_file_with_rules(file_path, config, rules) {
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

        let ids = registry.rule_ids();
        assert!(ids.contains(&"test-rule"));
    }
}
