# Contributing to AWS Durable Execution SDK

Thank you for your interest in contributing to the AWS Durable Execution SDK!

## Getting Started

### Prerequisites

- Rust 1.70 or later
- Node.js 18+ (for conformance tests)
- AWS CLI configured (for integration tests)

### Building

```bash
cd aws-durable-execution-sdk
cargo build
```

### Running Tests

```bash
# Unit tests
cargo test

# With all features
cargo test --all-features

# Property-based tests (may take longer)
cargo test --all-features -- --ignored
```

### Code Style

We use standard Rust formatting and linting:

```bash
# Format code
cargo fmt

# Run clippy
cargo clippy --all-features -- -D warnings
```

## Pull Request Process

1. Fork the repository and create your branch from `main`
2. Make your changes and add tests
3. Ensure all tests pass and code is formatted
4. Update documentation if needed
5. Submit a pull request

### Commit Messages

Use clear, descriptive commit messages:

- `feat: add new wait_for_condition operation`
- `fix: handle checkpoint token expiration`
- `docs: update determinism guide`
- `test: add property tests for map operation`
- `refactor: simplify operation ID generation`

## Release Process

Releases are automated via GitHub Actions:

1. Update version in `Cargo.toml` files (both SDK and macros)
2. Update CHANGELOG.md
3. Create and push a tag: `git tag v0.1.0 && git push origin v0.1.0`
4. GitHub Actions will publish to crates.io

### Version Format

- Release: `v1.2.3`
- Pre-release: `v1.2.3-alpha1`, `v1.2.3-beta1`, `v1.2.3-rc1`

## Code of Conduct

This project follows the [Amazon Open Source Code of Conduct](https://aws.github.io/code-of-conduct).

## License

By contributing, you agree that your contributions will be licensed under the Apache-2.0 License.
