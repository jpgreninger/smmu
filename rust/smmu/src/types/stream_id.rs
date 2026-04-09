//! StreamID newtype wrapper
//!
//! Provides type-safe StreamID handling per ARM SMMU v3 specification.
//!
//! # ARM SMMU v3 Compliance
//!
//! Per ARM SMMU v3 §3.2 and §6.3.2, `IDR1.SIDSIZE` determines the supported StreamID width.
//! When `IDR1.SIDSIZE=32`, all 32-bit values (`0..=0xFFFF_FFFF`) are architecturally valid
//! StreamIDs at the type level. Range enforcement against the configured stream table size
//! is a runtime concern handled by the `strtab_log2size` check, which produces
//! `C_BAD_STREAMID` for out-of-range accesses.
//!
//! # Examples
//!
//! ```rust
//! use smmu::types::StreamID;
//!
//! // Create a valid StreamID
//! let stream_id = StreamID::new(42).expect("Valid StreamID");
//! assert_eq!(stream_id.as_u32(), 42);
//!
//! // Full 32-bit range is valid when IDR1.SIDSIZE=32
//! let high = StreamID::new(u32::MAX).expect("32-bit StreamID valid per ARM §3.2");
//! assert_eq!(high.as_u32(), u32::MAX);
//! ```

use super::ValidationError;
use std::fmt;

// No type-level cap: ARM §3.2 IDR1.SIDSIZE=32 means all u32 values are valid StreamIDs.
// Runtime range enforcement is done by strtab_log2size (→ C_BAD_STREAMID).

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

/// Type-safe StreamID wrapper
///
/// Wraps a 32-bit unsigned integer representing a hardware StreamID.
/// Per ARM SMMU v3 §3.2, when `IDR1.SIDSIZE=32` all 32-bit values are valid at the
/// type level. Out-of-range enforcement relative to the configured stream table size
/// is handled at runtime via `strtab_log2size`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StreamID(u32);

impl StreamID {
    /// Creates a new StreamID
    ///
    /// Accepts the full 32-bit range (`0..=u32::MAX`). Per ARM SMMU v3 §3.2,
    /// `IDR1.SIDSIZE=32` means all u32 values are architecturally valid StreamIDs.
    /// Runtime enforcement against the configured stream table size (`strtab_log2size`)
    /// produces `C_BAD_STREAMID` for out-of-range accesses.
    ///
    /// The `Result` return type is preserved for API compatibility and to allow
    /// future per-instance configuration.
    ///
    /// # Arguments
    ///
    /// * `value` - The StreamID value to wrap
    ///
    /// # Returns
    ///
    /// Always returns `Ok(StreamID)`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::types::StreamID;
    /// let stream_id = StreamID::new(42).unwrap();
    /// assert_eq!(stream_id.as_u32(), 42);
    ///
    /// // Full 32-bit range accepted (IDR1.SIDSIZE=32, ARM §3.2)
    /// let high = StreamID::new(u32::MAX).unwrap();
    /// assert_eq!(high.as_u32(), u32::MAX);
    /// ```
    pub fn new(value: u32) -> Result<Self, ValidationError> {
        Ok(Self(value))
    }

    /// Converts the StreamID to its underlying `u32` value
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::types::StreamID;
    /// let stream_id = StreamID::new(42).unwrap();
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
