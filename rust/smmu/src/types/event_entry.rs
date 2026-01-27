//! Event queue types for ARM SMMU v3
//!
//! Event queue management per ARM SMMU v3 specification Section 6.3.

use crate::types::{SecurityState, StreamID, IOVA, PASID};

/// Event type enumeration
///
/// Defines the types of events that can be queued in the event queue.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EventType {
    /// Translation fault - page not mapped
    TranslationFault = 0,
    /// Permission fault - access denied
    PermissionFault = 1,
    /// Command SYNC completion
    CommandSyncCompletion = 2,
    /// Page Request Interface page request
    PriPageRequest = 3,
    /// ATC invalidation completion
    AtcInvalidateCompletion = 4,
    /// Configuration error
    ConfigurationError = 5,
    /// Internal error
    InternalError = 6,
}

impl Default for EventType {
    fn default() -> Self {
        Self::TranslationFault
    }
}

/// Event entry structure
///
/// Contains all information about a single event in the event queue.
/// Follows ARM SMMU v3 event record format.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct EventEntry {
    /// Type of event
    pub event_type: EventType,
    /// Source stream identifier (raw u32 for simpler access)
    pub stream_id: u32,
    /// Process Address Space ID (raw u32 for simpler access)
    pub pasid: u32,
    /// Faulting or relevant address (raw u64 for simpler access)
    pub address: u64,
    /// Security state context
    pub security_state: SecurityState,
    /// Event-specific error code
    pub error_code: u32,
    /// Event timestamp
    pub timestamp: u64,
}

impl EventEntry {
    /// Create a new event entry
    pub const fn new(
        event_type: EventType,
        stream_id: u32,
        pasid: u32,
        address: u64,
    ) -> Self {
        Self {
            event_type,
            stream_id,
            pasid,
            address,
            security_state: SecurityState::NonSecure,
            error_code: 0,
            timestamp: 0,
        }
    }
}
