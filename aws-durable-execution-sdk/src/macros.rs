//! Ergonomic macros for promise combinators.
//!
//! This module provides declarative macros (`all!`, `any!`, `race!`, `all_settled!`)
//! that enable combining heterogeneous futures without manual type erasure.
//!
//! # Problem
//!
//! The method-based API (`ctx.all()`, etc.) requires all futures in the vector to have
//! the exact same concrete type. In Rust, each closure has a unique anonymous type,
//! which means combining different step operations doesn't compile:
//!
//! ```rust,ignore
//! // This doesn't compile - each closure has a unique type!
//! let futures = vec![
//!     ctx.step(|_| Ok(1), None),  // Type A
//!     ctx.step(|_| Ok(2), None),  // Type B (different!)
//! ];
//! let results = ctx.all(futures).await?;
//! ```
//!
//! # Solution
//!
//! These macros automatically box each future to erase their concrete types,
//! enabling the common use case of combining different step operations:
//!
//! ```rust,ignore
//! use aws_durable_execution_sdk::all;
//!
//! // Clone contexts for each future
//! let ctx1 = ctx.clone();
//! let ctx2 = ctx.clone();
//! let ctx3 = ctx.clone();
//!
//! // This works with macros!
//! let results = all!(ctx,
//!     async move { ctx1.step(|_| Ok(1), None).await },
//!     async move { ctx2.step(|_| Ok(2), None).await },
//!     async move { ctx3.step(|_| Ok(3), None).await },
//! ).await?;
//! // results: Vec<i32> = [1, 2, 3]
//! ```
//!
//! # When to Use Macros vs Methods
//!
//! ## Use Macros When:
//!
//! - **Combining different step operations** - Each closure has a unique type
//! - **Mixing operation types** - Combining step + wait + callback operations
//! - **Writing inline futures** - Ad-hoc combinations of different operations
//! - **Small, fixed number of futures** - Known at compile time
//!
//! ```rust,ignore
//! use aws_durable_execution_sdk::{all, any};
//!
//! // Different closures = different types - use macros!
//! let ctx1 = ctx.clone();
//! let ctx2 = ctx.clone();
//! let ctx3 = ctx.clone();
//!
//! let results = all!(ctx,
//!     async move { ctx1.step(|_| fetch_user_data(), None).await },
//!     async move { ctx2.step(|_| fetch_preferences(), None).await },
//!     async move { ctx3.step(|_| fetch_notifications(), None).await },
//! ).await?;
//!
//! // Fallback pattern with any!
//! let ctx1 = ctx.clone();
//! let ctx2 = ctx.clone();
//! let ctx3 = ctx.clone();
//!
//! let data = any!(ctx,
//!     async move { ctx1.step(|_| fetch_from_primary(), None).await },
//!     async move { ctx2.step(|_| fetch_from_secondary(), None).await },
//!     async move { ctx3.step(|_| fetch_from_cache(), None).await },
//! ).await?;
//! ```
//!
//! ## Use Methods When:
//!
//! - **Processing homogeneous futures from iterators/loops** - Same closure applied to different data
//! - **Working with pre-boxed futures** - Already type-erased
//! - **Programmatically generating futures** - Dynamic number of futures from a single source
//! - **Using `ctx.map()`** - Preferred for iterating over collections
//!
//! ```rust,ignore
//! // Same closure applied to different data - use methods!
//! let user_ids = vec![1, 2, 3, 4, 5];
//! let futures: Vec<_> = user_ids
//!     .into_iter()
//!     .map(|id| {
//!         let ctx = ctx.clone();
//!         async move { ctx.step(move |_| fetch_user(id), None).await }
//!     })
//!     .collect();
//! let users = ctx.all(futures).await?;
//!
//! // Or even better, use ctx.map():
//! let batch = ctx.map(user_ids, |child_ctx, id, _| {
//!     Box::pin(async move { child_ctx.step(|_| fetch_user(id), None).await })
//! }, None).await?;
//! ```
//!
//! # Quick Reference
//!
//! | Macro | Behavior | Returns | Use Case |
//! |-------|----------|---------|----------|
//! | `all!` | Wait for all to succeed | `Vec<T>` or first error | Parallel fetch, batch processing |
//! | `any!` | First success wins | `T` or combined error | Fallback patterns, redundancy |
//! | `race!` | First to settle wins | `T` (success or error) | Timeouts, competitive operations |
//! | `all_settled!` | Wait for all to settle | `BatchResult<T>` | Collect all outcomes, partial success |
//!
//! # Available Macros
//!
//! - `all!` - Wait for all futures to succeed, return error on first failure
//! - `any!` - Return first successful result, error only if all fail
//! - `race!` - Return first result to settle (success or failure)
//! - `all_settled!` - Wait for all futures to settle, return all outcomes

/// Waits for all futures to complete successfully.
///
/// This macro boxes each future to enable combining heterogeneous future types,
/// then delegates to `ctx.all()`. All futures run concurrently, and the macro
/// returns when all have completed successfully or when the first error occurs.
///
/// # Arguments
///
/// * `$ctx` - The [`DurableContext`](crate::DurableContext) instance
/// * `$fut` - One or more future expressions (comma-separated)
///
/// # Returns
///
/// * `Ok(Vec<T>)` - All results in the same order as the input futures
/// * `Err(DurableError)` - The first error encountered (remaining futures are cancelled)
///
/// # Examples
///
/// ## Basic Usage
///
/// ```rust,ignore
/// use aws_durable_execution_sdk::all;
///
/// // Clone contexts for each future to satisfy lifetime requirements
/// let ctx1 = ctx.clone();
/// let ctx2 = ctx.clone();
/// let ctx3 = ctx.clone();
///
/// let results = all!(ctx,
///     async move { ctx1.step(|_| Ok(1), None).await },
///     async move { ctx2.step(|_| Ok(2), None).await },
///     async move { ctx3.step(|_| Ok(3), None).await },
/// ).await?;
/// assert_eq!(results, vec![1, 2, 3]);
/// ```
///
/// ## Parallel Data Fetching
///
/// ```rust,ignore
/// use aws_durable_execution_sdk::all;
///
/// // Clone contexts for parallel operations
/// let ctx1 = ctx.clone();
/// let ctx2 = ctx.clone();
/// let ctx3 = ctx.clone();
///
/// // Fetch multiple pieces of data in parallel
/// let results = all!(ctx,
///     async move { ctx1.step(|_| fetch_user(user_id), None).await },
///     async move { ctx2.step(|_| fetch_preferences(user_id), None).await },
///     async move { ctx3.step(|_| fetch_notifications(user_id), None).await },
/// ).await?;
/// let (user, preferences, notifications) = (
///     results[0].clone(),
///     results[1].clone(),
///     results[2].clone()
/// );
/// ```
///
/// ## Error Handling
///
/// ```rust,ignore
/// use aws_durable_execution_sdk::all;
///
/// let ctx1 = ctx.clone();
/// let ctx2 = ctx.clone();
/// let ctx3 = ctx.clone();
///
/// // If any step fails, the entire operation fails
/// let result = all!(ctx,
///     async move { ctx1.step(|_| Ok(1), None).await },
///     async move { ctx2.step(|_| Err::<i32, _>("failed".into()), None).await },
///     async move { ctx3.step(|_| Ok(3), None).await },  // This may not execute
/// ).await;
///
/// assert!(result.is_err());
/// ```
///
/// # When to Use
///
/// Use `all!` when you need all operations to succeed and want to fail fast
/// on the first error. For collecting all outcomes (including failures),
/// use `all_settled!` instead.
///
/// # See Also
///
/// * `any!` - Return first success
/// * `race!` - Return first to settle
/// * `all_settled!` - Collect all outcomes
#[macro_export]
macro_rules! all {
    ($ctx:expr, $($fut:expr),+ $(,)?) => {{
        let futures: ::std::vec::Vec<
            ::std::pin::Pin<
                ::std::boxed::Box<
                    dyn ::std::future::Future<
                        Output = $crate::error::DurableResult<_>
                    > + ::std::marker::Send
                >
            >
        > = ::std::vec![
            $(::std::boxed::Box::pin($fut)),+
        ];
        $ctx.all(futures)
    }};
}

/// Returns the first successful result from multiple futures.
///
/// This macro boxes each future to enable combining heterogeneous future types,
/// then delegates to `ctx.any()`. All futures run concurrently, and the macro
/// returns as soon as any future succeeds. If all futures fail, returns a
/// combined error.
///
/// # Arguments
///
/// * `$ctx` - The [`DurableContext`](crate::DurableContext) instance
/// * `$fut` - One or more future expressions (comma-separated)
///
/// # Returns
///
/// * `Ok(T)` - The first successful result (remaining futures are cancelled)
/// * `Err(DurableError)` - Combined error if all futures fail
///
/// # Examples
///
/// ## Basic Fallback Pattern
///
/// ```rust,ignore
/// use aws_durable_execution_sdk::any;
///
/// // Clone contexts for each future
/// let ctx1 = ctx.clone();
/// let ctx2 = ctx.clone();
/// let ctx3 = ctx.clone();
///
/// // Try multiple sources, return first success
/// let data = any!(ctx,
///     async move { ctx1.step(|_| fetch_from_primary(), None).await },
///     async move { ctx2.step(|_| fetch_from_secondary(), None).await },
///     async move { ctx3.step(|_| fetch_from_cache(), None).await },
/// ).await?;
/// ```
///
/// ## Redundant Service Calls
///
/// ```rust,ignore
/// use aws_durable_execution_sdk::any;
///
/// let ctx1 = ctx.clone();
/// let ctx2 = ctx.clone();
///
/// // Call multiple redundant services, use first response
/// let price = any!(ctx,
///     async move { ctx1.step(|_| get_price_from_service_a(item_id), None).await },
///     async move { ctx2.step(|_| get_price_from_service_b(item_id), None).await },
/// ).await?;
/// ```
///
/// ## Handling All Failures
///
/// ```rust,ignore
/// use aws_durable_execution_sdk::any;
///
/// let ctx1 = ctx.clone();
/// let ctx2 = ctx.clone();
///
/// // If all sources fail, get combined error
/// let result = any!(ctx,
///     async move { ctx1.step(|_| Err::<String, _>("primary failed".into()), None).await },
///     async move { ctx2.step(|_| Err::<String, _>("secondary failed".into()), None).await },
/// ).await;
///
/// // Error contains information about all failures
/// assert!(result.is_err());
/// ```
///
/// # When to Use
///
/// Use `any!` for fallback patterns where you want the first successful result
/// and don't care which source provides it. Unlike `race!`, `any!` ignores
/// failures and only returns an error if ALL futures fail.
///
/// # See Also
///
/// * `all!` - Wait for all to succeed
/// * `race!` - Return first to settle (success or failure)
/// * `all_settled!` - Collect all outcomes
#[macro_export]
macro_rules! any {
    ($ctx:expr, $($fut:expr),+ $(,)?) => {{
        let futures: ::std::vec::Vec<
            ::std::pin::Pin<
                ::std::boxed::Box<
                    dyn ::std::future::Future<
                        Output = $crate::error::DurableResult<_>
                    > + ::std::marker::Send
                >
            >
        > = ::std::vec![
            $(::std::boxed::Box::pin($fut)),+
        ];
        $ctx.any(futures)
    }};
}

/// Returns the result of the first future to settle (success or failure).
///
/// This macro boxes each future to enable combining heterogeneous future types,
/// then delegates to `ctx.race()`. All futures run concurrently, and the macro
/// returns as soon as any future completes, regardless of whether it succeeded
/// or failed.
///
/// # Arguments
///
/// * `$ctx` - The [`DurableContext`](crate::DurableContext) instance
/// * `$fut` - One or more future expressions (comma-separated)
///
/// # Returns
///
/// * `Ok(T)` - If the first future to settle succeeded
/// * `Err(DurableError)` - If the first future to settle failed
///
/// # Examples
///
/// ## Timeout Pattern
///
/// ```rust,ignore
/// use aws_durable_execution_sdk::race;
///
/// let ctx1 = ctx.clone();
/// let ctx2 = ctx.clone();
///
/// // Race between operation and timeout
/// let result = race!(ctx,
///     async move { ctx1.step(|_| slow_operation(), None).await },
///     async move {
///         ctx2.step(|_| {
///             std::thread::sleep(std::time::Duration::from_secs(5));
///             Err::<String, _>("timeout".into())
///         }, None).await
///     },
/// ).await;
///
/// match result {
///     Ok(data) => println!("Operation completed: {}", data),
///     Err(e) => println!("Timed out or failed: {}", e),
/// }
/// ```
///
/// ## Competitive Operations
///
/// ```rust,ignore
/// use aws_durable_execution_sdk::race;
///
/// let ctx1 = ctx.clone();
/// let ctx2 = ctx.clone();
///
/// // Use whichever service responds first
/// let result = race!(ctx,
///     async move { ctx1.step(|_| call_service_a(), None).await },
///     async move { ctx2.step(|_| call_service_b(), None).await },
/// ).await?;
/// ```
///
/// ## First Failure Wins
///
/// ```rust,ignore
/// use aws_durable_execution_sdk::race;
///
/// let ctx1 = ctx.clone();
/// let ctx2 = ctx.clone();
///
/// // If the first to settle is an error, that error is returned
/// let result = race!(ctx,
///     async move { ctx1.step(|_| Err::<i32, _>("fast error".into()), None).await },
///     async move {
///         ctx2.step(|_| {
///             std::thread::sleep(std::time::Duration::from_millis(100));
///             Ok(42)
///         }, None).await
///     },
/// ).await;
///
/// assert!(result.is_err()); // Error settled first
/// ```
///
/// # When to Use
///
/// Use `race!` when you want the first result regardless of success or failure.
/// This is useful for timeout patterns or when competing operations should
/// cancel each other. Unlike `any!`, `race!` returns immediately on the
/// first settlement, even if it's a failure.
///
/// # See Also
///
/// * `all!` - Wait for all to succeed
/// * `any!` - Return first success (ignores failures)
/// * `all_settled!` - Collect all outcomes
#[macro_export]
macro_rules! race {
    ($ctx:expr, $($fut:expr),+ $(,)?) => {{
        let futures: ::std::vec::Vec<
            ::std::pin::Pin<
                ::std::boxed::Box<
                    dyn ::std::future::Future<
                        Output = $crate::error::DurableResult<_>
                    > + ::std::marker::Send
                >
            >
        > = ::std::vec![
            $(::std::boxed::Box::pin($fut)),+
        ];
        $ctx.race(futures)
    }};
}


/// Waits for all futures to settle, returning all outcomes.
///
/// This macro boxes each future to enable combining heterogeneous future types,
/// then delegates to `ctx.all_settled()`. Unlike `all!`, this macro does not
/// short-circuit on failure - it waits for all futures to complete and returns
/// a [`BatchResult`](crate::concurrency::BatchResult) containing all outcomes.
///
/// # Arguments
///
/// * `$ctx` - The [`DurableContext`](crate::DurableContext) instance
/// * `$fut` - One or more future expressions (comma-separated)
///
/// # Returns
///
/// * `Ok(BatchResult<T>)` - Contains outcomes for all futures (never fails)
///
/// The [`BatchResult`](crate::concurrency::BatchResult) provides methods to:
/// - `successes()` / `succeeded()` - Get successful results
/// - `failures()` / `failed()` - Get failed results
/// - `success_count()` / `failure_count()` - Count outcomes
/// - `all_succeeded()` / `has_failures()` - Check overall status
/// - `get_results()` - Get all successful results or error if any failed
///
/// # Examples
///
/// ## Collecting All Outcomes
///
/// ```rust,ignore
/// use aws_durable_execution_sdk::all_settled;
///
/// let ctx1 = ctx.clone();
/// let ctx2 = ctx.clone();
/// let ctx3 = ctx.clone();
///
/// let batch = all_settled!(ctx,
///     async move { ctx1.step(|_| Ok(1), None).await },
///     async move { ctx2.step(|_| Err::<i32, _>("failed".into()), None).await },
///     async move { ctx3.step(|_| Ok(3), None).await },
/// ).await?;
///
/// println!("Total: {}", batch.items.len());        // 3
/// println!("Succeeded: {}", batch.success_count()); // 2
/// println!("Failed: {}", batch.failure_count());    // 1
/// ```
///
/// ## Processing Successes and Failures Separately
///
/// ```rust,ignore
/// use aws_durable_execution_sdk::all_settled;
///
/// let ctx1 = ctx.clone();
/// let ctx2 = ctx.clone();
/// let ctx3 = ctx.clone();
///
/// let batch = all_settled!(ctx,
///     async move { ctx1.step(|_| process_item_a(), None).await },
///     async move { ctx2.step(|_| process_item_b(), None).await },
///     async move { ctx3.step(|_| process_item_c(), None).await },
/// ).await?;
///
/// // Process successful results
/// for item in batch.succeeded() {
///     if let Some(result) = item.get_result() {
///         println!("Success: {:?}", result);
///     }
/// }
///
/// // Log failures for retry or investigation
/// for item in batch.failed() {
///     if let Some(error) = item.get_error() {
///         eprintln!("Failed at index {}: {:?}", item.index, error);
///     }
/// }
/// ```
///
/// ## Partial Success Handling
///
/// ```rust,ignore
/// use aws_durable_execution_sdk::all_settled;
///
/// let ctx1 = ctx.clone();
/// let ctx2 = ctx.clone();
/// let ctx3 = ctx.clone();
///
/// let batch = all_settled!(ctx,
///     async move { ctx1.step(|_| send_notification_email(), None).await },
///     async move { ctx2.step(|_| send_notification_sms(), None).await },
///     async move { ctx3.step(|_| send_notification_push(), None).await },
/// ).await?;
///
/// if batch.all_succeeded() {
///     println!("All notifications sent!");
/// } else {
///     println!("Sent {} of {} notifications",
///         batch.success_count(),
///         batch.items.len()
///     );
/// }
/// ```
///
/// ## Order Preservation
///
/// Results are returned in the same order as input futures:
///
/// ```rust,ignore
/// use aws_durable_execution_sdk::all_settled;
///
/// let ctx1 = ctx.clone();
/// let ctx2 = ctx.clone();
/// let ctx3 = ctx.clone();
///
/// let batch = all_settled!(ctx,
///     async move { ctx1.step(|_| Ok("first"), None).await },
///     async move { ctx2.step(|_| Ok("second"), None).await },
///     async move { ctx3.step(|_| Ok("third"), None).await },
/// ).await?;
///
/// assert_eq!(batch.items[0].index, 0);
/// assert_eq!(batch.items[1].index, 1);
/// assert_eq!(batch.items[2].index, 2);
/// ```
///
/// # When to Use
///
/// Use `all_settled!` when you need to:
/// - Collect all outcomes regardless of individual success/failure
/// - Implement partial success handling
/// - Gather errors for logging or retry logic
/// - Process results even when some operations fail
///
/// # See Also
///
/// * `all!` - Fail fast on first error
/// * `any!` - Return first success
/// * `race!` - Return first to settle
#[macro_export]
macro_rules! all_settled {
    ($ctx:expr, $($fut:expr),+ $(,)?) => {{
        let futures: ::std::vec::Vec<
            ::std::pin::Pin<
                ::std::boxed::Box<
                    dyn ::std::future::Future<
                        Output = $crate::error::DurableResult<_>
                    > + ::std::marker::Send
                >
            >
        > = ::std::vec![
            $(::std::boxed::Box::pin($fut)),+
        ];
        $ctx.all_settled(futures)
    }};
}


#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;

    use crate::client::{CheckpointResponse, MockDurableServiceClient, SharedDurableServiceClient};
    use crate::context::TracingLogger;
    use crate::error::DurableError;
    use crate::lambda::InitialExecutionState;
    use crate::state::ExecutionState;

    fn create_mock_client() -> SharedDurableServiceClient {
        Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse::new("token-1")))
                .with_checkpoint_response(Ok(CheckpointResponse::new("token-2")))
                .with_checkpoint_response(Ok(CheckpointResponse::new("token-3")))
                .with_checkpoint_response(Ok(CheckpointResponse::new("token-4")))
                .with_checkpoint_response(Ok(CheckpointResponse::new("token-5")))
        )
    }

    fn create_test_state(client: SharedDurableServiceClient) -> Arc<ExecutionState> {
        Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            InitialExecutionState::new(),
            client,
        ))
    }

    /// A mock context that provides the `all`, `any`, `race`, and `all_settled` methods for testing macros.
    /// This simulates the DurableContext interface without requiring the full context.
    struct MockContext {
        state: Arc<ExecutionState>,
        logger: Arc<dyn crate::context::Logger>,
        id_counter: std::sync::atomic::AtomicU64,
    }

    impl MockContext {
        fn new(state: Arc<ExecutionState>) -> Self {
            Self {
                state,
                logger: Arc::new(TracingLogger),
                id_counter: std::sync::atomic::AtomicU64::new(0),
            }
        }

        fn next_op_id(&self) -> crate::context::OperationIdentifier {
            let counter = self.id_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            crate::context::OperationIdentifier::new(
                format!("test-op-{}", counter),
                None,
                None,
            )
        }

        /// Simulates the `all` method from DurableContext.
        /// Returns all results if all futures succeed, or returns the first error.
        pub async fn all<T, Fut>(
            &self,
            futures: Vec<Fut>,
        ) -> Result<Vec<T>, DurableError>
        where
            T: serde::Serialize + serde::de::DeserializeOwned + Send + Clone + 'static,
            Fut: Future<Output = Result<T, DurableError>> + Send + 'static,
        {
            let op_id = self.next_op_id();
            crate::handlers::promise::all_handler(futures, &self.state, &op_id, &self.logger).await
        }

        /// Simulates the `any` method from DurableContext.
        /// Returns the first successful result, or an error if all fail.
        pub async fn any<T, Fut>(
            &self,
            futures: Vec<Fut>,
        ) -> Result<T, DurableError>
        where
            T: serde::Serialize + serde::de::DeserializeOwned + Send + Clone + 'static,
            Fut: Future<Output = Result<T, DurableError>> + Send + 'static,
        {
            let op_id = self.next_op_id();
            crate::handlers::promise::any_handler(futures, &self.state, &op_id, &self.logger).await
        }

        /// Simulates the `race` method from DurableContext.
        /// Returns the result of the first future to settle (success or failure).
        pub async fn race<T, Fut>(
            &self,
            futures: Vec<Fut>,
        ) -> Result<T, DurableError>
        where
            T: serde::Serialize + serde::de::DeserializeOwned + Send + Clone + 'static,
            Fut: Future<Output = Result<T, DurableError>> + Send + 'static,
        {
            let op_id = self.next_op_id();
            crate::handlers::promise::race_handler(futures, &self.state, &op_id, &self.logger).await
        }

        /// Simulates the `all_settled` method from DurableContext.
        /// Returns a BatchResult containing outcomes for all futures.
        pub async fn all_settled<T, Fut>(
            &self,
            futures: Vec<Fut>,
        ) -> Result<crate::concurrency::BatchResult<T>, DurableError>
        where
            T: serde::Serialize + serde::de::DeserializeOwned + Send + Clone + 'static,
            Fut: Future<Output = Result<T, DurableError>> + Send + 'static,
        {
            let op_id = self.next_op_id();
            crate::handlers::promise::all_settled_handler(futures, &self.state, &op_id, &self.logger).await
        }

    }

    // =========================================================================
    // all! macro tests
    // Validates: Requirements 1.1, 1.2, 1.3, 1.4, 1.5
    // =========================================================================

    /// Test that all! macro returns Vec<T> with all results in order when all succeed.
    /// Validates: Requirements 1.1, 1.2, 1.5
    #[tokio::test]
    async fn test_all_macro_success_returns_vec_in_order() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        let result = crate::all!(ctx,
            Box::pin(async { Ok::<_, DurableError>(1) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Ok(2) }),
            Box::pin(async { Ok(3) }),
        ).await;

        assert!(result.is_ok());
        let values = result.unwrap();
        assert_eq!(values, vec![1, 2, 3]);
    }

    /// Test that all! macro works with a single future.
    /// Validates: Requirements 1.4
    #[tokio::test]
    async fn test_all_macro_single_future() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        let result = crate::all!(ctx,
            Box::pin(async { Ok::<_, DurableError>(42) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
        ).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![42]);
    }

    /// Test that all! macro works with two futures.
    /// Validates: Requirements 1.4, 1.5
    #[tokio::test]
    async fn test_all_macro_two_futures() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        let result = crate::all!(ctx,
            Box::pin(async { Ok::<_, DurableError>("hello".to_string()) }) as Pin<Box<dyn Future<Output = Result<String, DurableError>> + Send>>,
            Box::pin(async { Ok("world".to_string()) }),
        ).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec!["hello".to_string(), "world".to_string()]);
    }

    /// Test that all! macro returns first error when any future fails.
    /// Validates: Requirements 1.3
    #[tokio::test]
    async fn test_all_macro_failure_returns_first_error() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        let result = crate::all!(ctx,
            Box::pin(async { Ok::<_, DurableError>(1) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Err(DurableError::execution("test error")) }),
            Box::pin(async { Ok(3) }),
        ).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        let error_msg = format!("{}", error);
        assert!(error_msg.contains("failed") || error_msg.contains("error"));
    }

    /// Test that all! macro returns error when first future fails.
    /// Validates: Requirements 1.3
    #[tokio::test]
    async fn test_all_macro_first_future_fails() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        let result = crate::all!(ctx,
            Box::pin(async { Err::<i32, _>(DurableError::execution("first error")) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Ok(2) }),
        ).await;

        assert!(result.is_err());
    }

    /// Test that all! macro supports trailing comma.
    /// Validates: Requirements 1.1
    #[tokio::test]
    async fn test_all_macro_trailing_comma() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        // Note the trailing comma after the last future
        let result = crate::all!(ctx,
            Box::pin(async { Ok::<_, DurableError>(1) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Ok(2) }),
        ).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![1, 2]);
    }

    /// Test that all! macro works with different output types (String).
    /// Validates: Requirements 1.1, 1.6
    #[tokio::test]
    async fn test_all_macro_string_type() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        let result = crate::all!(ctx,
            Box::pin(async { Ok::<_, DurableError>("a".to_string()) }) as Pin<Box<dyn Future<Output = Result<String, DurableError>> + Send>>,
            Box::pin(async { Ok("b".to_string()) }),
            Box::pin(async { Ok("c".to_string()) }),
        ).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    /// Property 1: Macro expansion equivalence test for all! macro.
    /// Verifies that all! macro produces the same result as manually boxing futures.
    /// Validates: Requirements 1.1, 1.2, 1.3, 1.4, 1.5
    #[tokio::test]
    async fn test_all_macro_equivalence_with_manual_boxing() {
        // Test with macro
        let client1 = create_mock_client();
        let state1 = create_test_state(client1);
        let ctx1 = MockContext::new(state1);

        let macro_result = crate::all!(ctx1,
            Box::pin(async { Ok::<_, DurableError>(10) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Ok(20) }),
            Box::pin(async { Ok(30) }),
        ).await;

        // Test with manual boxing (same as what macro expands to)
        let client2 = create_mock_client();
        let state2 = create_test_state(client2);
        let ctx2 = MockContext::new(state2);

        let futures: Vec<Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>> = vec![
            Box::pin(async { Ok::<_, DurableError>(10) }),
            Box::pin(async { Ok(20) }),
            Box::pin(async { Ok(30) }),
        ];
        let manual_result = ctx2.all(futures).await;

        // Both should succeed with the same values
        assert!(macro_result.is_ok());
        assert!(manual_result.is_ok());
        assert_eq!(macro_result.unwrap(), manual_result.unwrap());
    }

    /// Property 1: Macro expansion equivalence test for all! macro with failure.
    /// Verifies that all! macro produces the same error behavior as manually boxing futures.
    /// Validates: Requirements 1.1, 1.3
    #[tokio::test]
    async fn test_all_macro_equivalence_with_manual_boxing_failure() {
        // Test with macro
        let client1 = create_mock_client();
        let state1 = create_test_state(client1);
        let ctx1 = MockContext::new(state1);

        let macro_result = crate::all!(ctx1,
            Box::pin(async { Ok::<_, DurableError>(1) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Err(DurableError::execution("test error")) }),
        ).await;

        // Test with manual boxing
        let client2 = create_mock_client();
        let state2 = create_test_state(client2);
        let ctx2 = MockContext::new(state2);

        let futures: Vec<Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>> = vec![
            Box::pin(async { Ok::<_, DurableError>(1) }),
            Box::pin(async { Err(DurableError::execution("test error")) }),
        ];
        let manual_result = ctx2.all(futures).await;

        // Both should fail
        assert!(macro_result.is_err());
        assert!(manual_result.is_err());
    }

    // =========================================================================
    // any! macro tests
    // =========================================================================

    /// Test that any! macro returns the first successful result.
    /// Validates: Requirements 2.1, 2.2
    #[tokio::test]
    async fn test_any_macro_first_success_returned() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        // Use the any! macro with multiple futures where first succeeds
        let result = crate::any!(ctx,
            Box::pin(async { Ok::<_, DurableError>(1) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Ok(2) }),
            Box::pin(async { Ok(3) }),
        ).await;

        assert!(result.is_ok());
        let value = result.unwrap();
        // One of the successful values should be returned
        assert!(value == 1 || value == 2 || value == 3);
    }

    /// Test that any! macro returns first success even when some fail.
    /// Validates: Requirements 2.2, 2.4
    #[tokio::test]
    async fn test_any_macro_success_among_failures() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        let result = crate::any!(ctx,
            Box::pin(async { Err::<i32, _>(DurableError::execution("error 1")) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Ok(42) }),
            Box::pin(async { Err(DurableError::execution("error 2")) }),
        ).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    /// Test that any! macro returns combined error when all futures fail.
    /// Validates: Requirements 2.3
    #[tokio::test]
    async fn test_any_macro_all_failures_returns_combined_error() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        let result = crate::any!(ctx,
            Box::pin(async { Err::<i32, _>(DurableError::execution("error 1")) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Err(DurableError::execution("error 2")) }),
            Box::pin(async { Err(DurableError::execution("error 3")) }),
        ).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        // The error message should contain information about all failures
        let error_msg = format!("{}", error);
        assert!(error_msg.contains("All") || error_msg.contains("failed"));
    }

    /// Test that any! macro works with a single future.
    /// Validates: Requirements 2.4
    #[tokio::test]
    async fn test_any_macro_single_future_success() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        let result = crate::any!(ctx,
            Box::pin(async { Ok::<_, DurableError>(99) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
        ).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 99);
    }

    /// Test that any! macro works with a single failing future.
    /// Validates: Requirements 2.3, 2.4
    #[tokio::test]
    async fn test_any_macro_single_future_failure() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        let result = crate::any!(ctx,
            Box::pin(async { Err::<i32, _>(DurableError::execution("single error")) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
        ).await;

        assert!(result.is_err());
    }

    /// Test that any! macro supports trailing comma.
    /// Validates: Requirements 2.1
    #[tokio::test]
    async fn test_any_macro_trailing_comma() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        // Note the trailing comma after the last future
        let result = crate::any!(ctx,
            Box::pin(async { Ok::<_, DurableError>(1) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Ok(2) }),
        ).await;

        assert!(result.is_ok());
    }

    /// Test that any! macro works with different output types (String).
    /// Validates: Requirements 2.1, 2.4
    #[tokio::test]
    async fn test_any_macro_string_type() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        let result = crate::any!(ctx,
            Box::pin(async { Ok::<_, DurableError>("hello".to_string()) }) as Pin<Box<dyn Future<Output = Result<String, DurableError>> + Send>>,
            Box::pin(async { Ok("world".to_string()) }),
        ).await;

        assert!(result.is_ok());
        let value = result.unwrap();
        assert!(value == "hello" || value == "world");
    }

    // =========================================================================
    // Property 1: Macro expansion equivalence test
    // Validates: Requirements 1.1, 2.1, 3.1, 4.1
    // =========================================================================

    /// Test that any! macro produces the same result as manually boxing futures.
    /// This validates that the macro expansion is equivalent to manual boxing.
    #[tokio::test]
    async fn test_any_macro_equivalence_with_manual_boxing() {
        // Test with macro
        let client1 = create_mock_client();
        let state1 = create_test_state(client1);
        let ctx1 = MockContext::new(state1);

        let macro_result = crate::any!(ctx1,
            Box::pin(async { Err::<i32, _>(DurableError::execution("fail")) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Ok(42) }),
        ).await;

        // Test with manual boxing (same as what macro expands to)
        let client2 = create_mock_client();
        let state2 = create_test_state(client2);
        let ctx2 = MockContext::new(state2);

        let futures: Vec<Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>> = vec![
            Box::pin(async { Err::<i32, _>(DurableError::execution("fail")) }),
            Box::pin(async { Ok(42) }),
        ];
        let manual_result = ctx2.any(futures).await;

        // Both should succeed with the same value
        assert!(macro_result.is_ok());
        assert!(manual_result.is_ok());
        assert_eq!(macro_result.unwrap(), manual_result.unwrap());
    }

    // =========================================================================
    // race! macro tests
    // Validates: Requirements 3.1, 3.2, 3.3
    // =========================================================================

    /// Test that race! macro returns the first result to settle (success case).
    /// Validates: Requirements 3.1, 3.2
    #[tokio::test]
    async fn test_race_macro_first_success_settles() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        // First future completes immediately with success
        let result = crate::race!(ctx,
            Box::pin(async { Ok::<_, DurableError>(1) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                Ok(2)
            }),
        ).await;

        assert!(result.is_ok());
        // First one should win since it completes immediately
        assert_eq!(result.unwrap(), 1);
    }

    /// Test that race! macro returns the first result to settle (failure case).
    /// Validates: Requirements 3.1, 3.2
    #[tokio::test]
    async fn test_race_macro_first_failure_settles() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        // First future completes immediately with failure
        let result = crate::race!(ctx,
            Box::pin(async { Err::<i32, _>(DurableError::execution("fast error")) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                Ok(2)
            }),
        ).await;

        // The error should be returned since it settled first
        assert!(result.is_err());
    }

    /// Test that race! macro works with a single future.
    /// Validates: Requirements 3.3
    #[tokio::test]
    async fn test_race_macro_single_future_success() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        let result = crate::race!(ctx,
            Box::pin(async { Ok::<_, DurableError>(42) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
        ).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    /// Test that race! macro works with a single failing future.
    /// Validates: Requirements 3.2, 3.3
    #[tokio::test]
    async fn test_race_macro_single_future_failure() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        let result = crate::race!(ctx,
            Box::pin(async { Err::<i32, _>(DurableError::execution("single error")) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
        ).await;

        assert!(result.is_err());
    }

    /// Test that race! macro supports trailing comma.
    /// Validates: Requirements 3.1
    #[tokio::test]
    async fn test_race_macro_trailing_comma() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        // Note the trailing comma after the last future
        let result = crate::race!(ctx,
            Box::pin(async { Ok::<_, DurableError>(1) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Ok(2) }),
        ).await;

        assert!(result.is_ok());
    }

    /// Test that race! macro works with different output types (String).
    /// Validates: Requirements 3.1, 3.4
    #[tokio::test]
    async fn test_race_macro_string_type() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        let result = crate::race!(ctx,
            Box::pin(async { Ok::<_, DurableError>("hello".to_string()) }) as Pin<Box<dyn Future<Output = Result<String, DurableError>> + Send>>,
            Box::pin(async {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                Ok("world".to_string())
            }),
        ).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "hello");
    }

    /// Test that race! macro works with multiple futures where success settles first.
    /// Validates: Requirements 3.1, 3.2, 3.3
    #[tokio::test]
    async fn test_race_macro_multiple_futures_success_first() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        let result = crate::race!(ctx,
            Box::pin(async { Ok::<_, DurableError>(1) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Ok(2) }),
            Box::pin(async { Ok(3) }),
        ).await;

        assert!(result.is_ok());
        // One of the values should be returned (first to settle)
        let value = result.unwrap();
        assert!(value == 1 || value == 2 || value == 3);
    }

    /// Test that race! macro produces the same result as manually boxing futures.
    /// This validates that the macro expansion is equivalent to manual boxing.
    /// Validates: Requirements 3.1
    #[tokio::test]
    async fn test_race_macro_equivalence_with_manual_boxing() {
        // Test with macro
        let client1 = create_mock_client();
        let state1 = create_test_state(client1);
        let ctx1 = MockContext::new(state1);

        let macro_result = crate::race!(ctx1,
            Box::pin(async { Ok::<_, DurableError>(10) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                Ok(20)
            }),
        ).await;

        // Test with manual boxing (same as what macro expands to)
        let client2 = create_mock_client();
        let state2 = create_test_state(client2);
        let ctx2 = MockContext::new(state2);

        let futures: Vec<Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>> = vec![
            Box::pin(async { Ok::<_, DurableError>(10) }),
            Box::pin(async {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                Ok(20)
            }),
        ];
        let manual_result = ctx2.race(futures).await;

        // Both should succeed with the same value (first to settle)
        assert!(macro_result.is_ok());
        assert!(manual_result.is_ok());
        assert_eq!(macro_result.unwrap(), manual_result.unwrap());
    }

    /// Test that race! macro produces the same error behavior as manually boxing futures.
    /// Validates: Requirements 3.1, 3.2
    #[tokio::test]
    async fn test_race_macro_equivalence_with_manual_boxing_failure() {
        // Test with macro
        let client1 = create_mock_client();
        let state1 = create_test_state(client1);
        let ctx1 = MockContext::new(state1);

        let macro_result = crate::race!(ctx1,
            Box::pin(async { Err::<i32, _>(DurableError::execution("fast error")) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                Ok(20)
            }),
        ).await;

        // Test with manual boxing
        let client2 = create_mock_client();
        let state2 = create_test_state(client2);
        let ctx2 = MockContext::new(state2);

        let futures: Vec<Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>> = vec![
            Box::pin(async { Err::<i32, _>(DurableError::execution("fast error")) }),
            Box::pin(async {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                Ok(20)
            }),
        ];
        let manual_result = ctx2.race(futures).await;

        // Both should fail (error settled first)
        assert!(macro_result.is_err());
        assert!(manual_result.is_err());
    }

    // =========================================================================
    // all_settled! macro tests
    // Validates: Requirements 4.1, 4.2, 4.3, 4.4
    // =========================================================================

    /// Test that all_settled! macro returns BatchResult<T> with all outcomes.
    /// Validates: Requirements 4.1, 4.2
    #[tokio::test]
    async fn test_all_settled_macro_returns_batch_result() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        let result = crate::all_settled!(ctx,
            Box::pin(async { Ok::<_, DurableError>(1) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Ok(2) }),
            Box::pin(async { Ok(3) }),
        ).await;

        assert!(result.is_ok());
        let batch = result.unwrap();
        assert_eq!(batch.items.len(), 3);
        assert_eq!(batch.success_count(), 3);
        assert_eq!(batch.failure_count(), 0);
        assert!(batch.all_succeeded());
    }

    /// Test that all_settled! macro preserves order of results.
    /// Property 2: Result order preservation
    /// Validates: Requirements 4.4
    #[tokio::test]
    async fn test_all_settled_macro_preserves_order() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        let result = crate::all_settled!(ctx,
            Box::pin(async { Ok::<_, DurableError>(10) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Ok(20) }),
            Box::pin(async { Ok(30) }),
        ).await;

        assert!(result.is_ok());
        let batch = result.unwrap();
        
        // Verify order is preserved by checking indices and values
        assert_eq!(batch.items.len(), 3);
        assert_eq!(batch.items[0].index, 0);
        assert_eq!(batch.items[0].get_result(), Some(&10));
        assert_eq!(batch.items[1].index, 1);
        assert_eq!(batch.items[1].get_result(), Some(&20));
        assert_eq!(batch.items[2].index, 2);
        assert_eq!(batch.items[2].get_result(), Some(&30));
    }

    /// Test that all_settled! macro collects both successes and failures.
    /// Validates: Requirements 4.1, 4.2
    #[tokio::test]
    async fn test_all_settled_macro_mixed_results() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        let result = crate::all_settled!(ctx,
            Box::pin(async { Ok::<_, DurableError>(1) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Err(DurableError::execution("test error")) }),
            Box::pin(async { Ok(3) }),
        ).await;

        assert!(result.is_ok());
        let batch = result.unwrap();
        assert_eq!(batch.items.len(), 3);
        assert_eq!(batch.success_count(), 2);
        assert_eq!(batch.failure_count(), 1);
        assert!(!batch.all_succeeded());
        assert!(batch.has_failures());
    }

    /// Test that all_settled! macro works with a single future.
    /// Validates: Requirements 4.3
    #[tokio::test]
    async fn test_all_settled_macro_single_future() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        let result = crate::all_settled!(ctx,
            Box::pin(async { Ok::<_, DurableError>(42) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
        ).await;

        assert!(result.is_ok());
        let batch = result.unwrap();
        assert_eq!(batch.items.len(), 1);
        assert_eq!(batch.success_count(), 1);
        assert_eq!(batch.items[0].get_result(), Some(&42));
    }

    /// Test that all_settled! macro works with a single failing future.
    /// Validates: Requirements 4.2, 4.3
    #[tokio::test]
    async fn test_all_settled_macro_single_failure() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        let result = crate::all_settled!(ctx,
            Box::pin(async { Err::<i32, _>(DurableError::execution("single error")) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
        ).await;

        assert!(result.is_ok());
        let batch = result.unwrap();
        assert_eq!(batch.items.len(), 1);
        assert_eq!(batch.failure_count(), 1);
        assert!(batch.items[0].is_failed());
    }

    /// Test that all_settled! macro supports trailing comma.
    /// Validates: Requirements 4.1
    #[tokio::test]
    async fn test_all_settled_macro_trailing_comma() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        // Note the trailing comma after the last future
        let result = crate::all_settled!(ctx,
            Box::pin(async { Ok::<_, DurableError>(1) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Ok(2) }),
        ).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().items.len(), 2);
    }

    /// Test that all_settled! macro works with different output types (String).
    /// Validates: Requirements 4.1, 4.5
    #[tokio::test]
    async fn test_all_settled_macro_string_type() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        let result = crate::all_settled!(ctx,
            Box::pin(async { Ok::<_, DurableError>("a".to_string()) }) as Pin<Box<dyn Future<Output = Result<String, DurableError>> + Send>>,
            Box::pin(async { Ok("b".to_string()) }),
            Box::pin(async { Ok("c".to_string()) }),
        ).await;

        assert!(result.is_ok());
        let batch = result.unwrap();
        assert_eq!(batch.items.len(), 3);
        assert_eq!(batch.items[0].get_result(), Some(&"a".to_string()));
        assert_eq!(batch.items[1].get_result(), Some(&"b".to_string()));
        assert_eq!(batch.items[2].get_result(), Some(&"c".to_string()));
    }

    /// Test that all_settled! macro produces the same result as manually boxing futures.
    /// Property 1: Macro expansion equivalence
    /// Validates: Requirements 4.1
    #[tokio::test]
    async fn test_all_settled_macro_equivalence_with_manual_boxing() {
        // Test with macro
        let client1 = create_mock_client();
        let state1 = create_test_state(client1);
        let ctx1 = MockContext::new(state1);

        let macro_result = crate::all_settled!(ctx1,
            Box::pin(async { Ok::<_, DurableError>(10) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Err(DurableError::execution("test error")) }),
            Box::pin(async { Ok(30) }),
        ).await;

        // Test with manual boxing (same as what macro expands to)
        let client2 = create_mock_client();
        let state2 = create_test_state(client2);
        let ctx2 = MockContext::new(state2);

        let futures: Vec<Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>> = vec![
            Box::pin(async { Ok::<_, DurableError>(10) }),
            Box::pin(async { Err(DurableError::execution("test error")) }),
            Box::pin(async { Ok(30) }),
        ];
        let manual_result = ctx2.all_settled(futures).await;

        // Both should succeed with BatchResult
        assert!(macro_result.is_ok());
        assert!(manual_result.is_ok());
        
        let macro_batch = macro_result.unwrap();
        let manual_batch = manual_result.unwrap();
        
        // Both should have same counts
        assert_eq!(macro_batch.items.len(), manual_batch.items.len());
        assert_eq!(macro_batch.success_count(), manual_batch.success_count());
        assert_eq!(macro_batch.failure_count(), manual_batch.failure_count());
    }

    /// Test that all_settled! macro collects all failures without short-circuiting.
    /// Validates: Requirements 4.1, 4.2
    #[tokio::test]
    async fn test_all_settled_macro_all_failures() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let ctx = MockContext::new(state);

        let result = crate::all_settled!(ctx,
            Box::pin(async { Err::<i32, _>(DurableError::execution("error 1")) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Err(DurableError::execution("error 2")) }),
            Box::pin(async { Err(DurableError::execution("error 3")) }),
        ).await;

        // all_settled should still succeed (it collects all outcomes)
        assert!(result.is_ok());
        let batch = result.unwrap();
        assert_eq!(batch.items.len(), 3);
        assert_eq!(batch.success_count(), 0);
        assert_eq!(batch.failure_count(), 3);
        assert!(!batch.all_succeeded());
    }
}
