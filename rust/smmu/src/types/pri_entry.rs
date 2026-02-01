//! Page Request Interface types for ARM `SMMU` v3
//!
//! PRI queue management per ARM `SMMU` v3 specification Section 7.

use crate::types::AccessType;

/// Page Request Interface entry
///
/// Contains information about a page request in the PRI queue.
/// Follows ARM `SMMU` v3 PRI format.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PRIEntry {
    /// Source stream identifier (raw u32 for simpler access)
    pub stream_id: u32,
    /// Process Address Space ID (raw u32 for simpler access)
    pub pasid: u32,
    /// Requested page address (raw u64 for simpler access)
    pub requested_address: u64,
    /// Access type requested (Read/Write/Execute)
    pub access_type: AccessType,
    /// True if this is the last request in a group
    pub is_last_request: bool,
    /// Request timestamp
    pub timestamp: u64,
}

impl PRIEntry {
    /// Create a new PRI entry
    #[must_use]
    pub const fn new(stream_id: u32, pasid: u32, requested_address: u64, access_type: AccessType) -> Self {
        Self {
            stream_id,
            pasid,
            requested_address,
            access_type,
            is_last_request: false,
            timestamp: 0,
        }
    }
}
