//! Tests for ReplayStatus.

use crate::state::ReplayStatus;

#[test]
fn test_replay_status_replay() {
    let status = ReplayStatus::Replay;
    assert!(status.is_replay());
    assert!(!status.is_new());
}

#[test]
fn test_replay_status_new() {
    let status = ReplayStatus::New;
    assert!(!status.is_replay());
    assert!(status.is_new());
}

#[test]
fn test_replay_status_from_u8() {
    assert_eq!(ReplayStatus::from(0), ReplayStatus::Replay);
    assert_eq!(ReplayStatus::from(1), ReplayStatus::New);
    assert_eq!(ReplayStatus::from(2), ReplayStatus::New); // Any non-zero is New
}

#[test]
fn test_replay_status_to_u8() {
    assert_eq!(u8::from(ReplayStatus::Replay), 0);
    assert_eq!(u8::from(ReplayStatus::New), 1);
}
