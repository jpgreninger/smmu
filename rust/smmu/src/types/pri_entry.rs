//! Page Request Interface types for ARM SMMU v3
//!
//! PRI queue management per ARM SMMU v3 specification Section 7.

use crate::types::AccessType;

/// Page Request Interface entry
///
/// Contains information about a page request in the PRI queue.
/// Follows ARM SMMU v3 PRI format.
///
/// # ARM SMMU v3 Compliance
///
/// The `prg_index` field is the Page Request Group Index (ARM §8.2).  Every
/// page request belongs to a Page Request Group (PRG) identified by this value.
/// Software must echo the same `prg_index` back in `CMD_PRI_RESP` (ARM §8.3)
/// so that the SMMU can locate and retire the pending request.
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
    /// Page Request Group index (ARM §8.2).
    ///
    /// Identifies the Page Request Group (PRG) this entry belongs to.
    /// Software must echo this value in `CMD_PRI_RESP` to complete the
    /// response cycle (ARM §8.3).  Defaults to 0.
    pub prg_index: u16,
}

impl PRIEntry {
    /// Create a new PRI entry
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::types::{AccessType, PRIEntry};
    ///
    /// let entry = PRIEntry::new(1, 0, 0x1000, AccessType::Read);
    /// assert_eq!(entry.prg_index, 0);
    /// ```
    #[must_use]
    pub const fn new(stream_id: u32, pasid: u32, requested_address: u64, access_type: AccessType) -> Self {
        Self {
            stream_id,
            pasid,
            requested_address,
            access_type,
            is_last_request: false,
            timestamp: 0,
            prg_index: 0,
        }
    }

    /// Set the Page Request Group index and return `self` (builder pattern).
    ///
    /// Used to associate this entry with a specific PRG so that the matching
    /// `CMD_PRI_RESP` can retire it (ARM §8.2, §8.3).
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::types::{AccessType, PRIEntry};
    ///
    /// let entry = PRIEntry::new(1, 0, 0x1000, AccessType::Read)
    ///     .with_prg_index(42);
    /// assert_eq!(entry.prg_index, 42);
    /// ```
    #[must_use]
    pub const fn with_prg_index(mut self, prg_index: u16) -> Self {
        self.prg_index = prg_index;
        self
    }
}
