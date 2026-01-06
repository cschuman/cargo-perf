//! Inline suppression support for cargo-perf diagnostics.
//!
//! Supports suppressing warnings with:
//! - `#[allow(cargo_perf::rule_id)]` - suppress specific rule
//! - `#[allow(cargo_perf::all)]` - suppress all cargo-perf warnings
//! - `// cargo-perf-ignore: rule_id` - line-level suppression

use std::collections::HashSet;
use syn::visit::Visit;
use syn::{Attribute, File, ItemFn, ItemImpl, ItemMod, ItemStruct};

/// Extracts all cargo_perf suppressions from a file.
pub struct SuppressionExtractor {
    /// Suppressions by line number: (line, set of suppressed rule IDs)
    /// An empty set means all rules are suppressed
    pub line_suppressions: std::collections::HashMap<usize, HashSet<String>>,
    /// Global suppressions that apply to the entire file
    pub file_suppressions: HashSet<String>,
}

impl SuppressionExtractor {
    /// Extract suppressions from source code and AST.
    pub fn new(source: &str, ast: &File) -> Self {
        let mut extractor = Self {
            line_suppressions: std::collections::HashMap::new(),
            file_suppressions: HashSet::new(),
        };

        // Extract comment-based suppressions from source
        extractor.extract_comment_suppressions(source);

        // Extract attribute-based suppressions from AST
        extractor.visit_file(ast);

        // Check for file-level attributes
        for attr in &ast.attrs {
            extractor.process_attribute(attr, None);
        }

        extractor
    }

    /// Check if a diagnostic at the given line should be suppressed.
    pub fn is_suppressed(&self, rule_id: &str, line: usize) -> bool {
        // Check file-level suppressions
        if self.file_suppressions.contains("all") || self.file_suppressions.contains(rule_id) {
            return true;
        }

        // Check line-level suppressions
        if let Some(suppressions) = self.line_suppressions.get(&line) {
            if suppressions.is_empty() || suppressions.contains("all") || suppressions.contains(rule_id) {
                return true;
            }
        }

        false
    }

    /// Extract `// cargo-perf-ignore: rule_id` comments.
    fn extract_comment_suppressions(&mut self, source: &str) {
        for (line_num, line) in source.lines().enumerate() {
            let line_num = line_num + 1; // 1-indexed

            // Check for cargo-perf-ignore comment
            if let Some(idx) = line.find("cargo-perf-ignore") {
                let rest = &line[idx + "cargo-perf-ignore".len()..];
                let rest = rest.trim_start_matches(':').trim();

                let suppressions = self.line_suppressions
                    .entry(line_num + 1) // Suppress the *next* line
                    .or_default();

                if rest.is_empty() || rest == "all" {
                    suppressions.insert("all".to_string());
                } else {
                    // Parse comma-separated rule IDs
                    for rule in rest.split(',') {
                        let rule = rule.trim();
                        if !rule.is_empty() {
                            suppressions.insert(rule.to_string());
                        }
                    }
                }
            }
        }
    }

    /// Process an attribute to extract suppressions.
    fn process_attribute(&mut self, attr: &Attribute, span_line: Option<usize>) {
        // Check for #[allow(cargo_perf::...)]
        if attr.path().is_ident("allow") {
            if let Ok(nested) = attr.parse_args_with(
                syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
            ) {
                for path in nested {
                    let segments: Vec<_> = path.segments.iter().map(|s| s.ident.to_string()).collect();

                    if segments.first().map(|s| s.as_str()) == Some("cargo_perf") {
                        let rule_id = segments.get(1).map(|s| s.as_str()).unwrap_or("all");

                        if let Some(line) = span_line {
                            self.line_suppressions
                                .entry(line)
                                .or_default()
                                .insert(rule_id.replace('_', "-"));
                        } else {
                            self.file_suppressions.insert(rule_id.replace('_', "-"));
                        }
                    }
                }
            }
        }
    }

    /// Add suppressions for a range of lines based on attributes.
    fn add_item_suppressions(&mut self, attrs: &[Attribute], start_line: usize, end_line: usize) {
        let mut rules_to_suppress = HashSet::new();

        for attr in attrs {
            if attr.path().is_ident("allow") {
                if let Ok(nested) = attr.parse_args_with(
                    syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
                ) {
                    for path in nested {
                        let segments: Vec<_> = path.segments.iter().map(|s| s.ident.to_string()).collect();

                        if segments.first().map(|s| s.as_str()) == Some("cargo_perf") {
                            let rule_id = segments.get(1).map(|s| s.as_str()).unwrap_or("all");
                            rules_to_suppress.insert(rule_id.replace('_', "-"));
                        }
                    }
                }
            }
        }

        // Apply suppressions to all lines in the item's range
        if !rules_to_suppress.is_empty() {
            for line in start_line..=end_line {
                self.line_suppressions
                    .entry(line)
                    .or_default()
                    .extend(rules_to_suppress.iter().cloned());
            }
        }
    }
}

impl<'ast> Visit<'ast> for SuppressionExtractor {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let start = node.sig.fn_token.span.start().line;
        let end = node.block.brace_token.span.close().start().line;
        self.add_item_suppressions(&node.attrs, start, end);
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_item_struct(&mut self, node: &'ast ItemStruct) {
        let start = node.struct_token.span.start().line;
        // Get accurate end line based on struct variant
        let end = match &node.fields {
            syn::Fields::Named(fields) => fields.brace_token.span.close().start().line,
            syn::Fields::Unnamed(fields) => fields.paren_token.span.close().start().line,
            syn::Fields::Unit => node.semi_token.map(|t| t.span.start().line).unwrap_or(start),
        };
        self.add_item_suppressions(&node.attrs, start, end);
        syn::visit::visit_item_struct(self, node);
    }

    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
        let start = node.impl_token.span.start().line;
        let end = node.brace_token.span.close().start().line;
        self.add_item_suppressions(&node.attrs, start, end);
        syn::visit::visit_item_impl(self, node);
    }

    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        if let Some((brace, _)) = &node.content {
            let start = node.mod_token.span.start().line;
            let end = brace.span.close().start().line;
            self.add_item_suppressions(&node.attrs, start, end);
        }
        syn::visit::visit_item_mod(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comment_suppression() {
        let source = r#"
fn test() {
    // cargo-perf-ignore: clone-in-hot-loop
    let x = data.clone();
}
"#;
        let ast = syn::parse_file(source).unwrap();
        let extractor = SuppressionExtractor::new(source, &ast);

        assert!(extractor.is_suppressed("clone-in-hot-loop", 4));
        assert!(!extractor.is_suppressed("clone-in-hot-loop", 5));
    }

    #[test]
    fn test_comment_suppression_all() {
        let source = r#"
fn test() {
    // cargo-perf-ignore
    let x = data.clone();
}
"#;
        let ast = syn::parse_file(source).unwrap();
        let extractor = SuppressionExtractor::new(source, &ast);

        assert!(extractor.is_suppressed("clone-in-hot-loop", 4));
        assert!(extractor.is_suppressed("any-rule", 4));
    }

    #[test]
    fn test_attribute_suppression() {
        let source = r#"
#[allow(cargo_perf::clone_in_hot_loop)]
fn test() {
    let x = data.clone();
}
"#;
        let ast = syn::parse_file(source).unwrap();
        let extractor = SuppressionExtractor::new(source, &ast);

        // Function body should be suppressed
        assert!(extractor.is_suppressed("clone-in-hot-loop", 4));
    }

    #[test]
    fn test_no_suppression() {
        let source = r#"
fn test() {
    let x = data.clone();
}
"#;
        let ast = syn::parse_file(source).unwrap();
        let extractor = SuppressionExtractor::new(source, &ast);

        assert!(!extractor.is_suppressed("clone-in-hot-loop", 3));
    }
}
