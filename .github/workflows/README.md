# GitHub Actions Workflows

This directory contains CI/CD workflows for the AWS Durable Execution SDK for Rust.

## Workflows

### PR Validation (`pr.yml`)

Runs on every pull request and push to main. Validates code quality and correctness.

**Jobs:**
- **check** - Cargo check with all features
- **fmt** - Code formatting validation
- **clippy** - Linting with warnings as errors
- **test** - Run all tests and build examples
- **doc** - Documentation generation
- **msrv** - Minimum Supported Rust Version (1.70.0)
- **security** - Security audit with cargo-audit and cargo-deny
- **coverage** - Code coverage with cargo-tarpaulin (uploads to Codecov)
- **conformance** - Conformance tests against reference implementation

### Integration Tests (`integration-tests.yml`)

Tests against deployed AWS Lambda functions. Requires AWS credentials.

**Setup Required:**
1. Create IAM role for GitHub Actions with Lambda permissions
2. Add secrets:
   - `ACTIONS_INTEGRATION_ROLE_ARN` - IAM role ARN
3. Add variables:
   - `AWS_REGION` - AWS region (default: us-east-1)

**What it does:**
- Builds Lambda functions from examples
- Deploys CloudFormation stack with test infrastructure
- Runs integration tests against deployed functions
- Cleans up resources automatically

### Security Scorecard (`scorecard.yml`)

Runs OpenSSF Scorecard security analysis weekly and on main branch pushes.

**Features:**
- Analyzes repository security practices
- Uploads results to GitHub Security tab
- Runs weekly to catch new issues

### Release (`release.yml`)

Automated release management with release-plz.

**Jobs:**
- **pre-release-validation** - Runs tests and builds before release
- **release-plz** - Creates release PR with changelog
- **publish** - Publishes to crates.io (on release commit)
- **create-github-release** - Creates GitHub release with notes

**Trigger:** Automatically on commits with `chore(release)` in message

### Benchmark (`benchmark.yml`)

Performance benchmarking with historical tracking.

**Features:**
- Runs benchmarks on PRs and main
- Tracks performance over time
- Alerts on >150% performance regression
- Comments on PRs with benchmark comparison

### Nightly Tests (`nightly.yml`)

Tests against Rust nightly to catch future breakage early.

**Features:**
- Runs daily at 3am UTC
- Continues on error (doesn't fail)
- Creates GitHub issue on failure (once)

## Required Secrets

| Secret | Description | Required For |
|--------|-------------|--------------|
| `ACTIONS_INTEGRATION_ROLE_ARN` | IAM role for AWS access | Integration tests |
| `CRATES_IO_TOKEN` | Crates.io API token | Release publishing |
| `CODECOV_TOKEN` | Codecov upload token | Code coverage |

## Required Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `AWS_REGION` | AWS region for tests | us-east-1 |

## Local Testing

### Run checks locally:
```bash
# Format check
cargo fmt --all -- --check

# Clippy
cargo clippy --workspace --all-features -- -D warnings

# Tests
cargo test --workspace --all-features

# Documentation
cargo doc --workspace --no-deps --all-features

# Security audit
cargo install cargo-audit cargo-deny
cargo audit
cargo deny check
```

### Run integration tests locally:
```bash
# Requires AWS credentials configured
export SIMPLE_WORKFLOW_FUNCTION=your-function-name
cargo test --package aws-durable-execution-sdk-testing --test integration -- --ignored
```

## Workflow Dependencies

```
PR Validation
├── check
├── fmt
├── clippy
├── test
├── doc
├── msrv
├── security
├── coverage
└── conformance

Integration Tests
└── integration-test

Release
├── pre-release-validation
├── release-plz
├── publish (depends on pre-release-validation)
└── create-github-release (depends on publish)
```

## Caching Strategy

All workflows use GitHub Actions cache for:
- `~/.cargo/registry` - Cargo registry index
- `~/.cargo/git` - Git dependencies
- `aws-durable-execution-sdk/target` - Build artifacts

Cache keys include `Cargo.lock` hash for invalidation on dependency changes.

## Troubleshooting

### Integration tests failing
- Verify AWS credentials are configured
- Check IAM role has necessary permissions
- Ensure Lambda functions build successfully
- Check CloudFormation stack deployment logs

### Coverage upload failing
- Verify `CODECOV_TOKEN` secret is set
- Check Codecov service status
- Review tarpaulin output for errors

### Release not publishing
- Verify commit message contains `chore(release)`
- Check `CRATES_IO_TOKEN` is valid
- Ensure all pre-release validation passes
- Review crates.io publish logs

### Security audit failing
- Review cargo-audit output for vulnerabilities
- Check cargo-deny output for license issues
- Update dependencies or add exceptions in `deny.toml`
