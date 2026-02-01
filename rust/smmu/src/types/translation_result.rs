//! Translation Result types for ARM `SMMU` v3
//!
//! This module defines the result types for translation operations, including
//! successful translation data and comprehensive error types. All implementations
//! are safe with zero unsafe code.

use super::{AccessType, PagePermissions, SecurityState, PA};
use thiserror::Error;

/// Translation operation error types
///
/// Comprehensive error enumeration for all translation failure cases following
/// ARM `SMMU` v3 specification fault classifications.
///
/// # Examples
///
/// ```
/// use smmu::types::{`TranslationError`, `AccessType`};
///
/// let error = `TranslationError`::PermissionViolation { access: `AccessType`::Write };
/// println!("Translation failed: {}", error);
/// ```
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum TranslationError {
    /// Page not mapped in address space (Translation Fault)
    #[error("Page not mapped in address space")]
    PageNotMapped,

    /// Permission violation - access type not permitted
    #[error("Permission violation for access type: {access:?}")]
    PermissionViolation {
        /// The access type that was denied
        access: AccessType,
    },

    /// Invalid address
    #[error("Invalid address: 0x{address:x}")]
    InvalidAddress {
        /// The invalid address value
        address: u64,
    },

    /// Invalid `StreamID`
    #[error("Invalid StreamID")]
    InvalidStreamID,

    /// Invalid `PASID`
    #[error("Invalid PASID")]
    InvalidPASID,

    /// `PASID` not found in stream context
    #[error("PASID not found")]
    PASIDNotFound,

    /// Stream not configured
    #[error("Stream not configured")]
    StreamNotConfigured,

    /// Stream disabled
    #[error("Stream disabled")]
    StreamDisabled,

    /// Address size error
    #[error("Address size error - address exceeds supported size")]
    AddressSizeError,

    /// Alignment error
    #[error("Address alignment error")]
    AlignmentError,

    /// Security violation
    #[error("Security state violation")]
    SecurityViolation,

    /// External abort on translation table walk
    #[error("External abort during translation")]
    ExternalAbort,

    /// TLB conflict
    #[error("TLB conflict detected")]
    TlbConflict,
}

/// Translation result data structure
///
/// Contains successful translation output including physical address,
/// permissions, and security state. This is the success type for
/// `TranslationResult`.
///
/// # Examples
///
/// ```
/// use smmu::types::{TranslationData, `PagePermissions`, `PA`, `SecurityState`};
///
/// let pa = `PA`::new(0x1000).unwrap();
/// let perms = `PagePermissions`::read_write();
/// let data = TranslationData::new(pa, perms, `SecurityState`::NonSecure);
///
/// assert_eq!(data.physical_address(), pa);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranslationData {
    /// Physical address (translation result)
    physical_address: PA,
    /// Page permissions for the translated address
    permissions: PagePermissions,
    /// Security state of the translated address
    security_state: SecurityState,
}

impl TranslationData {
    /// Creates new TranslationData with full translation information
    ///
    /// # Arguments
    ///
    /// * `physical_address` - Physical address result
    /// * `permissions` - Page permissions
    /// * `security_state` - Security state
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::types::{TranslationData, `PagePermissions`, `PA`, `SecurityState`};
    ///
    /// let pa = `PA`::new(0x2000).unwrap();
    /// let perms = `PagePermissions`::read_only();
    /// let data = TranslationData::new(pa, perms, `SecurityState`::Secure);
    /// ```
    #[must_use]
    #[inline]
    pub const fn new(physical_address: PA, permissions: PagePermissions, security_state: SecurityState) -> Self {
        Self {
            physical_address,
            permissions,
            security_state,
        }
    }

    /// Creates TranslationData with physical address only
    ///
    /// Permissions default to none, security state defaults to NonSecure.
    ///
    /// # Arguments
    ///
    /// * `physical_address` - Physical address result
    #[must_use]
    #[inline]
    pub const fn with_pa(physical_address: PA) -> Self {
        Self {
            physical_address,
            permissions: PagePermissions::none(),
            security_state: SecurityState::NonSecure,
        }
    }

    /// Creates a builder for constructing TranslationData
    #[must_use]
    #[inline]
    pub const fn builder() -> TranslationDataBuilder {
        TranslationDataBuilder::new()
    }

    /// Returns the physical address
    #[must_use]
    #[inline]
    pub const fn physical_address(&self) -> PA {
        self.physical_address
    }

    /// Returns the permissions
    #[must_use]
    #[inline]
    pub const fn permissions(&self) -> PagePermissions {
        self.permissions
    }

    /// Returns the security state
    #[must_use]
    #[inline]
    pub const fn security_state(&self) -> SecurityState {
        self.security_state
    }
}

impl Default for TranslationData {
    fn default() -> Self {
        Self {
            physical_address: PA::new(0).unwrap_or_else(|_| unreachable!()),
            permissions: PagePermissions::default(),
            security_state: SecurityState::NonSecure,
        }
    }
}

/// Builder for TranslationData
///
/// Provides a fluent interface for constructing `TranslationData` instances.
///
/// # Examples
///
/// ```
/// use smmu::types::{TranslationData, `PagePermissions`, `PA`, `SecurityState`};
///
/// let pa = `PA`::new(0x3000).unwrap();
/// let data = TranslationData::builder()
///     .physical_address(pa)
///     .permissions(`PagePermissions`::read_execute())
///     .security_state(`SecurityState`::Realm)
///     .build();
/// ```
#[derive(Debug, Clone, Copy)]
pub struct TranslationDataBuilder {
    physical_address: Option<PA>,
    permissions: PagePermissions,
    security_state: SecurityState,
}

impl TranslationDataBuilder {
    /// Creates a new builder with default values
    #[must_use]
    const fn new() -> Self {
        Self {
            physical_address: None,
            permissions: PagePermissions::none(),
            security_state: SecurityState::NonSecure,
        }
    }

    /// Sets the physical address
    #[must_use]
    pub const fn physical_address(mut self, pa: PA) -> Self {
        self.physical_address = Some(pa);
        self
    }

    /// Sets the permissions
    #[must_use]
    pub const fn permissions(mut self, perms: PagePermissions) -> Self {
        self.permissions = perms;
        self
    }

    /// Sets the security state
    #[must_use]
    pub const fn security_state(mut self, state: SecurityState) -> Self {
        self.security_state = state;
        self
    }

    /// Builds the TranslationData
    ///
    /// # Panics
    ///
    /// Panics if physical address was not set
    #[must_use]
    pub fn build(self) -> TranslationData {
        TranslationData {
            physical_address: self.physical_address.expect("Physical address must be set"),
            permissions: self.permissions,
            security_state: self.security_state,
        }
    }
}

/// Type alias for translation operation results
///
/// Either successful translation with `TranslationData` or error with
/// detailed `TranslationError`. This is the primary return type for all
/// translation operations.
///
/// # Examples
///
/// ```
/// use smmu::types::{TranslationResult, TranslationData, `TranslationError`, `PA`};
///
/// fn translate(valid: bool) -> TranslationResult {
///     if valid {
///         let pa = `PA`::new(0x1000).unwrap();
///         Ok(TranslationData::with_pa(pa))
///     } else {
///         Err(`TranslationError`::PageNotMapped)
///     }
/// }
///
/// let result = translate(true);
/// assert!(result.is_ok());
/// ```
pub type TranslationResult = Result<TranslationData, TranslationError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_translation_data_basic() {
        let pa = PA::new(0x1000).unwrap();
        let perms = PagePermissions::read_write();
        let data = TranslationData::new(pa, perms, SecurityState::NonSecure);

        assert_eq!(data.physical_address(), pa);
        assert_eq!(data.permissions(), perms);
        assert_eq!(data.security_state(), SecurityState::NonSecure);
    }

    #[test]
    fn test_translation_result_ok() {
        let pa = PA::new(0x2000).unwrap();
        let data = TranslationData::with_pa(pa);
        let result: TranslationResult = Ok(data);

        assert!(result.is_ok());
    }

    #[test]
    fn test_translation_result_err() {
        let result: TranslationResult = Err(TranslationError::PageNotMapped);

        assert!(result.is_err());
    }

    #[test]
    fn test_translation_error_display() {
        let error = TranslationError::PageNotMapped;
        let msg = format!("{error}");

        assert!(!msg.is_empty());
    }
}
