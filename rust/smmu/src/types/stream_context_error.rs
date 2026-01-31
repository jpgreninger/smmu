//! `StreamContext` error types for ARM `SMMU` v3
//!
//! This module defines error types for `StreamContext` operations, including
//! `PASID` management, configuration, and resource limit errors.

use thiserror::Error;

/// `StreamContext` operation error types
///
/// Comprehensive error enumeration for all stream context operation failure cases.
///
/// # Examples
///
/// ```
/// use smmu::types::StreamContextError;
///
/// let error = StreamContextError::PASIDAlreadyExists(1);
/// println!("`StreamContext` error: {}", error);
/// ```
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum StreamContextError {
    /// `PASID` already exists in stream context
    #[error("PASID {0} already exists")]
    PASIDAlreadyExists(u32),

    /// `PASID` not found in stream context
    #[error("PASID {0} not found")]
    PASIDNotFound(u32),

    /// `PASID` limit exceeded
    #[error("PASID limit exceeded: {0} > {1}")]
    PASIDLimitExceeded(usize, usize),

    /// Invalid `PASID` value
    #[error("Invalid PASID: {0}")]
    InvalidPASID(u32),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    /// Internal error
    #[error("Internal error: {0}")]
    InternalError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_context_error_display() {
        let error = StreamContextError::PASIDAlreadyExists(1);
        let msg = format!("{error}");
        assert!(msg.contains("PASID 1 already exists"));
    }

    #[test]
    fn test_stream_context_error_clone() {
        let error = StreamContextError::PASIDNotFound(2);
        let cloned = error.clone();
        assert_eq!(error, cloned);
    }
}
