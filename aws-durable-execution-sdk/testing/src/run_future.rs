//! Future wrapper for non-blocking test execution.
//!
//! This module provides `RunFuture<O>`, a future returned by `run()` that
//! resolves to a `TestResult<O>`. It can wrap either a `tokio::task::JoinHandle`
//! (for spawned tasks) or a boxed future (for inline async execution), enabling
//! concurrent interaction with `OperationHandle`s while the handler is executing.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::error::TestError;
use crate::test_result::TestResult;

/// Internal enum to hold either a JoinHandle or a boxed future.
enum RunFutureInner<O> {
    /// A spawned tokio task
    Spawned(tokio::task::JoinHandle<Result<TestResult<O>, TestError>>),
    /// An inline boxed future (used when the future is not `Send`)
    Inline(Pin<Box<dyn Future<Output = Result<TestResult<O>, TestError>>>>),
}

/// A future returned by `run()` that resolves to a `TestResult<O>`.
///
/// Wraps the execution so callers can `await` the result concurrently with
/// `OperationHandle` interactions (e.g., waiting for callback readiness and
/// sending responses mid-execution).
///
/// # Type Parameters
///
/// * `O` - The output type of the handler under test
///
/// # Examples
///
/// ```ignore
/// let handle = runner.get_operation_handle("my-callback");
/// let run_future = runner.run(input);
///
/// // Interact with the operation while execution is in progress
/// handle.wait_for_data(WaitingOperationStatus::Submitted).await?;
/// handle.send_callback_success("result").await?;
///
/// // Await the final result
/// let result = run_future.await?;
/// ```
pub struct RunFuture<O> {
    inner: RunFutureInner<O>,
}

impl<O> RunFuture<O> {
    /// Creates a new `RunFuture` wrapping the given `JoinHandle`.
    pub fn new(handle: tokio::task::JoinHandle<Result<TestResult<O>, TestError>>) -> Self {
        Self {
            inner: RunFutureInner::Spawned(handle),
        }
    }

    /// Creates a new `RunFuture` wrapping a boxed future.
    ///
    /// Used when the execution future is not `Send` and cannot be spawned
    /// as a separate tokio task.
    pub fn from_future(
        future: Pin<Box<dyn Future<Output = Result<TestResult<O>, TestError>>>>,
    ) -> Self {
        Self {
            inner: RunFutureInner::Inline(future),
        }
    }
}

impl<O> Future for RunFuture<O> {
    type Output = Result<TestResult<O>, TestError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match &mut self.inner {
            RunFutureInner::Spawned(handle) => {
                // SAFETY: JoinHandle is Unpin.
                let handle = Pin::new(handle);
                match handle.poll(cx) {
                    Poll::Ready(Ok(result)) => Poll::Ready(result),
                    Poll::Ready(Err(join_error)) => Poll::Ready(Err(
                        TestError::CheckpointServerError(format!("Task failed: {}", join_error)),
                    )),
                    Poll::Pending => Poll::Pending,
                }
            }
            RunFutureInner::Inline(future) => future.as_mut().poll(cx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_result::TestResult;
    use crate::types::ExecutionStatus;

    #[tokio::test]
    async fn test_run_future_success() {
        let handle =
            tokio::spawn(async { Ok(TestResult::<String>::success("hello".to_string(), vec![])) });
        let future = RunFuture::new(handle);
        let result = future.await.unwrap();
        assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
        assert_eq!(result.get_result().unwrap(), "hello");
    }

    #[tokio::test]
    async fn test_run_future_error() {
        let handle =
            tokio::spawn(async { Err(TestError::CheckpointServerError("test error".to_string())) });
        let future: RunFuture<String> = RunFuture::new(handle);
        let result = future.await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TestError::CheckpointServerError(_)
        ));
    }

    #[tokio::test]
    async fn test_run_future_join_error() {
        let handle: tokio::task::JoinHandle<Result<TestResult<String>, TestError>> =
            tokio::spawn(async {
                panic!("task panicked");
            });
        let future = RunFuture::new(handle);
        let result = future.await;
        assert!(result.is_err());
        match result.unwrap_err() {
            TestError::CheckpointServerError(msg) => {
                assert!(msg.contains("Task failed"));
            }
            other => panic!("Expected CheckpointServerError, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_run_future_from_future_success() {
        let future = RunFuture::<String>::from_future(Box::pin(async {
            Ok(TestResult::success("inline".to_string(), vec![]))
        }));
        let result = future.await.unwrap();
        assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
        assert_eq!(result.get_result().unwrap(), "inline");
    }

    #[tokio::test]
    async fn test_run_future_from_future_error() {
        let future = RunFuture::<String>::from_future(Box::pin(async {
            Err(TestError::CheckpointServerError("inline error".to_string()))
        }));
        let result = future.await;
        assert!(result.is_err());
    }
}
