//! Scheduler implementations for test execution orchestration.
//!
//! This module provides scheduler implementations that manage when handler
//! re-invocations occur during test execution. Two implementations are provided:
//!
//! - `QueueScheduler`: For time-skipping mode, processes functions in FIFO order
//! - `TimerScheduler`: For real-time mode, respects actual timestamps using tokio timers
//!
//! # Requirements
//!
//! - 17.1: WHEN time skipping is enabled, THE Scheduler SHALL use a queue-based
//!   approach that processes operations in FIFO order
//! - 17.2: WHEN time skipping is disabled, THE Scheduler SHALL use timer-based
//!   scheduling that respects actual timestamps
//! - 17.3: WHEN a function is scheduled, THE Scheduler SHALL execute any checkpoint
//!   updates before invoking the handler
//! - 17.4: WHEN the scheduler has pending functions, THE Scheduler SHALL report
//!   that scheduled functions exist via has_scheduled_function()
//! - 17.5: WHEN execution completes, THE Scheduler SHALL flush any remaining
//!   scheduled functions

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::error::TestError;

/// Type alias for a boxed async function that returns nothing.
pub type BoxedAsyncFn = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send>;

/// Type alias for an error handler function.
pub type ErrorHandler = Box<dyn FnOnce(TestError) + Send>;

/// Type alias for a checkpoint update function.
pub type CheckpointUpdateFn =
    Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = Result<(), TestError>> + Send>> + Send>;

/// A scheduled function with its metadata.
pub struct ScheduledFunction {
    /// The function to execute
    pub start_invocation: BoxedAsyncFn,
    /// Error handler for failures
    pub on_error: ErrorHandler,
    /// Optional timestamp for when to execute (used by TimerScheduler)
    pub timestamp: Option<DateTime<Utc>>,
    /// Optional checkpoint update to run before invocation
    pub update_checkpoint: Option<CheckpointUpdateFn>,
}

impl std::fmt::Debug for ScheduledFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScheduledFunction")
            .field("timestamp", &self.timestamp)
            .field("has_update_checkpoint", &self.update_checkpoint.is_some())
            .finish()
    }
}

/// Trait for scheduling handler invocations.
///
/// Schedulers manage when handler re-invocations occur during test execution.
/// Different implementations provide different scheduling strategies.
pub trait Scheduler: Send {
    /// Schedule a function to be executed.
    ///
    /// # Arguments
    ///
    /// * `start_invocation` - The function to execute
    /// * `on_error` - Error handler for failures
    /// * `timestamp` - Optional timestamp for when to execute (ignored by QueueScheduler)
    /// * `update_checkpoint` - Optional checkpoint update to run before invocation
    fn schedule_function(
        &mut self,
        start_invocation: BoxedAsyncFn,
        on_error: ErrorHandler,
        timestamp: Option<DateTime<Utc>>,
        update_checkpoint: Option<CheckpointUpdateFn>,
    );

    /// Check if there are scheduled functions pending.
    fn has_scheduled_function(&self) -> bool;

    /// Flush all scheduled functions without executing them.
    fn flush_timers(&mut self);

    /// Process the next scheduled function (if any).
    ///
    /// Returns `true` if a function was processed, `false` if the queue is empty.
    fn process_next(&mut self) -> Pin<Box<dyn Future<Output = bool> + Send + '_>>;
}


/// Queue-based scheduler for time-skipping mode.
///
/// Executes functions sequentially in FIFO order, ignoring timestamps.
/// This is used when time skipping is enabled to process operations
/// as quickly as possible without waiting for actual time to pass.
///
/// # Requirements
///
/// - 17.1: WHEN time skipping is enabled, THE Scheduler SHALL use a queue-based
///   approach that processes operations in FIFO order
pub struct QueueScheduler {
    /// Queue of scheduled functions
    function_queue: VecDeque<ScheduledFunction>,
    /// Flag indicating if processing is in progress
    is_processing: Arc<AtomicBool>,
}

impl QueueScheduler {
    /// Create a new queue scheduler.
    pub fn new() -> Self {
        Self {
            function_queue: VecDeque::new(),
            is_processing: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Check if the scheduler is currently processing.
    pub fn is_processing(&self) -> bool {
        self.is_processing.load(Ordering::SeqCst)
    }

    /// Get the number of queued functions.
    pub fn queue_len(&self) -> usize {
        self.function_queue.len()
    }
}

impl Default for QueueScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler for QueueScheduler {
    fn schedule_function(
        &mut self,
        start_invocation: BoxedAsyncFn,
        on_error: ErrorHandler,
        timestamp: Option<DateTime<Utc>>,
        update_checkpoint: Option<CheckpointUpdateFn>,
    ) {
        let scheduled = ScheduledFunction {
            start_invocation,
            on_error,
            timestamp,
            update_checkpoint,
        };
        self.function_queue.push_back(scheduled);
    }

    fn has_scheduled_function(&self) -> bool {
        !self.function_queue.is_empty()
    }

    fn flush_timers(&mut self) {
        self.function_queue.clear();
    }

    fn process_next(&mut self) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        Box::pin(async move {
            if let Some(scheduled) = self.function_queue.pop_front() {
                self.is_processing.store(true, Ordering::SeqCst);

                // Execute checkpoint update if provided
                if let Some(update_fn) = scheduled.update_checkpoint {
                    let update_future = update_fn();
                    if let Err(e) = update_future.await {
                        (scheduled.on_error)(e);
                        self.is_processing.store(false, Ordering::SeqCst);
                        return true;
                    }
                }

                // Execute the scheduled function
                let invocation_future = (scheduled.start_invocation)();
                invocation_future.await;

                self.is_processing.store(false, Ordering::SeqCst);
                true
            } else {
                false
            }
        })
    }
}


/// Timer-based scheduler for real-time mode.
///
/// Respects actual timestamps using tokio timers. Functions are scheduled
/// to execute at their specified timestamps, with earlier timestamps
/// executing first.
///
/// # Requirements
///
/// - 17.2: WHEN time skipping is disabled, THE Scheduler SHALL use timer-based
///   scheduling that respects actual timestamps
pub struct TimerScheduler {
    /// Handles to spawned timer tasks
    scheduled_tasks: Vec<JoinHandle<()>>,
    /// Shared state for tracking pending functions
    pending_count: Arc<Mutex<usize>>,
}

impl TimerScheduler {
    /// Create a new timer scheduler.
    pub fn new() -> Self {
        Self {
            scheduled_tasks: Vec::new(),
            pending_count: Arc::new(Mutex::new(0)),
        }
    }

    /// Get the number of pending tasks.
    pub async fn pending_count(&self) -> usize {
        *self.pending_count.lock().await
    }
}

impl Default for TimerScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler for TimerScheduler {
    fn schedule_function(
        &mut self,
        start_invocation: BoxedAsyncFn,
        on_error: ErrorHandler,
        timestamp: Option<DateTime<Utc>>,
        update_checkpoint: Option<CheckpointUpdateFn>,
    ) {
        let pending_count = Arc::clone(&self.pending_count);

        // Increment pending count
        let pending_count_clone = Arc::clone(&pending_count);
        tokio::spawn(async move {
            let mut count = pending_count_clone.lock().await;
            *count += 1;
        });

        let handle = tokio::spawn(async move {
            // Calculate delay if timestamp is provided
            if let Some(ts) = timestamp {
                let now = Utc::now();
                if ts > now {
                    let duration = (ts - now).to_std().unwrap_or_default();
                    tokio::time::sleep(duration).await;
                }
            }

            // Execute checkpoint update if provided
            if let Some(update_fn) = update_checkpoint {
                let update_future = update_fn();
                if let Err(e) = update_future.await {
                    (on_error)(e);
                    // Decrement pending count
                    let mut count = pending_count.lock().await;
                    *count = count.saturating_sub(1);
                    return;
                }
            }

            // Execute the scheduled function
            let invocation_future = start_invocation();
            invocation_future.await;

            // Decrement pending count
            let mut count = pending_count.lock().await;
            *count = count.saturating_sub(1);
        });

        self.scheduled_tasks.push(handle);
    }

    fn has_scheduled_function(&self) -> bool {
        // Check if any tasks are still running
        self.scheduled_tasks.iter().any(|h| !h.is_finished())
    }

    fn flush_timers(&mut self) {
        // Abort all pending tasks
        for handle in self.scheduled_tasks.drain(..) {
            handle.abort();
        }
    }

    fn process_next(&mut self) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        // TimerScheduler processes functions asynchronously via spawned tasks,
        // so this method just cleans up finished tasks and returns whether
        // there are still pending tasks.
        Box::pin(async move {
            // Remove finished tasks
            self.scheduled_tasks.retain(|h| !h.is_finished());
            !self.scheduled_tasks.is_empty()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[tokio::test]
    async fn test_queue_scheduler_new() {
        let scheduler = QueueScheduler::new();
        assert!(!scheduler.has_scheduled_function());
        assert!(!scheduler.is_processing());
        assert_eq!(scheduler.queue_len(), 0);
    }

    #[tokio::test]
    async fn test_queue_scheduler_schedule_and_process() {
        let mut scheduler = QueueScheduler::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        scheduler.schedule_function(
            Box::new(move || {
                let counter = Arc::clone(&counter_clone);
                Box::pin(async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                })
            }),
            Box::new(|_| {}),
            None,
            None,
        );

        assert!(scheduler.has_scheduled_function());
        assert_eq!(scheduler.queue_len(), 1);

        let processed = scheduler.process_next().await;
        assert!(processed);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert!(!scheduler.has_scheduled_function());
    }

    #[tokio::test]
    async fn test_queue_scheduler_fifo_order() {
        let mut scheduler = QueueScheduler::new();
        let order = Arc::new(Mutex::new(Vec::new()));

        // Schedule three functions
        for i in 0..3 {
            let order_clone = Arc::clone(&order);
            scheduler.schedule_function(
                Box::new(move || {
                    Box::pin(async move {
                        order_clone.lock().await.push(i);
                    })
                }),
                Box::new(|_| {}),
                None,
                None,
            );
        }

        // Process all
        while scheduler.process_next().await {}

        // Verify FIFO order
        let result = order.lock().await;
        assert_eq!(*result, vec![0, 1, 2]);
    }

    #[tokio::test]
    async fn test_queue_scheduler_with_checkpoint_update() {
        let mut scheduler = QueueScheduler::new();
        let checkpoint_called = Arc::new(AtomicBool::new(false));
        let invocation_called = Arc::new(AtomicBool::new(false));

        let checkpoint_clone = Arc::clone(&checkpoint_called);
        let invocation_clone = Arc::clone(&invocation_called);

        scheduler.schedule_function(
            Box::new(move || {
                let invocation = Arc::clone(&invocation_clone);
                Box::pin(async move {
                    invocation.store(true, Ordering::SeqCst);
                })
            }),
            Box::new(|_| {}),
            None,
            Some(Box::new(move || {
                let checkpoint = Arc::clone(&checkpoint_clone);
                Box::pin(async move {
                    checkpoint.store(true, Ordering::SeqCst);
                    Ok(())
                })
            })),
        );

        scheduler.process_next().await;

        assert!(checkpoint_called.load(Ordering::SeqCst));
        assert!(invocation_called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_queue_scheduler_flush() {
        let mut scheduler = QueueScheduler::new();

        for _ in 0..5 {
            scheduler.schedule_function(
                Box::new(|| Box::pin(async {})),
                Box::new(|_| {}),
                None,
                None,
            );
        }

        assert_eq!(scheduler.queue_len(), 5);
        scheduler.flush_timers();
        assert_eq!(scheduler.queue_len(), 0);
        assert!(!scheduler.has_scheduled_function());
    }

    #[tokio::test]
    async fn test_timer_scheduler_new() {
        let scheduler = TimerScheduler::new();
        assert!(!scheduler.has_scheduled_function());
    }

    #[tokio::test]
    async fn test_timer_scheduler_schedule_immediate() {
        let mut scheduler = TimerScheduler::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        scheduler.schedule_function(
            Box::new(move || {
                let counter = Arc::clone(&counter_clone);
                Box::pin(async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                })
            }),
            Box::new(|_| {}),
            None, // No timestamp = immediate execution
            None,
        );

        // Wait a bit for the task to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_timer_scheduler_flush() {
        let mut scheduler = TimerScheduler::new();
        let counter = Arc::new(AtomicUsize::new(0));

        // Schedule a function with a future timestamp
        let counter_clone = Arc::clone(&counter);
        let future_time = Utc::now() + chrono::Duration::seconds(10);

        scheduler.schedule_function(
            Box::new(move || {
                let counter = Arc::clone(&counter_clone);
                Box::pin(async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                })
            }),
            Box::new(|_| {}),
            Some(future_time),
            None,
        );

        // Flush before it executes
        scheduler.flush_timers();

        // Wait a bit
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Counter should still be 0 because we flushed
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }
}
