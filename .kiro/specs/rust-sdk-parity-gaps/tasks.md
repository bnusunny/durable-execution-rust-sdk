# Tasks: Rust SDK Parity Gaps

## High Priority

- [x] 1. Jitter in Retry Strategies (Reqs 1.1–1.10)
  - [x] 1.1 Add `JitterStrategy` enum with `None`, `Full`, `Half` variants and `apply` method to `config.rs` (Reqs 1.1–1.4)
  - [x] 1.2 Implement `Default` for `JitterStrategy` returning `None` (Req 1.5)
  - [x] 1.3 Add `jitter: JitterStrategy` field to `ExponentialBackoff` and `ExponentialBackoffBuilder`, update `next_delay` to apply jitter (Reqs 1.6, 1.9)
  - [x] 1.4 Add `jitter: JitterStrategy` field to `FixedDelay` with `with_jitter` builder method, update `next_delay` (Req 1.7)
  - [x] 1.5 Add `jitter: JitterStrategy` field to `LinearBackoff`, update `next_delay` (Req 1.8)
  - [x] 1.6 Ensure all jittered delays have a minimum of 1 second (Req 1.10)
  - [x] 1.7 Add unit tests for `JitterStrategy::apply` bounds (None, Full, Half) and integration tests for each retry strategy with jitter
  - [x] 1.8 Export `JitterStrategy` from `lib.rs`

- [x] 2. Typed Retryable Error Filtering (Reqs 2.1–2.7)
  - [x] 2.1 Add `ErrorPattern` enum (`Contains`, `Regex`) and `RetryableErrorFilter` struct with `is_retryable` and `is_retryable_with_type` methods to `config.rs` (Reqs 2.1–2.5)
  - [x] 2.2 Add `retryable_error_filter: Option<RetryableErrorFilter>` field to `StepConfig` (Req 2.6)
  - [x] 2.3 Update step handler to check `retryable_error_filter` before delegating to retry strategy (Reqs 2.6, 2.7)
  - [x] 2.4 Add unit tests for `RetryableErrorFilter` (empty filter retries all, Contains matching, Regex matching, OR logic, type matching)
  - [x] 2.5 Add `regex` crate dependency to `Cargo.toml` if not already present
  - [x] 2.6 Export `ErrorPattern` and `RetryableErrorFilter` from `lib.rs`

- [x] 3. Retry Presets (Reqs 3.1–3.2)
  - [x] 3.1 Create `retry_presets` module with `default_retry()` and `no_retry()` functions (Reqs 3.1, 3.2)
  - [x] 3.2 Add unit tests verifying preset configurations match Node.js defaults
  - [x] 3.3 Export `retry_presets` module from `lib.rs`

- [x] 4. Distinct Wait Strategy for Polling (Reqs 4.1–4.8)
  - [x] 4.1 Add `WaitDecision` enum (`Continue { delay }`, `Done`) to `config.rs` (Req 4.1)
  - [x] 4.2 Add `WaitStrategyConfig<T>` struct and `create_wait_strategy` function (Reqs 4.2–4.6)
  - [x] 4.3 Update `WaitForConditionConfig` to use `wait_strategy` field instead of `interval` + `max_attempts` (Req 4.7)
  - [x] 4.4 Add backward-compatible constructor for `WaitForConditionConfig` that converts interval + max_attempts to a WaitStrategy (Req 4.8)
  - [x] 4.5 Update `wait_for_condition_handler` to use `WaitDecision` from the wait strategy (Req 4.7)
  - [x] 4.6 Add unit tests for `create_wait_strategy` (Done when predicate false, Continue with backoff, max attempts panic, jitter application)
  - [x] 4.7 Export `WaitDecision`, `WaitStrategyConfig`, `create_wait_strategy` from `lib.rs`

- [x] 5. Graceful Termination / Lambda Timeout Detection (Reqs 5.1–5.5)
  - [x] 5.1 Create `TerminationManager` struct with `from_lambda_context`, `wait_for_timeout`, and `remaining_ms` methods (Reqs 5.1, 5.4, 5.5)
  - [x] 5.2 Update `run_durable_handler` in `runtime.rs` to use `tokio::select!` racing handler against `TerminationManager::wait_for_timeout` (Reqs 5.2, 5.3)
  - [x] 5.3 On timeout: flush batcher and return `DurableExecutionInvocationOutput::pending()` (Req 5.2)
  - [x] 5.4 Add unit tests for `TerminationManager` timing (fires at deadline - margin, immediate fire when remaining < margin)
  - [x] 5.5 Add integration test verifying PENDING output on simulated timeout

## Medium Priority

- [x] 6. Child Context Error Mapping (Reqs 6.1–6.3)
  - [x] 6.1 Add `error_mapper: Option<Arc<dyn Fn(DurableError) -> DurableError + Send + Sync>>` field to `ChildConfig` (Req 6.1)
  - [x] 6.2 Update `child_handler` to apply `error_mapper` on failure, skipping Suspend errors (Reqs 6.1, 6.2, 6.3)
  - [x] 6.3 Add unit tests for error mapping (mapper applied, mapper skipped for Suspend, None preserves behavior)

- [x] 7. Child Context Summary Generation (Reqs 7.1–7.3)
  - [x] 7.1 Add `summary_generator: Option<Arc<dyn Fn(&str) -> String + Send + Sync>>` field to `ChildConfig` (Req 7.1)
  - [x] 7.2 Update `child_handler` to check serialized result size and invoke summary_generator when > 256KB (Reqs 7.1, 7.2, 7.3)
  - [x] 7.3 Add unit tests for summary generation (invoked when > 256KB, not invoked when <= 256KB, None preserves behavior)

- [x] 8. getChildOperations in Testing (Reqs 8.1–8.3)
  - [x] 8.1 Add `get_child_operations` method to `DurableOperation` in the testing module (Reqs 8.1, 8.2, 8.3)
  - [x] 8.2 Add unit tests for child operation enumeration (matching parent_id, empty result, ordering)

- [x] 9. Structured JSON Log Output (Reqs 9.1–9.5)
  - [x] 9.1 Create `StructuredJsonLogger` struct implementing the sealed `Logger` trait with `JsonLogContext` and `JsonLogEntry` types (Reqs 9.1, 9.2)
  - [x] 9.2 Implement JSON serialization for log entries with required fields (level, timestamp, requestId, executionArn, message) and optional fields (tenantId, operationId, attempt, errorType, errorMessage) (Reqs 9.2, 9.3, 9.4)
  - [x] 9.3 Implement log level filtering via `min_level` field (Req 9.5)
  - [x] 9.4 Add unit tests for JSON output format, required fields presence, optional fields, and level filtering
  - [x] 9.5 Export `StructuredJsonLogger` and `JsonLogContext` from `lib.rs`

- [x] 10. AWS_LAMBDA_LOG_LEVEL Integration (Reqs 10.1–10.3)
  - [x] 10.1 Read `AWS_LAMBDA_LOG_LEVEL` env var in `StructuredJsonLogger::new()` to set `min_level` (Reqs 10.1, 10.2)
  - [x] 10.2 Add unit tests for env var parsing (valid levels, invalid/missing defaults to DEBUG)

## Low Priority

- [x] 11. Enriched Termination Reasons (Reqs 11.1–11.3)
  - [x] 11.1 Add new variants to `TerminationReason` enum: OperationTerminated, RetryScheduled, WaitScheduled, CallbackPending, ContextValidationError, LambdaTimeoutApproaching (Reqs 11.1, 11.2)
  - [x] 11.2 Add serde round-trip tests for all TerminationReason variants (Req 11.3)
  - [x] 11.3 Verify existing discriminant values are unchanged in tests (Req 11.1)

- [x] 12. Summary Generators for Batch Operations (Reqs 12.1–12.2)
  - [x] 12.1 Create `summary_generators` module with `parallel_summary` and `map_summary` functions (Reqs 12.1, 12.2)
  - [x] 12.2 Add unit tests verifying JSON output structure for both generators
  - [x] 12.3 Export `summary_generators` module from `lib.rs`

- [x] 13. Runtime Logger Reconfiguration (Reqs 13.1–13.2)
  - [x] 13.1 Add `configure_logger` method to `DurableContext` using `Arc<RwLock<Arc<dyn Logger>>>` (Reqs 13.1, 13.2)
  - [x] 13.2 Add unit tests verifying logger swap at runtime

- [x] 14. Custom SDK User-Agent (Reqs 14.1–14.2)
  - [x] 14.1 Add `from_aws_config_with_user_agent` method to `LambdaDurableServiceClient` (Req 14.1)
  - [x] 14.2 Update `run_durable_handler` to use the new method with SDK name/version constants (Req 14.1)
  - [x] 14.3 Add unit test verifying user-agent is set on client configuration
