# Implementation Plan: Rust Cloud Testing Parity

## Overview

Close the cloud testing parity gap by adding history polling, operation tracking, and callback interaction to the Rust `CloudDurableTestRunner`. Implementation proceeds bottom-up: new types and traits first, then the `HistoryPoller`, then `OperationStorage` deduplication, then `OperationHandle` creation/notification, then `CloudCallbackSender`, and finally rewiring `CloudDurableTestRunner::run()` to use the polling loop.

## Tasks

- [x] 1. Define new types, traits, and test infrastructure
  - [x] 1.1 Create `history_poller.rs` with `HistoryApiClient` trait, `HistoryPage`, `PollResult`, and `TerminalState` structs
    - Define the `HistoryApiClient` async trait with `get_history(arn, marker) -> Result<HistoryPage, TestError>`
    - Define `HistoryPage` struct with fields: `events: Vec<HistoryEvent>`, `operations: Vec<Operation>`, `next_marker: Option<String>`, `is_terminal: bool`, `terminal_status: Option<ExecutionStatus>`, `terminal_result: Option<String>`, `terminal_error: Option<TestResultError>`
    - Define `PollResult` struct with fields: `operations: Vec<Operation>`, `events: Vec<HistoryEvent>`, `terminal: Option<TerminalState>`
    - Define `TerminalState` struct with fields: `status: ExecutionStatus`, `result: Option<String>`, `error: Option<TestResultError>`
    - Register the module in `lib.rs`
    - _Requirements: 1.1, 1.5, 2.1_

  - [x] 1.2 Create `MockHistoryApiClient` for testing
    - Implement a mock that records calls (arn, marker) and returns configurable `HistoryPage` responses from a `VecDeque`
    - Support configuring transient errors for retry testing
    - Place in `history_poller.rs` under `#[cfg(test)]` module
    - _Requirements: 1.1, 4.1_

- [x] 2. Implement `HistoryPoller` core logic
  - [x] 2.1 Implement `HistoryPoller::new()` and `HistoryPoller::call_with_retries()`
    - Constructor takes `api_client`, `arn`, `poll_interval`, sets `max_retries = 3`, `last_marker = None`
    - `call_with_retries` retries transient errors up to `max_retries` times with exponential backoff using formula `(attempt² - 1) * 150 + 1000` ms
    - Return error if all retries exhausted
    - _Requirements: 4.1, 4.2, 4.3_

  - [ ]* 2.2 Write property test for retry behavior (Property 9)
    - **Property 9: Retry with exponential backoff on transient errors**
    - Generate random failure counts (0–4) before success. Verify up to 3 failures are retried and succeed, 4+ failures propagate the error.
    - **Validates: Requirements 4.1, 4.3**

  - [x] 2.3 Implement `HistoryPoller::poll_once()` with pagination
    - Execute one poll cycle: call API with `last_marker`, paginate while `next_marker` is present (waiting `poll_interval` between pages), aggregate operations and events into `PollResult`
    - Update `last_marker` to the last page's `next_marker` for cross-cycle marker continuity
    - Detect terminal state from any page and include in `PollResult`
    - _Requirements: 1.1, 1.2, 1.5, 2.1, 2.2, 2.3, 2.4_

  - [ ]* 2.4 Write property test for pagination exhaustion (Property 4)
    - **Property 4: Pagination exhausts all pages within a poll cycle**
    - Generate random page counts (1–10) with random operations per page. Verify `poll_once` makes exactly N API calls and collects all operations.
    - **Validates: Requirements 2.1, 2.2**

  - [ ]* 2.5 Write property test for marker continuity (Property 5)
    - **Property 5: Marker continuity across poll cycles**
    - Generate random marker strings across 2–5 poll cycles. Verify each cycle starts with the previous cycle's last marker.
    - **Validates: Requirements 2.4**

  - [ ]* 2.6 Write property test for terminal state mapping (Property 2)
    - **Property 2: Terminal state stops polling and produces correct result**
    - Generate random terminal statuses and payloads. Verify `PollResult.terminal` matches the API response.
    - **Validates: Requirements 1.2, 1.5, 8.2, 8.3**

  - [ ]* 2.7 Write unit tests for `HistoryPoller` edge cases
    - Test empty execution (immediate terminal, no operations)
    - Test single-page single-cycle happy path
    - Test backoff timing formula for attempts 1, 2, 3
    - Test pagination delay between pages within a cycle
    - _Requirements: 1.2, 1.5, 2.3, 4.2_

- [x] 3. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 4. Implement `OperationStorage.add_or_update()` and deduplication
  - [x] 4.1 Add `add_or_update()` method to `OperationStorage` in `cloud_runner.rs`
    - If an operation with the same `operation_id` exists, update it in place; otherwise call `add_operation`
    - Use the existing `operations_by_id` HashMap for O(1) lookup
    - _Requirements: 3.2_

  - [ ]* 4.2 Write property test for operation deduplication (Property 7)
    - **Property 7: Operation deduplication by ID**
    - Generate random operations, then updates with same IDs but different statuses. Verify `add_or_update` keeps only the latest version and storage size equals unique ID count.
    - **Validates: Requirements 3.2**

  - [ ]* 4.3 Write property test for storage cleared between runs (Property 8)
    - **Property 8: Storage cleared between runs**
    - Pre-populate storage with random operations, call `clear()`, verify empty. Then add new operations and verify only new ones present.
    - **Validates: Requirements 3.4**

- [x] 5. Implement `OperationHandle` creation and notification on `CloudDurableTestRunner`
  - [x] 5.1 Add `handles` field and `get_operation_handle` methods to `CloudDurableTestRunner`
    - Add `handles: Vec<OperationHandle>` field
    - Implement `get_operation_handle(name)`, `get_operation_handle_by_index(index)`, `get_operation_handle_by_name_and_index(name, index)`, `get_operation_handle_by_id(id)` methods that create and register handles
    - Each handle shares the runner's `all_operations: Arc<RwLock<Vec<Operation>>>` for child enumeration
    - _Requirements: 5.1, 5.2, 5.3, 5.4_

  - [x] 5.2 Implement `notify_handles()` method on `CloudDurableTestRunner`
    - Iterate registered handles, match against `OperationStorage`, populate/update via `inner` + `status_tx` watch channel
    - Update shared `all_operations` Arc for child enumeration
    - _Requirements: 5.5_

  - [ ]* 5.3 Write property test for handle matcher correctness (Property 10)
    - **Property 10: Operation handle creation returns correct matcher**
    - Generate random names, indices, and IDs. Verify the returned handle's matcher matches the requested lookup criteria.
    - **Validates: Requirements 5.1, 5.2, 5.3, 5.4**

  - [ ]* 5.4 Write property test for handle notification (Property 11)
    - **Property 11: Handle notification on new operation data**
    - Create handles with various matchers, populate storage with matching operations, call `notify_handles`, verify handles are populated.
    - **Validates: Requirements 5.5**

- [x] 6. Implement `LambdaHistoryApiClient` and `CloudCallbackSender`
  - [x] 6.1 Implement `LambdaHistoryApiClient` struct
    - Wraps `LambdaClient`, implements `HistoryApiClient` trait
    - Calls `get_durable_execution_history()` on the client, parses response into `HistoryPage`
    - Detect terminal events from response and set `is_terminal`, `terminal_status`, `terminal_result`, `terminal_error`
    - _Requirements: 1.1, 1.3_

  - [x] 6.2 Implement `CloudCallbackSender` struct
    - Wraps `LambdaClient` and `durable_execution_arn`
    - Implements `CallbackSender` trait with `send_success`, `send_failure`, `send_heartbeat` methods
    - Each method calls the appropriate API endpoint with the callback ID and payload
    - _Requirements: 6.1, 6.2, 6.3, 6.4_

  - [ ]* 6.3 Write property test for callback delegation (Property 12)
    - **Property 12: Callback signals delegate to CallbackSender**
    - Generate random callback IDs and payloads. Verify `send_callback_success`, `send_callback_failure`, `send_callback_heartbeat` delegate to the sender with correct arguments.
    - **Validates: Requirements 6.1, 6.2, 6.3**

- [x] 7. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 8. Rewrite `CloudDurableTestRunner::run()` with polling loop
  - [x] 8.1 Rewrite `run()` method to invoke Lambda, create `HistoryPoller`, and poll with timeout
    - Clear `OperationStorage` at start
    - Invoke Lambda, extract `DurableExecutionArn` (return error if invocation fails, no polling)
    - Create `HistoryPoller` with `LambdaHistoryApiClient`, ARN, and configured `poll_interval`
    - Create `CloudCallbackSender` with ARN and configure handles with it
    - Poll loop: sleep `poll_interval`, call `poll_once`, populate storage via `add_or_update`, call `notify_handles`, collect history events, check terminal or timeout
    - On terminal: construct `TestResult` with correct status, result/error, operations from storage, and all history events
    - On timeout: return `TestResult` with `ExecutionStatus::TimedOut`
    - Stop poller in all exit paths (success, failure, timeout)
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 3.1, 3.3, 3.4, 7.1, 7.2, 8.1, 8.2, 8.3, 8.4, 8.5, 9.1, 9.2, 9.3_

  - [ ]* 8.2 Write property test for timeout behavior (Property 3)
    - **Property 3: Timeout produces TimedOut result**
    - Configure short timeouts with a mock that never returns terminal. Verify `TestResult` has `ExecutionStatus::TimedOut`.
    - **Validates: Requirements 1.4**

  - [ ]* 8.3 Write property test for configuration propagation (Property 13)
    - **Property 13: Configuration propagation**
    - Generate random `Duration` values for `poll_interval` and `timeout`. Verify the poller and runner use them.
    - **Validates: Requirements 7.1, 7.2, 7.3, 7.4**

  - [ ]* 8.4 Write property test for history event ordering (Property 14)
    - **Property 14: History events collected in chronological order**
    - Generate random events with random timestamps across multiple pages/cycles. Verify the final `TestResult` events are in chronological order.
    - **Validates: Requirements 9.1, 9.2, 9.3**

  - [ ]* 8.5 Write property test for all polled operations in TestResult (Property 6)
    - **Property 6: All polled operations appear in TestResult**
    - Generate random operations across multiple poll cycles. Verify every operation appears in the final `TestResult` and no extra operations are present.
    - **Validates: Requirements 3.1, 3.3**

  - [ ]* 8.6 Write unit tests for `run()` integration scenarios
    - Test Lambda invocation failure (no ARN) returns error without polling
    - Test successful execution end-to-end with mock
    - Test failed execution returns `TestResult` with `Failed` status and error
    - Test callback sender wiring: verify handles get a `CallbackSender` after ARN is known
    - Test storage clear on re-run: pre-populate storage, call `run()`, verify old operations gone
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5, 3.4, 6.4_

  - [ ]* 8.7 Write property test for polling ARN and interval (Property 1)
    - **Property 1: Polling uses correct ARN and interval**
    - Generate random ARN strings and intervals. Verify every API call uses the exact ARN.
    - **Validates: Requirements 1.1, 1.3**

- [x] 9. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties from the design document
- Unit tests validate specific examples and edge cases
- All new code goes in `aws-durable-execution-sdk/testing/src/` with `history_poller.rs` as the only new file
- The `proptest` crate is already a dev-dependency in the testing crate
- Existing `proptest` strategies for `Operation`, `OperationType`, `OperationStatus` in `test_result.rs` and `operation.rs` should be reused
