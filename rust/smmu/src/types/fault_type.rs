//! Fault type definitions for ARM `SMMU` v3
//!
//! This module defines all 15 ARM `SMMU` v3 fault types with detailed classification and severity.

use crate::types::{ValidationError, StreamID, PASID, IOVA};
use core::fmt;

/// Fault severity level
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FaultSeverity {
    /// Warning - recoverable fault
    Warning,
    /// Error - non-recoverable but operation can be terminated cleanly
    Error,
    /// Critical - configuration or system error
    Critical,
}

/// Translation step where fault occurred
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TranslationStep {
    /// Stage 1 translation
    Stage1,
    /// Stage 2 translation
    Stage2,
}

/// Address type
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AddressType {
    /// Input/Output Virtual Address
    IOVA,
    /// Intermediate Physical Address
    IPA,
    /// Physical Address
    PA,
}

/// ARM `SMMU` v3 Fault Type
///
/// Represents all 15 fault types defined in the ARM `SMMU` v3 specification.
///
/// # Fault Codes
///
/// Each fault type has a unique code (0x01-0x0F) as defined in the ARM `SMMU` v3 spec.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FaultType {
    /// Translation fault - no valid translation found
    TranslationFault = 0x01,

    /// Address size fault - address exceeds supported size
    AddressSizeFault = 0x02,

    /// Access flag fault - access flag not set in descriptor
    AccessFlagFault = 0x03,

    /// Permission fault - requested access not permitted
    PermissionFault = 0x04,

    /// External abort on translation table walk or descriptor fetch
    ExternalAbort = 0x05,

    /// TLB conflict abort
    TLBConflictAbort = 0x06,

    /// Unsupported atomic update to translation table
    UnsupportedAtomicUpdate = 0x07,

    /// Alignment fault
    AlignmentFault = 0x08,

    /// Output address range fault
    OutputAddressRangeFault = 0x09,

    /// Bad `StreamID`
    BadStreamID = 0x0A,

    /// Context Descriptor fetch fault
    CDFetchFault = 0x0B,

    /// Bad Context Descriptor
    BadCD = 0x0C,

    /// Page table walk external abort
    WalkEABT = 0x0D,

    /// Bad Stream Table Entry
    BadSTE = 0x0E,

    /// Stream Table Entry fetch fault
    STEFetchFault = 0x0F,
}

impl FaultType {
    /// Get the ARM `SMMU` v3 fault code
    #[inline]
    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// Get the fault name
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::TranslationFault => "Translation Fault",
            Self::AddressSizeFault => "Address Size Fault",
            Self::AccessFlagFault => "Access Flag Fault",
            Self::PermissionFault => "Permission Fault",
            Self::ExternalAbort => "External Abort",
            Self::TLBConflictAbort => "TLB Conflict Abort",
            Self::UnsupportedAtomicUpdate => "Unsupported Atomic Update",
            Self::AlignmentFault => "Alignment Fault",
            Self::OutputAddressRangeFault => "Output Address Range Fault",
            Self::BadStreamID => "Bad StreamID",
            Self::CDFetchFault => "Context Descriptor Fetch Fault",
            Self::BadCD => "Bad Context Descriptor",
            Self::WalkEABT => "Page Table Walk External Abort",
            Self::BadSTE => "Bad Stream Table Entry",
            Self::STEFetchFault => "Stream Table Entry Fetch Fault",
        }
    }

    /// Get fault description
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::TranslationFault => "No valid translation found for the input address",
            Self::AddressSizeFault => "Input address exceeds the supported address size",
            Self::AccessFlagFault => "Access flag not set in translation descriptor",
            Self::PermissionFault => "Requested access type not permitted by page permissions",
            Self::ExternalAbort => "External abort during descriptor fetch",
            Self::TLBConflictAbort => "Conflicting TLB entries detected",
            Self::UnsupportedAtomicUpdate => "Atomic update to translation table not supported",
            Self::AlignmentFault => "Address is not properly aligned",
            Self::OutputAddressRangeFault => "Output address exceeds addressable range",
            Self::BadStreamID => "Invalid or misconfigured StreamID",
            Self::CDFetchFault => "Error fetching Context Descriptor",
            Self::BadCD => "Invalid or misconfigured Context Descriptor",
            Self::WalkEABT => "External abort during page table walk",
            Self::BadSTE => "Invalid or misconfigured Stream Table Entry",
            Self::STEFetchFault => "Error fetching Stream Table Entry",
        }
    }

    /// Check if this is a translation fault
    #[inline]
    #[must_use]
    pub const fn is_translation_fault(self) -> bool {
        matches!(self, Self::TranslationFault)
    }

    /// Check if this is a permission-related fault
    #[inline]
    #[must_use]
    pub const fn is_permission_fault(self) -> bool {
        matches!(self, Self::PermissionFault | Self::AccessFlagFault)
    }

    /// Check if this is a configuration fault
    #[inline]
    #[must_use]
    pub const fn is_configuration_fault(self) -> bool {
        matches!(
            self,
            Self::BadStreamID | Self::CDFetchFault | Self::BadCD | Self::BadSTE | Self::STEFetchFault
        )
    }

    /// Check if this is an address-related fault
    #[inline]
    #[must_use]
    pub const fn is_address_fault(self) -> bool {
        matches!(
            self,
            Self::AddressSizeFault | Self::AlignmentFault | Self::OutputAddressRangeFault
        )
    }

    /// Check if this is an external fault
    #[inline]
    #[must_use]
    pub const fn is_external_fault(self) -> bool {
        matches!(self, Self::ExternalAbort | Self::WalkEABT)
    }

    /// Get fault severity
    #[must_use]
    pub const fn severity(self) -> FaultSeverity {
        match self {
            Self::BadSTE | Self::BadCD | Self::BadStreamID => FaultSeverity::Critical,
            Self::TranslationFault
            | Self::PermissionFault
            | Self::ExternalAbort
            | Self::AddressSizeFault
            | Self::AlignmentFault
            | Self::OutputAddressRangeFault
            | Self::CDFetchFault
            | Self::STEFetchFault
            | Self::WalkEABT
            | Self::UnsupportedAtomicUpdate => FaultSeverity::Error,
            Self::AccessFlagFault | Self::TLBConflictAbort => FaultSeverity::Warning,
        }
    }

    /// Check if fault is potentially recoverable
    #[must_use]
    pub const fn is_recoverable(self) -> bool {
        matches!(self, Self::AccessFlagFault | Self::TLBConflictAbort)
    }

    /// Check if fault can occur in Stage 1
    #[must_use]
    pub const fn can_occur_in_stage1(self) -> bool {
        !matches!(self, Self::BadSTE | Self::STEFetchFault)
    }

    /// Check if fault can occur in Stage 2
    #[must_use]
    pub const fn can_occur_in_stage2(self) -> bool {
        !matches!(
            self,
            Self::BadStreamID | Self::BadSTE | Self::STEFetchFault | Self::BadCD | Self::CDFetchFault
        )
    }

    /// Check if fault is stage-agnostic
    #[must_use]
    pub const fn is_stage_agnostic(self) -> bool {
        matches!(
            self,
            Self::BadStreamID | Self::BadSTE | Self::ExternalAbort | Self::STEFetchFault
        )
    }

    /// Create from fault code
    pub const fn from_code(code: u8) -> Result<Self, ValidationError> {
        match code {
            0x01 => Ok(Self::TranslationFault),
            0x02 => Ok(Self::AddressSizeFault),
            0x03 => Ok(Self::AccessFlagFault),
            0x04 => Ok(Self::PermissionFault),
            0x05 => Ok(Self::ExternalAbort),
            0x06 => Ok(Self::TLBConflictAbort),
            0x07 => Ok(Self::UnsupportedAtomicUpdate),
            0x08 => Ok(Self::AlignmentFault),
            0x09 => Ok(Self::OutputAddressRangeFault),
            0x0A => Ok(Self::BadStreamID),
            0x0B => Ok(Self::CDFetchFault),
            0x0C => Ok(Self::BadCD),
            0x0D => Ok(Self::WalkEABT),
            0x0E => Ok(Self::BadSTE),
            0x0F => Ok(Self::STEFetchFault),
            _ => Err(ValidationError::InvalidFaultType { code }),
        }
    }
}

impl fmt::Display for FaultType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Fault context information
#[derive(Debug, Clone)]
pub struct FaultContext {
    fault_type: FaultType,
    stream_id: StreamID,
    pasid: PASID,
    address: IOVA,
}

impl FaultContext {
    /// Create a new fault context
    #[must_use]
    pub const fn new(fault_type: FaultType, stream_id: StreamID, pasid: PASID, address: IOVA) -> Self {
        Self {
            fault_type,
            stream_id,
            pasid,
            address,
        }
    }

    /// Get the fault type
    #[must_use]
    pub const fn fault_type(&self) -> FaultType {
        self.fault_type
    }

    /// Get the stream ID
    #[must_use]
    pub const fn stream_id(&self) -> StreamID {
        self.stream_id
    }

    /// Get the `PASID`
    #[must_use]
    pub const fn pasid(&self) -> PASID {
        self.pasid
    }

    /// Get the address
    #[must_use]
    pub const fn address(&self) -> IOVA {
        self.address
    }
}
