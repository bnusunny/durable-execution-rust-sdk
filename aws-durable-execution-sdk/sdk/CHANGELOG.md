# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
