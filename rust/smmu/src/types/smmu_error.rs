//! SMMU error types
//!
//! Comprehensive error type for SMMU controller operations, including stream management,
//! configuration validation, and translation errors.

use crate::address_space::AddressSpaceError;
use crate::types::{StreamContextError, StreamID, TranslationError};
use thiserror::Error;

/// SMMU controller error type
///
/// Represents all possible errors that can occur during SMMU controller operations,
/// including stream management, configuration, and translation.
///
/// # ARM SMMU v3 Compliance
///
/// Error types align with ARM SMMU v3 specification fault conditions and
/// resource management requirements.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SMMUError {
    /// Stream with specified ID not found
    #[error("Stream with ID {0} not found")]
    StreamNotFound(StreamID),

    /// Stream with specified ID already exists
    #[error("Stream with ID {0} already exists")]
    StreamAlreadyExists(StreamID),

    /// Stream limit exceeded
    #[error("Stream limit exceeded: current={current}, limit={limit}")]
    StreamLimitExceeded {
        /// Current stream count
        current: usize,
        /// Maximum allowed streams
        limit: usize,
    },

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// SMMU shutdown in progress
    #[error("SMMU shutdown in progress")]
    ShutdownInProgress,

    /// Translation error (forwarded from address space)
    #[error("Translation error: {0}")]
    TranslationError(#[from] TranslationError),

    /// Stream context error (forwarded from stream operations)
    #[error("Stream context error: {0}")]
    StreamContextError(#[from] StreamContextError),

    /// Address space error (forwarded from address space operations)
    #[error("Address space error: {0}")]
    AddressSpaceError(#[from] AddressSpaceError),

    /// Event queue is full
    #[error("Event queue is full")]
    EventQueueFull,

    /// Event queue is disabled (CR0.EVENTQEN=0)
    #[error("Event queue is disabled (CR0.EVENTQEN=0)")]
    EventQueueDisabled,

    /// Command queue is full
    #[error("Command queue is full")]
    CommandQueueFull,

    /// PRI queue is full
    #[error("PRI queue is full")]
    PriQueueFull,

    /// Invalid command parameters
    #[error("Invalid command parameters: {0}")]
    InvalidCommandParameters(String),
}

impl SMMUError {
    /// Create a stream not found error
    #[inline]
    #[must_use]
    pub const fn stream_not_found(stream_id: StreamID) -> Self {
        Self::StreamNotFound(stream_id)
    }

    /// Create a stream already exists error
    #[inline]
    #[must_use]
    pub const fn stream_already_exists(stream_id: StreamID) -> Self {
        Self::StreamAlreadyExists(stream_id)
    }

    /// Create a stream limit exceeded error
    #[inline]
    #[must_use]
    pub const fn stream_limit_exceeded(current: usize, limit: usize) -> Self {
        Self::StreamLimitExceeded { current, limit }
    }

    /// Create an invalid configuration error
    #[inline]
    #[must_use]
    pub fn invalid_configuration(reason: impl Into<String>) -> Self {
        Self::InvalidConfiguration(reason.into())
    }

    /// Check if error is due to stream not found
    #[inline]
    #[must_use]
    pub const fn is_stream_not_found(&self) -> bool {
        matches!(self, Self::StreamNotFound(_))
    }

    /// Check if error is due to stream already exists
    #[inline]
    #[must_use]
    pub const fn is_stream_already_exists(&self) -> bool {
        matches!(self, Self::StreamAlreadyExists(_))
    }

    /// Check if error is due to shutdown
    #[inline]
    #[must_use]
    pub const fn is_shutdown(&self) -> bool {
        matches!(self, Self::ShutdownInProgress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smmu_error_construction() {
        let stream_id = StreamID::new(1).unwrap();

        let err = SMMUError::stream_not_found(stream_id);
        assert!(err.is_stream_not_found());

        let err = SMMUError::stream_already_exists(stream_id);
        assert!(err.is_stream_already_exists());

        let err = SMMUError::stream_limit_exceeded(10, 5);
        match err {
            SMMUError::StreamLimitExceeded { current, limit } => {
                assert_eq!(current, 10);
                assert_eq!(limit, 5);
            },
            _ => panic!("Expected StreamLimitExceeded"),
        }
    }

    #[test]
    fn test_smmu_error_display() {
        let stream_id = StreamID::new(42).unwrap();
        let err = SMMUError::stream_not_found(stream_id);
        assert!(err.to_string().contains("42"));

        let err = SMMUError::ShutdownInProgress;
        assert!(err.to_string().contains("shutdown"));
    }
}
