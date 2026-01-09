//! Replay status tracking for durable executions.
//!
//! This module provides the [`ReplayStatus`] enum for tracking whether
//! the execution is replaying previously checkpointed operations or
//! executing new operations.

/// Replay status indicating whether we're replaying or executing new operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ReplayStatus {
    /// Currently replaying previously checkpointed operations
    Replay = 0,
    /// Executing new operations (past the replay point)
    New = 1,
}

impl ReplayStatus {
    /// Returns true if currently in replay mode.
    pub fn is_replay(&self) -> bool {
        matches!(self, Self::Replay)
    }

    /// Returns true if executing new operations.
    pub fn is_new(&self) -> bool {
        matches!(self, Self::New)
    }
}

impl From<u8> for ReplayStatus {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Replay,
            _ => Self::New,
        }
    }
}

impl From<ReplayStatus> for u8 {
    fn from(status: ReplayStatus) -> Self {
        status as u8
    }
}
