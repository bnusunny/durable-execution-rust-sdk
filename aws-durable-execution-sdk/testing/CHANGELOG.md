# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0-alpha3](https://github.com/bnusunny/durable-execution-rust-sdk/compare/durable-execution-sdk-testing-v0.1.0-alpha2...durable-execution-sdk-testing-v0.1.0-alpha3) - 2026-03-25

### Added

- [**breaking**] accept async closures in step, step_named, and wait_for_condition

### Fixed

- generate WaitSucceeded events in local runner to match cloud behavior

### Other

- fix all 63 clippy warnings across the workspace
- cargo fmt
- Align local runner with cloud behavior, add cloud integration tests
- Add cloud integration tests, align local runner with cloud behavior
- Add NodeJS event history support to CloudDurableTestRunner
- Fix integration tests: deploy via Lambda API, fix CloudDurableTestRunner

## [0.1.0-alpha2](https://github.com/bnusunny/durable-execution-rust-sdk/compare/durable-execution-sdk-testing-v0.1.0-alpha1...durable-execution-sdk-testing-v0.1.0-alpha2) - 2026-03-10

### Fixed

- add timeout to orchestrator callback loop and remove wait strategy panic
- address code review issues for robustness and consistency
- use unordered event assertions for concurrent tests
- decouple wait completion from skip_time_config in orchestrator
- treat Context/Invoke operations as non-blocking in orchestrator

### Other

- Merge pull request #14 from bnusunny/fix/cleanup-and-safety-improvements
- Merge pull request #12 from bnusunny/release-plz-2026-03-08T23-57-31Z
- remove '# Requirements' sections from doc comments

## [0.1.0-alpha1](https://github.com/bnusunny/durable-execution-rust-sdk/releases/tag/durable-execution-sdk-testing-v0.1.0-alpha1) - 2026-03-08

### Added

- *(testing)* update public API exports
- *(testing)* add cloud test runner
- *(testing)* enhance local test runner
- *(testing)* enhance checkpoint server
- *(testing)* add comprehensive testing utilities crate for durable execution SDK

### Fixed

- clippy unused variable and flaky env var test race condition
- *(ci)* resolve all PR validation workflow failures

### Other

- remove 'aws-' prefix from all crate names
- *(testing)* add cross-SDK compatibility tests
