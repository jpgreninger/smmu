//! Queue statistics for ARM SMMU v3
//!
//! Runtime statistics for queue monitoring.

/// Queue statistics structure
///
/// Provides runtime statistics for all SMMU queues.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    #[must_use]
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
    #[must_use]
    pub const fn event_queue_size(&self) -> u64 {
        self.event_queue_size
    }

    /// Get command queue size
    #[must_use]
    pub const fn command_queue_size(&self) -> u64 {
        self.command_queue_size
    }

    /// Get PRI queue size
    #[must_use]
    pub const fn pri_queue_size(&self) -> u64 {
        self.pri_queue_size
    }

    /// Get event queue utilization (0.0 - 1.0)
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn event_queue_utilization(&self) -> f64 {
        if self.event_queue_capacity == 0 {
            0.0
        } else {
            self.event_queue_size as f64 / self.event_queue_capacity as f64
        }
    }

    /// Get command queue utilization (0.0 - 1.0)
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn command_queue_utilization(&self) -> f64 {
        if self.command_queue_capacity == 0 {
            0.0
        } else {
            self.command_queue_size as f64 / self.command_queue_capacity as f64
        }
    }

    /// Get PRI queue utilization (0.0 - 1.0)
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn pri_queue_utilization(&self) -> f64 {
        if self.pri_queue_capacity == 0 {
            0.0
        } else {
            self.pri_queue_size as f64 / self.pri_queue_capacity as f64
        }
    }
}
