# Implementation Plan: Promise Combinator Macros

## Overview

This plan implements ergonomic macros (`all!`, `any!`, `race!`, `all_settled!`) for combining heterogeneous futures in the AWS Durable Execution SDK for Rust.

## Tasks

- [x] 1. Create macros module and implement all! macro
  - [x] 1.1 Create `aws-durable-execution-sdk/src/macros.rs` with the `all!` macro
    - Implement using `macro_rules!` with `$ctx:expr` and `$($fut:expr),+` pattern
    - Use `Box::pin()` to box each future
    - Use fully qualified paths (`::std::vec::Vec`, etc.)
    - Support optional trailing comma with `$(,)?`
    - _Requirements: 1.1, 1.4, 1.6_
  - [x] 1.2 Add `mod macros;` to `lib.rs` to export the macro
    - _Requirements: 5.1, 5.3_
  - [x] 1.3 Write unit tests for `all!` macro
    - Test with 1, 2, and 3+ futures
    - Test success case returns `Vec<T>` in order
    - Test failure case returns first error
    - **Property 1: Macro expansion equivalence** - verify macro produces same result as manual boxing
    - **Validates: Requirements 1.1, 1.2, 1.3, 1.4, 1.5**

- [x] 2. Implement any! macro
  - [x] 2.1 Add `any!` macro to `macros.rs`
    - Same pattern as `all!` but calls `ctx.any()`
    - _Requirements: 2.1, 2.4, 2.5_
  - [x] 2.2 Write unit tests for `any!` macro
    - Test first success is returned
    - Test all failures returns combined error
    - **Validates: Requirements 2.1, 2.2, 2.3, 2.4**

- [x] 3. Implement race! macro
  - [x] 3.1 Add `race!` macro to `macros.rs`
    - Same pattern as `all!` but calls `ctx.race()`
    - _Requirements: 3.1, 3.3, 3.4_
  - [x] 3.2 Write unit tests for `race!` macro
    - Test first to settle (success or failure) is returned
    - **Validates: Requirements 3.1, 3.2, 3.3**

- [x] 4. Implement all_settled! macro
  - [x] 4.1 Add `all_settled!` macro to `macros.rs`
    - Same pattern as `all!` but calls `ctx.all_settled()`
    - _Requirements: 4.1, 4.3, 4.5_
  - [x] 4.2 Write unit tests for `all_settled!` macro
    - Test returns `BatchResult<T>` with all outcomes
    - Test order is preserved
    - **Property 2: Result order preservation** - verify results match input order
    - **Validates: Requirements 4.1, 4.2, 4.3, 4.4**

- [x] 5. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 6. Add documentation and examples
  - [x] 6.1 Add comprehensive doc comments to each macro in `macros.rs`
    - Include usage examples
    - Explain when to use macros vs method-based API
    - _Requirements: 5.2, 5.4, 5.5, 5.6_
  - [x] 6.2 Update `examples/promise_combinators/` with macro examples
    - Update `all/promise_all.rs` to demonstrate `all!` macro
    - Update `any/promise_any.rs` to demonstrate `any!` macro
    - Update `race/promise_race.rs` to demonstrate `race!` macro
    - Update `all_settled/promise_all_settled.rs` to demonstrate `all_settled!` macro
    - _Requirements: 5.2, 5.5_

- [x] 7. Add type safety compile-time tests
  - [x] 7.1 Add compile-fail tests for type mismatches
    - Test that incompatible output types produce compile errors
    - Use `trybuild` crate or manual verification
    - **Property 4: Compile-time type mismatch detection**
    - **Validates: Requirements 6.1, 6.2**

- [x] 8. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties
- Unit tests validate specific examples and edge cases
