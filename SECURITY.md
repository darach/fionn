# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

We take security vulnerabilities seriously. If you discover a security issue, please report it responsibly.

### How to Report

**Please do NOT report security vulnerabilities through public GitHub issues.**

Instead, please report them via one of the following methods:

1. **GitHub Security Advisories** (Preferred): Use [GitHub's private vulnerability reporting](https://github.com/darach/fionn/security/advisories/new) to report the issue directly.

2. **Email**: Send details to the maintainer at the email address listed in the repository.

### What to Include

Please include the following information in your report:

- Type of issue (e.g., buffer overflow, SQL injection, cross-site scripting, etc.)
- Full paths of source file(s) related to the issue
- Location of the affected source code (tag/branch/commit or direct URL)
- Any special configuration required to reproduce the issue
- Step-by-step instructions to reproduce the issue
- Proof-of-concept or exploit code (if possible)
- Impact of the issue, including how an attacker might exploit it

### Response Timeline

- **Initial Response**: Within 48 hours of report submission
- **Status Update**: Within 7 days with an assessment of the issue
- **Resolution Target**: Security patches within 30 days for critical issues

### Disclosure Policy

- We follow coordinated disclosure practices
- We will credit reporters in security advisories (unless anonymity is requested)
- We ask that you give us reasonable time to address the issue before public disclosure

## Security Measures

This project implements the following security practices:

### Development
- All dependencies are regularly audited using `cargo-audit` and `cargo-deny`
- Automated security scanning via GitHub Dependabot
- OpenSSF Scorecard monitoring for supply chain security
- Fuzz testing with AFL++ for robustness

### CI/CD
- Pinned GitHub Actions versions with SHA hashes
- Minimal permissions for CI workflows
- Automated dependency updates via Dependabot
- SLSA provenance for release artifacts

### Code Quality
- Strict Clippy lints enabled
- Memory-safe Rust with `#[forbid(unsafe_code)]` where applicable
- Regular security-focused code reviews

## Security.txt

For automated security tooling, see [/.well-known/security.txt](/.well-known/security.txt) (if available).
