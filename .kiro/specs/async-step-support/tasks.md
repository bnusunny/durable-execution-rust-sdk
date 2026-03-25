# Implementation Plan: Async Step Support

## Overview

Modify `step`, `step_named`, and `wait_for_condition` in-place to accept async closures using the `F + Fut` two-generic pattern. Remove the `StepFn` trait alias, update all handler functions to `.await` closure futures, and update all examples and tests to use `async move { ... }` wrappers. This is a breaking change acceptable for the alpha API.

## Tasks

- [x] 1. Remove StepFn trait alias and update traits.rs
  - [x] 1.1 Remove the `StepFn` trait alias, its blanket impl, and update module docs in `sdk/src/traits.rs`
    - Remove the `StepFn<T>` trait definition and its `impl<T, F> StepFn<T> for F` blanket impl
    - Remove `StepFn` from module-level doc comments and examples
    - Remove `use crate::handlers::StepContext` import if no longer needed
    - Update the `StepFn`-related unit tests: remove `test_step_fn_closure`, `test_step_fn_with_capture`, `test_step_fn_returning_struct`, `test_step_fn_named_function`, `test_type_inference`, `test_step_fn_borrowing_closure`
    - _Requirements: 2.1, 2.2_

  - [ ]* 1.2 Write property test: async closure type bounds compile correctly
    - **Property 6: Checkpoint format parity** — verify that a closure `|ctx| async move { Ok(42) }` satisfies the new `F + Fut` bounds at compile time
    - **Validates: Requirements 1.1, 8.1, 8.2**

- [x] 2. Update step handler functions in sdk/src/handlers/step.rs
  - [x] 2.1 Update `step_handler` signature to use `F + Fut` pattern
    - Add `Fut` generic parameter with bounds `Fut: std::future::Future<Output = Result<T, Box<dyn std::error::Error + Send + Sync>>> + Send`
    - Change `F` bound from `F: StepFn<T>` to `F: FnOnce(StepContext) -> Fut + Send`
    - Pass updated generics through to `execute_at_most_once` and `execute_at_least_once`
    - _Requirements: 1.1, 1.4, 2.2, 8.1, 8.2_

  - [x] 2.2 Update `execute_with_retry` to be async and `.await` the closure
    - Change `fn execute_with_retry` to `async fn execute_with_retry`
    - Add `Fut` generic parameter with same bounds
    - Change `F` bound from `FnOnce(StepContext) -> Result<T, ...>` to `FnOnce(StepContext) -> Fut`
    - Replace `func(step_ctx)` with `func(step_ctx).await`
    - _Requirements: 1.4, 2.2_

  - [x] 2.3 Update `execute_at_most_once` to use `F + Fut` pattern
    - Add `Fut` generic parameter with same bounds
    - Change `F` bound to `FnOnce(StepContext) -> Fut + Send`
    - Update call to `execute_with_retry` to `.await`
    - _Requirements: 1.4, 2.2, 5.1_

  - [x] 2.4 Update `execute_at_least_once` to use `F + Fut` pattern
    - Add `Fut` generic parameter with same bounds
    - Change `F` bound to `FnOnce(StepContext) -> Fut + Send`
    - Update call to `execute_with_retry` to `.await`
    - _Requirements: 1.4, 2.2, 5.2_

  - [x] 2.5 Update all unit tests in `handlers/step.rs` to use async closures
    - Wrap all sync closures in `|ctx| async move { ... }` in tests: `test_step_handler_success`, `test_step_handler_failure`, `test_step_handler_replay_success`, `test_step_handler_replay_failure`, `test_step_handler_non_deterministic_detection`, `test_step_handler_at_most_once_semantics`, `test_step_handler_at_least_once_semantics`
    - _Requirements: 1.1, 1.4, 3.1, 3.2, 4.1, 5.1, 5.2_

  - [ ]* 2.6 Write property test for step replay skips closure execution
    - **Property 1: Step replay skips closure execution**
    - For any DurableValue type T, if a SUCCEED or FAIL checkpoint exists, the async closure is not invoked and the cached result is returned unchanged
    - **Validates: Requirements 3.1, 3.2, 7.3**

  - [ ]* 2.7 Write property test for non-determinism detection
    - **Property 3: Non-determinism detection**
    - If a checkpoint exists with an OperationType different from Step, the handler returns NonDeterministic error without invoking the closure
    - **Validates: Requirement 4.1**

  - [ ]* 2.8 Write property test for execution ordering respects step semantics
    - **Property 4: Execution ordering respects step semantics**
    - AtMostOnce creates START checkpoint before closure; AtLeastOnce invokes closure before any checkpoint
    - **Validates: Requirements 5.1, 5.2**

  - [ ]* 2.9 Write property test for step error wrapping and checkpointing
    - **Property 5: Step error wrapping and checkpointing**
    - For any error message from an async closure, the handler wraps it in `DurableError::UserCode` and creates a FAIL checkpoint
    - **Validates: Requirements 5.4, 11.1, 11.2**

- [x] 3. Update DurableContext methods in sdk/src/context.rs
  - [x] 3.1 Update `step` method signature to use `F + Fut` pattern
    - Add `Fut` generic parameter
    - Change `F: StepFn<T>` to `F: FnOnce(StepContext) -> Fut + Send` and `Fut: Future<Output = Result<T, Box<dyn Error + Send + Sync>>> + Send`
    - Update doc comments and examples to show async closure usage
    - _Requirements: 1.1, 6.1, 8.1, 8.2, 9.1_

  - [x] 3.2 Update `step_named` method signature to use `F + Fut` pattern
    - Same signature changes as `step`
    - Update doc comments and examples
    - _Requirements: 1.2, 6.1, 8.1, 8.2, 9.1_

  - [x] 3.3 Update `wait_for_condition` method signature to use `F + Fut` pattern
    - Add `Fut` generic parameter
    - Change `F: Fn(&S, &WaitForConditionContext) -> Result<T, ...> + Send + Sync` to `F: Fn(&S, &WaitForConditionContext) -> Fut + Send + Sync` and `Fut: Future<Output = Result<T, Box<dyn Error + Send + Sync>>> + Send`
    - Update doc comments and examples
    - _Requirements: 1.3, 6.2, 8.3_

- [x] 4. Update wait_for_condition handler in sdk/src/handlers/wait_for_condition.rs
  - [x] 4.1 Update `wait_for_condition_handler` signature to use `F + Fut` pattern
    - Add `Fut` generic parameter with bounds `Fut: Future<Output = Result<T, Box<dyn Error + Send + Sync>>> + Send`
    - Change `F` bound from `Fn(&S, &WaitForConditionContext) -> Result<T, ...>` to `Fn(&S, &WaitForConditionContext) -> Fut`
    - Replace `check(&user_state, &check_ctx)` with `check(&user_state, &check_ctx).await`
    - _Requirements: 1.3, 1.4, 10.1, 10.2, 10.3, 10.4_

  - [x] 4.2 Update all unit tests in `handlers/wait_for_condition.rs` to use async closures
    - Wrap all sync check closures in async equivalents across all tests: `test_initial_condition_check_executed_on_new_operation`, `test_initial_check_receives_initial_state`, `test_retry_action_checkpointed_with_state_payload`, `test_succeed_action_checkpointed_when_condition_passes`, `test_suspension_when_replaying_pending_status`, `test_fail_action_checkpointed_when_retries_exhausted`, `test_retry_payload_contains_previous_state`, `test_condition_receives_initial_state_on_first_attempt`, `test_condition_function_error_triggers_retry`, `test_replay_succeeded_operation_returns_cached_result`, `test_replay_failed_operation_returns_error`, `test_non_deterministic_detection_wrong_operation_type`, `test_ready_status_continues_execution`
    - _Requirements: 1.3, 3.3, 3.4, 10.1, 10.2, 10.3_

  - [ ]* 4.3 Write property test for wait-for-condition replay skips closure execution
    - **Property 2: Wait-for-condition replay skips closure execution**
    - If a SUCCEED or FAIL checkpoint exists, the async check closure is not invoked and the cached result is returned unchanged
    - **Validates: Requirements 3.3, 3.4**

  - [ ]* 4.4 Write property test for wait-for-condition checkpoint correctness
    - **Property 8: Wait-for-condition checkpoint correctness**
    - Ok → SUCCEED checkpoint; Err + Continue → RETRY checkpoint; Err + Done → FAIL checkpoint
    - **Validates: Requirements 10.1, 10.2, 10.3**

- [x] 5. Checkpoint - Ensure SDK core compiles and unit tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 6. Update examples to use async closures
  - [x] 6.1 Update all example binaries in `examples/src/bin/` that use `ctx.step(...)` or `ctx.step_named(...)`
    - Wrap sync closures in `|ctx| async move { ... }` in all affected binaries across subdirectories: `hello_world/`, `step/`, `advanced/`, `comprehensive/`, `error_handling/`, `concurrency/`, `context_validation/`, `serde/`, `callback/`, `child_context/`, `invoke/`, `map/`, `parallel/`, `promise_combinators/`, `wait/`, `wait_for_condition/`, `multiple_waits/`, `replay_safe/`, `logging/`
    - Also update standalone binaries: `simple_workflow.rs`, `callback_workflow.rs`, `parallel_processing.rs`
    - _Requirements: 1.1, 1.2, 1.3_

  - [x] 6.2 Update `wait_for_condition` calls in examples to use async closures
    - Wrap sync check closures in async equivalents in `wait_for_condition/` and `multiple_waits/` examples
    - _Requirements: 1.3_

- [x] 7. Update integration tests to use async closures
  - [x] 7.1 Update `examples/tests/step_tests.rs` to use async closures
    - Wrap all step closures in `|ctx| async move { ... }`
    - _Requirements: 1.1, 1.2, 7.3_

  - [x] 7.2 Update `examples/tests/wait_tests.rs` to use async closures
    - Wrap all wait_for_condition check closures in async equivalents
    - _Requirements: 1.3, 10.1, 10.2, 10.3_

  - [x] 7.3 Update remaining integration test files to use async closures
    - Update `hello_world_tests.rs`, `comprehensive_tests.rs`, `advanced_tests.rs`, `error_handling_tests.rs`, `concurrency_tests.rs`, `context_validation_tests.rs`, `serde_tests.rs`, `callback_tests.rs`, `child_context_tests.rs`, `invoke_tests.rs`, `parallel_map_tests.rs`, `promise_combinator_tests.rs`
    - _Requirements: 1.1, 1.2, 7.3_

- [x] 8. Checkpoint - Ensure all examples compile and integration tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 9. Add new tests with genuinely async closures
  - [x] 9.1 Add unit test for step_handler with genuinely async closure
    - Add test in `handlers/step.rs` that uses `tokio::time::sleep` inside the async closure to verify `.await` works correctly end-to-end
    - _Requirements: 1.1, 1.4_

  - [x] 9.2 Add unit test for wait_for_condition_handler with genuinely async closure
    - Add test in `handlers/wait_for_condition.rs` that uses `tokio::time::sleep` inside the async check closure
    - _Requirements: 1.3, 1.4, 10.4_

  - [ ]* 9.3 Write property test for operation ID determinism
    - **Property 7: Operation ID determinism**
    - For any sequence of step operations at the same positions, operation IDs are identical regardless of closure type
    - **Validates: Requirement 9.1**

- [x] 10. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- The design uses Rust throughout — no language selection needed
- Existing JSON history files do NOT need modification since checkpoint format is unchanged (Requirement 7)
- Zero new dependencies required (Requirement 12) — only `std::future::Future` and existing `tokio`
- Sync closures wrapped in `async move { ... }` compile to zero-cost futures
