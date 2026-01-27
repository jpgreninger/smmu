//! Command queue types for ARM SMMU v3
//!
//! Command queue processing per ARM SMMU v3 specification Section 6.4.

use crate::types::{StreamID, IOVA, PASID};

/// Command type enumeration
///
/// Defines all ARM SMMU v3 command types supported by the command queue.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum CommandType {
    /// Prefetch configuration
    PrefetchConfig = 0,
    /// Prefetch address
    PrefetchAddr = 1,
    /// Stream Table Entry invalidation
    CfgiSte = 2,
    /// All configuration invalidation
    CfgiAll = 3,
    /// TLB invalidation non-secure hyp all
    TlbiNhAll = 4,
    /// TLB invalidation EL2 all
    TlbiEl2All = 5,
    /// TLB invalidation stage 1&2 VM all
    TlbiS12Vmall = 6,
    /// Address Translation Cache invalidation
    AtcInv = 7,
    /// Page Request Interface response
    PriResp = 8,
    /// Resume processing
    Resume = 9,
    /// Synchronization barrier
    Sync = 10,
}

impl Default for CommandType {
    fn default() -> Self {
        Self::Sync
    }
}

/// Command entry structure
///
/// Contains all information about a single command in the command queue.
/// Follows ARM SMMU v3 command format.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CommandEntry {
    /// Command type
    pub cmd_type: CommandType,
    /// Target stream identifier (raw u32 for simpler access)
    pub stream_id: u32,
    /// Target Process Address Space ID (raw u32 for simpler access)
    pub pasid: u32,
    /// Start address for range operations (raw u64 for simpler access)
    pub start_address: u64,
    /// End address for range operations (raw u64 for simpler access)
    pub end_address: u64,
    /// Command-specific flags
    pub flags: u32,
    /// Command timestamp
    pub timestamp: u64,
}

impl CommandEntry {
    /// Create a new command entry
    pub const fn new(cmd_type: CommandType, stream_id: u32, pasid: u32) -> Self {
        Self {
            cmd_type,
            stream_id,
            pasid,
            start_address: 0,
            end_address: 0,
            flags: 0,
            timestamp: 0,
        }
    }
}
