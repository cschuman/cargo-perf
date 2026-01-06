# Contributing to cargo-perf

Thank you for your interest in contributing to cargo-perf! This document provides guidelines for contributing.

## Development Setup

```bash
# Clone the repository
git clone https://github.com/cschuman/cargo-perf.git
cd cargo-perf

# Build
cargo build

# Run tests
cargo test

# Run clippy
cargo clippy

# Format code
cargo fmt
```

## Project Structure

```
src/
├── main.rs           # CLI entry point
├── lib.rs            # Library exports
├── config.rs         # Configuration handling
├── error.rs          # Custom error types
├── fix.rs            # Auto-fix infrastructure
├── suppression.rs    # Inline suppression support
├── engine/
│   ├── mod.rs        # Analysis engine
│   ├── context.rs    # Analysis context & line index
│   └── parser.rs     # Syn parsing utilities
├── rules/
│   ├── mod.rs        # Rule trait definition
│   ├── registry.rs   # Static rule registry
│   ├── visitor.rs    # Shared visitor utilities
│   ├── async_rules.rs
│   ├── memory_rules.rs
│   ├── iter_rules.rs
│   ├── allocation_rules.rs
│   └── lock_across_await.rs
└── reporter/
    ├── mod.rs        # Reporter trait
    ├── console.rs    # Console output
    ├── json.rs       # JSON output
    └── sarif.rs      # SARIF output for CI
```

## Adding a New Rule

1. **Create the rule module** in `src/rules/`:

```rust
use super::visitor::VisitorState;
use super::{Diagnostic, Rule, Severity};
use crate::engine::AnalysisContext;
use syn::visit::Visit;

pub struct MyNewRule;

impl Rule for MyNewRule {
    fn id(&self) -> &'static str {
        "my-rule-id"
    }

    fn name(&self) -> &'static str {
        "My Rule Name"
    }

    fn description(&self) -> &'static str {
        "Description of what this rule detects"
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning // or Severity::Error
    }

    fn check(&self, ctx: &AnalysisContext) -> Vec<Diagnostic> {
        let mut visitor = MyVisitor {
            ctx,
            diagnostics: Vec::new(),
            state: VisitorState::new(),
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct MyVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    state: VisitorState,
}

impl<'ast> Visit<'ast> for MyVisitor<'_> {
    // Use state.should_bail() for recursion limits
    fn visit_expr(&mut self, node: &'ast syn::Expr) {
        if self.state.should_bail() { return; }
        self.state.enter_expr();
        // Your detection logic here
        syn::visit::visit_expr(self, node);
        self.state.exit_expr();
    }
}
```

2. **Register the rule** in `src/rules/registry.rs`:

```rust
use super::my_new_rule::MyNewRule;

static RULES: LazyLock<Vec<Box<dyn Rule>>> = LazyLock::new(|| {
    vec![
        // ... existing rules
        Box::new(MyNewRule),
    ]
});
```

3. **Add tests** for your rule:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::AnalysisContext;
    use crate::Config;
    use std::path::Path;

    fn check_code(source: &str) -> Vec<Diagnostic> {
        let ast = syn::parse_file(source).expect("Failed to parse");
        let config = Config::default();
        let ctx = AnalysisContext::new(Path::new("test.rs"), source, &ast, &config);
        MyNewRule.check(&ctx)
    }

    #[test]
    fn test_detects_bad_pattern() {
        let source = r#"
            fn bad() {
                // code that should trigger the rule
            }
        "#;
        let diagnostics = check_code(source);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_ignores_good_pattern() {
        let source = r#"
            fn good() {
                // code that should NOT trigger the rule
            }
        "#;
        let diagnostics = check_code(source);
        assert!(diagnostics.is_empty());
    }
}
```

4. **Update documentation** in README.md

## Guidelines

### Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy` and fix all warnings
- Add tests for all new functionality

### Security

- Use `VisitorState` for recursion depth limits in all visitors
- Never trust file paths - use file descriptors when possible
- Be careful with symlinks and path traversal

### Testing

- Aim for >80% test coverage on new code
- Include both positive (detect) and negative (ignore) test cases
- Test edge cases (empty input, deeply nested code, etc.)

### Commits

- Write clear, descriptive commit messages
- Reference issues when applicable
- Keep commits focused on a single change

## Running the Test Suite

```bash
# All tests
cargo test

# Specific test
cargo test test_name

# With output
cargo test -- --nocapture

# Run cargo-perf on itself (dogfooding)
cargo run -- check src/
```

## Questions?

Open an issue on GitHub for questions or discussion.
