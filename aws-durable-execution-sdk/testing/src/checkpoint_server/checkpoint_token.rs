//! Checkpoint token encoding and decoding utilities.
//!
//! This module provides utilities for encoding and decoding checkpoint tokens,
//! which are used to track the state of an execution across invocations.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};

use crate::error::TestError;

use super::types::{CheckpointToken, ExecutionId, InvocationId};

/// Data contained in a checkpoint token.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheckpointTokenData {
    /// The execution ID
    pub execution_id: ExecutionId,
    /// A unique token for this checkpoint
    pub token: String,
    /// The invocation ID
    pub invocation_id: InvocationId,
}

/// Encode checkpoint token data to a base64 string.
///
/// # Arguments
///
/// * `data` - The checkpoint token data to encode
///
/// # Returns
///
/// A base64-encoded string representation of the token data.
pub fn encode_checkpoint_token(data: &CheckpointTokenData) -> CheckpointToken {
    let json = serde_json::to_string(data).expect("CheckpointTokenData should serialize");
    URL_SAFE_NO_PAD.encode(json.as_bytes())
}

/// Decode a checkpoint token string to data.
///
/// # Arguments
///
/// * `token` - The base64-encoded checkpoint token string
///
/// # Returns
///
/// The decoded checkpoint token data, or an error if decoding fails.
pub fn decode_checkpoint_token(token: &str) -> Result<CheckpointTokenData, TestError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(token)
        .map_err(|e| TestError::InvalidCheckpointToken(format!("base64 decode error: {}", e)))?;

    let json = String::from_utf8(bytes)
        .map_err(|e| TestError::InvalidCheckpointToken(format!("utf8 decode error: {}", e)))?;

    serde_json::from_str(&json)
        .map_err(|e| TestError::InvalidCheckpointToken(format!("json parse error: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_roundtrip() {
        let data = CheckpointTokenData {
            execution_id: "exec-123".to_string(),
            token: "token-abc".to_string(),
            invocation_id: "inv-456".to_string(),
        };

        let encoded = encode_checkpoint_token(&data);
        let decoded = decode_checkpoint_token(&encoded).unwrap();

        assert_eq!(data, decoded);
    }

    #[test]
    fn test_decode_invalid_base64() {
        let result = decode_checkpoint_token("not-valid-base64!!!");
        assert!(result.is_err());
        match result {
            Err(TestError::InvalidCheckpointToken(msg)) => {
                assert!(msg.contains("base64"));
            }
            _ => panic!("Expected InvalidCheckpointToken error"),
        }
    }

    #[test]
    fn test_decode_invalid_json() {
        // Valid base64 but not valid JSON
        let invalid_json = URL_SAFE_NO_PAD.encode(b"not json");
        let result = decode_checkpoint_token(&invalid_json);
        assert!(result.is_err());
        match result {
            Err(TestError::InvalidCheckpointToken(msg)) => {
                assert!(msg.contains("json"));
            }
            _ => panic!("Expected InvalidCheckpointToken error"),
        }
    }

    #[test]
    fn test_encode_produces_url_safe_string() {
        let data = CheckpointTokenData {
            execution_id: "exec/with+special=chars".to_string(),
            token: "token".to_string(),
            invocation_id: "inv".to_string(),
        };

        let encoded = encode_checkpoint_token(&data);

        // URL-safe base64 should not contain +, /, or =
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
        // URL_SAFE_NO_PAD doesn't use padding
    }
}
