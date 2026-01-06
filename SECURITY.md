# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.3.x   | :white_check_mark: |
| < 0.3   | :x:                |

## Reporting a Vulnerability

If you discover a security vulnerability in cargo-perf, please report it responsibly:

1. **Do not** open a public issue
2. Email the maintainers directly or use GitHub's private vulnerability reporting
3. Include:
   - Description of the vulnerability
   - Steps to reproduce
   - Potential impact
   - Suggested fix (if any)

## Response Timeline

- **Acknowledgment**: Within 48 hours
- **Initial assessment**: Within 1 week
- **Fix timeline**: Depends on severity
  - Critical: 24-48 hours
  - High: 1 week
  - Medium: 2 weeks
  - Low: Next release

## Security Measures in cargo-perf

cargo-perf implements several security measures:

### File Operations
- **TOCTOU protection**: All file operations use file descriptors to prevent race conditions
- **Symlink protection**: Symlinks are not followed during file traversal
- **Path traversal protection**: Auto-fix feature validates paths before writing
- **File size limits**: 10MB maximum to prevent resource exhaustion

### Analysis Engine
- **Recursion limits**: AST visitors bail at depth 256 to prevent stack overflow
- **Memory safety**: Pure Rust implementation with no unsafe code

### Dependencies
- Regular security audits via `cargo audit` in CI
- Dependabot enabled for automated security updates

## Scope

This security policy covers:
- The cargo-perf binary
- The cargo_perf library crate
- Official CI/CD configurations

Third-party integrations and forks are not covered.
