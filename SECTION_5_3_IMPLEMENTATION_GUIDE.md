# ARM SMMU v3 Section 5.3 Implementation Guide

**Quick Reference for rust-engineer**

This guide provides the exact type definitions and API signatures needed to make the Section 5.3 test suite compile and pass.

## Phase 1: Type Definitions (2 hours)

### File: `rust/smmu/src/types/event.rs`
```rust
//! Event queue types for ARM SMMU v3
//!
//! Event queue management per ARM SMMU v3 specification Section 6.3.

use crate::types::{StreamID, PASID, IOVA, SecurityState};

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

/// Event entry structure
///
/// Contains all information about a single event in the event queue.
/// Follows ARM SMMU v3 event record format.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct EventEntry {
    /// Type of event
    pub event_type: EventType,
    /// Source stream identifier
    pub stream_id: StreamID,
    /// Process Address Space ID
    pub pasid: PASID,
    /// Faulting or relevant address
    pub address: IOVA,
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
        stream_id: StreamID,
        pasid: PASID,
        address: IOVA,
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
```

### File: `rust/smmu/src/types/command.rs`
```rust
//! Command queue types for ARM SMMU v3
//!
//! Command queue processing per ARM SMMU v3 specification Section 6.4.

use crate::types::{StreamID, PASID, IOVA};

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

/// Command entry structure
///
/// Contains all information about a single command in the command queue.
/// Follows ARM SMMU v3 command format.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CommandEntry {
    /// Command type
    pub cmd_type: CommandType,
    /// Target stream identifier
    pub stream_id: StreamID,
    /// Target Process Address Space ID
    pub pasid: PASID,
    /// Start address for range operations
    pub start_address: IOVA,
    /// End address for range operations
    pub end_address: IOVA,
    /// Command-specific flags
    pub flags: u32,
    /// Command timestamp
    pub timestamp: u64,
}

impl CommandEntry {
    /// Create a new command entry
    pub const fn new(cmd_type: CommandType, stream_id: StreamID, pasid: PASID) -> Self {
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
```

### File: `rust/smmu/src/types/pri.rs`
```rust
//! Page Request Interface types for ARM SMMU v3
//!
//! PRI queue management per ARM SMMU v3 specification Section 7.

use crate::types::{StreamID, PASID, IOVA, AccessType};

/// Page Request Interface entry
///
/// Contains information about a page request in the PRI queue.
/// Follows ARM SMMU v3 PRI format.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PRIEntry {
    /// Source stream identifier
    pub stream_id: StreamID,
    /// Process Address Space ID
    pub pasid: PASID,
    /// Requested page address
    pub requested_address: IOVA,
    /// Access type requested (Read/Write/Execute)
    pub access_type: AccessType,
    /// True if this is the last request in a group
    pub is_last_request: bool,
    /// Request timestamp
    pub timestamp: u64,
}

impl PRIEntry {
    /// Create a new PRI entry
    pub const fn new(
        stream_id: StreamID,
        pasid: PASID,
        requested_address: IOVA,
        access_type: AccessType,
    ) -> Self {
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
```

### File: `rust/smmu/src/types/queue_config.rs`
```rust
//! Queue configuration for ARM SMMU v3
//!
//! Configuration structures for event, command, and PRI queues.

/// Queue configuration structure
///
/// Defines capacity limits for all SMMU queues.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueConfig {
    /// Event queue capacity (minimum 512 per ARM spec)
    event_queue_size: usize,
    /// Command queue capacity (minimum 256 per ARM spec)
    command_queue_size: usize,
    /// PRI queue capacity (minimum 128 per ARM spec)
    pri_queue_size: usize,
}

impl QueueConfig {
    /// ARM SMMU v3 minimum event queue size
    pub const MIN_EVENT_QUEUE_SIZE: usize = 512;
    /// ARM SMMU v3 minimum command queue size
    pub const MIN_COMMAND_QUEUE_SIZE: usize = 256;
    /// ARM SMMU v3 minimum PRI queue size
    pub const MIN_PRI_QUEUE_SIZE: usize = 128;

    /// Create a new queue configuration
    pub const fn new(
        event_queue_size: usize,
        command_queue_size: usize,
        pri_queue_size: usize,
    ) -> Self {
        Self {
            event_queue_size,
            command_queue_size,
            pri_queue_size,
        }
    }

    /// Get event queue size
    pub const fn event_queue_size(&self) -> usize {
        self.event_queue_size
    }

    /// Get command queue size
    pub const fn command_queue_size(&self) -> usize {
        self.command_queue_size
    }

    /// Get PRI queue size
    pub const fn pri_queue_size(&self) -> usize {
        self.pri_queue_size
    }

    /// Set event queue size
    pub fn with_event_queue_size(mut self, size: usize) -> Self {
        self.event_queue_size = size;
        self
    }

    /// Set command queue size
    pub fn with_command_queue_size(mut self, size: usize) -> Self {
        self.command_queue_size = size;
        self
    }

    /// Set PRI queue size
    pub fn with_pri_queue_size(mut self, size: usize) -> Self {
        self.pri_queue_size = size;
        self
    }
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            event_queue_size: Self::MIN_EVENT_QUEUE_SIZE,
            command_queue_size: Self::MIN_COMMAND_QUEUE_SIZE,
            pri_queue_size: Self::MIN_PRI_QUEUE_SIZE,
        }
    }
}

impl From<QueueConfig> for crate::types::SMMUConfig {
    fn from(queue_config: QueueConfig) -> Self {
        crate::types::SMMUConfig::default()
            // Add queue config to SMMUConfig
            // This will require extending SMMUConfig struct
    }
}
```

### File: `rust/smmu/src/types/queue_statistics.rs`
```rust
//! Queue statistics for ARM SMMU v3
//!
//! Runtime statistics for queue monitoring.

/// Queue statistics structure
///
/// Provides runtime statistics for all SMMU queues.
#[derive(Clone, Debug, Default)]
pub struct QueueStatistics {
    /// Current event queue size
    event_queue_size: u64,
    /// Current command queue size
    command_queue_size: u64,
    /// Current PRI queue size
    pri_queue_size: u64,
    /// Event queue capacity
    event_queue_capacity: usize,
    /// Command queue capacity
    command_queue_capacity: usize,
    /// PRI queue capacity
    pri_queue_capacity: usize,
}

impl QueueStatistics {
    /// Create new queue statistics
    pub const fn new(
        event_queue_size: u64,
        command_queue_size: u64,
        pri_queue_size: u64,
        event_queue_capacity: usize,
        command_queue_capacity: usize,
        pri_queue_capacity: usize,
    ) -> Self {
        Self {
            event_queue_size,
            command_queue_size,
            pri_queue_size,
            event_queue_capacity,
            command_queue_capacity,
            pri_queue_capacity,
        }
    }

    /// Get event queue size
    pub const fn event_queue_size(&self) -> u64 {
        self.event_queue_size
    }

    /// Get command queue size
    pub const fn command_queue_size(&self) -> u64 {
        self.command_queue_size
    }

    /// Get PRI queue size
    pub const fn pri_queue_size(&self) -> u64 {
        self.pri_queue_size
    }

    /// Get event queue utilization (0.0 - 1.0)
    pub fn event_queue_utilization(&self) -> f64 {
        if self.event_queue_capacity == 0 {
            0.0
        } else {
            self.event_queue_size as f64 / self.event_queue_capacity as f64
        }
    }

    /// Get command queue utilization (0.0 - 1.0)
    pub fn command_queue_utilization(&self) -> f64 {
        if self.command_queue_capacity == 0 {
            0.0
        } else {
            self.command_queue_size as f64 / self.command_queue_capacity as f64
        }
    }

    /// Get PRI queue utilization (0.0 - 1.0)
    pub fn pri_queue_utilization(&self) -> f64 {
        if self.pri_queue_capacity == 0 {
            0.0
        } else {
            self.pri_queue_size as f64 / self.pri_queue_capacity as f64
        }
    }
}
```

### Update: `rust/smmu/src/types/mod.rs`
```rust
// Add new module declarations
mod event;
mod command;
mod pri;
mod queue_config;
mod queue_statistics;

// Add new exports
pub use event::{EventType, EventEntry};
pub use command::{CommandType, CommandEntry};
pub use pri::PRIEntry;
pub use queue_config::QueueConfig;
pub use queue_statistics::QueueStatistics;
```

### Update: `rust/smmu/src/types/smmu_error.rs`
```rust
// Add new error variants
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SMMUError {
    // ... existing variants ...

    /// Event queue is full
    EventQueueFull,
    /// Command queue is full
    CommandQueueFull,
    /// PRI queue is full
    PriQueueFull,
    /// Invalid command parameters
    InvalidCommandParameters,
}
```

### Update: `rust/smmu/src/cache/mod.rs`
```rust
// Add to CacheStatistics struct
pub struct CacheStatistics {
    // ... existing fields ...

    /// Number of cache invalidation operations performed
    invalidation_count: u64,
}

impl CacheStatistics {
    /// Get invalidation count
    pub const fn invalidation_count(&self) -> u64 {
        self.invalidation_count
    }
}
```

## Phase 2: Event Queue Implementation (8 hours)

### File: `rust/smmu/src/smmu/event_queue.rs`
```rust
//! Event queue implementation for ARM SMMU v3

use crate::types::{EventEntry, EventType, StreamID, SMMUError};
use std::collections::VecDeque;
use std::sync::RwLock;

/// Event queue manager
pub struct EventQueue {
    queue: RwLock<VecDeque<EventEntry>>,
    capacity: usize,
}

impl EventQueue {
    /// Create a new event queue with specified capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: RwLock::new(VecDeque::with_capacity(capacity)),
            capacity,
        }
    }

    /// Submit an event to the queue
    pub fn submit(&self, event: EventEntry) -> Result<(), SMMUError> {
        let mut queue = self.queue.write().unwrap();
        if queue.len() >= self.capacity {
            return Err(SMMUError::EventQueueFull);
        }
        queue.push_back(event);
        Ok(())
    }

    /// Get all events (non-destructive)
    pub fn get_all(&self) -> Vec<EventEntry> {
        let queue = self.queue.read().unwrap();
        queue.iter().copied().collect()
    }

    /// Check if queue has events
    pub fn has_events(&self) -> bool {
        let queue = self.queue.read().unwrap();
        !queue.is_empty()
    }

    /// Get queue size
    pub fn size(&self) -> u64 {
        let queue = self.queue.read().unwrap();
        queue.len() as u64
    }

    /// Clear the queue
    pub fn clear(&self) {
        let mut queue = self.queue.write().unwrap();
        queue.clear();
    }

    /// Get events filtered by type
    pub fn get_by_type(&self, event_type: EventType) -> Vec<EventEntry> {
        let queue = self.queue.read().unwrap();
        queue.iter()
            .filter(|e| e.event_type == event_type)
            .copied()
            .collect()
    }

    /// Get events filtered by stream
    pub fn get_by_stream(&self, stream_id: StreamID) -> Vec<EventEntry> {
        let queue = self.queue.read().unwrap();
        queue.iter()
            .filter(|e| e.stream_id == stream_id)
            .copied()
            .collect()
    }
}
```

### Update: `rust/smmu/src/smmu/mod.rs`
```rust
mod event_queue;
use event_queue::EventQueue;

pub struct SMMU {
    // ... existing fields ...

    /// Event queue (Section 5.3.1)
    event_queue: EventQueue,
}

impl SMMU {
    pub fn new() -> Self {
        let config = SMMUConfig::default();
        Self::with_config(config)
    }

    pub fn with_config(config: SMMUConfig) -> Self {
        let queue_config = config.queue_config();
        Self {
            // ... existing fields ...
            event_queue: EventQueue::new(queue_config.event_queue_size()),
        }
    }

    // Event queue APIs
    pub fn submit_event(&self, event: EventEntry) -> Result<(), SMMUError> {
        self.event_queue.submit(event)
    }

    pub fn get_events(&self) -> Vec<EventEntry> {
        self.event_queue.get_all()
    }

    pub fn get_events_by_type(&self, event_type: EventType) -> Vec<EventEntry> {
        self.event_queue.get_by_type(event_type)
    }

    pub fn get_events_by_stream(&self, stream_id: StreamID) -> Vec<EventEntry> {
        self.event_queue.get_by_stream(stream_id)
    }

    pub fn has_events(&self) -> bool {
        self.event_queue.has_events()
    }

    pub fn get_event_queue_size(&self) -> u64 {
        self.event_queue.size()
    }

    pub fn clear_event_queue(&self) {
        self.event_queue.clear()
    }
}
```

## Phase 3: Command Queue Implementation (10 hours)

Similar structure to event queue, but with command processing logic.

### File: `rust/smmu/src/smmu/command_queue.rs`
```rust
//! Command queue implementation for ARM SMMU v3

use crate::types::{CommandEntry, CommandType, SMMUError};
use std::collections::VecDeque;
use std::sync::RwLock;

pub struct CommandQueue {
    queue: RwLock<VecDeque<CommandEntry>>,
    capacity: usize,
}

impl CommandQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: RwLock::new(VecDeque::with_capacity(capacity)),
            capacity,
        }
    }

    pub fn submit(&self, command: CommandEntry) -> Result<(), SMMUError> {
        // Validate command
        if command.end_address < command.start_address {
            return Err(SMMUError::InvalidCommandParameters);
        }

        let mut queue = self.queue.write().unwrap();
        if queue.len() >= self.capacity {
            return Err(SMMUError::CommandQueueFull);
        }
        queue.push_back(command);
        Ok(())
    }

    pub fn pop(&self) -> Option<CommandEntry> {
        let mut queue = self.queue.write().unwrap();
        queue.pop_front()
    }

    pub fn is_full(&self) -> bool {
        let queue = self.queue.read().unwrap();
        queue.len() >= self.capacity
    }

    pub fn size(&self) -> u64 {
        let queue = self.queue.read().unwrap();
        queue.len() as u64
    }

    pub fn clear(&self) {
        let mut queue = self.queue.write().unwrap();
        queue.clear();
    }
}
```

## Testing After Each Phase

### Phase 1: Types compile
```bash
cargo test test_section_5_3 --no-run
# Expected: Success (tests compile)
```

### Phase 2: Event queue tests pass
```bash
cargo test test_section_5_3_1
# Expected: 9 tests pass
```

### Phase 3: Command queue tests pass
```bash
cargo test test_section_5_3_2
# Expected: 8 tests pass
```

## Quick Checklist

- [ ] Phase 1: Create event.rs, command.rs, pri.rs, queue_config.rs, queue_statistics.rs
- [ ] Phase 1: Update types/mod.rs with exports
- [ ] Phase 1: Add SMMUError variants
- [ ] Phase 1: Tests compile successfully
- [ ] Phase 2: Create smmu/event_queue.rs
- [ ] Phase 2: Update smmu/mod.rs with event queue APIs
- [ ] Phase 2: Tests 5.3.1 pass (9 tests)
- [ ] Phase 3: Create smmu/command_queue.rs
- [ ] Phase 3: Update smmu/mod.rs with command queue APIs
- [ ] Phase 3: Tests 5.3.2 pass (8 tests)
- [ ] Phase 4: Create smmu/pri_queue.rs
- [ ] Phase 4: Update smmu/mod.rs with PRI queue APIs
- [ ] Phase 4: Tests 5.3.3 pass (7 tests)
- [ ] Phase 5: Implement cache invalidation handlers
- [ ] Phase 5: Tests 5.3.4 pass (8 tests)
- [ ] Phase 6: Add queue statistics APIs
- [ ] Phase 6: Tests 5.3.5 pass (7 tests)
- [ ] Phase 7: Optimize performance
- [ ] Phase 7: Tests 5.3.6 pass (3 tests)

## Final Verification

```bash
# All tests pass
cargo test test_section_5_3

# Coverage >95%
cargo tarpaulin --test test_queues_section_5_3

# No warnings
cargo clippy --tests

# Documentation complete
cargo doc --no-deps --open
```

---

**rust-engineer:** Use this guide to implement Section 5.3 in TDD phases.
