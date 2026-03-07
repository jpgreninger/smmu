//! Translation Result types for ARM SMMU v3
//!
//! This module defines the result types for translation operations, including
//! successful translation data and comprehensive error types. All implementations
//! are safe with zero unsafe code.

use super::{AccessType, PagePermissions, SecurityState, PA};
use thiserror::Error;

/// Translation operation error types
///
/// Comprehensive error enumeration for all translation failure cases following
/// ARM SMMU v3 specification fault classifications.
///
/// # Examples
///
/// ```
/// use smmu::types::{TranslationError, AccessType};
///
/// let error = TranslationError::PermissionViolation { access: AccessType::Write };
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

    /// Invalid StreamID
    #[error("Invalid StreamID")]
    InvalidStreamID,

    /// Invalid PASID
    #[error("Invalid PASID")]
    InvalidPASID,

    /// PASID not found in stream context
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

    /// Transaction stalled pending CMD_RESUME (ARM §3.12.2)
    ///
    /// Returned when a stream is configured with `FaultMode::Stall` and a
    /// translation fault occurs.  The `stag` is a unique Stall TAG that the
    /// software must pass back in a `CMD_RESUME` or `CMD_STALL_TERM` command
    /// to complete or abort the stalled transaction.
    #[error("Transaction stalled (STAG={stag:#06x})")]
    Stalled {
        /// Stall TAG — unique identifier for this stalled transaction
        stag: u16,
    },

    /// Transaction aborted by GBPA.ABORT (ARM §3.11, §13.2)
    ///
    /// Returned when `SMMUEN=0` (SMMU disabled) and `GBPA.ABORT=1`.
    /// In this state the SMMU aborts all incoming transactions instead of
    /// bypassing them with an identity mapping.  No fault event is enqueued.
    #[error("Transaction aborted by GBPA.ABORT (SMMUEN=0, GBPA.ABORT=1)")]
    GbpaAbort,

    /// Non-zero PASID (SubstreamID) on stage-2-only or bypass stream (ARM §3.9, §7.3.9)
    ///
    /// Generates `C_BAD_SUBSTREAMID` (event code 0x08). Always terminates with
    /// an abort; the stall path is never taken for this fault class.
    #[error("Non-zero PASID on stage-2-only or bypass stream (§3.9 C_BAD_SUBSTREAMID)")]
    BadSubstreamId,

    /// Invalid or unsupported Context Descriptor (ARM §7.3.11, §3.12.2)
    ///
    /// Generates `C_BAD_CD` (event code 0x0A). Occurs when CD fields are
    /// invalid — for example T0SZ/T1SZ out of range (> 39) or CD.AA64=false
    /// (AArch32 LPAE unsupported). Always terminates with an abort; the stall
    /// path is never taken for configuration faults (ARM §11.639).
    #[error("Invalid context descriptor (§7.3.11 C_BAD_CD)")]
    BadCD,
}

/// Translation result data structure
///
/// Contains successful translation output including physical address,
/// permissions, security state, and STE output-attribute overrides
/// (GAP-1: §5.2 STE output-attribute override fields).
///
/// The six output-attribute fields carry the resolved STE override values
/// applied by [`StreamContext`](crate::stream_context::StreamContext) after
/// a successful translation:
///
/// - `mem_type`: resolved memory type (STE.MTCFG / STE.MemAttr)
/// - `shareability`: shareability override (STE.SHCFG)
/// - `alloc_hint`: allocation hint override (STE.ALLOCCFG)
/// - `inst_cfg`: instruction/data attribute (STE.INSTCFG)
/// - `priv_cfg`: privilege attribute (STE.PRIVCFG)
/// - `ns_cfg_out`: resolved NS output attribute (STE.NSCFG)
///
/// All six fields default to `0` when constructed via [`TranslationData::new`]
/// and are populated by the stream context via [`TranslationData::with_output_attrs`].
///
/// # Examples
///
/// ```
/// use smmu::types::{TranslationData, PagePermissions, PA, SecurityState};
///
/// let pa = PA::new(0x1000).unwrap();
/// let perms = PagePermissions::read_write();
/// let data = TranslationData::new(pa, perms, SecurityState::NonSecure);
///
/// assert_eq!(data.physical_address(), pa);
/// assert_eq!(data.mem_type(), 0u8);
/// assert_eq!(data.shareability(), 0u8);
/// ```
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranslationData {
    /// Physical address (translation result)
    physical_address: PA,
    /// Page permissions for the translated address
    permissions: PagePermissions,
    /// Security state of the translated address
    security_state: SecurityState,
    /// §5.2 STE.MTCFG / STE.MemAttr: resolved memory type (0 = from-translation)
    mem_type: u8,
    /// §5.2 STE.SHCFG: shareability override (0 = from-translation)
    shareability: u8,
    /// §5.2 STE.ALLOCCFG: allocation hint override
    alloc_hint: u8,
    /// §5.2 STE.INSTCFG: instruction/data attribute override
    inst_cfg: u8,
    /// §5.2 STE.PRIVCFG: privilege attribute override
    priv_cfg: u8,
    /// §5.2 STE.NSCFG: resolved NS output attribute
    ns_cfg_out: u8,
}

impl TranslationData {
    /// Creates new TranslationData with full translation information
    ///
    /// All six STE output-attribute override fields default to `0`.
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
    /// use smmu::types::{TranslationData, PagePermissions, PA, SecurityState};
    ///
    /// let pa = PA::new(0x2000).unwrap();
    /// let perms = PagePermissions::read_only();
    /// let data = TranslationData::new(pa, perms, SecurityState::Secure);
    /// assert_eq!(data.mem_type(), 0u8);
    /// ```
    #[must_use]
    #[inline]
    pub const fn new(physical_address: PA, permissions: PagePermissions, security_state: SecurityState) -> Self {
        Self {
            physical_address,
            permissions,
            security_state,
            mem_type: 0,
            shareability: 0,
            alloc_hint: 0,
            inst_cfg: 0,
            priv_cfg: 0,
            ns_cfg_out: 0,
        }
    }

    /// Creates TranslationData with physical address only
    ///
    /// Permissions default to none, security state defaults to NonSecure.
    /// All six STE output-attribute override fields default to `0`.
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
            mem_type: 0,
            shareability: 0,
            alloc_hint: 0,
            inst_cfg: 0,
            priv_cfg: 0,
            ns_cfg_out: 0,
        }
    }

    /// Returns the resolved memory type override (§5.2 STE.MTCFG/MemAttr, GAP-1)
    ///
    /// `0` means the memory type from the translation is used unchanged.
    /// Non-zero means the STE.MemAttr value overrode the translation result.
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::types::{TranslationData, PagePermissions, PA, SecurityState};
    ///
    /// let pa = PA::new(0x1000).unwrap();
    /// let data = TranslationData::new(pa, PagePermissions::read_only(), SecurityState::NonSecure);
    /// assert_eq!(data.mem_type(), 0u8);
    /// ```
    #[must_use]
    #[inline]
    pub const fn mem_type(&self) -> u8 {
        self.mem_type
    }

    /// Returns the shareability override (§5.2 STE.SHCFG, GAP-1)
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::types::{TranslationData, PagePermissions, PA, SecurityState};
    ///
    /// let pa = PA::new(0x1000).unwrap();
    /// let data = TranslationData::new(pa, PagePermissions::read_only(), SecurityState::NonSecure);
    /// assert_eq!(data.shareability(), 0u8);
    /// ```
    #[must_use]
    #[inline]
    pub const fn shareability(&self) -> u8 {
        self.shareability
    }

    /// Returns the allocation hint override (§5.2 STE.ALLOCCFG, GAP-1)
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::types::{TranslationData, PagePermissions, PA, SecurityState};
    ///
    /// let pa = PA::new(0x1000).unwrap();
    /// let data = TranslationData::new(pa, PagePermissions::read_only(), SecurityState::NonSecure);
    /// assert_eq!(data.alloc_hint(), 0u8);
    /// ```
    #[must_use]
    #[inline]
    pub const fn alloc_hint(&self) -> u8 {
        self.alloc_hint
    }

    /// Returns the instruction/data attribute override (§5.2 STE.INSTCFG, GAP-1)
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::types::{TranslationData, PagePermissions, PA, SecurityState};
    ///
    /// let pa = PA::new(0x1000).unwrap();
    /// let data = TranslationData::new(pa, PagePermissions::read_only(), SecurityState::NonSecure);
    /// assert_eq!(data.inst_cfg(), 0u8);
    /// ```
    #[must_use]
    #[inline]
    pub const fn inst_cfg(&self) -> u8 {
        self.inst_cfg
    }

    /// Returns the privilege attribute override (§5.2 STE.PRIVCFG, GAP-1)
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::types::{TranslationData, PagePermissions, PA, SecurityState};
    ///
    /// let pa = PA::new(0x1000).unwrap();
    /// let data = TranslationData::new(pa, PagePermissions::read_only(), SecurityState::NonSecure);
    /// assert_eq!(data.priv_cfg(), 0u8);
    /// ```
    #[must_use]
    #[inline]
    pub const fn priv_cfg(&self) -> u8 {
        self.priv_cfg
    }

    /// Returns the resolved NS output attribute (§5.2 STE.NSCFG, GAP-1)
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::types::{TranslationData, PagePermissions, PA, SecurityState};
    ///
    /// let pa = PA::new(0x1000).unwrap();
    /// let data = TranslationData::new(pa, PagePermissions::read_only(), SecurityState::NonSecure);
    /// assert_eq!(data.ns_cfg_out(), 0u8);
    /// ```
    #[must_use]
    #[inline]
    pub const fn ns_cfg_out(&self) -> u8 {
        self.ns_cfg_out
    }

    /// Returns a new `TranslationData` with all six STE output-attribute fields set (GAP-1)
    ///
    /// Called by the stream context after a successful translation to apply STE
    /// output-attribute overrides per ARM §5.2.
    ///
    /// # Arguments
    ///
    /// * `mem_type` - Resolved memory type (STE.MTCFG/MemAttr)
    /// * `shareability` - Shareability override (STE.SHCFG)
    /// * `alloc_hint` - Allocation hint override (STE.ALLOCCFG)
    /// * `inst_cfg` - Instruction/data attribute (STE.INSTCFG)
    /// * `priv_cfg` - Privilege attribute (STE.PRIVCFG)
    /// * `ns_cfg_out` - Resolved NS output attribute (STE.NSCFG)
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::types::{TranslationData, PagePermissions, PA, SecurityState};
    ///
    /// let pa = PA::new(0x1000).unwrap();
    /// let data = TranslationData::new(pa, PagePermissions::read_only(), SecurityState::NonSecure)
    ///     .with_output_attrs(7, 3, 0xF, 2, 1, 2);
    /// assert_eq!(data.mem_type(), 7u8);
    /// assert_eq!(data.shareability(), 3u8);
    /// assert_eq!(data.alloc_hint(), 0xFu8);
    /// assert_eq!(data.inst_cfg(), 2u8);
    /// assert_eq!(data.priv_cfg(), 1u8);
    /// assert_eq!(data.ns_cfg_out(), 2u8);
    /// ```
    #[must_use]
    pub const fn with_output_attrs(
        mut self,
        mem_type: u8,
        shareability: u8,
        alloc_hint: u8,
        inst_cfg: u8,
        priv_cfg: u8,
        ns_cfg_out: u8,
    ) -> Self {
        self.mem_type = mem_type;
        self.shareability = shareability;
        self.alloc_hint = alloc_hint;
        self.inst_cfg = inst_cfg;
        self.priv_cfg = priv_cfg;
        self.ns_cfg_out = ns_cfg_out;
        self
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
            mem_type: 0,
            shareability: 0,
            alloc_hint: 0,
            inst_cfg: 0,
            priv_cfg: 0,
            ns_cfg_out: 0,
        }
    }
}

/// Builder for TranslationData
///
/// Provides a fluent interface for constructing TranslationData instances.
///
/// # Examples
///
/// ```
/// use smmu::types::{TranslationData, PagePermissions, PA, SecurityState};
///
/// let pa = PA::new(0x3000).unwrap();
/// let data = TranslationData::builder()
///     .physical_address(pa)
///     .permissions(PagePermissions::read_execute())
///     .security_state(SecurityState::Realm)
///     .build();
/// ```
#[derive(Debug, Clone, Copy)]
pub struct TranslationDataBuilder {
    physical_address: Option<PA>,
    permissions: PagePermissions,
    security_state: SecurityState,
    mem_type: u8,
    shareability: u8,
    alloc_hint: u8,
    inst_cfg: u8,
    priv_cfg: u8,
    ns_cfg_out: u8,
}

impl TranslationDataBuilder {
    /// Creates a new builder with default values
    #[must_use]
    const fn new() -> Self {
        Self {
            physical_address: None,
            permissions: PagePermissions::none(),
            security_state: SecurityState::NonSecure,
            mem_type: 0,
            shareability: 0,
            alloc_hint: 0,
            inst_cfg: 0,
            priv_cfg: 0,
            ns_cfg_out: 0,
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

    /// Sets all six STE output-attribute override fields (GAP-1)
    #[must_use]
    pub const fn output_attrs(
        mut self,
        mem_type: u8,
        shareability: u8,
        alloc_hint: u8,
        inst_cfg: u8,
        priv_cfg: u8,
        ns_cfg_out: u8,
    ) -> Self {
        self.mem_type = mem_type;
        self.shareability = shareability;
        self.alloc_hint = alloc_hint;
        self.inst_cfg = inst_cfg;
        self.priv_cfg = priv_cfg;
        self.ns_cfg_out = ns_cfg_out;
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
            mem_type: self.mem_type,
            shareability: self.shareability,
            alloc_hint: self.alloc_hint,
            inst_cfg: self.inst_cfg,
            priv_cfg: self.priv_cfg,
            ns_cfg_out: self.ns_cfg_out,
        }
    }
}

/// Type alias for translation operation results
///
/// Either successful translation with TranslationData or error with
/// detailed TranslationError. This is the primary return type for all
/// translation operations.
///
/// # Examples
///
/// ```
/// use smmu::types::{TranslationResult, TranslationData, TranslationError, PA};
///
/// fn translate(valid: bool) -> TranslationResult {
///     if valid {
///         let pa = PA::new(0x1000).unwrap();
///         Ok(TranslationData::with_pa(pa))
///     } else {
///         Err(TranslationError::PageNotMapped)
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
