# Requirements Document

## Introduction

The Rust SDK's `CloudDurableTestRunner` currently invokes a Lambda function and returns immediately with the final result. It lacks the history polling, operation tracking, and real-time monitoring capabilities that the JavaScript SDK provides through its `HistoryPoller`, `OperationStorage` population, and `TestExecutionState` classes. This feature closes the cloud testing parity gap so that Rust SDK users have the same test observability and interaction capabilities as JavaScript SDK users.

## Glossary

- **Cloud_Runner**: The `CloudDurableTestRunner` struct in the Rust SDK testing crate that invokes Lambda functions and returns test results.
- **History_Poller**: A new component that periodically calls the `GetDurableExecutionHistory` API to retrieve operation events during a durable execution.
- **Operation_Storage**: The existing `OperationStorage` struct that stores and indexes `Operation` objects for lookup by id, name, index, or name-and-index.
- **Durable_Service_Client**: The existing `DurableServiceClient` trait with `get_operations()` method that calls the `GetDurableExecutionHistory` API.
- **Operation**: An individual unit of work within a durable execution (step, callback, wait, invoke, or context).
- **GetDurableExecutionHistory_API**: The AWS API endpoint that returns paginated operation history for a durable execution, identified by its ARN.
- **Poll_Interval**: The configurable duration between successive history polling requests (default 1000ms).
- **Timeout**: The configurable maximum duration the Cloud_Runner waits for execution completion before timing out (default 300s).
- **Durable_Execution_ARN**: The Amazon Resource Name returned by Lambda invocation that uniquely identifies a durable execution.
- **Next_Marker**: A pagination token returned by the GetDurableExecutionHistory_API when more results are available.
- **Callback_Sender**: The trait that enables sending callback success, failure, and heartbeat signals to a durable execution's callback operations.
- **Operation_Handle**: A handle returned by the Cloud_Runner that allows waiting for operation data and interacting with operations during execution.

## Requirements

### Requirement 1: History Polling Loop

**User Story:** As a test author, I want the Cloud_Runner to poll for operation history after invoking a Lambda function, so that I can observe operation progress during cloud execution.

#### Acceptance Criteria

1. WHEN a Lambda invocation returns a Durable_Execution_ARN, THE History_Poller SHALL begin polling the GetDurableExecutionHistory_API at the configured Poll_Interval.
2. THE History_Poller SHALL continue polling until the execution reaches a terminal state (Succeeded, Failed, Cancelled, or TimedOut).
3. WHILE the History_Poller is polling, THE History_Poller SHALL use the Durable_Execution_ARN from the Lambda invocation response to identify the execution.
4. WHEN the configured Timeout elapses before the execution reaches a terminal state, THE History_Poller SHALL stop polling and THE Cloud_Runner SHALL return a TestResult with a TimedOut status.
5. WHEN the History_Poller detects a terminal execution event, THE History_Poller SHALL stop polling and THE Cloud_Runner SHALL construct the final TestResult from the terminal event data.

### Requirement 2: Pagination Support

**User Story:** As a test author, I want the History_Poller to handle paginated API responses, so that all operations are retrieved regardless of execution size.

#### Acceptance Criteria

1. WHEN the GetDurableExecutionHistory_API response contains a Next_Marker, THE History_Poller SHALL issue subsequent requests using that Next_Marker to retrieve the next page.
2. THE History_Poller SHALL continue paginating within a single poll cycle until no Next_Marker is returned.
3. WHILE paginating within a single poll cycle, THE History_Poller SHALL wait for the configured Poll_Interval between each page request to avoid throttling.
4. THE History_Poller SHALL track the last Next_Marker from the previous poll cycle and use it as the starting marker for the next poll cycle, so that only new events are retrieved.

### Requirement 3: Operation Storage Population

**User Story:** As a test author, I want operations discovered during polling to be stored in Operation_Storage, so that I can look up operations by id, name, or index after execution.

#### Acceptance Criteria

1. WHEN the History_Poller receives operation data from the GetDurableExecutionHistory_API, THE Cloud_Runner SHALL populate the Operation_Storage with the retrieved Operation objects.
2. THE Operation_Storage SHALL deduplicate operations by operation id, updating existing entries when newer data is received for the same operation.
3. WHEN the Cloud_Runner run method completes, THE TestResult SHALL contain all operations collected from the Operation_Storage.
4. THE Cloud_Runner SHALL clear the Operation_Storage at the start of each run invocation.

### Requirement 4: Retry Logic with Exponential Backoff

**User Story:** As a test author, I want the History_Poller to retry transient API failures, so that intermittent network or throttling errors do not cause test failures.

#### Acceptance Criteria

1. WHEN a call to the GetDurableExecutionHistory_API fails with a transient error (throttling or network error), THE History_Poller SHALL retry the request up to 3 times.
2. THE History_Poller SHALL use exponential backoff between retry attempts, with a base delay that increases with each attempt.
3. IF all retry attempts for a single API call are exhausted, THEN THE History_Poller SHALL stop polling and THE Cloud_Runner SHALL return an error describing the failure.

### Requirement 5: Operation Handle Support for Cloud Execution

**User Story:** As a test author, I want to obtain Operation_Handle instances from the Cloud_Runner before execution completes, so that I can wait for specific operations and interact with them during cloud execution.

#### Acceptance Criteria

1. THE Cloud_Runner SHALL provide a method to obtain an Operation_Handle by operation name before or during execution.
2. THE Cloud_Runner SHALL provide a method to obtain an Operation_Handle by operation index before or during execution.
3. THE Cloud_Runner SHALL provide a method to obtain an Operation_Handle by operation name and index before or during execution.
4. THE Cloud_Runner SHALL provide a method to obtain an Operation_Handle by operation id before or during execution.
5. WHEN the History_Poller populates new operation data, THE Cloud_Runner SHALL notify waiting Operation_Handle instances so that their `wait_for_data` calls resolve.

### Requirement 6: Callback Interaction During Cloud Execution

**User Story:** As a test author, I want to send callback signals (success, failure, heartbeat) to operations during cloud execution, so that I can test callback-driven workflows end-to-end.

#### Acceptance Criteria

1. WHEN an Operation_Handle is populated with a callback operation that has a callback id, THE Operation_Handle SHALL allow sending a callback success signal via the Callback_Sender.
2. WHEN an Operation_Handle is populated with a callback operation that has a callback id, THE Operation_Handle SHALL allow sending a callback failure signal via the Callback_Sender.
3. WHEN an Operation_Handle is populated with a callback operation that has a callback id, THE Operation_Handle SHALL allow sending a callback heartbeat signal via the Callback_Sender.
4. THE Cloud_Runner SHALL configure Operation_Handle instances with a Callback_Sender that communicates with the durable execution service.

### Requirement 7: Configuration Usage

**User Story:** As a test author, I want the Poll_Interval and Timeout configuration fields to control polling behavior, so that I can tune cloud test execution for different environments.

#### Acceptance Criteria

1. THE Cloud_Runner SHALL use the configured Poll_Interval (default 1000ms) as the delay between polling cycles.
2. THE Cloud_Runner SHALL use the configured Timeout (default 300s) as the maximum wait duration for execution completion.
3. WHEN a custom Poll_Interval is provided via `CloudTestRunnerConfig`, THE History_Poller SHALL use that interval instead of the default.
4. WHEN a custom Timeout is provided via `CloudTestRunnerConfig`, THE Cloud_Runner SHALL use that timeout instead of the default.

### Requirement 8: Async Run Method

**User Story:** As a test author, I want the Cloud_Runner's `run` method to await execution completion via polling rather than returning immediately after Lambda invocation, so that the test result reflects the full execution outcome.

#### Acceptance Criteria

1. WHEN `run` is called, THE Cloud_Runner SHALL invoke the Lambda function, start the History_Poller, and await execution completion before returning the TestResult.
2. WHEN the execution completes successfully, THE Cloud_Runner SHALL parse the execution result from the terminal history event and return a TestResult with Succeeded status.
3. WHEN the execution fails, THE Cloud_Runner SHALL parse the error from the terminal history event and return a TestResult with Failed status.
4. IF the Lambda invocation itself fails (before a Durable_Execution_ARN is returned), THEN THE Cloud_Runner SHALL return an error without starting the History_Poller.
5. THE Cloud_Runner SHALL stop the History_Poller in all cases (success, failure, timeout) before returning from `run`.

### Requirement 9: History Event Collection

**User Story:** As a test author, I want the TestResult to include the raw history events collected during polling, so that I can inspect the full execution timeline for debugging.

#### Acceptance Criteria

1. THE Cloud_Runner SHALL collect all history events received during polling into the TestResult's history events list.
2. THE Cloud_Runner SHALL preserve the chronological order of history events in the TestResult.
3. WHEN the execution completes, THE TestResult SHALL contain history events from all poll cycles, including paginated results.
