//! PASID (Process Address Space ID) newtype wrapper
//!
//! Provides type-safe PASID handling with validation per ARM SMMU v3 specification.
//!
//! # ARM SMMU v3 Compliance
//!
//! Per ARM SMMU v3 specification Section 3.6:
//! - PASID is a 20-bit value (0-1048575, i.e., 0x00000-0xFFFFF)
//! - PASID 0 represents the default address space and must always be supported
//! - Maximum value is 0xFFFFF (1048575)
//!
//! # Examples
//!
//! ```ignore
//! use smmu::types::PASID;
//!
//! // Create PASID for default address space
//! let default_pasid = PASID::new(0).expect("PASID 0 must be valid");
//! assert_eq!(default_pasid.as_u32(), 0);
//!
//! // Or use Default
//! let default_pasid = PASID::default();
//!
//! // Create PASID with maximum value
//! let max_pasid = PASID::new(0xFFFFF).expect("Maximum PASID");
//!
//! // Values exceeding 20-bit fail validation
//! let result = PASID::new(0x100000);
//! assert!(result.is_err());
//! ```

use super::ValidationError;
use std::fmt;

/// ARM SMMU v3 PASID maximum value (20-bit)
pub const PASID_MAX: u32 = 0xFFFFF;

/// Type-safe PASID wrapper
///
/// Wraps a 32-bit unsigned integer with validation to ensure it is a valid 20-bit
/// PASID value (0-0xFFFFF) as required by ARM SMMU v3 specification.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PASID(u32);

impl PASID {
    /// Creates a new PASID with validation
    ///
    /// # Arguments
    ///
    /// * `value` - The PASID value to validate and wrap (must be <= 0xFFFFF)
    ///
    /// # Returns
    ///
    /// `Ok(PASID)` if the value is valid (0-0xFFFFF), `Err(ValidationError)` otherwise
    ///
    /// # Errors
    ///
    /// Returns `ValidationError` if value exceeds 20-bit maximum (0xFFFFF)
    ///
    /// # ARM SMMU v3 Compliance
    ///
    /// - Validates that value is within 20-bit range (0-0xFFFFF)
    /// - PASID 0 (default address space) is always valid
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let pasid = PASID::new(42)?;
    /// let default_pasid = PASID::new(0)?; // Default address space
    /// let max_pasid = PASID::new(0xFFFFF)?; // Maximum value
    /// ```
    pub fn new(value: u32) -> Result<Self, ValidationError> {
        if value > PASID_MAX {
            return Err(ValidationError::new(
                "PASID",
                &format!("0x{value:X}"),
                &format!("must be <= 0x{PASID_MAX:X} (20-bit maximum)"),
            ));
        }
        Ok(Self(value))
    }

    /// Converts the PASID to its underlying `u32` value
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let pasid = PASID::new(42)?;
    /// assert_eq!(pasid.as_u32(), 42);
    /// ```
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl fmt::Display for PASID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PASID({})", self.0)
    }
}

impl TryFrom<u32> for PASID {
    type Error = ValidationError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<PASID> for u32 {
    fn from(pasid: PASID) -> Self {
        pasid.0
    }
}
