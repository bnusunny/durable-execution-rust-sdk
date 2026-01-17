//! Callback manager for managing callback lifecycle.
//!
//! This module implements the CallbackManager which manages callback lifecycle
//! including timeouts and heartbeats, matching the Node.js SDK's callback manager.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use aws_durable_execution_sdk::ErrorObject;

use crate::error::TestError;

/// Status of a completed callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompleteCallbackStatus {
    /// Callback completed successfully
    Success,
    /// Callback completed with failure
    Failure,
    /// Callback timed out
    TimedOut,
}

/// Internal state of a callback.
#[derive(Debug, Clone)]
pub struct CallbackState {
    /// The callback ID
    pub callback_id: String,
    /// Optional timeout duration
    pub timeout: Option<Duration>,
    /// When the callback was registered
    pub registered_at: Instant,
    /// Last heartbeat time
    pub last_heartbeat: Instant,
    /// Completion status (None if still pending)
    pub completion_status: Option<CompleteCallbackStatus>,
    /// Result if completed successfully
    pub result: Option<String>,
    /// Error if completed with failure
    pub error: Option<ErrorObject>,
}


impl CallbackState {
    /// Create a new callback state.
    pub fn new(callback_id: String, timeout: Option<Duration>) -> Self {
        let now = Instant::now();
        Self {
            callback_id,
            timeout,
            registered_at: now,
            last_heartbeat: now,
            completion_status: None,
            result: None,
            error: None,
        }
    }

    /// Check if this callback is completed.
    pub fn is_completed(&self) -> bool {
        self.completion_status.is_some()
    }

    /// Check if this callback has timed out.
    pub fn is_timed_out(&self) -> bool {
        if let Some(timeout) = self.timeout {
            self.last_heartbeat.elapsed() > timeout
        } else {
            false
        }
    }
}

/// Manages callback lifecycle including timeouts and heartbeats.
#[derive(Debug, Default)]
pub struct CallbackManager {
    /// The execution ID this manager belongs to
    execution_id: String,
    /// Map of callback ID to callback state
    callbacks: HashMap<String, CallbackState>,
}

impl CallbackManager {
    /// Create a new callback manager.
    pub fn new(execution_id: &str) -> Self {
        Self {
            execution_id: execution_id.to_string(),
            callbacks: HashMap::new(),
        }
    }

    /// Register a new callback.
    pub fn register_callback(
        &mut self,
        callback_id: &str,
        timeout: Option<Duration>,
    ) -> Result<(), TestError> {
        if self.callbacks.contains_key(callback_id) {
            return Err(TestError::CallbackAlreadyCompleted(format!(
                "Callback {} already registered",
                callback_id
            )));
        }

        let state = CallbackState::new(callback_id.to_string(), timeout);
        self.callbacks.insert(callback_id.to_string(), state);
        Ok(())
    }


    /// Send callback success.
    pub fn send_success(&mut self, callback_id: &str, result: &str) -> Result<(), TestError> {
        let state = self
            .callbacks
            .get_mut(callback_id)
            .ok_or_else(|| TestError::CallbackNotFound(callback_id.to_string()))?;

        if state.is_completed() {
            return Err(TestError::CallbackAlreadyCompleted(callback_id.to_string()));
        }

        state.completion_status = Some(CompleteCallbackStatus::Success);
        state.result = Some(result.to_string());
        Ok(())
    }

    /// Send callback failure.
    pub fn send_failure(&mut self, callback_id: &str, error: &ErrorObject) -> Result<(), TestError> {
        let state = self
            .callbacks
            .get_mut(callback_id)
            .ok_or_else(|| TestError::CallbackNotFound(callback_id.to_string()))?;

        if state.is_completed() {
            return Err(TestError::CallbackAlreadyCompleted(callback_id.to_string()));
        }

        state.completion_status = Some(CompleteCallbackStatus::Failure);
        state.error = Some(error.clone());
        Ok(())
    }

    /// Send callback heartbeat.
    pub fn send_heartbeat(&mut self, callback_id: &str) -> Result<(), TestError> {
        let state = self
            .callbacks
            .get_mut(callback_id)
            .ok_or_else(|| TestError::CallbackNotFound(callback_id.to_string()))?;

        if state.is_completed() {
            return Err(TestError::CallbackAlreadyCompleted(callback_id.to_string()));
        }

        state.last_heartbeat = Instant::now();
        Ok(())
    }

    /// Check for timed out callbacks and mark them as timed out.
    /// Returns the IDs of callbacks that timed out.
    pub fn check_timeouts(&mut self) -> Vec<String> {
        let mut timed_out = Vec::new();

        for (id, state) in self.callbacks.iter_mut() {
            if !state.is_completed() && state.is_timed_out() {
                state.completion_status = Some(CompleteCallbackStatus::TimedOut);
                timed_out.push(id.clone());
            }
        }

        timed_out
    }

    /// Get callback status.
    pub fn get_callback_status(&self, callback_id: &str) -> Option<CompleteCallbackStatus> {
        self.callbacks
            .get(callback_id)
            .and_then(|s| s.completion_status)
    }

    /// Get callback state.
    pub fn get_callback_state(&self, callback_id: &str) -> Option<&CallbackState> {
        self.callbacks.get(callback_id)
    }

    /// Get the execution ID.
    pub fn execution_id(&self) -> &str {
        &self.execution_id
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_callback() {
        let mut manager = CallbackManager::new("exec-1");
        manager.register_callback("cb-1", None).unwrap();

        let state = manager.get_callback_state("cb-1").unwrap();
        assert_eq!(state.callback_id, "cb-1");
        assert!(!state.is_completed());
    }

    #[test]
    fn test_register_duplicate_callback_fails() {
        let mut manager = CallbackManager::new("exec-1");
        manager.register_callback("cb-1", None).unwrap();

        let result = manager.register_callback("cb-1", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_send_success() {
        let mut manager = CallbackManager::new("exec-1");
        manager.register_callback("cb-1", None).unwrap();
        manager.send_success("cb-1", r#"{"result": "ok"}"#).unwrap();

        let state = manager.get_callback_state("cb-1").unwrap();
        assert!(state.is_completed());
        assert_eq!(state.completion_status, Some(CompleteCallbackStatus::Success));
        assert_eq!(state.result, Some(r#"{"result": "ok"}"#.to_string()));
    }

    #[test]
    fn test_send_failure() {
        let mut manager = CallbackManager::new("exec-1");
        manager.register_callback("cb-1", None).unwrap();

        let error = ErrorObject::new("TestError", "Something went wrong");
        manager.send_failure("cb-1", &error).unwrap();

        let state = manager.get_callback_state("cb-1").unwrap();
        assert!(state.is_completed());
        assert_eq!(state.completion_status, Some(CompleteCallbackStatus::Failure));
        assert!(state.error.is_some());
    }

    #[test]
    fn test_send_heartbeat() {
        let mut manager = CallbackManager::new("exec-1");
        manager.register_callback("cb-1", Some(Duration::from_secs(60))).unwrap();

        // Wait a tiny bit
        std::thread::sleep(Duration::from_millis(10));

        let before = manager.get_callback_state("cb-1").unwrap().last_heartbeat;
        manager.send_heartbeat("cb-1").unwrap();
        let after = manager.get_callback_state("cb-1").unwrap().last_heartbeat;

        assert!(after > before);
    }

    #[test]
    fn test_double_complete_fails() {
        let mut manager = CallbackManager::new("exec-1");
        manager.register_callback("cb-1", None).unwrap();
        manager.send_success("cb-1", "result").unwrap();

        let result = manager.send_success("cb-1", "another result");
        assert!(result.is_err());
    }

    #[test]
    fn test_callback_not_found() {
        let mut manager = CallbackManager::new("exec-1");
        let result = manager.send_success("nonexistent", "result");
        assert!(matches!(result, Err(TestError::CallbackNotFound(_))));
    }
}
