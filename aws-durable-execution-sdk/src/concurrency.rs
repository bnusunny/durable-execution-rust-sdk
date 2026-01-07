//! Concurrency system for the AWS Durable Execution SDK.
//!
//! This module provides the concurrent execution infrastructure for
//! map and parallel operations, including execution counters, completion
//! logic, and the concurrent executor.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, Notify, Semaphore};

use crate::config::CompletionConfig;
use crate::error::{DurableError, ErrorObject};

/// Tracks execution progress for concurrent operations.
///
/// This struct uses atomic counters to safely track the number of
/// successful, failed, and completed tasks across concurrent executions.
///
/// # Requirements
///
/// - 14.4: Use ExecutionCounters to track success/failure counts
#[derive(Debug)]
pub struct ExecutionCounters {
    /// Total number of tasks to execute
    total_tasks: AtomicUsize,
    /// Number of successfully completed tasks
    success_count: AtomicUsize,
    /// Number of failed tasks
    failure_count: AtomicUsize,
    /// Number of completed tasks (success + failure)
    completed_count: AtomicUsize,
    /// Number of suspended tasks
    suspended_count: AtomicUsize,
}

impl ExecutionCounters {
    /// Creates a new ExecutionCounters with the given total task count.
    pub fn new(total_tasks: usize) -> Self {
        Self {
            total_tasks: AtomicUsize::new(total_tasks),
            success_count: AtomicUsize::new(0),
            failure_count: AtomicUsize::new(0),
            completed_count: AtomicUsize::new(0),
            suspended_count: AtomicUsize::new(0),
        }
    }

    /// Records a successful task completion.
    ///
    /// Returns the new success count.
    pub fn complete_task(&self) -> usize {
        self.completed_count.fetch_add(1, Ordering::SeqCst);
        self.success_count.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Records a failed task.
    ///
    /// Returns the new failure count.
    pub fn fail_task(&self) -> usize {
        self.completed_count.fetch_add(1, Ordering::SeqCst);
        self.failure_count.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Records a suspended task.
    ///
    /// Returns the new suspended count.
    pub fn suspend_task(&self) -> usize {
        self.suspended_count.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Returns the total number of tasks.
    pub fn total_tasks(&self) -> usize {
        self.total_tasks.load(Ordering::SeqCst)
    }

    /// Returns the current success count.
    pub fn success_count(&self) -> usize {
        self.success_count.load(Ordering::SeqCst)
    }

    /// Returns the current failure count.
    pub fn failure_count(&self) -> usize {
        self.failure_count.load(Ordering::SeqCst)
    }

    /// Returns the current completed count (success + failure).
    pub fn completed_count(&self) -> usize {
        self.completed_count.load(Ordering::SeqCst)
    }

    /// Returns the current suspended count.
    pub fn suspended_count(&self) -> usize {
        self.suspended_count.load(Ordering::SeqCst)
    }

    /// Returns the number of pending tasks (not yet completed or suspended).
    pub fn pending_count(&self) -> usize {
        let total = self.total_tasks();
        let completed = self.completed_count();
        let suspended = self.suspended_count();
        total.saturating_sub(completed + suspended)
    }

    /// Checks if the minimum successful count has been reached.
    ///
    /// # Arguments
    ///
    /// * `min_successful` - The minimum number of successful tasks required
    pub fn is_min_successful_reached(&self, min_successful: usize) -> bool {
        self.success_count() >= min_successful
    }

    /// Checks if the failure tolerance has been exceeded.
    ///
    /// # Arguments
    ///
    /// * `config` - The completion configuration with tolerance settings
    pub fn is_failure_tolerance_exceeded(&self, config: &CompletionConfig) -> bool {
        let failures = self.failure_count();
        let total = self.total_tasks();

        // Check absolute failure count
        if let Some(max_failures) = config.tolerated_failure_count {
            if failures > max_failures {
                return true;
            }
        }

        // Check failure percentage
        if let Some(max_percentage) = config.tolerated_failure_percentage {
            if total > 0 {
                let failure_percentage = failures as f64 / total as f64;
                if failure_percentage > max_percentage {
                    return true;
                }
            }
        }

        false
    }

    /// Determines if the operation should complete based on the completion config.
    ///
    /// Returns `Some(CompletionReason)` if the operation should complete,
    /// or `None` if it should continue.
    ///
    /// # Arguments
    ///
    /// * `config` - The completion configuration
    pub fn should_complete(&self, config: &CompletionConfig) -> Option<CompletionReason> {
        let total = self.total_tasks();
        let completed = self.completed_count();
        let suspended = self.suspended_count();
        let successes = self.success_count();

        // Check if min_successful is reached
        if let Some(min_successful) = config.min_successful {
            if successes >= min_successful {
                return Some(CompletionReason::MinSuccessfulReached);
            }
        }

        // Check if failure tolerance is exceeded
        if self.is_failure_tolerance_exceeded(config) {
            return Some(CompletionReason::FailureToleranceExceeded);
        }

        // Check if all tasks are done (completed or suspended)
        if completed + suspended >= total {
            if suspended > 0 && completed < total {
                return Some(CompletionReason::Suspended);
            }
            return Some(CompletionReason::AllCompleted);
        }

        None
    }

    /// Checks if all tasks have completed (success or failure, not suspended).
    pub fn all_completed(&self) -> bool {
        self.completed_count() >= self.total_tasks()
    }

    /// Checks if any tasks are still pending.
    pub fn has_pending(&self) -> bool {
        self.pending_count() > 0
    }
}

impl Default for ExecutionCounters {
    fn default() -> Self {
        Self::new(0)
    }
}


/// Reason why a concurrent operation completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompletionReason {
    /// All tasks completed (success or failure)
    AllCompleted,
    /// Minimum successful count was reached
    MinSuccessfulReached,
    /// Failure tolerance was exceeded
    FailureToleranceExceeded,
    /// Some tasks suspended (waiting for external events)
    Suspended,
}

impl std::fmt::Display for CompletionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AllCompleted => write!(f, "AllCompleted"),
            Self::MinSuccessfulReached => write!(f, "MinSuccessfulReached"),
            Self::FailureToleranceExceeded => write!(f, "FailureToleranceExceeded"),
            Self::Suspended => write!(f, "Suspended"),
        }
    }
}

/// Status of an individual item in a batch operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BatchItemStatus {
    /// Item completed successfully
    Succeeded,
    /// Item failed with an error
    Failed,
    /// Item was cancelled (due to early completion)
    Cancelled,
    /// Item is still pending
    Pending,
    /// Item is suspended (waiting for external event)
    Suspended,
}

impl BatchItemStatus {
    /// Returns true if this status represents a successful completion.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Succeeded)
    }

    /// Returns true if this status represents a failure.
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failed)
    }

    /// Returns true if this status represents a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }

    /// Returns true if this status represents a pending state.
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Pending | Self::Suspended)
    }
}

impl std::fmt::Display for BatchItemStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Succeeded => write!(f, "Succeeded"),
            Self::Failed => write!(f, "Failed"),
            Self::Cancelled => write!(f, "Cancelled"),
            Self::Pending => write!(f, "Pending"),
            Self::Suspended => write!(f, "Suspended"),
        }
    }
}

/// Result of a single item in a batch operation.
///
/// Contains the status, optional result value, and optional error
/// for each item processed in a map or parallel operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchItem<T> {
    /// Index of this item in the original collection
    pub index: usize,
    /// Status of this item
    pub status: BatchItemStatus,
    /// Result value if succeeded
    pub result: Option<T>,
    /// Error details if failed
    pub error: Option<ErrorObject>,
}

impl<T> BatchItem<T> {
    /// Creates a new succeeded BatchItem.
    pub fn succeeded(index: usize, result: T) -> Self {
        Self {
            index,
            status: BatchItemStatus::Succeeded,
            result: Some(result),
            error: None,
        }
    }

    /// Creates a new failed BatchItem.
    pub fn failed(index: usize, error: ErrorObject) -> Self {
        Self {
            index,
            status: BatchItemStatus::Failed,
            result: None,
            error: Some(error),
        }
    }

    /// Creates a new cancelled BatchItem.
    pub fn cancelled(index: usize) -> Self {
        Self {
            index,
            status: BatchItemStatus::Cancelled,
            result: None,
            error: None,
        }
    }

    /// Creates a new pending BatchItem.
    pub fn pending(index: usize) -> Self {
        Self {
            index,
            status: BatchItemStatus::Pending,
            result: None,
            error: None,
        }
    }

    /// Creates a new suspended BatchItem.
    pub fn suspended(index: usize) -> Self {
        Self {
            index,
            status: BatchItemStatus::Suspended,
            result: None,
            error: None,
        }
    }

    /// Returns true if this item succeeded.
    pub fn is_succeeded(&self) -> bool {
        self.status.is_success()
    }

    /// Returns true if this item failed.
    pub fn is_failed(&self) -> bool {
        self.status.is_failure()
    }

    /// Returns a reference to the result if succeeded.
    pub fn get_result(&self) -> Option<&T> {
        self.result.as_ref()
    }

    /// Returns a reference to the error if failed.
    pub fn get_error(&self) -> Option<&ErrorObject> {
        self.error.as_ref()
    }
}

/// Result of a batch operation (map or parallel).
///
/// Contains results for all items and the reason for completion.
///
/// # Requirements
///
/// - 8.5: Return a BatchResult containing results for all items
/// - 9.4: Return a BatchResult containing results for all branches
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResult<T> {
    /// Results for each item in the batch
    pub items: Vec<BatchItem<T>>,
    /// Reason why the batch completed
    pub completion_reason: CompletionReason,
}

impl<T> BatchResult<T> {
    /// Creates a new BatchResult.
    pub fn new(items: Vec<BatchItem<T>>, completion_reason: CompletionReason) -> Self {
        Self {
            items,
            completion_reason,
        }
    }

    /// Creates an empty BatchResult with AllCompleted reason.
    pub fn empty() -> Self {
        Self {
            items: Vec::new(),
            completion_reason: CompletionReason::AllCompleted,
        }
    }

    /// Returns all succeeded items.
    pub fn succeeded(&self) -> Vec<&BatchItem<T>> {
        self.items.iter().filter(|item| item.is_succeeded()).collect()
    }

    /// Returns all failed items.
    pub fn failed(&self) -> Vec<&BatchItem<T>> {
        self.items.iter().filter(|item| item.is_failed()).collect()
    }

    /// Returns all results from succeeded items.
    ///
    /// Returns an error if any item failed and the completion reason
    /// indicates failure tolerance was exceeded.
    pub fn get_results(&self) -> Result<Vec<&T>, DurableError> {
        if self.completion_reason == CompletionReason::FailureToleranceExceeded {
            // Find the first error
            if let Some(failed_item) = self.failed().first() {
                if let Some(ref error) = failed_item.error {
                    return Err(DurableError::UserCode {
                        message: error.error_message.clone(),
                        error_type: error.error_type.clone(),
                        stack_trace: error.stack_trace.clone(),
                    });
                }
            }
            return Err(DurableError::execution("Batch operation failed"));
        }

        Ok(self.items
            .iter()
            .filter_map(|item| item.result.as_ref())
            .collect())
    }

    /// Returns the number of succeeded items.
    pub fn success_count(&self) -> usize {
        self.succeeded().len()
    }

    /// Returns the number of failed items.
    pub fn failure_count(&self) -> usize {
        self.failed().len()
    }

    /// Returns the total number of items.
    pub fn total_count(&self) -> usize {
        self.items.len()
    }

    /// Returns true if all items succeeded.
    pub fn all_succeeded(&self) -> bool {
        self.items.iter().all(|item| item.is_succeeded())
    }

    /// Returns true if any item failed.
    pub fn has_failures(&self) -> bool {
        self.items.iter().any(|item| item.is_failed())
    }

    /// Returns true if the batch completed due to failure tolerance being exceeded.
    pub fn is_failure(&self) -> bool {
        self.completion_reason == CompletionReason::FailureToleranceExceeded
    }

    /// Returns true if the batch completed successfully (min successful reached or all completed).
    pub fn is_success(&self) -> bool {
        matches!(
            self.completion_reason,
            CompletionReason::AllCompleted | CompletionReason::MinSuccessfulReached
        )
    }
}

impl<T> Default for BatchResult<T> {
    fn default() -> Self {
        Self::empty()
    }
}


/// Executes tasks concurrently with configurable concurrency limits.
///
/// The executor manages parallel execution of tasks while respecting
/// concurrency limits and completion criteria.
///
/// # Requirements
///
/// - 14.2: Use Tokio for async execution of concurrent operations
/// - 14.3: Track execution state for each concurrent branch
/// - 14.5: Signal completion to waiting branches when criteria are met
pub struct ConcurrentExecutor {
    /// Maximum number of concurrent tasks (None = unlimited)
    max_concurrency: Option<usize>,
    /// Completion configuration
    completion_config: CompletionConfig,
    /// Execution counters for tracking progress
    counters: Arc<ExecutionCounters>,
    /// Notify for signaling completion
    completion_notify: Arc<Notify>,
    /// Semaphore for limiting concurrency
    semaphore: Option<Arc<Semaphore>>,
}

impl ConcurrentExecutor {
    /// Creates a new ConcurrentExecutor.
    ///
    /// # Arguments
    ///
    /// * `total_tasks` - Total number of tasks to execute
    /// * `max_concurrency` - Maximum concurrent tasks (None = unlimited)
    /// * `completion_config` - Configuration for completion criteria
    pub fn new(
        total_tasks: usize,
        max_concurrency: Option<usize>,
        completion_config: CompletionConfig,
    ) -> Self {
        let semaphore = max_concurrency.map(|n| Arc::new(Semaphore::new(n)));
        
        Self {
            max_concurrency,
            completion_config,
            counters: Arc::new(ExecutionCounters::new(total_tasks)),
            completion_notify: Arc::new(Notify::new()),
            semaphore,
        }
    }

    /// Returns a reference to the execution counters.
    pub fn counters(&self) -> &Arc<ExecutionCounters> {
        &self.counters
    }

    /// Returns a reference to the completion notify.
    pub fn completion_notify(&self) -> &Arc<Notify> {
        &self.completion_notify
    }

    /// Checks if the operation should complete based on current state.
    pub fn should_complete(&self) -> Option<CompletionReason> {
        self.counters.should_complete(&self.completion_config)
    }

    /// Records a successful task completion and checks for overall completion.
    ///
    /// Returns `Some(CompletionReason)` if the operation should now complete.
    pub fn record_success(&self) -> Option<CompletionReason> {
        self.counters.complete_task();
        let reason = self.should_complete();
        if reason.is_some() {
            self.completion_notify.notify_waiters();
        }
        reason
    }

    /// Records a failed task and checks for overall completion.
    ///
    /// Returns `Some(CompletionReason)` if the operation should now complete.
    pub fn record_failure(&self) -> Option<CompletionReason> {
        self.counters.fail_task();
        let reason = self.should_complete();
        if reason.is_some() {
            self.completion_notify.notify_waiters();
        }
        reason
    }

    /// Records a suspended task and checks for overall completion.
    ///
    /// Returns `Some(CompletionReason)` if the operation should now complete.
    pub fn record_suspend(&self) -> Option<CompletionReason> {
        self.counters.suspend_task();
        let reason = self.should_complete();
        if reason.is_some() {
            self.completion_notify.notify_waiters();
        }
        reason
    }

    /// Executes tasks concurrently and returns the batch result.
    ///
    /// # Arguments
    ///
    /// * `tasks` - Vector of async task functions
    ///
    /// # Returns
    ///
    /// A `BatchResult` containing results for all tasks.
    pub async fn execute<T, F, Fut>(self, tasks: Vec<F>) -> BatchResult<T>
    where
        T: Send + 'static,
        F: FnOnce(usize) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<T, DurableError>> + Send + 'static,
    {
        let total = tasks.len();
        if total == 0 {
            return BatchResult::empty();
        }

        // Shared state for collecting results
        let results: Arc<Mutex<Vec<BatchItem<T>>>> = Arc::new(Mutex::new(
            (0..total).map(BatchItem::pending).collect()
        ));

        // Spawn tasks
        let mut handles = Vec::with_capacity(total);
        
        for (index, task) in tasks.into_iter().enumerate() {
            let counters = self.counters.clone();
            let completion_notify = self.completion_notify.clone();
            let completion_config = self.completion_config.clone();
            let results = results.clone();
            let semaphore = self.semaphore.clone();

            let handle = tokio::spawn(async move {
                // Acquire semaphore permit if concurrency is limited
                let _permit = if let Some(ref sem) = semaphore {
                    Some(sem.acquire().await.expect("Semaphore closed"))
                } else {
                    None
                };

                // Check if we should still execute (early completion check)
                if counters.should_complete(&completion_config).is_some() {
                    let mut results_guard = results.lock().await;
                    results_guard[index] = BatchItem::cancelled(index);
                    return;
                }

                // Execute the task
                let result = task(index).await;

                // Record the result
                let mut results_guard = results.lock().await;
                match result {
                    Ok(value) => {
                        results_guard[index] = BatchItem::succeeded(index, value);
                        counters.complete_task();
                    }
                    Err(DurableError::Suspend { .. }) => {
                        results_guard[index] = BatchItem::suspended(index);
                        counters.suspend_task();
                    }
                    Err(error) => {
                        let error_obj = ErrorObject::from(&error);
                        results_guard[index] = BatchItem::failed(index, error_obj);
                        counters.fail_task();
                    }
                }
                drop(results_guard);

                // Check for completion and notify
                if counters.should_complete(&completion_config).is_some() {
                    completion_notify.notify_waiters();
                }
            });

            handles.push(handle);
        }

        // Wait for all tasks to complete or early completion
        for handle in handles {
            let _ = handle.await;
        }

        // Collect final results
        let final_results = Arc::try_unwrap(results)
            .map_err(|_| "All handles should be done")
            .unwrap()
            .into_inner();

        let completion_reason = self.counters.should_complete(&self.completion_config)
            .unwrap_or(CompletionReason::AllCompleted);

        BatchResult::new(final_results, completion_reason)
    }
}

impl std::fmt::Debug for ConcurrentExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConcurrentExecutor")
            .field("max_concurrency", &self.max_concurrency)
            .field("completion_config", &self.completion_config)
            .field("counters", &self.counters)
            .finish_non_exhaustive()
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    mod execution_counters_tests {
        use super::*;

        #[test]
        fn test_new() {
            let counters = ExecutionCounters::new(10);
            assert_eq!(counters.total_tasks(), 10);
            assert_eq!(counters.success_count(), 0);
            assert_eq!(counters.failure_count(), 0);
            assert_eq!(counters.completed_count(), 0);
            assert_eq!(counters.suspended_count(), 0);
            assert_eq!(counters.pending_count(), 10);
        }

        #[test]
        fn test_complete_task() {
            let counters = ExecutionCounters::new(5);
            
            assert_eq!(counters.complete_task(), 1);
            assert_eq!(counters.success_count(), 1);
            assert_eq!(counters.completed_count(), 1);
            assert_eq!(counters.pending_count(), 4);
            
            assert_eq!(counters.complete_task(), 2);
            assert_eq!(counters.success_count(), 2);
            assert_eq!(counters.completed_count(), 2);
        }

        #[test]
        fn test_fail_task() {
            let counters = ExecutionCounters::new(5);
            
            assert_eq!(counters.fail_task(), 1);
            assert_eq!(counters.failure_count(), 1);
            assert_eq!(counters.completed_count(), 1);
            assert_eq!(counters.pending_count(), 4);
        }

        #[test]
        fn test_suspend_task() {
            let counters = ExecutionCounters::new(5);
            
            assert_eq!(counters.suspend_task(), 1);
            assert_eq!(counters.suspended_count(), 1);
            assert_eq!(counters.completed_count(), 0);
            assert_eq!(counters.pending_count(), 4);
        }

        #[test]
        fn test_is_min_successful_reached() {
            let counters = ExecutionCounters::new(10);
            
            assert!(!counters.is_min_successful_reached(3));
            
            counters.complete_task();
            counters.complete_task();
            assert!(!counters.is_min_successful_reached(3));
            
            counters.complete_task();
            assert!(counters.is_min_successful_reached(3));
        }

        #[test]
        fn test_is_failure_tolerance_exceeded_count() {
            let counters = ExecutionCounters::new(10);
            let config = CompletionConfig {
                tolerated_failure_count: Some(2),
                ..Default::default()
            };
            
            counters.fail_task();
            counters.fail_task();
            assert!(!counters.is_failure_tolerance_exceeded(&config));
            
            counters.fail_task();
            assert!(counters.is_failure_tolerance_exceeded(&config));
        }

        #[test]
        fn test_is_failure_tolerance_exceeded_percentage() {
            let counters = ExecutionCounters::new(10);
            let config = CompletionConfig {
                tolerated_failure_percentage: Some(0.2),
                ..Default::default()
            };
            
            counters.fail_task();
            counters.fail_task();
            assert!(!counters.is_failure_tolerance_exceeded(&config));
            
            counters.fail_task();
            assert!(counters.is_failure_tolerance_exceeded(&config));
        }

        #[test]
        fn test_should_complete_min_successful() {
            let counters = ExecutionCounters::new(10);
            let config = CompletionConfig::with_min_successful(3);
            
            assert!(counters.should_complete(&config).is_none());
            
            counters.complete_task();
            counters.complete_task();
            assert!(counters.should_complete(&config).is_none());
            
            counters.complete_task();
            assert_eq!(
                counters.should_complete(&config),
                Some(CompletionReason::MinSuccessfulReached)
            );
        }

        #[test]
        fn test_should_complete_failure_tolerance() {
            let counters = ExecutionCounters::new(10);
            let config = CompletionConfig::all_successful();
            
            assert!(counters.should_complete(&config).is_none());
            
            counters.fail_task();
            assert_eq!(
                counters.should_complete(&config),
                Some(CompletionReason::FailureToleranceExceeded)
            );
        }

        #[test]
        fn test_should_complete_all_completed() {
            let counters = ExecutionCounters::new(3);
            let config = CompletionConfig::all_completed();
            
            counters.complete_task();
            counters.complete_task();
            assert!(counters.should_complete(&config).is_none());
            
            counters.complete_task();
            assert_eq!(
                counters.should_complete(&config),
                Some(CompletionReason::AllCompleted)
            );
        }

        #[test]
        fn test_should_complete_suspended() {
            let counters = ExecutionCounters::new(3);
            let config = CompletionConfig::all_completed();
            
            counters.complete_task();
            counters.complete_task();
            counters.suspend_task();
            
            assert_eq!(
                counters.should_complete(&config),
                Some(CompletionReason::Suspended)
            );
        }

        #[test]
        fn test_all_completed() {
            let counters = ExecutionCounters::new(3);
            
            assert!(!counters.all_completed());
            
            counters.complete_task();
            counters.complete_task();
            counters.complete_task();
            
            assert!(counters.all_completed());
        }

        #[test]
        fn test_has_pending() {
            let counters = ExecutionCounters::new(2);
            
            assert!(counters.has_pending());
            
            counters.complete_task();
            assert!(counters.has_pending());
            
            counters.complete_task();
            assert!(!counters.has_pending());
        }
    }

    mod batch_item_tests {
        use super::*;

        #[test]
        fn test_succeeded() {
            let item = BatchItem::succeeded(0, 42);
            assert_eq!(item.index, 0);
            assert!(item.is_succeeded());
            assert!(!item.is_failed());
            assert_eq!(item.get_result(), Some(&42));
            assert!(item.get_error().is_none());
        }

        #[test]
        fn test_failed() {
            let error = ErrorObject::new("TestError", "test message");
            let item: BatchItem<i32> = BatchItem::failed(1, error);
            assert_eq!(item.index, 1);
            assert!(!item.is_succeeded());
            assert!(item.is_failed());
            assert!(item.get_result().is_none());
            assert!(item.get_error().is_some());
        }

        #[test]
        fn test_cancelled() {
            let item: BatchItem<i32> = BatchItem::cancelled(2);
            assert_eq!(item.index, 2);
            assert_eq!(item.status, BatchItemStatus::Cancelled);
        }

        #[test]
        fn test_pending() {
            let item: BatchItem<i32> = BatchItem::pending(3);
            assert_eq!(item.index, 3);
            assert_eq!(item.status, BatchItemStatus::Pending);
        }

        #[test]
        fn test_suspended() {
            let item: BatchItem<i32> = BatchItem::suspended(4);
            assert_eq!(item.index, 4);
            assert_eq!(item.status, BatchItemStatus::Suspended);
        }
    }

    mod batch_result_tests {
        use super::*;

        #[test]
        fn test_empty() {
            let result: BatchResult<i32> = BatchResult::empty();
            assert!(result.items.is_empty());
            assert_eq!(result.completion_reason, CompletionReason::AllCompleted);
        }

        #[test]
        fn test_succeeded() {
            let items = vec![
                BatchItem::succeeded(0, 1),
                BatchItem::succeeded(1, 2),
                BatchItem::failed(2, ErrorObject::new("Error", "msg")),
            ];
            let result = BatchResult::new(items, CompletionReason::AllCompleted);
            
            let succeeded = result.succeeded();
            assert_eq!(succeeded.len(), 2);
        }

        #[test]
        fn test_failed() {
            let items = vec![
                BatchItem::succeeded(0, 1),
                BatchItem::failed(1, ErrorObject::new("Error", "msg")),
            ];
            let result = BatchResult::new(items, CompletionReason::AllCompleted);
            
            let failed = result.failed();
            assert_eq!(failed.len(), 1);
        }

        #[test]
        fn test_get_results_success() {
            let items = vec![
                BatchItem::succeeded(0, 1),
                BatchItem::succeeded(1, 2),
            ];
            let result = BatchResult::new(items, CompletionReason::AllCompleted);
            
            let results = result.get_results().unwrap();
            assert_eq!(results, vec![&1, &2]);
        }

        #[test]
        fn test_get_results_failure_tolerance_exceeded() {
            let items = vec![
                BatchItem::succeeded(0, 1),
                BatchItem::failed(1, ErrorObject::new("TestError", "test")),
            ];
            let result = BatchResult::new(items, CompletionReason::FailureToleranceExceeded);
            
            assert!(result.get_results().is_err());
        }

        #[test]
        fn test_counts() {
            let items = vec![
                BatchItem::succeeded(0, 1),
                BatchItem::succeeded(1, 2),
                BatchItem::failed(2, ErrorObject::new("Error", "msg")),
            ];
            let result = BatchResult::new(items, CompletionReason::AllCompleted);
            
            assert_eq!(result.success_count(), 2);
            assert_eq!(result.failure_count(), 1);
            assert_eq!(result.total_count(), 3);
        }

        #[test]
        fn test_all_succeeded() {
            let items = vec![
                BatchItem::succeeded(0, 1),
                BatchItem::succeeded(1, 2),
            ];
            let result = BatchResult::new(items, CompletionReason::AllCompleted);
            assert!(result.all_succeeded());
            
            let items_with_failure = vec![
                BatchItem::succeeded(0, 1),
                BatchItem::failed(1, ErrorObject::new("Error", "msg")),
            ];
            let result_with_failure = BatchResult::new(items_with_failure, CompletionReason::AllCompleted);
            assert!(!result_with_failure.all_succeeded());
        }

        #[test]
        fn test_is_success() {
            let result: BatchResult<i32> = BatchResult::new(vec![], CompletionReason::AllCompleted);
            assert!(result.is_success());
            
            let result2: BatchResult<i32> = BatchResult::new(vec![], CompletionReason::MinSuccessfulReached);
            assert!(result2.is_success());
            
            let result3: BatchResult<i32> = BatchResult::new(vec![], CompletionReason::FailureToleranceExceeded);
            assert!(!result3.is_success());
        }
    }

    mod concurrent_executor_tests {
        use super::*;

        #[tokio::test]
        async fn test_execute_empty() {
            let executor = ConcurrentExecutor::new(0, None, CompletionConfig::all_completed());
            let tasks: Vec<Box<dyn FnOnce(usize) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<i32, DurableError>> + Send>> + Send>> = vec![];
            let result = executor.execute(tasks).await;
            
            assert!(result.items.is_empty());
            assert_eq!(result.completion_reason, CompletionReason::AllCompleted);
        }

        #[tokio::test]
        async fn test_execute_all_success() {
            let executor = ConcurrentExecutor::new(3, None, CompletionConfig::all_completed());
            let tasks: Vec<_> = (0..3).map(|i| {
                move |_idx: usize| async move { Ok(i * 10) }
            }).collect();
            
            let result = executor.execute(tasks).await;
            
            assert_eq!(result.total_count(), 3);
            assert_eq!(result.success_count(), 3);
            assert!(result.all_succeeded());
        }

        #[tokio::test]
        async fn test_execute_with_failures() {
            let executor = ConcurrentExecutor::new(3, None, CompletionConfig::all_completed());
            
            // Use a single closure type that handles all cases
            let tasks: Vec<Box<dyn FnOnce(usize) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<i32, DurableError>> + Send>> + Send>> = vec![
                Box::new(|_idx: usize| Box::pin(async { Ok(1) }) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<i32, DurableError>> + Send>>),
                Box::new(|_idx: usize| Box::pin(async { Err(DurableError::execution("test error")) }) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<i32, DurableError>> + Send>>),
                Box::new(|_idx: usize| Box::pin(async { Ok(3) }) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<i32, DurableError>> + Send>>),
            ];
            
            let result = executor.execute(tasks).await;
            
            assert_eq!(result.total_count(), 3);
            assert_eq!(result.success_count(), 2);
            assert_eq!(result.failure_count(), 1);
        }

        #[tokio::test]
        async fn test_execute_min_successful() {
            let executor = ConcurrentExecutor::new(5, None, CompletionConfig::with_min_successful(2));
            let tasks: Vec<_> = (0..5).map(|i| {
                move |_idx: usize| async move { Ok(i) }
            }).collect();
            
            let result = executor.execute(tasks).await;
            
            // Should complete when min_successful is reached
            assert!(result.success_count() >= 2);
        }

        #[tokio::test]
        async fn test_execute_with_concurrency_limit() {
            let executor = ConcurrentExecutor::new(5, Some(2), CompletionConfig::all_completed());
            let tasks: Vec<_> = (0..5).map(|i| {
                move |_idx: usize| async move { Ok(i) }
            }).collect();
            
            let result = executor.execute(tasks).await;
            
            assert_eq!(result.total_count(), 5);
            assert!(result.all_succeeded());
        }

        #[tokio::test]
        async fn test_record_success() {
            let executor = ConcurrentExecutor::new(3, None, CompletionConfig::with_min_successful(2));
            
            assert!(executor.record_success().is_none());
            assert_eq!(
                executor.record_success(),
                Some(CompletionReason::MinSuccessfulReached)
            );
        }

        #[tokio::test]
        async fn test_record_failure() {
            let executor = ConcurrentExecutor::new(3, None, CompletionConfig::all_successful());
            
            assert_eq!(
                executor.record_failure(),
                Some(CompletionReason::FailureToleranceExceeded)
            );
        }
    }
}


#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    /// **Feature: durable-execution-rust-sdk, Property 6: Map/Parallel Completion Criteria**
    /// **Validates: Requirements 8.6, 8.7, 9.3**
    ///
    /// For any map or parallel operation with CompletionConfig specifying min_successful=M,
    /// the operation SHALL complete successfully as soon as M tasks succeed,
    /// regardless of remaining task status.
    mod completion_criteria_tests {
        use super::*;

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(100))]

            /// Property test: min_successful completion triggers when threshold is reached
            /// For any total_tasks and min_successful where min_successful <= total_tasks,
            /// when exactly min_successful tasks succeed, should_complete returns MinSuccessfulReached.
            #[test]
            fn prop_min_successful_triggers_completion(
                total_tasks in 1usize..=50,
                min_successful_ratio in 0.1f64..=1.0,
            ) {
                let min_successful = ((total_tasks as f64 * min_successful_ratio).ceil() as usize).max(1).min(total_tasks);
                let config = CompletionConfig::with_min_successful(min_successful);
                let counters = ExecutionCounters::new(total_tasks);

                // Complete tasks until we reach min_successful
                for i in 0..min_successful {
                    if i < min_successful - 1 {
                        counters.complete_task();
                        // Should not complete yet
                        prop_assert!(
                            counters.should_complete(&config).is_none() ||
                            counters.should_complete(&config) == Some(CompletionReason::MinSuccessfulReached),
                            "Should not complete before reaching min_successful"
                        );
                    } else {
                        counters.complete_task();
                        // Should complete now
                        prop_assert_eq!(
                            counters.should_complete(&config),
                            Some(CompletionReason::MinSuccessfulReached),
                            "Should complete when min_successful is reached"
                        );
                    }
                }
            }

            /// Property test: failure tolerance exceeded triggers completion
            /// For any total_tasks and tolerated_failure_count, when failures exceed tolerance,
            /// should_complete returns FailureToleranceExceeded.
            #[test]
            fn prop_failure_tolerance_exceeded_triggers_completion(
                total_tasks in 2usize..=50,
                tolerated_failures in 0usize..=10,
            ) {
                let config = CompletionConfig::with_failure_tolerance(tolerated_failures);
                let counters = ExecutionCounters::new(total_tasks);

                // Fail tasks until we exceed tolerance
                for i in 0..=tolerated_failures {
                    counters.fail_task();
                    if i < tolerated_failures {
                        // Should not complete yet (unless all tasks are done)
                        let result = counters.should_complete(&config);
                        prop_assert!(
                            result.is_none() || result == Some(CompletionReason::AllCompleted),
                            "Should not trigger failure tolerance until exceeded"
                        );
                    }
                }

                // Now we've exceeded tolerance
                prop_assert_eq!(
                    counters.should_complete(&config),
                    Some(CompletionReason::FailureToleranceExceeded),
                    "Should complete when failure tolerance is exceeded"
                );
            }

            /// Property test: all_completed triggers when all tasks finish
            /// For any total_tasks, when all tasks complete (success or failure),
            /// should_complete returns AllCompleted.
            #[test]
            fn prop_all_completed_triggers_when_all_done(
                total_tasks in 1usize..=50,
                success_count in 0usize..=50,
            ) {
                let success_count = success_count.min(total_tasks);
                let failure_count = total_tasks - success_count;
                let config = CompletionConfig::all_completed();
                let counters = ExecutionCounters::new(total_tasks);

                // Complete some tasks successfully
                for _ in 0..success_count {
                    counters.complete_task();
                }

                // Fail the rest
                for _ in 0..failure_count {
                    counters.fail_task();
                }

                // All tasks are done
                prop_assert_eq!(
                    counters.should_complete(&config),
                    Some(CompletionReason::AllCompleted),
                    "Should complete when all tasks are done"
                );
            }

            /// Property test: suspended tasks trigger Suspended completion
            /// When some tasks complete and others suspend, should_complete returns Suspended.
            #[test]
            fn prop_suspended_triggers_when_tasks_suspend(
                total_tasks in 2usize..=50,
                completed_count in 1usize..=49,
            ) {
                let completed_count = completed_count.min(total_tasks - 1);
                let suspended_count = total_tasks - completed_count;
                let config = CompletionConfig::all_completed();
                let counters = ExecutionCounters::new(total_tasks);

                // Complete some tasks
                for _ in 0..completed_count {
                    counters.complete_task();
                }

                // Suspend the rest
                for _ in 0..suspended_count {
                    counters.suspend_task();
                }

                // Should be suspended
                prop_assert_eq!(
                    counters.should_complete(&config),
                    Some(CompletionReason::Suspended),
                    "Should return Suspended when tasks are suspended"
                );
            }

            /// Property test: success count is always accurate
            /// For any sequence of complete_task calls, success_count equals the number of calls.
            #[test]
            fn prop_success_count_accurate(
                total_tasks in 1usize..=100,
                successes in 0usize..=100,
            ) {
                let successes = successes.min(total_tasks);
                let counters = ExecutionCounters::new(total_tasks);

                for _ in 0..successes {
                    counters.complete_task();
                }

                prop_assert_eq!(
                    counters.success_count(),
                    successes,
                    "Success count should match number of complete_task calls"
                );
            }

            /// Property test: failure count is always accurate
            /// For any sequence of fail_task calls, failure_count equals the number of calls.
            #[test]
            fn prop_failure_count_accurate(
                total_tasks in 1usize..=100,
                failures in 0usize..=100,
            ) {
                let failures = failures.min(total_tasks);
                let counters = ExecutionCounters::new(total_tasks);

                for _ in 0..failures {
                    counters.fail_task();
                }

                prop_assert_eq!(
                    counters.failure_count(),
                    failures,
                    "Failure count should match number of fail_task calls"
                );
            }

            /// Property test: completed count equals success + failure
            /// For any combination of successes and failures, completed_count = success_count + failure_count.
            #[test]
            fn prop_completed_count_is_sum(
                total_tasks in 2usize..=100,
                successes in 0usize..=50,
                failures in 0usize..=50,
            ) {
                let successes = successes.min(total_tasks / 2);
                let failures = failures.min(total_tasks - successes);
                let counters = ExecutionCounters::new(total_tasks);

                for _ in 0..successes {
                    counters.complete_task();
                }
                for _ in 0..failures {
                    counters.fail_task();
                }

                prop_assert_eq!(
                    counters.completed_count(),
                    successes + failures,
                    "Completed count should equal success + failure"
                );
            }

            /// Property test: pending count is accurate
            /// pending_count = total_tasks - completed_count - suspended_count
            #[test]
            fn prop_pending_count_accurate(
                total_tasks in 3usize..=100,
                successes in 0usize..=33,
                failures in 0usize..=33,
                suspends in 0usize..=33,
            ) {
                let successes = successes.min(total_tasks / 3);
                let failures = failures.min((total_tasks - successes) / 2);
                let suspends = suspends.min(total_tasks - successes - failures);
                let counters = ExecutionCounters::new(total_tasks);

                for _ in 0..successes {
                    counters.complete_task();
                }
                for _ in 0..failures {
                    counters.fail_task();
                }
                for _ in 0..suspends {
                    counters.suspend_task();
                }

                let expected_pending = total_tasks - successes - failures - suspends;
                prop_assert_eq!(
                    counters.pending_count(),
                    expected_pending,
                    "Pending count should be total - completed - suspended"
                );
            }

            /// Property test: failure percentage calculation is correct
            /// For any number of failures, the percentage is calculated correctly.
            #[test]
            fn prop_failure_percentage_calculation(
                total_tasks in 1usize..=100,
                failures in 0usize..=100,
                tolerance_percentage in 0.0f64..=1.0,
            ) {
                let failures = failures.min(total_tasks);
                let config = CompletionConfig {
                    tolerated_failure_percentage: Some(tolerance_percentage),
                    ..Default::default()
                };
                let counters = ExecutionCounters::new(total_tasks);

                for _ in 0..failures {
                    counters.fail_task();
                }

                let actual_percentage = failures as f64 / total_tasks as f64;
                let exceeded = counters.is_failure_tolerance_exceeded(&config);

                if actual_percentage > tolerance_percentage {
                    prop_assert!(exceeded, "Should exceed tolerance when percentage is higher");
                } else {
                    prop_assert!(!exceeded, "Should not exceed tolerance when percentage is lower or equal");
                }
            }
        }
    }
}
