# Requirements: Rust SDK Parity Gaps

## Requirement 1: Jitter in Retry Strategies

### 1.1
GIVEN a JitterStrategy enum
WHEN the SDK is compiled
THEN it SHALL provide three variants: None, Full, and Half

### 1.2
GIVEN JitterStrategy::None and a base delay d
WHEN apply is called
THEN the returned delay SHALL equal d exactly

### 1.3
GIVEN JitterStrategy::Full and a base delay d >= 0
WHEN apply is called
THEN the returned delay SHALL be in the range [0, d]

### 1.4
GIVEN JitterStrategy::Half and a base delay d >= 0
WHEN apply is called
THEN the returned delay SHALL be in the range [d/2, d]

### 1.5
GIVEN the Default trait implementation for JitterStrategy
WHEN Default::default() is called
THEN it SHALL return JitterStrategy::None to preserve backward compatibility

### 1.6
GIVEN an ExponentialBackoff with a jitter field
WHEN next_delay is called
THEN the jitter SHALL be applied to the computed base delay before returning

### 1.7
GIVEN a FixedDelay with a jitter field
WHEN next_delay is called
THEN the jitter SHALL be applied to the fixed delay value before returning

### 1.8
GIVEN a LinearBackoff with a jitter field
WHEN next_delay is called
THEN the jitter SHALL be applied to the computed linear delay before returning

### 1.9
GIVEN an ExponentialBackoffBuilder
WHEN jitter() is called with a JitterStrategy value
THEN the built ExponentialBackoff SHALL use that jitter strategy

### 1.10
GIVEN any retry strategy with jitter applied
WHEN the jittered delay is computed
THEN the final delay SHALL be at least 1 second

## Requirement 2: Typed Retryable Error Filtering

### 2.1
GIVEN an ErrorPattern::Contains with a substring
WHEN is_retryable is called with an error message containing that substring
THEN it SHALL return true

### 2.2
GIVEN an ErrorPattern::Contains with a substring
WHEN is_retryable is called with an error message NOT containing that substring
THEN it SHALL return false (assuming no other patterns match)

### 2.3
GIVEN an ErrorPattern::Regex with a valid regex pattern
WHEN is_retryable is called with an error message matching the regex
THEN it SHALL return true

### 2.4
GIVEN a RetryableErrorFilter with empty patterns and empty error_types
WHEN is_retryable is called with any error message
THEN it SHALL return true (retry all errors by default)

### 2.5
GIVEN a RetryableErrorFilter with configured patterns and error_types
WHEN is_retryable_with_type is called
THEN it SHALL return true if the error matches ANY pattern OR ANY error type (OR logic)

### 2.6
GIVEN a StepConfig with a retryable_error_filter set to Some
WHEN a step fails with an error that does NOT match the filter
THEN the step handler SHALL NOT retry the error regardless of the retry strategy

### 2.7
GIVEN a StepConfig with retryable_error_filter set to None
WHEN a step fails with any error
THEN the step handler SHALL delegate retry decisions entirely to the retry strategy (current behavior)

## Requirement 3: Retry Presets

### 3.1
GIVEN the retry_presets module
WHEN default_retry() is called
THEN it SHALL return an ExponentialBackoff with max_attempts=6, base_delay=5s, max_delay=60s, multiplier=2.0, jitter=Full

### 3.2
GIVEN the retry_presets module
WHEN no_retry() is called
THEN it SHALL return a NoRetry strategy that returns None for any attempt

## Requirement 4: Distinct Wait Strategy for Polling

### 4.1
GIVEN a WaitDecision enum
WHEN the SDK is compiled
THEN it SHALL provide Continue { delay: Duration } and Done variants

### 4.2
GIVEN a WaitStrategyConfig with a should_continue_polling predicate
WHEN the predicate returns false for a given state
THEN the created wait strategy SHALL return WaitDecision::Done

### 4.3
GIVEN a WaitStrategyConfig with a should_continue_polling predicate
WHEN the predicate returns true and attempts_made < max_attempts
THEN the created wait strategy SHALL return WaitDecision::Continue with a delay >= 1 second

### 4.4
GIVEN a WaitStrategyConfig with max_attempts set
WHEN attempts_made >= max_attempts and should_continue_polling returns true
THEN the created wait strategy SHALL panic with a message indicating max attempts exceeded

### 4.5
GIVEN a WaitStrategyConfig with backoff_rate and initial_delay
WHEN the wait strategy computes delay for attempt N
THEN the base delay SHALL be min(initial_delay * backoff_rate^(N-1), max_delay)

### 4.6
GIVEN a WaitStrategyConfig with a jitter strategy
WHEN the wait strategy computes delay
THEN jitter SHALL be applied to the base delay before returning

### 4.7
GIVEN a WaitForConditionConfig with a wait_strategy field
WHEN wait_for_condition is called
THEN the handler SHALL use the wait_strategy to determine polling delay instead of a fixed interval

### 4.8
GIVEN the existing WaitForConditionConfig API
WHEN users upgrade to the new SDK version
THEN a backward-compatible constructor SHALL be available that converts interval + max_attempts to a WaitStrategy

## Requirement 5: Graceful Termination / Lambda Timeout Detection

### 5.1
GIVEN a TerminationManager created from a Lambda context
WHEN the Lambda deadline minus safety margin is reached
THEN wait_for_timeout SHALL resolve

### 5.2
GIVEN run_durable_handler using tokio::select! with handler and termination manager
WHEN the termination manager fires before the handler completes
THEN the runtime SHALL flush pending checkpoint batches and return a PENDING output

### 5.3
GIVEN run_durable_handler using tokio::select!
WHEN the handler completes before the termination manager fires
THEN the runtime SHALL process the handler result normally

### 5.4
GIVEN a TerminationManager with a default safety margin
WHEN from_lambda_context is called
THEN the safety margin SHALL be 5 seconds

### 5.5
GIVEN a Lambda context where remaining time is less than the safety margin
WHEN TerminationManager is created
THEN wait_for_timeout SHALL resolve immediately

## Requirement 6: Child Context Error Mapping

### 6.1
GIVEN a ChildConfig with error_mapper set to Some(mapper_fn)
WHEN a child context execution fails with an error
THEN the child handler SHALL apply mapper_fn to the error before checkpointing and propagating it

### 6.2
GIVEN a ChildConfig with error_mapper set to None
WHEN a child context execution fails with an error
THEN the child handler SHALL propagate the error unchanged (current behavior)

### 6.3
GIVEN a ChildConfig with error_mapper set to Some(mapper_fn)
WHEN the child context returns a Suspend error
THEN the error mapper SHALL NOT be applied to Suspend errors

## Requirement 7: Child Context Summary Generation

### 7.1
GIVEN a ChildConfig with summary_generator set to Some(generator_fn)
WHEN the serialized child result exceeds 256KB
THEN the child handler SHALL invoke generator_fn and store the summary instead of the full result

### 7.2
GIVEN a ChildConfig with summary_generator set to Some(generator_fn)
WHEN the serialized child result is 256KB or less
THEN the child handler SHALL store the full result without invoking the summary generator

### 7.3
GIVEN a ChildConfig with summary_generator set to None
WHEN the child context completes
THEN the child handler SHALL store the full serialized result regardless of size (current behavior)

## Requirement 8: getChildOperations in Testing

### 8.1
GIVEN a DurableOperation in the testing module
WHEN get_child_operations is called
THEN it SHALL return all operations whose parent_id matches this operation's id

### 8.2
GIVEN a DurableOperation with no child operations
WHEN get_child_operations is called
THEN it SHALL return an empty Vec

### 8.3
GIVEN a DurableOperation with multiple child operations
WHEN get_child_operations is called
THEN the returned operations SHALL be ordered by their position in the operations list

## Requirement 9: Structured JSON Log Output

### 9.1
GIVEN a StructuredJsonLogger
WHEN any log method (debug, info, warn, error) is called
THEN the output SHALL be a valid JSON string written to stdout

### 9.2
GIVEN a StructuredJsonLogger with a configured JsonLogContext
WHEN a log entry is emitted
THEN it SHALL contain the fields: level, timestamp, requestId, executionArn, and message

### 9.3
GIVEN a StructuredJsonLogger with a configured JsonLogContext that includes tenantId
WHEN a log entry is emitted
THEN it SHALL include the tenantId field

### 9.4
GIVEN a StructuredJsonLogger receiving LogInfo with an operation_id
WHEN a log entry is emitted
THEN it SHALL include the operationId field

### 9.5
GIVEN a StructuredJsonLogger
WHEN the min_level is set to a specific level
THEN log calls below that level SHALL NOT produce any output

## Requirement 10: AWS_LAMBDA_LOG_LEVEL Integration

### 10.1
GIVEN the AWS_LAMBDA_LOG_LEVEL environment variable set to a valid level (DEBUG, INFO, WARN, ERROR)
WHEN a StructuredJsonLogger is constructed
THEN its min_level SHALL be set to the corresponding log level

### 10.2
GIVEN the AWS_LAMBDA_LOG_LEVEL environment variable is not set or set to an invalid value
WHEN a StructuredJsonLogger is constructed
THEN its min_level SHALL default to DEBUG (all messages emitted)

### 10.3
GIVEN a StructuredJsonLogger with min_level derived from AWS_LAMBDA_LOG_LEVEL
WHEN log methods are called
THEN only messages at or above the min_level priority SHALL be emitted

## Requirement 11: Enriched Termination Reasons

### 11.1
GIVEN the TerminationReason enum
WHEN new variants are added
THEN existing variant discriminant values (0-8) SHALL remain unchanged

### 11.2
GIVEN the TerminationReason enum
WHEN the SDK is compiled
THEN it SHALL include the new variants: OperationTerminated, RetryScheduled, WaitScheduled, CallbackPending, ContextValidationError, LambdaTimeoutApproaching

### 11.3
GIVEN any TerminationReason variant (existing or new)
WHEN serialized and deserialized via serde
THEN the round-trip SHALL produce the same variant

## Requirement 12: Summary Generators for Batch Operations

### 12.1
GIVEN the summary_generators module
WHEN parallel_summary is called with a serialized BatchResult
THEN it SHALL return a JSON string containing type, totalCount, successCount, failureCount, and status fields

### 12.2
GIVEN the summary_generators module
WHEN map_summary is called with a serialized BatchResult
THEN it SHALL return a JSON string containing type, totalCount, successCount, failureCount, and status fields

## Requirement 13: Runtime Logger Reconfiguration

### 13.1
GIVEN a DurableContext with a configure_logger method
WHEN configure_logger is called with a new Logger instance
THEN all subsequent log calls on that context SHALL use the new logger

### 13.2
GIVEN a DurableContext where configure_logger has NOT been called
WHEN log calls are made
THEN the original logger provided at construction time SHALL be used

## Requirement 14: Custom SDK User-Agent

### 14.1
GIVEN a LambdaDurableServiceClient created with from_aws_config_with_user_agent
WHEN AWS API requests are made
THEN the HTTP User-Agent header SHALL include the provided sdk_name and sdk_version

### 14.2
GIVEN a LambdaDurableServiceClient created with the existing from_aws_config method
WHEN AWS API requests are made
THEN the User-Agent header SHALL remain unchanged (current behavior)
