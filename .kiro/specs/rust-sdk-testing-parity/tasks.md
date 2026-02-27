# Implementation Plan: Rust SDK Testing Parity

## Overview

This plan implements the new abstractions (`OperationHandle`, `InvokeRequest`, `RunFuture`) and wires them into the existing `LocalDurableTestRunner` and `TestExecutionOrchestrator` pipeline. Existing functionality (child operations, `wait_for_data`, `reset`, function registration, `TestResult` accessors) is already implemented — tasks focus on the new lazy handle registration, non-blocking run, and structured input wrapper.

## Tasks

- [x] 1. Define `InvokeRequest<T>` struct and `From` impl
  - [x] 1.1 Create `InvokeRequest<T>` in `aws-durable-execution-sdk/testing/src/types.rs`
    - Add `InvokeRequest<T>` with optional `payload: Option<T>`, `Default`, `Serialize`, `Deserialize` derives
    - Implement `InvokeRequest::new()` and `InvokeRequest::with_payload(T)`
    - Implement `From<T> for InvokeRequest<T>` so raw payloads still work with `run()`
    - Re-export `InvokeRequest` from `lib.rs`
    - _Requirements: 9.1, 9.2, 9.3_

  - [ ]* 1.2 Write property test for `InvokeRequest` payload round-trip
    - **Property 14: InvokeRequest payload round-trip**
    - **Validates: Requirements 9.1, 9.2, 9.3**
    - Generate random JSON-serializable values, wrap in `InvokeRequest::with_payload`, verify payload is `Some(value)`
    - Verify `InvokeRequest::new()` has `None` payload

- [x] 2. Define `OperationHandle` and `OperationMatcher`
  - [x] 2.1 Create `OperationHandle` struct in a new file `aws-durable-execution-sdk/testing/src/operation_handle.rs`
    - Define `OperationMatcher` enum with `ByName(String)`, `ByIndex(usize)`, `ById(String)` variants
    - Define `OperationHandle` struct with fields: `matcher`, `inner: Arc<RwLock<Option<Operation>>>`, `status_tx`/`status_rx` (`tokio::sync::watch`), `callback_sender: Option<Arc<dyn CallbackSender>>`, `all_operations: Arc<RwLock<Vec<Operation>>>`
    - Implement `OperationHandle::new(matcher, all_operations)` constructor
    - Implement `Clone` for `OperationHandle`
    - Add `pub mod operation_handle;` to `lib.rs` and re-export `OperationHandle`
    - _Requirements: 1.1, 1.7, 1.8_

  - [x] 2.2 Implement inspection methods on `OperationHandle`
    - Implement `get_id()`, `get_name()`, `get_type()`, `get_status()`, `get_step_details()`, `get_callback_details()`, `get_wait_details()`, `get_invoke_details()`, `get_context_details()`
    - Each method reads `inner` via `RwLock`, returns `Err(TestError::OperationNotFound(...))` if `None`
    - When populated, delegate to a `DurableOperation` constructed from the inner `Operation`
    - Implement `is_populated()` helper
    - _Requirements: 1.3, 1.6_

  - [x] 2.3 Implement `wait_for_data(status)` on `OperationHandle`
    - Subscribe to `status_rx` watch channel
    - Resolve immediately if the operation has already reached the target `WaitingOperationStatus`
    - If the watch channel closes (execution ended) before target status, return `Err(TestError::ExecutionCompletedEarly(...))`
    - _Requirements: 1.4, 4.1, 4.2, 4.3, 4.5_

  - [x] 2.4 Implement callback methods on `OperationHandle`
    - Implement `send_callback_success(result)`, `send_callback_failure(error)`, `send_callback_heartbeat()`
    - Validate handle is populated and is a callback operation; return `Err(TestError::NotCallbackOperation)` otherwise
    - Validate callback_id is available (Submitted status); return `Err(TestError::ResultNotAvailable(...))` if not ready
    - Delegate to `callback_sender`
    - _Requirements: 1.5, 5.4_

  - [x] 2.5 Implement `get_child_operations()` on `OperationHandle`
    - Read `all_operations` via `RwLock`, filter by `parent_id` matching this handle's operation ID
    - Return `Vec<DurableOperation>` preserving execution order
    - Return error if handle is unpopulated
    - _Requirements: 3.1, 3.2, 3.3, 3.4_

  - [ ]* 2.6 Write property test: unpopulated handles error on inspection
    - **Property 1: Unpopulated handles error on inspection**
    - **Validates: Requirements 1.1, 1.6**
    - Generate random matcher types, verify all inspection methods return `Err(TestError::OperationNotFound(...))`

  - [ ]* 2.7 Write property test: handle population matches correctly
    - **Property 2: Handle population matches correctly**
    - **Validates: Requirements 1.2, 1.7, 1.8**
    - Generate random `Vec<Operation>` and matchers, populate handles, verify correct operation is matched

  - [ ]* 2.8 Write property test: populated handle delegates to DurableOperation
    - **Property 3: Populated handle delegates to DurableOperation**
    - **Validates: Requirements 1.3, 1.4, 1.5**
    - Generate random operations, populate handles, compare inspection results with `DurableOperation`

- [x] 3. Define `RunFuture<O>` wrapper
  - [x] 3.1 Create `RunFuture<O>` in `aws-durable-execution-sdk/testing/src/run_future.rs`
    - Define `RunFuture<O>` wrapping `tokio::task::JoinHandle<Result<TestResult<O>, TestError>>`
    - Implement `Future` for `RunFuture<O>` delegating to the inner `JoinHandle`
    - Handle `JoinError` by converting to `TestError::CheckpointServerError`
    - Add `pub mod run_future;` to `lib.rs` and re-export `RunFuture`
    - _Requirements: 2.1_

- [x] 4. Checkpoint — Ensure all new types compile and unit tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 5. Integrate handle registration into `LocalDurableTestRunner`
  - [x] 5.1 Add handle registry fields to `LocalDurableTestRunner`
    - Add `registered_handles: Vec<OperationHandle>` and `shared_operations: Arc<RwLock<Vec<Operation>>>` fields to the struct in `aws-durable-execution-sdk/testing/src/local_runner.rs`
    - Initialize in `new()`, `with_mock_client()`, and `with_checkpoint_params()` constructors
    - _Requirements: 1.1_

  - [x] 5.2 Implement `get_operation_handle(name)`, `get_operation_handle_by_index(index)`, `get_operation_handle_by_id(id)`
    - Each method creates an `OperationHandle` with the appropriate `OperationMatcher`, stores it in `registered_handles`, and returns a clone
    - _Requirements: 1.1, 1.7, 1.8_

  - [x] 5.3 Update `reset()` to clear registered handles
    - Clear `registered_handles` vec and reset `shared_operations` to empty
    - _Requirements: 6.2_

  - [ ]* 5.4 Write property test: reset produces clean state
    - **Property 11: Reset produces clean state**
    - **Validates: Requirements 6.1, 6.2, 6.3, 6.4**
    - Register random handles, call `reset()`, verify handles list is empty and shared operations cleared

- [x] 6. Wire handle population into the orchestrator execution loop
  - [x] 6.1 Pass registered handles to `TestExecutionOrchestrator`
    - Modify `execute_handler()` or `run_with_orchestrator()` in `local_runner.rs` to pass `registered_handles` and `shared_operations` to the orchestrator
    - In the orchestrator's operation processing loop (`process_operations` / `process_operation` in `orchestrator.rs`), when an operation is added or updated, check all registered handles for a match
    - When a match is found: write the `Operation` into the handle's `inner`, send status update via `status_tx`
    - _Requirements: 1.2, 2.2_

  - [x] 6.2 Update `run()` to accept `InvokeRequest` and return `RunFuture`
    - Change `run()` signature to accept `impl Into<InvokeRequest<I>>`
    - Extract payload from `InvokeRequest` (use default empty value if `None`)
    - Spawn the execution as a tokio task, wrap the `JoinHandle` in `RunFuture`
    - Return `RunFuture<O>` so callers can `await` concurrently with handle interactions
    - Maintain backward compatibility: `From<I> for InvokeRequest<I>` ensures existing `run(payload)` calls still work
    - _Requirements: 2.1, 9.1, 9.2, 9.3_

  - [x] 6.3 Populate `shared_operations` during execution
    - When the orchestrator adds/updates operations, also push them into `shared_operations: Arc<RwLock<Vec<Operation>>>`
    - This enables `get_child_operations()` on handles to work during and after execution
    - _Requirements: 3.1, 2.2_

- [x] 7. Checkpoint — Ensure handle registration and population work end-to-end
  - Ensure all tests pass, ask the user if questions arise.

- [x] 8. Integration tests for the concurrent callback pattern
  - [x] 8.1 Write integration test: single callback round-trip
    - In `aws-durable-execution-sdk/testing/tests/`, create a test handler that creates a callback operation and waits for its result
    - Pre-register handle with `get_operation_handle("callback-op")`
    - Call `run()`, `await` handle's `wait_for_data(Submitted)`, send callback success, `await` the `RunFuture`
    - Assert `TestResult` contains the callback result
    - _Requirements: 2.3, 2.5, 5.1_

  - [x] 8.2 Write integration test: callback failure propagation
    - Same pattern as 8.1 but send `send_callback_failure(error)` instead
    - Assert the handler receives the failure
    - _Requirements: 2.4, 5.2_

  - [x] 8.3 Write integration test: multiple concurrent callbacks
    - Handler creates 2+ callback operations concurrently
    - Pre-register handles for each, send independent responses
    - Assert each callback receives its own response with no cross-contamination
    - _Requirements: 5.3_

  - [ ]* 8.4 Write property test: callback testing pattern round-trip
    - **Property 7: Callback testing pattern round-trip**
    - **Validates: Requirements 2.3, 2.4, 2.5, 5.1, 5.2**
    - Generate random callback result strings, run the pattern, verify round-trip

  - [ ]* 8.5 Write property test: multiple concurrent callbacks are independent
    - **Property 8: Multiple concurrent callbacks are independent**
    - **Validates: Requirements 5.3**
    - Generate random N (2-5) callback result strings, verify independence

  - [ ]* 8.6 Write property test: premature callback send errors
    - **Property 9: Premature callback send errors**
    - **Validates: Requirements 5.4**
    - Attempt `send_callback_success` before `Submitted` status, verify error

  - [ ]* 8.7 Write property test: execution-completed-early error
    - **Property 10: Execution completed early error**
    - **Validates: Requirements 2.6**
    - Run a handler that completes quickly, `wait_for_data` on a non-existent operation, verify error

- [ ] 9. Property tests for existing functionality (child ops, wait_for_data, function registration, TestResult)
  - [ ]* 9.1 Write property test: child operations filtered by parent_id and ordered
    - **Property 4: Child operations filtered by parent_id and ordered**
    - **Validates: Requirements 3.1, 3.2, 3.4**
    - Generate random operation trees with parent_id relationships, verify filtering and order

  - [ ]* 9.2 Write property test: recursive child enumeration
    - **Property 5: Recursive child enumeration**
    - **Validates: Requirements 3.3**
    - Generate random operation trees of depth 1-3, verify recursive `get_child_operations()`

  - [ ]* 9.3 Write property test: wait_for_data resolves immediately for reached statuses
    - **Property 6: wait_for_data resolves immediately for reached statuses**
    - **Validates: Requirements 4.1, 4.2, 4.3, 4.5**
    - Generate random `OperationStatus` × `WaitingOperationStatus` combinations, verify immediate resolution

  - [ ]* 9.4 Write property test: function registration round-trip
    - **Property 12: Function registration round-trip**
    - **Validates: Requirements 7.1, 7.2, 7.5**
    - Generate random function names, register, verify `has_registered_function`, clear, verify gone

  - [ ]* 9.5 Write property test: TestResult accessor consistency
    - **Property 13: TestResult accessor consistency**
    - **Validates: Requirements 8.1, 8.2, 8.3, 8.4**
    - Generate random `TestResult` (success/failure), verify `get_result()` and `get_error()` are mutually exclusive

- [x] 10. Final checkpoint — Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Existing functionality (child operations, `wait_for_data`, `reset`, function registration, `TestResult` accessors) is already implemented — tasks focus on new code
- The `run()` signature change uses `impl Into<InvokeRequest<I>>` for backward compatibility
- Property tests use `proptest` (already a dev-dependency) with `ProptestConfig::with_cases(100)`
- Each property test is tagged with `// Feature: rust-sdk-testing-parity, Property {N}: {title}`
