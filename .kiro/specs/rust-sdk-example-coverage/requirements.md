# Requirements Document

## Introduction

The Rust durable execution SDK has ~35 examples compared to the JS SDK's ~99. While the Rust SDK already implements the APIs for all major features (callbacks, parallel, map, child context, concurrency, etc.), many lack example code and corresponding replay tests. This spec defines the requirements for closing the example and test coverage gap by adding missing examples, history files, and replay tests for features that already exist in the Rust SDK.

## Glossary

- **Example**: A standalone Rust binary in `examples/src/bin/{category}/{name}/` demonstrating a specific SDK feature with a `#[durable_execution]` handler
- **Replay_Test**: An integration test in `examples/tests/{category}_tests.rs` that replays a handler against a recorded history JSON file to verify deterministic behavior
- **History_File**: A JSON file in `examples/tests/history/` containing serialized operation events used by Replay_Tests for deterministic replay
- **Example_Suite**: The complete set of Example, History_File, and Replay_Test for a single feature demonstration
- **DurableContext**: The SDK context struct providing step, callback, wait, parallel, map, child context, and combinator operations
- **CallbackConfig**: Configuration struct supporting `timeout` and `heartbeat_timeout` fields for callback operations
- **CompletionConfig**: Configuration struct supporting `min_successful`, `tolerated_failure_count`, and `tolerated_failure_percentage` for parallel and map operations
- **ParallelConfig**: Configuration struct for `ctx.parallel()` with `max_concurrency`, `completion_config`, and optional custom serdes
- **MapConfig**: Configuration struct for `ctx.map()` with `max_concurrency`, `item_batcher`, `completion_config`, and optional custom serdes
- **ChildConfig**: Configuration struct for `ctx.run_in_child_context()` with `replay_children`, `serdes`, `error_mapper`, and `summary_generator`
- **StepConfig**: Configuration struct for `ctx.step()` with `retry_strategy`, `retryable_error_filter`, `checkpointing_mode`, and `step_semantics`
- **CheckpointingMode**: Enum controlling checkpoint behavior: Default, Eager, Batched, or Optimistic
- **Not_Applicable**: A gap identified in the JS SDK that has no meaningful Rust equivalent due to language differences

## Requirements

### Requirement 1: Wait-for-Callback Examples

**User Story:** As a Rust SDK developer, I want comprehensive wait-for-callback examples covering timeout, failure, heartbeat, nested, and concurrent patterns, so that I can learn how to use `ctx.wait_for_callback()` in real-world scenarios.

#### Acceptance Criteria

1. THE Example_Suite SHALL include a `wait_for_callback/timeout` example demonstrating `CallbackConfig` with a `timeout` field and a handler that exercises the timeout path
2. THE Example_Suite SHALL include a `wait_for_callback/failure` example demonstrating a callback submitter that returns an error, and the handler catching the callback failure
3. THE Example_Suite SHALL include a `wait_for_callback/heartbeat` example demonstrating a callback submitter that sends heartbeat signals via `send_heartbeat()` before completing
4. THE Example_Suite SHALL include a `wait_for_callback/nested` example demonstrating `wait_for_callback` called inside a child context
5. THE Example_Suite SHALL include a `wait_for_callback/multiple_invocations` example demonstrating multiple `wait_for_callback` calls within a single handler execution
6. THE Example_Suite SHALL include a `wait_for_callback/child_context` example demonstrating `wait_for_callback` used within `ctx.run_in_child_context()`
7. THE Example_Suite SHALL include a `wait_for_callback/custom_serdes` example demonstrating `wait_for_callback` with a custom serializer/deserializer for the callback payload
8. WHEN any wait_for_callback Example is created, THE Example_Suite SHALL include a corresponding History_File and Replay_Test verifying deterministic replay
9. THE requirements document SHALL note that `wait_for_callback/submitter_retry`, `wait_for_callback/failing_submitter`, `wait_for_callback/submitter_failure_catchable`, and `wait_for_callback/quick_completion` are Not_Applicable because the Rust SDK's `wait_for_callback` uses an async closure model where submitter retry semantics are handled by the caller, not the SDK framework
10. THE requirements document SHALL note that `wait_for_callback/error_instances` variants are Not_Applicable because Rust uses typed `DurableError` enums rather than JS error class hierarchies

### Requirement 2: Callback Creation Examples

**User Story:** As a Rust SDK developer, I want examples for callback creation patterns including heartbeat, failures, custom serdes, and mixed operations, so that I can learn how to use `ctx.create_callback()` and `ctx.create_callback_named()` effectively.

#### Acceptance Criteria

1. THE Example_Suite SHALL include a `callback/heartbeat` example demonstrating dedicated heartbeat usage with `CallbackConfig::heartbeat_timeout` and periodic heartbeat sends
2. THE Example_Suite SHALL include a `callback/failure` example demonstrating a callback that receives a failure response and the handler processing the error
3. THE Example_Suite SHALL include a `callback/custom_serdes` example demonstrating `create_callback` with a custom `SerDesAny` implementation for non-JSON payload formats
4. THE Example_Suite SHALL include a `callback/mixed_ops` example demonstrating a handler that combines `create_callback` with steps, waits, and parallel operations in a single workflow
5. WHEN any callback creation Example is created, THE Example_Suite SHALL include a corresponding History_File and Replay_Test verifying deterministic replay
6. THE requirements document SHALL note that `callback/error_instance` is Not_Applicable because Rust uses typed `DurableError` enums rather than JS error class hierarchies

### Requirement 3: Parallel Operation Examples

**User Story:** As a Rust SDK developer, I want examples for parallel edge cases including empty arrays, error preservation, min-successful with callbacks, and failure threshold behavior, so that I can understand how `CompletionConfig` controls parallel execution outcomes.

#### Acceptance Criteria

1. THE Example_Suite SHALL include a `parallel/empty_array` example demonstrating `ctx.parallel()` called with an empty task vector, showing the resulting `BatchResult::empty()` behavior
2. THE Example_Suite SHALL include a `parallel/with_wait` example demonstrating parallel tasks that include `ctx.wait()` calls within their branches
3. THE Example_Suite SHALL include a `parallel/error_preservation` example demonstrating that individual task errors are preserved in `BatchItem::get_error()` when some branches fail
4. THE Example_Suite SHALL include a `parallel/min_successful` example demonstrating `CompletionConfig::with_min_successful(n)` where the parallel completes after n successes regardless of remaining tasks
5. THE Example_Suite SHALL include a `parallel/min_successful_with_callback` example demonstrating `CompletionConfig::with_min_successful(n)` combined with callback operations in parallel branches
6. THE Example_Suite SHALL include a `parallel/tolerated_failure_count` example demonstrating `CompletionConfig` with `tolerated_failure_count` set, where failures up to the threshold do not cause overall failure
7. THE Example_Suite SHALL include a `parallel/tolerated_failure_percentage` example demonstrating `CompletionConfig` with `tolerated_failure_percentage` set
8. THE Example_Suite SHALL include a `parallel/failure_threshold_exceeded` example demonstrating the behavior when failures exceed the configured tolerance, causing the parallel operation to fail
9. WHEN any parallel Example is created, THE Example_Suite SHALL include a corresponding History_File and Replay_Test verifying deterministic replay

### Requirement 4: Map Operation Examples

**User Story:** As a Rust SDK developer, I want examples for map edge cases including empty arrays, error preservation, high concurrency, and large-scale processing, so that I can understand `ctx.map()` behavior at scale and under failure conditions.

#### Acceptance Criteria

1. THE Example_Suite SHALL include a `map/empty_array` example demonstrating `ctx.map()` called with an empty input vector, showing the resulting `BatchResult::empty()` behavior
2. THE Example_Suite SHALL include a `map/error_preservation` example demonstrating that individual item errors are preserved in `BatchItem::get_error()` when some items fail during mapping
3. THE Example_Suite SHALL include a `map/high_concurrency` example demonstrating `MapConfig` with `max_concurrency` set to a high value (e.g., 10+) processing many items concurrently
4. THE Example_Suite SHALL include a `map/large_scale` example demonstrating `ctx.map()` processing a large input set (50+ items) with `ItemBatcher` configured for batch processing
5. WHEN any map Example is created, THE Example_Suite SHALL include a corresponding History_File and Replay_Test verifying deterministic replay

### Requirement 5: Promise Combinator Examples

**User Story:** As a Rust SDK developer, I want examples for promise combinators combined with waits and demonstrating replay behavior, so that I can understand how `ctx.all()`, `ctx.race()`, `ctx.any()`, and `ctx.all_settled()` interact with other durable operations.

#### Acceptance Criteria

1. THE Example_Suite SHALL include a `promise_combinators/all_with_wait` example demonstrating `ctx.all()` where some futures include `ctx.wait()` calls
2. THE Example_Suite SHALL include a `promise_combinators/race_with_timeout` example demonstrating `ctx.race()` used as a timeout pattern where one future is a `ctx.wait()` and another is a long-running operation
3. THE Example_Suite SHALL include a `promise_combinators/replay_behavior` example demonstrating that combinator results are deterministic across replays by using multiple combinators in sequence
4. WHEN any promise combinator Example is created, THE Example_Suite SHALL include a corresponding History_File and Replay_Test verifying deterministic replay
5. THE requirements document SHALL note that `promise_combinators/unhandled_rejection` is Not_Applicable because Rust's `Result` type and `?` operator handle errors explicitly at compile time, unlike JS unhandled promise rejections

### Requirement 6: Child Context Examples

**User Story:** As a Rust SDK developer, I want examples for child context edge cases including large data handling, checkpoint size limits, and failing steps within child contexts, so that I can understand child context behavior under stress.

#### Acceptance Criteria

1. THE Example_Suite SHALL include a `child_context/large_data` example demonstrating a child context that produces a large result, exercising `ChildConfig::set_summary_generator()` when the result exceeds 256KB
2. THE Example_Suite SHALL include a `child_context/checkpoint_size_limit` example demonstrating the behavior when a child context result approaches or exceeds the checkpoint size limit, using `ChildConfig::set_summary_generator()` as a mitigation
3. THE Example_Suite SHALL include a `child_context/with_failing_step` example demonstrating error propagation when a step inside a child context fails, including `ChildConfig::set_error_mapper()` usage
4. WHEN any child context Example is created, THE Example_Suite SHALL include a corresponding History_File and Replay_Test verifying deterministic replay

### Requirement 7: Context Validation Examples (New Category)

**User Story:** As a Rust SDK developer, I want examples demonstrating that parent context operations cannot be used inside child contexts, steps, or wait conditions, so that I understand the SDK's context scoping rules.

#### Acceptance Criteria

1. THE Example_Suite SHALL include a `context_validation/parent_context_in_child` example demonstrating that using a parent DurableContext inside `run_in_child_context` produces an error
2. THE Example_Suite SHALL include a `context_validation/parent_context_in_step` example demonstrating that using a parent DurableContext to call durable operations inside a `step` closure produces an error or incorrect behavior
3. THE Example_Suite SHALL include a `context_validation/parent_context_in_wait_condition` example demonstrating that using a parent DurableContext inside a `wait_for_condition` predicate produces an error
4. WHEN any context validation Example is created, THE Example_Suite SHALL include a corresponding History_File and Replay_Test verifying the error behavior is deterministic

### Requirement 8: Concurrency Examples (New Category)

**User Story:** As a Rust SDK developer, I want examples demonstrating concurrent durable operations using `tokio::join!` and `tokio::select!` patterns, so that I can understand how to safely compose concurrent durable operations.

#### Acceptance Criteria

1. THE Example_Suite SHALL include a `concurrency/concurrent_operations` example demonstrating multiple `ctx.step()` calls executed concurrently via `tokio::join!`
2. THE Example_Suite SHALL include a `concurrency/concurrent_wait` example demonstrating multiple `ctx.wait()` calls executed concurrently
3. THE Example_Suite SHALL include a `concurrency/concurrent_callback_wait` example demonstrating a `ctx.wait_for_callback()` and a `ctx.wait()` running concurrently via `tokio::select!`
4. THE Example_Suite SHALL include a `concurrency/concurrent_callback_submitter` example demonstrating multiple callback operations with independent submitters running concurrently
5. WHEN any concurrency Example is created, THE Example_Suite SHALL include a corresponding History_File and Replay_Test verifying deterministic replay

### Requirement 9: Step Operation Examples

**User Story:** As a Rust SDK developer, I want examples for step retry configuration and error determinism, so that I can understand how `StepConfig` retry strategies and error filters work in practice.

#### Acceptance Criteria

1. THE Example_Suite SHALL include a `step/retry_exponential_backoff` example demonstrating `StepConfig` with `ExponentialBackoff::builder()` configured with custom `max_attempts`, `base_delay`, `max_delay`, `multiplier`, and `jitter`
2. THE Example_Suite SHALL include a `step/retry_with_filter` example demonstrating `StepConfig` with a `RetryableErrorFilter` that retries only specific error patterns using `ErrorPattern::Contains` or `ErrorPattern::Regex`
3. THE Example_Suite SHALL include a `step/error_determinism` example demonstrating that a step which fails deterministically produces the same error across replays
4. WHEN any step Example is created, THE Example_Suite SHALL include a corresponding History_File and Replay_Test verifying deterministic replay

### Requirement 10: Advanced Feature Examples (New Category)

**User Story:** As a Rust SDK developer, I want examples for advanced SDK features including force checkpointing modes, retry exhaustion, large payloads, and handler-level errors, so that I can understand edge-case SDK behavior.

#### Acceptance Criteria

1. THE Example_Suite SHALL include an `advanced/checkpointing_eager` example demonstrating `StepConfig` with `CheckpointingMode::Eager` and verifying that checkpoints are flushed immediately
2. THE Example_Suite SHALL include an `advanced/checkpointing_batched` example demonstrating `StepConfig` with `CheckpointingMode::Batched` and verifying that checkpoints are batched
3. THE Example_Suite SHALL include an `advanced/checkpointing_optimistic` example demonstrating `StepConfig` with `CheckpointingMode::Optimistic`
4. THE Example_Suite SHALL include an `advanced/retry_exhaustion` example demonstrating a step that exhausts all retry attempts and surfaces the final error
5. THE Example_Suite SHALL include an `advanced/large_payload` example demonstrating a handler that processes a large input payload and produces a large output, exercising `complete_execution_if_large()`
6. THE Example_Suite SHALL include an `advanced/handler_error` example demonstrating a top-level handler that returns a `DurableError` and verifying the error is properly serialized in the execution output
7. WHEN any advanced Example is created, THE Example_Suite SHALL include a corresponding History_File and Replay_Test verifying deterministic replay
8. THE requirements document SHALL note that `advanced/undefined_results` is Not_Applicable because Rust does not have an `undefined` type; `Option<T>` with `None` is the idiomatic equivalent and is already covered by serde examples
9. THE requirements document SHALL note that `advanced/powertools_logger_integration` is Not_Applicable because the Rust SDK uses its own `StructuredJsonLogger` and `ReplayAwareLogger` rather than a third-party Powertools library

### Requirement 11: Example Structural Consistency

**User Story:** As a Rust SDK maintainer, I want all new examples to follow the established project conventions, so that the example suite remains consistent and maintainable.

#### Acceptance Criteria

1. WHEN a new Example is created, THE Example SHALL be placed in `examples/src/bin/{category}/{name}/` with a descriptive `.rs` file following the existing naming convention (e.g., `callback_heartbeat.rs`)
2. WHEN a new Example is created, THE Example SHALL include a module-level doc comment (`//!`) describing the feature being demonstrated
3. WHEN a new Example is created, THE Example SHALL use `#[durable_execution]` macro on the handler function
4. WHEN a new Example is created, THE Example SHALL include a `#[tokio::main] async fn main()` that wires the handler to `lambda_runtime::run()`
5. WHEN a new Replay_Test is created, THE Replay_Test SHALL use the `assert_event_signatures()` or `assert_nodejs_event_signatures()` helper from `test_helper.rs` to validate against the History_File
6. WHEN a new History_File is created, THE History_File SHALL be placed in `examples/tests/history/` with a descriptive name matching the example
7. THE Example_Suite SHALL register each new example binary in `examples/Cargo.toml` under `[[bin]]`

### Requirement 12: Not-Applicable Gap Documentation

**User Story:** As a Rust SDK maintainer, I want a clear record of which JS SDK examples are intentionally not ported to Rust, so that future audits do not re-investigate these gaps.

#### Acceptance Criteria

1. THE requirements document SHALL document all Not_Applicable gaps with a rationale for each
2. THE following gaps SHALL be documented as Not_Applicable:
   - `wait_for_callback/submitter_retry` — Rust's async closure model handles submitter retry at the caller level
   - `wait_for_callback/failing_submitter` — Same as above
   - `wait_for_callback/submitter_failure_catchable` — Same as above
   - `wait_for_callback/quick_completion` — Rust's async model does not have the same race condition concerns as JS event loop
   - `wait_for_callback/error_instances` (3 variants) — Rust uses typed `DurableError` enums, not JS error class hierarchies
   - `callback/error_instance` — Same as error_instances rationale
   - `promise_combinators/unhandled_rejection` — Rust's `Result` type handles errors explicitly at compile time
   - `advanced/undefined_results` — Rust has no `undefined`; `Option<T>` with `None` is idiomatic and already covered
   - `advanced/powertools_logger_integration` — Rust SDK uses its own structured logger, not a third-party Powertools library
3. WHEN a gap is documented as Not_Applicable, THE documentation SHALL include the JS example name and a one-sentence rationale
