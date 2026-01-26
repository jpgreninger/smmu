//! Validation error type for newtype wrappers
//!
//! This module provides the `ValidationError` type used throughout the SMMU implementation
//! for reporting validation failures in newtype constructors.

use core::fmt;

/// Validation error for newtype wrapper construction and operations
///
/// Provides detailed context about validation failures including:
/// - Field name (e.g., `StreamID`, `PASID`)
/// - Invalid value that was provided
/// - Constraint that was violated
/// - Specific error types for different validation scenarios
///
/// # Examples
///
/// ```
/// use smmu::ValidationError;
///
/// let error = ValidationError::OutOfRange {
///     field: "PASID".to_string(),
///     value: 1048576,
///     max: 1048575,
/// };
///
/// println!("Validation failed: {}", error);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// Value is out of the valid range
    OutOfRange {
        /// Field name
        field: String,
        /// Invalid value provided
        value: u64,
        /// Maximum allowed value
        max: u64,
    },

    /// Address is not properly aligned
    InvalidAlignment {
        /// Address value
        address: u64,
        /// Required alignment (e.g., 0x1000 for 4KB pages)
        required_alignment: u64,
    },

    /// Invalid access type bit pattern
    InvalidAccessType {
        /// Invalid bit pattern
        bits: u8,
    },

    /// Invalid security state encoding
    InvalidSecurityState {
        /// Invalid security state bits
        bits: u8,
    },

    /// Invalid translation stage configuration
    InvalidTranslationStage {
        /// Invalid stage bits
        bits: u8,
    },

    /// Invalid fault type code
    InvalidFaultType {
        /// Invalid fault code
        code: u8,
    },

    /// Invalid state transition
    InvalidStateTransition {
        /// Current state
        from: String,
        /// Attempted target state
        to: String,
    },

    /// Permission denied
    PermissionDenied {
        /// Requested permissions
        requested: String,
        /// Available permissions
        available: String,
    },

    /// Security violation
    SecurityViolation {
        /// Accessing security state
        from_state: String,
        /// Target security state
        to_state: String,
    },

    /// Invalid configuration
    InvalidConfiguration {
        /// Reason for configuration error
        reason: String,
    },

    /// Invalid PASID value
    InvalidPASID {
        /// Invalid PASID value
        value: u32,
    },

    /// Generic validation error (for backward compatibility)
    Generic {
        /// Field name
        field: String,
        /// Invalid value as string
        value: String,
        /// Constraint description
        constraint: String,
    },
}

impl ValidationError {
    /// Creates a new generic validation error with context
    ///
    /// # Arguments
    ///
    /// * `field` - Name of the field that failed validation
    /// * `value` - The invalid value that was provided
    /// * `constraint` - Description of the constraint that was violated
    #[must_use]
    pub fn new(field: &str, value: &str, constraint: &str) -> Self {
        Self::Generic {
            field: field.to_string(),
            value: value.to_string(),
            constraint: constraint.to_string(),
        }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange { field, value, max } => {
                write!(
                    f,
                    "Validation error for {}: value {} exceeds maximum {}",
                    field, value, max
                )
            }
            Self::InvalidAlignment {
                address,
                required_alignment,
            } => {
                write!(
                    f,
                    "Address {:#x} is not aligned to {:#x}",
                    address, required_alignment
                )
            }
            Self::InvalidAccessType { bits } => {
                write!(f, "Invalid access type bit pattern: {:#b}", bits)
            }
            Self::InvalidSecurityState { bits } => {
                write!(f, "Invalid security state encoding: {:#b}", bits)
            }
            Self::InvalidTranslationStage { bits } => {
                write!(f, "Invalid translation stage configuration: {:#b}", bits)
            }
            Self::InvalidFaultType { code } => {
                write!(f, "Invalid fault type code: {:#x}", code)
            }
            Self::InvalidStateTransition { from, to } => {
                write!(f, "Invalid state transition from {} to {}", from, to)
            }
            Self::PermissionDenied {
                requested,
                available,
            } => {
                write!(
                    f,
                    "Permission denied: requested {} but only {} available",
                    requested, available
                )
            }
            Self::SecurityViolation {
                from_state,
                to_state,
            } => {
                write!(
                    f,
                    "Security violation: {} cannot access {}",
                    from_state, to_state
                )
            }
            Self::InvalidConfiguration { reason } => {
                write!(f, "Invalid configuration: {}", reason)
            }
            Self::InvalidPASID { value } => {
                write!(f, "Invalid PASID value: {}", value)
            }
            Self::Generic {
                field,
                value,
                constraint,
            } => {
                write!(
                    f,
                    "Validation error for {}: value '{}' {}",
                    field, value, constraint
                )
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ValidationError {}
