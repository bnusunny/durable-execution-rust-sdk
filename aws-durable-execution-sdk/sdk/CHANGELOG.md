# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0-alpha2](https://github.com/bnusunny/durable-execution-rust-sdk/compare/durable-execution-sdk-v0.1.0-alpha1...durable-execution-sdk-v0.1.0-alpha2) - 2026-03-10

### Fixed

- add timeout to orchestrator callback loop and remove wait strategy panic
- address code review issues for robustness and consistency
- replace unsafe Send/Sync impls with static_assertions for Callback<T>

### Other

- add README.md to macros and sdk crates
- Merge pull request #14 from bnusunny/fix/cleanup-and-safety-improvements
- Merge pull request #12 from bnusunny/release-plz-2026-03-08T23-57-31Z
- remove '# Requirements' sections from doc comments

## [0.1.0-alpha1](https://github.com/bnusunny/durable-execution-rust-sdk/releases/tag/durable-execution-sdk-v0.1.0-alpha1) - 2026-03-08

### Added

- *(sdk)* add runtime utilities and logging
- *(sdk)* improve context and client
- *(sdk)* enhance operation handlers
- *(sdk)* enhance error handling
- *(sdk)* add retry strategies with jitter support
- *(testing)* add comprehensive testing utilities crate for durable execution SDK

### Fixed

- clippy unused variable and flaky env var test race condition
- *(ci)* fix trybuild tests and mark coverage as non-blocking
- *(ci)* resolve all PR validation workflow failures

### Other

- apply cargo fmt
- remove 'aws-' prefix from all crate names
