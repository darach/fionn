# Contributing to fionn

Thank you for your interest in contributing to fionn! This document provides guidelines and information for contributors.

## Code of Conduct

By participating in this project, you agree to maintain a respectful and inclusive environment for everyone.

## Getting Started

### Prerequisites

- Rust 1.89 or later (edition 2024)
- Git

### Development Setup

```bash
# Clone the repository
git clone https://github.com/darach/fionn.git
cd fionn

# Build the project
cargo build

# Run tests
cargo test

# Run clippy lints
cargo clippy --all-targets --all-features -- -D warnings

# Format code
cargo fmt
```

## How to Contribute

### Reporting Bugs

1. Check if the bug has already been reported in [Issues](https://github.com/darach/fionn/issues)
2. If not, create a new issue with:
   - Clear, descriptive title
   - Steps to reproduce
   - Expected vs actual behavior
   - Environment details (OS, Rust version)

### Suggesting Features

1. Check existing issues and discussions for similar suggestions
2. Open a new issue with the `enhancement` label
3. Describe the feature and its use case

### Pull Requests

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/my-feature`
3. Make your changes following our coding standards
4. Add tests for new functionality
5. Ensure all tests pass: `cargo test`
6. Ensure lints pass: `cargo clippy --all-targets --all-features -- -D warnings`
7. Format your code: `cargo fmt`
8. Commit with clear messages (see commit guidelines below)
9. Push and open a pull request

### Commit Messages

We follow conventional commits:

```
type(scope): description

[optional body]

[optional footer]
```

Types:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style changes (formatting, etc.)
- `refactor`: Code refactoring
- `perf`: Performance improvements
- `test`: Adding or updating tests
- `chore`: Maintenance tasks
- `ci`: CI/CD changes

Example:
```
feat(tape): add SIMD-accelerated JSON parsing

Implements AVX2-based parsing for improved performance on x86_64.

Closes #123
```

## Coding Standards

### Rust Style

- Follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- Use `rustfmt` with default settings
- Pass all Clippy lints (pedantic and nursery enabled)
- Document public APIs with doc comments
- Include examples in documentation where helpful

### Testing

- Write unit tests for new functionality
- Include integration tests for user-facing features
- Aim for meaningful test coverage, not just high percentages
- Use property-based testing where appropriate

### Performance

- This project prioritizes performance; benchmark significant changes
- Use `cargo bench` to run benchmarks
- Document any performance trade-offs

### Security

- Never commit secrets or credentials
- Validate all external input
- Use safe Rust; minimize `unsafe` code
- Report security issues privately (see [SECURITY.md](SECURITY.md))

## Project Structure

```
fionn/
├── crates/
│   ├── fionn-core/    # Core types and utilities
│   ├── fionn-tape/    # JSON tape representation
│   ├── fionn-simd/    # SIMD utilities
│   ├── fionn-ops/     # Operations on JSON
│   ├── fionn-gron/    # Gron format support
│   ├── fionn-diff/    # JSON diff functionality
│   ├── fionn-crdt/    # CRDT implementations
│   ├── fionn-stream/  # Streaming support
│   ├── fionn-pool/    # Memory pool
│   └── fionn-cli/     # CLI application
├── benches/           # Benchmarks
├── fuzz/              # Fuzz testing targets
└── docs/              # Documentation
```

## Development Commands

```bash
# Run all checks (recommended before committing)
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo doc --no-deps

# Run benchmarks
cargo bench

# Run fuzz tests (requires AFL++)
./fuzz/scripts/fuzz.sh tape

# Check for security advisories
cargo audit

# Check dependency licenses
cargo deny check
```

## License

By contributing, you agree that your contributions will be licensed under the same terms as the project (MIT OR Apache-2.0).

## Questions?

Feel free to open an issue for questions or join discussions in the repository.
