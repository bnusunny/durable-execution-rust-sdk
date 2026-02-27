# Implementation Plan: Rust SDK Example Coverage

## Overview

Add ~45 new example binaries, replay tests, and history files across 5 phases to close the example coverage gap with the JS SDK. All SDK APIs already exist — this is purely additive example and test code. Each task creates an Example Suite (binary + `[[bin]]` entry + replay test + history file generation). History files are auto-generated via `GENERATE_HISTORY=true`.

## Tasks

- [x] 1. Phase 1: Wait-for-Callback Examples (7 examples)
  - [x] 1.1 Create `wait_for_callback/timeout` example and replay test
    - Create `examples/src/bin/callback/wait_for_callback/wfc_timeout.rs` with a handler that configures `CallbackConfig { timeout: Duration }` and exercises the timeout path
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_wfc_timeout` in `examples/tests/callback_tests.rs` using `assert_nodejs_event_signatures()`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/wfc_timeout.history.json`
    - _Requirements: 1.1, 1.8, 11.1–11.7_

  - [x] 1.2 Create `wait_for_callback/failure` example and replay test
    - Create `examples/src/bin/callback/wait_for_callback/wfc_failure.rs` with a handler where the callback submitter returns an error and the handler catches the callback failure
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_wfc_failure` in `examples/tests/callback_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/wfc_failure.history.json`
    - _Requirements: 1.2, 1.8, 11.1–11.7_

  - [x] 1.3 Create `wait_for_callback/heartbeat` example and replay test
    - Create `examples/src/bin/callback/wait_for_callback/wfc_heartbeat.rs` with a handler demonstrating a callback submitter that sends heartbeat signals via `send_heartbeat()` before completing
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_wfc_heartbeat` in `examples/tests/callback_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/wfc_heartbeat.history.json`
    - _Requirements: 1.3, 1.8, 11.1–11.7_

  - [x] 1.4 Create `wait_for_callback/nested` example and replay test
    - Create `examples/src/bin/callback/wait_for_callback/wfc_nested.rs` with a handler demonstrating `wait_for_callback` called inside a child context
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_wfc_nested` in `examples/tests/callback_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/wfc_nested.history.json`
    - _Requirements: 1.4, 1.8, 11.1–11.7_

  - [x] 1.5 Create `wait_for_callback/multiple_invocations` example and replay test
    - Create `examples/src/bin/callback/wait_for_callback/wfc_multiple.rs` with a handler demonstrating multiple `wait_for_callback` calls within a single handler execution
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_wfc_multiple` in `examples/tests/callback_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/wfc_multiple.history.json`
    - _Requirements: 1.5, 1.8, 11.1–11.7_

  - [x] 1.6 Create `wait_for_callback/child_context` example and replay test
    - Create `examples/src/bin/callback/wait_for_callback/wfc_child_context.rs` with a handler demonstrating `wait_for_callback` used within `ctx.run_in_child_context()`
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_wfc_child_context` in `examples/tests/callback_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/wfc_child_context.history.json`
    - _Requirements: 1.6, 1.8, 11.1–11.7_

  - [x] 1.7 Create `wait_for_callback/custom_serdes` example and replay test
    - Create `examples/src/bin/callback/wait_for_callback/wfc_custom_serdes.rs` with a handler demonstrating `wait_for_callback` with a custom `SerDesAny` implementation for the callback payload
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_wfc_custom_serdes` in `examples/tests/callback_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/wfc_custom_serdes.history.json`
    - _Requirements: 1.7, 1.8, 11.1–11.7_

- [x] 2. Phase 1: Callback Creation Examples (4 examples)
  - [x] 2.1 Create `callback/heartbeat` example and replay test
    - Create `examples/src/bin/callback/heartbeat/callback_heartbeat.rs` with a handler demonstrating dedicated heartbeat usage with `CallbackConfig::heartbeat_timeout` and periodic heartbeat sends
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_callback_heartbeat` in `examples/tests/callback_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/callback_heartbeat.history.json`
    - _Requirements: 2.1, 2.5, 11.1–11.7_

  - [x] 2.2 Create `callback/failure` example and replay test
    - Create `examples/src/bin/callback/failure/callback_failure.rs` with a handler demonstrating a callback that receives a failure response and the handler processing the error
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_callback_failure` in `examples/tests/callback_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/callback_failure.history.json`
    - _Requirements: 2.2, 2.5, 11.1–11.7_

  - [x] 2.3 Create `callback/custom_serdes` example and replay test
    - Create `examples/src/bin/callback/custom_serdes/callback_custom_serdes.rs` with a handler demonstrating `create_callback` with a custom `SerDesAny` implementation for non-JSON payload formats
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_callback_custom_serdes` in `examples/tests/callback_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/callback_custom_serdes.history.json`
    - _Requirements: 2.3, 2.5, 11.1–11.7_

  - [x] 2.4 Create `callback/mixed_ops` example and replay test
    - Create `examples/src/bin/callback/mixed_ops/callback_mixed_ops.rs` with a handler that combines `create_callback` with steps, waits, and parallel operations in a single workflow
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_callback_mixed_ops` in `examples/tests/callback_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/callback_mixed_ops.history.json`
    - _Requirements: 2.4, 2.5, 11.1–11.7_

- [x] 3. Phase 1 Checkpoint
  - Ensure all 11 callback/wait-for-callback examples compile, replay tests pass, and history files are generated. Ask the user if questions arise.

- [x] 4. Phase 2: Parallel Operation Examples (8 examples)
  - [x] 4.1 Create `parallel/empty_array` example and replay test
    - Create `examples/src/bin/parallel/empty_array/parallel_empty_array.rs` with a handler demonstrating `ctx.parallel()` called with an empty task vector, showing `BatchResult::empty()` behavior
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_parallel_empty_array` in `examples/tests/parallel_map_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/parallel_empty_array.history.json`
    - _Requirements: 3.1, 3.9, 11.1–11.7_

  - [x] 4.2 Create `parallel/with_wait` example and replay test
    - Create `examples/src/bin/parallel/with_wait/parallel_with_wait.rs` with a handler demonstrating parallel tasks that include `ctx.wait()` calls within their branches
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_parallel_with_wait` in `examples/tests/parallel_map_tests.rs` using `assert_nodejs_event_signatures_unordered()`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/parallel_with_wait.history.json`
    - _Requirements: 3.2, 3.9, 11.1–11.7_

  - [x] 4.3 Create `parallel/error_preservation` example and replay test
    - Create `examples/src/bin/parallel/error_preservation/parallel_error_preservation.rs` with a handler demonstrating that individual task errors are preserved in `BatchItem::get_error()` when some branches fail
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_parallel_error_preservation` in `examples/tests/parallel_map_tests.rs` using `assert_nodejs_event_signatures_unordered()`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/parallel_error_preservation.history.json`
    - _Requirements: 3.3, 3.9, 11.1–11.7_

  - [x] 4.4 Create `parallel/min_successful` example and replay test
    - Create `examples/src/bin/parallel/min_successful/parallel_min_successful.rs` with a handler demonstrating `CompletionConfig::with_min_successful(n)` where the parallel completes after n successes
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_parallel_min_successful` in `examples/tests/parallel_map_tests.rs` using `assert_nodejs_event_signatures_unordered()`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/parallel_min_successful.history.json`
    - _Requirements: 3.4, 3.9, 11.1–11.7_

  - [x] 4.5 Create `parallel/min_successful_with_callback` example and replay test
    - Create `examples/src/bin/parallel/min_successful_callback/parallel_min_successful_callback.rs` with a handler demonstrating `CompletionConfig::with_min_successful(n)` combined with callback operations in parallel branches
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_parallel_min_successful_callback` in `examples/tests/parallel_map_tests.rs` using `assert_nodejs_event_signatures_unordered()`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/parallel_min_successful_callback.history.json`
    - _Requirements: 3.5, 3.9, 11.1–11.7_

  - [x] 4.6 Create `parallel/tolerated_failure_count` example and replay test
    - Create `examples/src/bin/parallel/tolerated_failure_count/parallel_tolerated_failure_count.rs` with a handler demonstrating `CompletionConfig` with `tolerated_failure_count` set, where failures up to the threshold do not cause overall failure
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_parallel_tolerated_failure_count` in `examples/tests/parallel_map_tests.rs` using `assert_nodejs_event_signatures_unordered()`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/parallel_tolerated_failure_count.history.json`
    - _Requirements: 3.6, 3.9, 11.1–11.7_

  - [x] 4.7 Create `parallel/tolerated_failure_percentage` example and replay test
    - Create `examples/src/bin/parallel/tolerated_failure_pct/parallel_tolerated_failure_pct.rs` with a handler demonstrating `CompletionConfig` with `tolerated_failure_percentage` set
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_parallel_tolerated_failure_pct` in `examples/tests/parallel_map_tests.rs` using `assert_nodejs_event_signatures_unordered()`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/parallel_tolerated_failure_pct.history.json`
    - _Requirements: 3.7, 3.9, 11.1–11.7_

  - [x] 4.8 Create `parallel/failure_threshold_exceeded` example and replay test
    - Create `examples/src/bin/parallel/failure_threshold_exceeded/parallel_failure_threshold_exceeded.rs` with a handler demonstrating the behavior when failures exceed the configured tolerance, causing the parallel operation to fail
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_parallel_failure_threshold_exceeded` in `examples/tests/parallel_map_tests.rs` using `assert_nodejs_event_signatures_unordered()`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/parallel_failure_threshold_exceeded.history.json`
    - _Requirements: 3.8, 3.9, 11.1–11.7_

- [x] 5. Phase 2: Map Operation Examples (4 examples)
  - [x] 5.1 Create `map/empty_array` example and replay test
    - Create `examples/src/bin/map/empty_array/map_empty_array.rs` with a handler demonstrating `ctx.map()` called with an empty input vector, showing `BatchResult::empty()` behavior
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_map_empty_array` in `examples/tests/parallel_map_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/map_empty_array.history.json`
    - _Requirements: 4.1, 4.5, 11.1–11.7_

  - [x] 5.2 Create `map/error_preservation` example and replay test
    - Create `examples/src/bin/map/error_preservation/map_error_preservation.rs` with a handler demonstrating that individual item errors are preserved in `BatchItem::get_error()` when some items fail during mapping
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_map_error_preservation` in `examples/tests/parallel_map_tests.rs` using `assert_nodejs_event_signatures_unordered()`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/map_error_preservation.history.json`
    - _Requirements: 4.2, 4.5, 11.1–11.7_

  - [x] 5.3 Create `map/high_concurrency` example and replay test
    - Create `examples/src/bin/map/high_concurrency/map_high_concurrency.rs` with a handler demonstrating `MapConfig` with `max_concurrency` set to 10+ processing many items concurrently
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_map_high_concurrency` in `examples/tests/parallel_map_tests.rs` using `assert_nodejs_event_signatures_unordered()`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/map_high_concurrency.history.json`
    - _Requirements: 4.3, 4.5, 11.1–11.7_

  - [x] 5.4 Create `map/large_scale` example and replay test
    - Create `examples/src/bin/map/large_scale/map_large_scale.rs` with a handler demonstrating `ctx.map()` processing 50+ items with `ItemBatcher` configured for batch processing
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_map_large_scale` in `examples/tests/parallel_map_tests.rs` using `assert_nodejs_event_signatures_unordered()`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/map_large_scale.history.json`
    - _Requirements: 4.4, 4.5, 11.1–11.7_

- [x] 6. Phase 2 Checkpoint
  - Ensure all 12 parallel and map examples compile, replay tests pass, and history files are generated. Ask the user if questions arise.

- [x] 7. Phase 3: Promise Combinator Examples (3 examples)
  - [x] 7.1 Create `promise_combinators/all_with_wait` example and replay test
    - Create `examples/src/bin/promise_combinators/all_with_wait/all_with_wait.rs` with a handler demonstrating `ctx.all()` where some futures include `ctx.wait()` calls
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_all_with_wait` in `examples/tests/promise_combinator_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/all_with_wait.history.json`
    - _Requirements: 5.1, 5.4, 11.1–11.7_

  - [x] 7.2 Create `promise_combinators/race_with_timeout` example and replay test
    - Create `examples/src/bin/promise_combinators/race_with_timeout/race_with_timeout.rs` with a handler demonstrating `ctx.race()` used as a timeout pattern where one future is a `ctx.wait()` and another is a long-running operation
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_race_with_timeout` in `examples/tests/promise_combinator_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/race_with_timeout.history.json`
    - _Requirements: 5.2, 5.4, 11.1–11.7_

  - [x] 7.3 Create `promise_combinators/replay_behavior` example and replay test
    - Create `examples/src/bin/promise_combinators/replay_behavior/replay_behavior.rs` with a handler demonstrating that combinator results are deterministic across replays by using multiple combinators in sequence
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_replay_behavior` in `examples/tests/promise_combinator_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/replay_behavior.history.json`
    - _Requirements: 5.3, 5.4, 11.1–11.7_

- [x] 8. Phase 3: Child Context Examples (3 examples)
  - [x] 8.1 Create `child_context/large_data` example and replay test
    - Create `examples/src/bin/child_context/large_data/child_context_large_data.rs` with a handler demonstrating a child context that produces a large result, exercising `ChildConfig::set_summary_generator()` when the result exceeds 256KB
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_child_context_large_data` in `examples/tests/child_context_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/child_context_large_data.history.json`
    - _Requirements: 6.1, 6.4, 11.1–11.7_

  - [x] 8.2 Create `child_context/checkpoint_size_limit` example and replay test
    - Create `examples/src/bin/child_context/checkpoint_size_limit/child_context_checkpoint_size_limit.rs` with a handler demonstrating behavior when a child context result approaches the checkpoint size limit, using `ChildConfig::set_summary_generator()` as mitigation
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_child_context_checkpoint_size_limit` in `examples/tests/child_context_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/child_context_checkpoint_size_limit.history.json`
    - _Requirements: 6.2, 6.4, 11.1–11.7_

  - [x] 8.3 Create `child_context/with_failing_step` example and replay test
    - Create `examples/src/bin/child_context/with_failing_step/child_context_with_failing_step.rs` with a handler demonstrating error propagation when a step inside a child context fails, including `ChildConfig::set_error_mapper()` usage
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_child_context_with_failing_step` in `examples/tests/child_context_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/child_context_with_failing_step.history.json`
    - _Requirements: 6.3, 6.4, 11.1–11.7_

- [x] 9. Phase 3 Checkpoint
  - Ensure all 6 promise combinator and child context examples compile, replay tests pass, and history files are generated. Ask the user if questions arise.

- [x] 10. Phase 4: Context Validation Examples (3 examples, new category)
  - [x] 10.1 Create `context_validation/parent_in_child` example, test file, and replay test
    - Create `examples/src/bin/context_validation/parent_in_child/context_validation_parent_in_child.rs` with a handler demonstrating that using a parent `DurableContext` inside `run_in_child_context` produces an error
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Create new `examples/tests/context_validation_tests.rs` test file with `test_context_validation_parent_in_child` asserting `ExecutionStatus::Failed`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/context_validation_parent_in_child.history.json`
    - _Requirements: 7.1, 7.4, 11.1–11.7_

  - [x] 10.2 Create `context_validation/parent_in_step` example and replay test
    - Create `examples/src/bin/context_validation/parent_in_step/context_validation_parent_in_step.rs` with a handler demonstrating that using a parent `DurableContext` to call durable operations inside a `step` closure produces an error
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_context_validation_parent_in_step` in `examples/tests/context_validation_tests.rs` asserting `ExecutionStatus::Failed`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/context_validation_parent_in_step.history.json`
    - _Requirements: 7.2, 7.4, 11.1–11.7_

  - [x] 10.3 Create `context_validation/parent_in_wait_condition` example and replay test
    - Create `examples/src/bin/context_validation/parent_in_wait_condition/context_validation_parent_in_wait_condition.rs` with a handler demonstrating that using a parent `DurableContext` inside a `wait_for_condition` predicate produces an error
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_context_validation_parent_in_wait_condition` in `examples/tests/context_validation_tests.rs` asserting `ExecutionStatus::Failed`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/context_validation_parent_in_wait_condition.history.json`
    - _Requirements: 7.3, 7.4, 11.1–11.7_

- [x] 11. Phase 4: Concurrency Examples (4 examples, new category)
  - [x] 11.1 Create `concurrency/concurrent_operations` example, test file, and replay test
    - Create `examples/src/bin/concurrency/concurrent_operations/concurrent_operations.rs` with a handler demonstrating multiple `ctx.step()` calls executed concurrently via `tokio::join!`
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Create new `examples/tests/concurrency_tests.rs` test file with `test_concurrent_operations` using `assert_nodejs_event_signatures_unordered()`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/concurrent_operations.history.json`
    - _Requirements: 8.1, 8.5, 11.1–11.7_

  - [x] 11.2 Create `concurrency/concurrent_wait` example and replay test
    - Create `examples/src/bin/concurrency/concurrent_wait/concurrent_wait.rs` with a handler demonstrating multiple `ctx.wait()` calls executed concurrently
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_concurrent_wait` in `examples/tests/concurrency_tests.rs` using `assert_nodejs_event_signatures_unordered()`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/concurrent_wait.history.json`
    - _Requirements: 8.2, 8.5, 11.1–11.7_

  - [x] 11.3 Create `concurrency/concurrent_callback_wait` example and replay test
    - Create `examples/src/bin/concurrency/concurrent_callback_wait/concurrent_callback_wait.rs` with a handler demonstrating a `ctx.wait_for_callback()` and a `ctx.wait()` running concurrently via `tokio::select!`
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_concurrent_callback_wait` in `examples/tests/concurrency_tests.rs` using `assert_nodejs_event_signatures_unordered()`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/concurrent_callback_wait.history.json`
    - _Requirements: 8.3, 8.5, 11.1–11.7_

  - [x] 11.4 Create `concurrency/concurrent_callback_submitter` example and replay test
    - Create `examples/src/bin/concurrency/concurrent_callback_submitter/concurrent_callback_submitter.rs` with a handler demonstrating multiple callback operations with independent submitters running concurrently
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_concurrent_callback_submitter` in `examples/tests/concurrency_tests.rs` using `assert_nodejs_event_signatures_unordered()`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/concurrent_callback_submitter.history.json`
    - _Requirements: 8.4, 8.5, 11.1–11.7_

- [x] 12. Phase 4 Checkpoint
  - Ensure all 7 context validation and concurrency examples compile, replay tests pass, and history files are generated. Ask the user if questions arise.

- [x] 13. Phase 5: Step Operation Examples (3 examples)
  - [x] 13.1 Create `step/retry_exponential_backoff` example and replay test
    - Create `examples/src/bin/step/retry_exponential_backoff/step_retry_exponential_backoff.rs` with a handler demonstrating `StepConfig` with `ExponentialBackoff::builder()` configured with custom `max_attempts`, `base_delay`, `max_delay`, `multiplier`, and `jitter` (using `JitterStrategy`)
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_step_retry_exponential_backoff` in `examples/tests/step_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/step_retry_exponential_backoff.history.json`
    - _Requirements: 9.1, 9.4, 11.1–11.7_

  - [x] 13.2 Create `step/retry_with_filter` example and replay test
    - Create `examples/src/bin/step/retry_with_filter/step_retry_with_filter.rs` with a handler demonstrating `StepConfig` with a `RetryableErrorFilter` that retries only specific error patterns using `ErrorPattern::Contains` or `ErrorPattern::Regex`
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_step_retry_with_filter` in `examples/tests/step_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/step_retry_with_filter.history.json`
    - _Requirements: 9.2, 9.4, 11.1–11.7_

  - [x] 13.3 Create `step/error_determinism` example and replay test
    - Create `examples/src/bin/step/error_determinism/step_error_determinism.rs` with a handler demonstrating that a step which fails deterministically produces the same error across replays
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_step_error_determinism` in `examples/tests/step_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/step_error_determinism.history.json`
    - _Requirements: 9.3, 9.4, 11.1–11.7_

- [x] 14. Phase 5: Advanced Feature Examples (6 examples, new category)
  - [x] 14.1 Create `advanced/checkpointing_eager` example, test file, and replay test
    - Create `examples/src/bin/advanced/checkpointing_eager/checkpointing_eager.rs` with a handler demonstrating `StepConfig` with `CheckpointingMode::Eager` and verifying that checkpoints are flushed immediately
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Create new `examples/tests/advanced_tests.rs` test file with `test_checkpointing_eager`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/checkpointing_eager.history.json`
    - _Requirements: 10.1, 10.7, 11.1–11.7_

  - [x] 14.2 Create `advanced/checkpointing_batched` example and replay test
    - Create `examples/src/bin/advanced/checkpointing_batched/checkpointing_batched.rs` with a handler demonstrating `StepConfig` with `CheckpointingMode::Batched` and verifying that checkpoints are batched
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_checkpointing_batched` in `examples/tests/advanced_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/checkpointing_batched.history.json`
    - _Requirements: 10.2, 10.7, 11.1–11.7_

  - [x] 14.3 Create `advanced/checkpointing_optimistic` example and replay test
    - Create `examples/src/bin/advanced/checkpointing_optimistic/checkpointing_optimistic.rs` with a handler demonstrating `StepConfig` with `CheckpointingMode::Optimistic`
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_checkpointing_optimistic` in `examples/tests/advanced_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/checkpointing_optimistic.history.json`
    - _Requirements: 10.3, 10.7, 11.1–11.7_

  - [x] 14.4 Create `advanced/retry_exhaustion` example and replay test
    - Create `examples/src/bin/advanced/retry_exhaustion/retry_exhaustion.rs` with a handler demonstrating a step that exhausts all retry attempts and surfaces the final error
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_retry_exhaustion` in `examples/tests/advanced_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/retry_exhaustion.history.json`
    - _Requirements: 10.4, 10.7, 11.1–11.7_

  - [x] 14.5 Create `advanced/large_payload` example and replay test
    - Create `examples/src/bin/advanced/large_payload/large_payload.rs` with a handler that processes a large input payload and produces a large output, exercising `ctx.complete_execution_if_large()`
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_large_payload` in `examples/tests/advanced_tests.rs`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/large_payload.history.json`
    - _Requirements: 10.5, 10.7, 11.1–11.7_

  - [x] 14.6 Create `advanced/handler_error` example and replay test
    - Create `examples/src/bin/advanced/handler_error/handler_error.rs` with a handler that returns a `DurableError` at the top level and verifying the error is properly serialized in the execution output
    - Add `[[bin]]` entry in `examples/Cargo.toml`
    - Add replay test `test_handler_error` in `examples/tests/advanced_tests.rs` asserting `ExecutionStatus::Failed`
    - Run test with `GENERATE_HISTORY=true` to produce `examples/tests/history/handler_error.history.json`
    - _Requirements: 10.6, 10.7, 11.1–11.7_

- [x] 15. Phase 5 Checkpoint
  - Ensure all 9 step and advanced examples compile, replay tests pass, and history files are generated. Ask the user if questions arise.

- [ ] 16. Structural Property Tests
  - [ ]* 16.1 Write property test for Example Suite Completeness
    - **Property 1: Example Suite Completeness**
    - **Validates: Requirements 1.8, 2.5, 3.9, 4.5, 5.4, 6.4, 7.4, 8.5, 9.4, 10.7**
    - For every `[[bin]]` entry in `examples/Cargo.toml`, verify a corresponding `.history.json` file exists in `examples/tests/history/` and a test function exists in the appropriate `{category}_tests.rs` file

  - [ ]* 16.2 Write property test for Example File Structure
    - **Property 2: Example File Structure**
    - **Validates: Requirements 11.2, 11.3, 11.4**
    - For every example `.rs` file referenced by a `[[bin]]` entry, verify it contains: (a) a `//!` doc comment, (b) a `#[durable_execution]` annotation, and (c) a `#[tokio::main] async fn main()` calling `lambda_runtime::run()`

  - [ ]* 16.3 Write property test for Example Path Convention
    - **Property 3: Example Path Convention**
    - **Validates: Requirements 11.1**
    - For every `[[bin]]` entry, verify its path matches `src/bin/{category}/{name}/{example_name}.rs` where `{category}` is a known category directory

  - [ ]* 16.4 Write property test for Replay Test Assertion Usage
    - **Property 4: Replay Test Assertion Usage**
    - **Validates: Requirements 11.5**
    - For every test function in the test suite, verify the test body calls one of `assert_event_signatures()`, `assert_nodejs_event_signatures()`, `assert_event_signatures_unordered()`, or `assert_nodejs_event_signatures_unordered()` with a history file path

  - [ ]* 16.5 Write property test for Example Registration and History Completeness
    - **Property 5: Example Registration and History Completeness**
    - **Validates: Requirements 11.6, 11.7**
    - For every example binary, verify its name appears as a `[[bin]]` entry in `Cargo.toml` and its history file exists in `examples/tests/history/`

- [x] 17. Final Checkpoint — All phases complete
  - Ensure all ~45 examples compile, all replay tests pass, all history files are generated, and structural property tests (if implemented) pass. Ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- History files are auto-generated by running tests with `GENERATE_HISTORY=true`, not hand-written
- For concurrent/map operations, use `assert_nodejs_event_signatures_unordered()` instead of `assert_nodejs_event_signatures()`
- New test files are created in tasks 10.1 (`context_validation_tests.rs`), 11.1 (`concurrency_tests.rs`), and 14.1 (`advanced_tests.rs`)
- Existing test files are extended: `callback_tests.rs`, `parallel_map_tests.rs`, `promise_combinator_tests.rs`, `child_context_tests.rs`, `step_tests.rs`
- Property tests (tasks 16.1–16.5) verify structural consistency of the entire example suite
- Replay determinism (Property 6 from design) is inherently validated by every individual replay test
