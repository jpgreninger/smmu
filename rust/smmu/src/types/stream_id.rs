//! `StreamID` newtype wrapper
//!
//! Provides type-safe `StreamID` handling with validation per ARM `SMMU` v3 specification.
//!
//! # ARM `SMMU` v3 Compliance
//!
//! `StreamID` is a hardware-dependent identifier, typically in the range 0-65_535 (16-bit).
//! The implementation supports configurable maximum values.
//!
//! # Examples
//!
//! ```ignore
//! use smmu::types::`StreamID`;
//!
//! // Create a valid `StreamID`
//! let stream_id = `StreamID`::new(42).expect("Valid `StreamID`");
//! assert_eq!(stream_id.as_u32(), 42);
//!
//! // Invalid `StreamID` construction fails
//! let result = `StreamID`::new(u32::MAX);
//! assert!(result.is_err());
//! ```

use super::ValidationError;
use std::fmt;

/// Maximum `StreamID` value (typical hardware limit - 16-bit)
const STREAM_ID_MAX: u32 = 65_535;

/// Helper function to format u32 with underscores for readability
fn format_with_underscores(value: u32) -> String {
    let s = value.to_string();
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut result = String::new();

    for (i, &byte) in bytes.iter().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            result.push('_');
        }
        result.push(byte as char);
    }
    result
}

/// Type-safe `StreamID` wrapper
///
/// Wraps a 32-bit unsigned integer with validation to ensure it falls within
/// the hardware-supported range (typically 0-65_535).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StreamID(u32);

impl StreamID {
    /// Creates a new `StreamID` with validation
    ///
    /// # Arguments
    ///
    /// * `value` - The `StreamID` value to validate and wrap
    ///
    /// # Returns
    ///
    /// `Ok(`StreamID`)` if the value is valid, `Err(`ValidationError`)` otherwise
    ///
    /// # Errors
    ///
    /// Returns `ValidationError` if value exceeds the configured maximum
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let stream_id = `StreamID`::new(42)?;
    /// ```
    pub fn new(value: u32) -> Result<Self, ValidationError> {
        if value > STREAM_ID_MAX {
            return Err(ValidationError::new(
                "StreamID",
                &format_with_underscores(value),
                "must be <= 65_535",
            ));
        }
        Ok(Self(value))
    }

    /// Converts the `StreamID` to its underlying `u32` value
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let stream_id = `StreamID`::new(42)?;
    /// assert_eq!(stream_id.as_u32(), 42);
    /// ```
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl fmt::Display for StreamID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StreamID({})", format_with_underscores(self.0))
    }
}

impl TryFrom<u32> for StreamID {
    type Error = ValidationError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<StreamID> for u32 {
    fn from(stream_id: StreamID) -> Self {
        stream_id.0
    }
}
