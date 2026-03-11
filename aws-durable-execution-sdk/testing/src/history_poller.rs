//! History polling types and traits for cloud test execution.
//!
//! This module defines the `HistoryApiClient` trait for abstracting history API calls,
//! along with the data types used to represent history pages, poll results, and terminal states.
//! These are the building blocks for the `HistoryPoller` which periodically retrieves
//! operation history during cloud durable execution testing.

use std::time::Duration;

use crate::error::TestError;
use crate::test_result::HistoryEvent;
use crate::types::{ExecutionStatus, TestResultError};
use durable_execution_sdk::Operation;

/// Trait abstracting the `GetDurableExecutionHistory` API call for testability.
///
/// Implementations of this trait provide the ability to retrieve paginated history
/// for a durable execution identified by its ARN. The trait is object-safe and
/// async, enabling both real API clients and mock implementations for testing.
#[async_trait::async_trait]
pub trait HistoryApiClient: Send + Sync {
    /// Retrieves a single page of execution history.
    ///
    /// # Arguments
    ///
    /// * `arn` - The Durable Execution ARN identifying the execution
    /// * `marker` - Optional pagination marker from a previous response
    ///
    /// # Returns
    ///
    /// A `HistoryPage` containing events, operations, and pagination/terminal info,
    /// or a `TestError` if the API call fails.
    async fn get_history(&self, arn: &str, marker: Option<&str>) -> Result<HistoryPage, TestError>;
}

/// A single page of results from the `GetDurableExecutionHistory` API.
///
/// Each page may contain history events, operations, and pagination metadata.
/// When the execution reaches a terminal state, the page indicates this along
/// with the terminal status and result/error information.
#[derive(Debug, Clone)]
pub struct HistoryPage {
    /// History events from this page.
    pub events: Vec<HistoryEvent>,
    /// Node.js-compatible history events from this page (populated by cloud runner).
    #[allow(dead_code)]
    pub nodejs_events: Vec<crate::NodeJsHistoryEvent>,
    /// Operations discovered in this page.
    pub operations: Vec<Operation>,
    /// Pagination marker for the next page, if more results are available.
    pub next_marker: Option<String>,
    /// Whether this page indicates the execution has reached a terminal state.
    pub is_terminal: bool,
    /// The terminal execution status, if `is_terminal` is true.
    pub terminal_status: Option<ExecutionStatus>,
    /// The serialized result payload, if the execution succeeded.
    pub terminal_result: Option<String>,
    /// The error details, if the execution failed.
    pub terminal_error: Option<TestResultError>,
}

/// Aggregated result of a single poll cycle, which may span multiple pages.
///
/// A poll cycle exhausts all available pages (following `next_marker` pagination)
/// and collects all operations and events into a single result. If any page
/// indicates a terminal state, it is captured here.
#[derive(Debug, Clone)]
pub struct PollResult {
    /// All operations collected across all pages in this poll cycle.
    pub operations: Vec<Operation>,
    /// All history events collected across all pages in this poll cycle.
    pub events: Vec<HistoryEvent>,
    /// Node.js-compatible history events collected across all pages.
    pub nodejs_events: Vec<crate::NodeJsHistoryEvent>,
    /// Terminal state information, if the execution completed during this cycle.
    pub terminal: Option<TerminalState>,
}

/// Terminal execution outcome extracted from a history page.
///
/// Captures the final status and result/error of a completed durable execution.
#[derive(Debug, Clone)]
pub struct TerminalState {
    /// The terminal execution status (Succeeded, Failed, Cancelled, or TimedOut).
    pub status: ExecutionStatus,
    /// The serialized result payload, if the execution succeeded.
    pub result: Option<String>,
    /// The error details, if the execution failed.
    pub error: Option<TestResultError>,
}

/// Polls `GetDurableExecutionHistory` and feeds results into `OperationStorage`.
///
/// The `HistoryPoller` periodically calls the history API, handles pagination
/// within each poll cycle, and retries transient errors with exponential backoff.
pub struct HistoryPoller<C: HistoryApiClient> {
    api_client: C,
    durable_execution_arn: String,
    poll_interval: Duration,
    pub(crate) last_marker: Option<String>,
    max_retries: usize,
}

impl<C: HistoryApiClient> HistoryPoller<C> {
    /// Creates a new `HistoryPoller`.
    ///
    /// # Arguments
    ///
    /// * `api_client` - The client used to call the history API
    /// * `arn` - The Durable Execution ARN identifying the execution to poll
    /// * `poll_interval` - Duration to wait between page requests within a poll cycle
    pub fn new(api_client: C, arn: String, poll_interval: Duration) -> Self {
        Self {
            api_client,
            durable_execution_arn: arn,
            poll_interval,
            last_marker: None,
            max_retries: 3,
        }
    }

    /// Makes a single API call with retry logic and exponential backoff.
    ///
    /// Retries transient errors up to `max_retries` times using the formula:
    /// `(attempt² - 1) * 150 + 1000` milliseconds.
    ///
    /// For attempt 1: (1 - 1) * 150 + 1000 = 1000ms
    /// For attempt 2: (4 - 1) * 150 + 1000 = 1450ms
    /// For attempt 3: (9 - 1) * 150 + 1000 = 2200ms
    ///
    /// # Arguments
    ///
    /// * `marker` - Optional pagination marker for the API call
    ///
    /// # Returns
    ///
    /// The `HistoryPage` on success, or a `TestError` if all retries are exhausted.
    pub(crate) async fn call_with_retries(
        &self,
        marker: Option<&str>,
    ) -> Result<HistoryPage, TestError> {
        let mut attempts: u64 = 0;
        loop {
            match self
                .api_client
                .get_history(&self.durable_execution_arn, marker)
                .await
            {
                Ok(page) => return Ok(page),
                Err(_e) if attempts < self.max_retries as u64 => {
                    attempts += 1;
                    let delay_ms = (attempts.pow(2) - 1) * 150 + 1000;
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Executes one poll cycle, exhausting all pages via pagination.
    ///
    /// Starting from `self.last_marker`, this method calls the history API
    /// and follows `next_marker` pagination tokens until no more pages remain.
    /// Between each page request, it waits `poll_interval` to avoid throttling.
    ///
    /// After the cycle completes, `self.last_marker` is updated to the last
    /// page's `next_marker` so the next poll cycle starts where this one left off.
    ///
    /// # Returns
    ///
    /// A `PollResult` containing all operations and events aggregated across
    /// all pages, plus any terminal state detected during the cycle.
    pub async fn poll_once(&mut self) -> Result<PollResult, TestError> {
        let mut all_operations = Vec::new();
        let mut all_events = Vec::new();
        let mut all_nodejs_events = Vec::new();
        let mut terminal = None;

        // First page uses the marker carried over from the previous cycle
        let mut current_marker = self.last_marker.clone();

        loop {
            let page = self.call_with_retries(current_marker.as_deref()).await?;

            all_operations.extend(page.operations);
            all_events.extend(page.events);
            all_nodejs_events.extend(page.nodejs_events);

            // Capture terminal state from any page (first one wins)
            if page.is_terminal && terminal.is_none() {
                terminal = Some(TerminalState {
                    status: page.terminal_status.unwrap_or(ExecutionStatus::Failed),
                    result: page.terminal_result,
                    error: page.terminal_error,
                });
            }

            match page.next_marker {
                Some(next) => {
                    current_marker = Some(next);
                    // Wait between pages within a cycle to avoid throttling (Req 2.3)
                    tokio::time::sleep(self.poll_interval).await;
                }
                None => {
                    // Update last_marker for cross-cycle continuity (Req 2.4)
                    self.last_marker = current_marker;
                    break;
                }
            }
        }

        Ok(PollResult {
            operations: all_operations,
            events: all_events,
            nodejs_events: all_nodejs_events,
            terminal,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    /// A mock implementation of `HistoryApiClient` for testing.
    ///
    /// Records every `(arn, marker)` call and returns pre-configured responses
    /// from a `VecDeque`. Supports injecting transient errors for retry testing.
    pub(crate) struct MockHistoryApiClient {
        /// Pre-configured responses returned in FIFO order.
        responses: Mutex<VecDeque<Result<HistoryPage, TestError>>>,
        /// Recorded `(arn, marker)` pairs from each `get_history` call.
        pub(crate) calls: Mutex<Vec<(String, Option<String>)>>,
    }

    impl MockHistoryApiClient {
        /// Creates a new mock with the given sequence of responses.
        pub(crate) fn new(responses: VecDeque<Result<HistoryPage, TestError>>) -> Self {
            Self {
                responses: Mutex::new(responses),
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl HistoryApiClient for MockHistoryApiClient {
        async fn get_history(
            &self,
            arn: &str,
            marker: Option<&str>,
        ) -> Result<HistoryPage, TestError> {
            // Record the call
            self.calls
                .lock()
                .unwrap()
                .push((arn.to_string(), marker.map(|m| m.to_string())));

            // Pop the next response, or panic if none left
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .expect("MockHistoryApiClient: no more responses configured")
        }
    }

    #[test]
    fn mock_returns_configured_responses() {
        let page = HistoryPage {
            events: vec![],
            nodejs_events: Vec::new(),
            operations: vec![],
            next_marker: None,
            is_terminal: false,
            terminal_status: None,
            terminal_result: None,
            terminal_error: None,
        };

        let mut responses = VecDeque::new();
        responses.push_back(Ok(page));
        responses.push_back(Err(TestError::aws_error("transient failure")));

        let mock = MockHistoryApiClient::new(responses);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        // First call returns Ok
        let result = rt.block_on(mock.get_history("arn:test", None));
        assert!(result.is_ok());

        // Second call returns Err
        let result = rt.block_on(mock.get_history("arn:test", Some("marker-1")));
        assert!(result.is_err());

        // Verify recorded calls
        let calls = mock.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], ("arn:test".to_string(), None));
        assert_eq!(
            calls[1],
            ("arn:test".to_string(), Some("marker-1".to_string()))
        );
    }

    #[test]
    fn mock_records_all_call_arguments() {
        let mut responses = VecDeque::new();
        for _ in 0..3 {
            responses.push_back(Ok(HistoryPage {
                events: vec![],
                nodejs_events: Vec::new(),
                operations: vec![],
                next_marker: None,
                is_terminal: false,
                terminal_status: None,
                terminal_result: None,
                terminal_error: None,
            }));
        }

        let mock = MockHistoryApiClient::new(responses);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(mock.get_history("arn:exec:1", None)).unwrap();
        rt.block_on(mock.get_history("arn:exec:1", Some("m1")))
            .unwrap();
        rt.block_on(mock.get_history("arn:exec:2", Some("m2")))
            .unwrap();

        let calls = mock.calls.lock().unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].0, "arn:exec:1");
        assert_eq!(calls[0].1, None);
        assert_eq!(calls[1].1, Some("m1".to_string()));
        assert_eq!(calls[2].0, "arn:exec:2");
        assert_eq!(calls[2].1, Some("m2".to_string()));
    }

    #[test]
    #[should_panic(expected = "no more responses configured")]
    fn mock_panics_when_responses_exhausted() {
        let mock = MockHistoryApiClient::new(VecDeque::new());

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        // Should panic — no responses configured
        let _ = rt.block_on(mock.get_history("arn:test", None));
    }

    // --- HistoryPoller tests ---

    fn empty_page() -> HistoryPage {
        HistoryPage {
            events: vec![],
            nodejs_events: Vec::new(),
            operations: vec![],
            next_marker: None,
            is_terminal: false,
            terminal_status: None,
            terminal_result: None,
            terminal_error: None,
        }
    }

    #[test]
    fn new_sets_defaults() {
        let mock = MockHistoryApiClient::new(VecDeque::new());
        let poller =
            HistoryPoller::new(mock, "arn:test:123".to_string(), Duration::from_millis(500));

        assert_eq!(poller.durable_execution_arn, "arn:test:123");
        assert_eq!(poller.poll_interval, Duration::from_millis(500));
        assert!(poller.last_marker.is_none());
        assert_eq!(poller.max_retries, 3);
    }

    #[tokio::test]
    async fn call_with_retries_succeeds_on_first_try() {
        let mut responses = VecDeque::new();
        responses.push_back(Ok(empty_page()));

        let mock = MockHistoryApiClient::new(responses);
        let poller = HistoryPoller::new(mock, "arn:test".to_string(), Duration::from_millis(100));

        let result = poller.call_with_retries(None).await;
        assert!(result.is_ok());

        let calls = poller.api_client.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
    }

    #[tokio::test]
    async fn call_with_retries_succeeds_after_transient_errors() {
        tokio::time::pause();

        let mut responses = VecDeque::new();
        // 2 failures then success
        responses.push_back(Err(TestError::aws_error("transient 1")));
        responses.push_back(Err(TestError::aws_error("transient 2")));
        responses.push_back(Ok(empty_page()));

        let mock = MockHistoryApiClient::new(responses);
        let poller = HistoryPoller::new(mock, "arn:test".to_string(), Duration::from_millis(100));

        let result = poller.call_with_retries(None).await;
        assert!(result.is_ok());

        let calls = poller.api_client.calls.lock().unwrap();
        assert_eq!(calls.len(), 3);
    }

    #[tokio::test]
    async fn call_with_retries_fails_after_max_retries() {
        tokio::time::pause();

        let mut responses = VecDeque::new();
        // 4 failures (exceeds max_retries of 3)
        responses.push_back(Err(TestError::aws_error("fail 1")));
        responses.push_back(Err(TestError::aws_error("fail 2")));
        responses.push_back(Err(TestError::aws_error("fail 3")));
        responses.push_back(Err(TestError::aws_error("fail 4")));

        let mock = MockHistoryApiClient::new(responses);
        let poller = HistoryPoller::new(mock, "arn:test".to_string(), Duration::from_millis(100));

        let result = poller.call_with_retries(None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("fail 4"));

        // Should have made 4 calls: initial + 3 retries
        let calls = poller.api_client.calls.lock().unwrap();
        assert_eq!(calls.len(), 4);
    }

    #[tokio::test]
    async fn call_with_retries_passes_marker() {
        let mut responses = VecDeque::new();
        responses.push_back(Ok(empty_page()));

        let mock = MockHistoryApiClient::new(responses);
        let poller = HistoryPoller::new(mock, "arn:test".to_string(), Duration::from_millis(100));

        let result = poller.call_with_retries(Some("my-marker")).await;
        assert!(result.is_ok());

        let calls = poller.api_client.calls.lock().unwrap();
        assert_eq!(calls[0].1, Some("my-marker".to_string()));
    }

    #[tokio::test]
    async fn call_with_retries_uses_correct_arn() {
        let mut responses = VecDeque::new();
        responses.push_back(Ok(empty_page()));

        let mock = MockHistoryApiClient::new(responses);
        let poller = HistoryPoller::new(
            mock,
            "arn:aws:lambda:us-east-1:123:exec-456".to_string(),
            Duration::from_millis(100),
        );

        poller.call_with_retries(None).await.unwrap();

        let calls = poller.api_client.calls.lock().unwrap();
        assert_eq!(calls[0].0, "arn:aws:lambda:us-east-1:123:exec-456");
    }

    #[tokio::test]
    async fn call_with_retries_backoff_timing() {
        tokio::time::pause();

        let mut responses = VecDeque::new();
        // 3 failures then success
        responses.push_back(Err(TestError::aws_error("fail 1")));
        responses.push_back(Err(TestError::aws_error("fail 2")));
        responses.push_back(Err(TestError::aws_error("fail 3")));
        responses.push_back(Ok(empty_page()));

        let mock = MockHistoryApiClient::new(responses);
        let poller = HistoryPoller::new(mock, "arn:test".to_string(), Duration::from_millis(100));

        let start = tokio::time::Instant::now();
        let result = poller.call_with_retries(None).await;
        let elapsed = start.elapsed();

        assert!(result.is_ok());

        // Expected delays:
        // attempt 1: (1² - 1) * 150 + 1000 = 1000ms
        // attempt 2: (2² - 1) * 150 + 1000 = 1450ms
        // attempt 3: (3² - 1) * 150 + 1000 = 2200ms
        // Total: 4650ms
        let expected_total = Duration::from_millis(1000 + 1450 + 2200);
        assert!(
            elapsed >= expected_total,
            "Expected at least {:?} elapsed, got {:?}",
            expected_total,
            elapsed
        );
    }

    // --- poll_once tests ---

    fn make_operation(id: &str) -> Operation {
        Operation::new(id, durable_execution_sdk::OperationType::Step)
    }

    fn make_event(event_type: &str) -> crate::test_result::HistoryEvent {
        crate::test_result::HistoryEvent::new(event_type)
    }

    #[tokio::test]
    async fn poll_once_single_page_no_terminal() {
        let op = make_operation("op-1");
        let evt = make_event("StepStarted");

        let mut responses = VecDeque::new();
        responses.push_back(Ok(HistoryPage {
            events: vec![evt],
            nodejs_events: Vec::new(),
            operations: vec![op],
            next_marker: None,
            is_terminal: false,
            terminal_status: None,
            terminal_result: None,
            terminal_error: None,
        }));

        let mock = MockHistoryApiClient::new(responses);
        let mut poller =
            HistoryPoller::new(mock, "arn:test".to_string(), Duration::from_millis(10));

        let result = poller.poll_once().await.unwrap();
        assert_eq!(result.operations.len(), 1);
        assert_eq!(result.operations[0].operation_id, "op-1");
        assert_eq!(result.events.len(), 1);
        assert!(result.terminal.is_none());
        // last_marker should be None (no next_marker on the page)
        assert!(poller.last_marker.is_none());
    }

    #[tokio::test]
    async fn poll_once_multi_page_pagination() {
        tokio::time::pause();

        let mut responses = VecDeque::new();
        // Page 1: has next_marker
        responses.push_back(Ok(HistoryPage {
            events: vec![make_event("E1")],
            nodejs_events: Vec::new(),
            operations: vec![make_operation("op-1")],
            next_marker: Some("marker-A".to_string()),
            is_terminal: false,
            terminal_status: None,
            terminal_result: None,
            terminal_error: None,
        }));
        // Page 2: has next_marker
        responses.push_back(Ok(HistoryPage {
            events: vec![make_event("E2")],
            nodejs_events: Vec::new(),
            operations: vec![make_operation("op-2")],
            next_marker: Some("marker-B".to_string()),
            is_terminal: false,
            terminal_status: None,
            terminal_result: None,
            terminal_error: None,
        }));
        // Page 3: no next_marker (end of pagination)
        responses.push_back(Ok(HistoryPage {
            events: vec![make_event("E3")],
            nodejs_events: Vec::new(),
            operations: vec![make_operation("op-3")],
            next_marker: None,
            is_terminal: false,
            terminal_status: None,
            terminal_result: None,
            terminal_error: None,
        }));

        let mock = MockHistoryApiClient::new(responses);
        let mut poller =
            HistoryPoller::new(mock, "arn:test".to_string(), Duration::from_millis(50));

        let result = poller.poll_once().await.unwrap();

        // All 3 pages aggregated
        assert_eq!(result.operations.len(), 3);
        assert_eq!(result.events.len(), 3);
        assert!(result.terminal.is_none());

        // Verify API calls used correct markers
        let calls = poller.api_client.calls.lock().unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].1, None); // first call: no marker
        assert_eq!(calls[1].1, Some("marker-A".to_string()));
        assert_eq!(calls[2].1, Some("marker-B".to_string()));

        // last_marker should be the last page's current_marker (marker-B)
        // since the last page had no next_marker, current_marker stays as marker-B
        assert_eq!(poller.last_marker, Some("marker-B".to_string()));
    }

    #[tokio::test]
    async fn poll_once_detects_terminal_state() {
        let mut responses = VecDeque::new();
        responses.push_back(Ok(HistoryPage {
            events: vec![make_event("ExecutionSucceeded")],
            nodejs_events: Vec::new(),
            operations: vec![make_operation("op-1")],
            next_marker: None,
            is_terminal: true,
            terminal_status: Some(crate::types::ExecutionStatus::Succeeded),
            terminal_result: Some(r#"{"value": 42}"#.to_string()),
            terminal_error: None,
        }));

        let mock = MockHistoryApiClient::new(responses);
        let mut poller =
            HistoryPoller::new(mock, "arn:test".to_string(), Duration::from_millis(10));

        let result = poller.poll_once().await.unwrap();
        assert!(result.terminal.is_some());
        let terminal = result.terminal.unwrap();
        assert_eq!(terminal.status, crate::types::ExecutionStatus::Succeeded);
        assert_eq!(terminal.result, Some(r#"{"value": 42}"#.to_string()));
        assert!(terminal.error.is_none());
    }

    #[tokio::test]
    async fn poll_once_terminal_with_error() {
        let mut responses = VecDeque::new();
        responses.push_back(Ok(HistoryPage {
            events: vec![],
            nodejs_events: Vec::new(),
            operations: vec![],
            next_marker: None,
            is_terminal: true,
            terminal_status: Some(crate::types::ExecutionStatus::Failed),
            terminal_result: None,
            terminal_error: Some(crate::types::TestResultError::new("RuntimeError", "boom")),
        }));

        let mock = MockHistoryApiClient::new(responses);
        let mut poller =
            HistoryPoller::new(mock, "arn:test".to_string(), Duration::from_millis(10));

        let result = poller.poll_once().await.unwrap();
        let terminal = result.terminal.unwrap();
        assert_eq!(terminal.status, crate::types::ExecutionStatus::Failed);
        assert!(terminal.error.is_some());
        assert_eq!(
            terminal.error.unwrap().error_type,
            Some("RuntimeError".to_string())
        );
    }

    #[tokio::test]
    async fn poll_once_marker_continuity_across_cycles() {
        tokio::time::pause();

        let mut responses = VecDeque::new();
        // Cycle 1: single page with next_marker
        responses.push_back(Ok(HistoryPage {
            events: vec![],
            nodejs_events: Vec::new(),
            operations: vec![make_operation("op-1")],
            next_marker: Some("cycle1-marker".to_string()),
            is_terminal: false,
            terminal_status: None,
            terminal_result: None,
            terminal_error: None,
        }));
        // Cycle 1 page 2: no next_marker
        responses.push_back(Ok(HistoryPage {
            events: vec![],
            nodejs_events: Vec::new(),
            operations: vec![],
            next_marker: None,
            is_terminal: false,
            terminal_status: None,
            terminal_result: None,
            terminal_error: None,
        }));
        // Cycle 2: should start with cycle1-marker
        responses.push_back(Ok(HistoryPage {
            events: vec![],
            nodejs_events: Vec::new(),
            operations: vec![make_operation("op-2")],
            next_marker: None,
            is_terminal: true,
            terminal_status: Some(crate::types::ExecutionStatus::Succeeded),
            terminal_result: Some("\"done\"".to_string()),
            terminal_error: None,
        }));

        let mock = MockHistoryApiClient::new(responses);
        let mut poller =
            HistoryPoller::new(mock, "arn:test".to_string(), Duration::from_millis(10));

        // Cycle 1
        let _r1 = poller.poll_once().await.unwrap();
        assert_eq!(poller.last_marker, Some("cycle1-marker".to_string()));

        // Cycle 2: should use cycle1-marker as starting marker
        let _r2 = poller.poll_once().await.unwrap();

        let calls = poller.api_client.calls.lock().unwrap();
        // Cycle 2's first call should use the marker from cycle 1
        assert_eq!(calls[2].1, Some("cycle1-marker".to_string()));
    }

    #[tokio::test]
    async fn poll_once_propagates_api_error() {
        tokio::time::pause();

        let mut responses = VecDeque::new();
        // All retries fail
        for _ in 0..4 {
            responses.push_back(Err(TestError::aws_error("service unavailable")));
        }

        let mock = MockHistoryApiClient::new(responses);
        let mut poller =
            HistoryPoller::new(mock, "arn:test".to_string(), Duration::from_millis(10));

        let result = poller.poll_once().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn poll_once_empty_page() {
        let mut responses = VecDeque::new();
        responses.push_back(Ok(empty_page()));

        let mock = MockHistoryApiClient::new(responses);
        let mut poller =
            HistoryPoller::new(mock, "arn:test".to_string(), Duration::from_millis(10));

        let result = poller.poll_once().await.unwrap();
        assert!(result.operations.is_empty());
        assert!(result.events.is_empty());
        assert!(result.terminal.is_none());
    }

    #[tokio::test]
    async fn poll_once_pagination_waits_between_pages() {
        tokio::time::pause();

        let mut responses = VecDeque::new();
        responses.push_back(Ok(HistoryPage {
            events: vec![],
            nodejs_events: Vec::new(),
            operations: vec![],
            next_marker: Some("m1".to_string()),
            is_terminal: false,
            terminal_status: None,
            terminal_result: None,
            terminal_error: None,
        }));
        responses.push_back(Ok(empty_page()));

        let mock = MockHistoryApiClient::new(responses);
        let poll_interval = Duration::from_millis(200);
        let mut poller = HistoryPoller::new(mock, "arn:test".to_string(), poll_interval);

        let start = tokio::time::Instant::now();
        poller.poll_once().await.unwrap();
        let elapsed = start.elapsed();

        // Should have waited at least poll_interval between the two pages
        assert!(
            elapsed >= poll_interval,
            "Expected at least {:?} elapsed, got {:?}",
            poll_interval,
            elapsed
        );
    }
}
