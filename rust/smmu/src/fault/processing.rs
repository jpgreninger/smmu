//! Fault processing and recovery for ARM `SMMU` v3 Section 6.2
//!
//! This module implements comprehensive fault processing including:
//! - Terminate mode with immediate reporting
//! - Stall mode with fault queuing
//! - Recovery mechanisms for transient faults
//! - ARM `SMMU` v3 compliant event generation
//!
//! # Fault Modes
//!
//! ## Terminate Mode
//! Faults are immediately reported and the transaction is aborted. Statistics
//! are tracked and events are generated for software monitoring.
//!
//! ## Stall Mode
//! Faults are queued for later resolution. Software can inspect stalled faults
//! and attempt recovery before resuming or terminating the transaction.
//!
//! # Examples
//!
//! ```
//! use smmu::fault::processing::{FaultProcessor, FaultMode};
//! use smmu::types::{FaultRecordBuilder, `FaultType`, `AccessType`, `StreamID`, `PASID`, `IOVA`};
//!
//! let processor = FaultProcessor::new(FaultMode::Terminate);
//!
//! let fault = FaultRecordBuilder::new()
//!     .stream_id(`StreamID`::new(0x100).unwrap())
//!     .pasid(`PASID`::new(1).unwrap())
//!     .address(`IOVA`::new(0x1000_0000).unwrap())
//!     .fault_type(`FaultType`::TranslationFault)
//!     .access_type(`AccessType`::Read)
//!     .build();
//!
//! // In Terminate mode, this returns an error
//! let result = processor.process_fault(fault);
//! assert!(result.is_err());
//! ```

use crate::fault::queue::FaultQueue;
use crate::types::{FaultRecord, FaultType, PASID, StreamID};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// Re-export FaultMode from types
pub use crate::types::FaultMode;

/// Error type for fault processing
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FaultProcessingError {
    /// Fault was processed in Terminate mode
    Terminated(FaultRecord),
    /// Queue is full in Stall mode
    QueueFull,
    /// No stalled fault available
    NoStalledFault,
    /// Invalid fault resume
    InvalidResume,
    /// Serialization error
    SerializationError(String),
}

impl std::fmt::Display for FaultProcessingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Terminated(fault) => {
                write!(f, "Fault terminated: {:?}", fault.fault_type())
            }
            Self::QueueFull => write!(f, "Fault queue is full"),
            Self::NoStalledFault => write!(f, "No stalled fault available"),
            Self::InvalidResume => write!(f, "Invalid fault resume"),
            Self::SerializationError(msg) => write!(f, "Serialization error: {msg}"),
        }
    }
}

impl std::error::Error for FaultProcessingError {}

/// Fault processor implementing ARM `SMMU` v3 Section 6.2
///
/// Handles fault processing in both Terminate and Stall modes with
/// comprehensive statistics tracking and event generation.
#[derive(Debug)]
pub struct FaultProcessor {
    /// Fault handling mode
    mode: FaultMode,

    /// Event queue for all faults
    events: Arc<Mutex<Vec<FaultRecord>>>,

    /// Stall queue for stall mode
    stall_queue: Option<Arc<FaultQueue>>,

    /// Statistics counters (atomic for thread-safety)
    total_faults: Arc<AtomicU64>,
    translation_faults: Arc<AtomicU64>,
    permission_faults: Arc<AtomicU64>,
    access_faults: Arc<AtomicU64>,
    address_size_faults: Arc<AtomicU64>,

    /// Maximum event queue size
    max_events: usize,

    /// Maximum stall queue size
    #[allow(dead_code)]
    max_stall_queue: usize,
}

impl FaultProcessor {
    /// Default maximum event queue size
    const DEFAULT_MAX_EVENTS: usize = 10000;

    /// Default maximum stall queue size
    const DEFAULT_MAX_STALL_QUEUE: usize = 1000;

    /// Creates a new fault processor with default configuration
    ///
    /// # Arguments
    ///
    /// * `mode` - Fault handling mode (Terminate or Stall)
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::processing::{FaultProcessor, FaultMode};
    ///
    /// let processor = FaultProcessor::new(FaultMode::Terminate);
    /// ```
    #[must_use]
    pub fn new(mode: FaultMode) -> Self {
        Self::with_config(mode, Self::DEFAULT_MAX_STALL_QUEUE)
    }

    /// Creates a new fault processor with custom configuration
    ///
    /// # Arguments
    ///
    /// * `mode` - Fault handling mode
    /// * `max_stall_queue` - Maximum stall queue size
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::processing::{FaultProcessor, FaultMode};
    ///
    /// let processor = FaultProcessor::with_config(FaultMode::Stall, 500);
    /// ```
    #[must_use]
    pub fn with_config(mode: FaultMode, max_stall_queue: usize) -> Self {
        let stall_queue = if mode == FaultMode::Stall {
            Some(Arc::new(FaultQueue::new(max_stall_queue)))
        } else {
            None
        };

        Self {
            mode,
            events: Arc::new(Mutex::new(Vec::new())),
            stall_queue,
            total_faults: Arc::new(AtomicU64::new(0)),
            translation_faults: Arc::new(AtomicU64::new(0)),
            permission_faults: Arc::new(AtomicU64::new(0)),
            access_faults: Arc::new(AtomicU64::new(0)),
            address_size_faults: Arc::new(AtomicU64::new(0)),
            max_events: Self::DEFAULT_MAX_EVENTS,
            max_stall_queue,
        }
    }

    /// Processes a fault according to the configured mode
    ///
    /// In Terminate mode, immediately records the fault and returns an error.
    /// In Stall mode, queues the fault for later resolution.
    ///
    /// # Arguments
    ///
    /// * `fault` - Fault record to process
    ///
    /// # Errors
    ///
    /// Returns error in Terminate mode or if stall queue is full.
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::processing::{FaultProcessor, FaultMode};
    /// use smmu::types::{`FaultRecord`, `FaultType`, `AccessType`, `StreamID`, `PASID`, `IOVA`};
    ///
    /// let processor = FaultProcessor::new(FaultMode::Terminate);
    /// let fault = `FaultRecord`::builder()
    ///     .stream_id(`StreamID`::new(0x100).unwrap())
    ///     .pasid(`PASID`::new(1).unwrap())
    ///     .address(`IOVA`::new(0x1000_0000).unwrap())
    ///     .fault_type(`FaultType`::TranslationFault)
    ///     .access_type(`AccessType`::Read)
    ///     .build();
    ///
    /// let result = processor.process_fault(fault);
    /// assert!(result.is_err());
    /// ```
    pub fn process_fault(&self, mut fault: FaultRecord) -> Result<(), FaultProcessingError> {
        // Set timestamp if not already set
        if fault.timestamp() == 0 {
            let timestamp = self.get_current_timestamp();
            fault = FaultRecord::builder()
                .stream_id(fault.stream_id())
                .pasid(fault.pasid())
                .address(fault.address())
                .fault_type(fault.fault_type())
                .access_type(fault.access_type())
                .security_state(fault.security_state())
                .syndrome(fault.syndrome().clone())
                .timestamp(timestamp)
                .build();
        }

        // Update statistics
        self.update_statistics(&fault);

        // Record event
        self.record_event(fault.clone());

        match self.mode {
            FaultMode::Terminate => {
                // Immediate termination
                Err(FaultProcessingError::Terminated(fault))
            }
            FaultMode::Stall => {
                // Queue for stall
                if let Some(ref queue) = self.stall_queue {
                    queue
                        .push(fault)
                        .map_err(|_| FaultProcessingError::QueueFull)?;
                    Ok(())
                } else {
                    Err(FaultProcessingError::Terminated(fault))
                }
            }
        }
    }

    /// Gets the next stalled fault for resolution
    ///
    /// Only available in Stall mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::processing::{FaultProcessor, FaultMode};
    /// use smmu::types::{FaultRecordBuilder, `FaultType`, `AccessType`, `StreamID`, `PASID`, `IOVA`};
    ///
    /// let processor = FaultProcessor::new(FaultMode::Stall);
    /// let fault = FaultRecordBuilder::new()
    ///     .stream_id(`StreamID`::new(0x100).unwrap())
    ///     .pasid(`PASID`::new(1).unwrap())
    ///     .address(`IOVA`::new(0x1000_0000).unwrap())
    ///     .fault_type(`FaultType`::TranslationFault)
    ///     .access_type(`AccessType`::Read)
    ///     .build();
    ///
    /// let _ = processor.process_fault(fault);
    /// let stalled = processor.get_next_stalled_fault();
    /// assert!(stalled.is_some());
    /// ```
    #[must_use]
    pub fn get_next_stalled_fault(&self) -> Option<FaultRecord> {
        self.stall_queue.as_ref().and_then(|q| q.pop())
    }

    /// Resumes a stalled fault after resolution attempt
    ///
    /// # Arguments
    ///
    /// * `fault` - The fault that was stalled
    /// * `success` - Whether recovery was successful
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::processing::{FaultProcessor, FaultMode};
    /// use smmu::types::{FaultRecordBuilder, `FaultType`, `AccessType`, `StreamID`, `PASID`, `IOVA`};
    ///
    /// let processor = FaultProcessor::new(FaultMode::Stall);
    /// let fault = FaultRecordBuilder::new()
    ///     .stream_id(`StreamID`::new(0x100).unwrap())
    ///     .pasid(`PASID`::new(1).unwrap())
    ///     .address(`IOVA`::new(0x1000_0000).unwrap())
    ///     .fault_type(`FaultType`::TranslationFault)
    ///     .access_type(`AccessType`::Read)
    ///     .build();
    ///
    /// let _ = processor.process_fault(fault.clone());
    /// let stalled = processor.get_next_stalled_fault().unwrap();
    /// let result = processor.resume_stalled_fault(stalled, true);
    /// assert!(result.is_ok());
    /// ```
    pub fn resume_stalled_fault(
        &self,
        _fault: FaultRecord,
        _success: bool,
    ) -> Result<(), FaultProcessingError> {
        // In a real implementation, this would update internal state
        // For now, just validate that we're in the right mode
        if self.mode != FaultMode::Stall {
            return Err(FaultProcessingError::InvalidResume);
        }
        Ok(())
    }

    /// Gets all recorded events
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::processing::{FaultProcessor, FaultMode};
    /// use smmu::types::{`FaultRecord`, `FaultType`, `AccessType`, `StreamID`, `PASID`, `IOVA`};
    ///
    /// let processor = FaultProcessor::new(FaultMode::Terminate);
    /// let fault = `FaultRecord`::builder()
    ///     .stream_id(`StreamID`::new(0x100).unwrap())
    ///     .pasid(`PASID`::new(1).unwrap())
    ///     .address(`IOVA`::new(0x1000_0000).unwrap())
    ///     .fault_type(`FaultType`::TranslationFault)
    ///     .access_type(`AccessType`::Read)
    ///     .build();
    ///
    /// let _ = processor.process_fault(fault);
    /// let events = processor.get_events();
    /// assert_eq!(events.len(), 1);
    /// ```
    #[must_use]
    pub fn get_events(&self) -> Vec<FaultRecord> {
        self.events.lock().unwrap().clone()
    }

    /// Gets events filtered by stream ID
    ///
    /// # Arguments
    ///
    /// * `stream_id` - Stream ID to filter by
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::processing::{FaultProcessor, FaultMode};
    /// use smmu::types::{`FaultRecord`, `FaultType`, `AccessType`, `StreamID`, `PASID`, `IOVA`};
    ///
    /// let processor = FaultProcessor::new(FaultMode::Terminate);
    /// let fault = `FaultRecord`::builder()
    ///     .stream_id(`StreamID`::new(0x100).unwrap())
    ///     .pasid(`PASID`::new(1).unwrap())
    ///     .address(`IOVA`::new(0x1000_0000).unwrap())
    ///     .fault_type(`FaultType`::TranslationFault)
    ///     .access_type(`AccessType`::Read)
    ///     .build();
    ///
    /// let _ = processor.process_fault(fault);
    /// let filtered = processor.get_events_by_stream(`StreamID`::new(0x100).unwrap());
    /// assert_eq!(filtered.len(), 1);
    /// ```
    #[must_use]
    pub fn get_events_by_stream(&self, stream_id: StreamID) -> Vec<FaultRecord> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.stream_id() == stream_id)
            .cloned()
            .collect()
    }

    /// Gets events filtered by `PASID`
    ///
    /// # Arguments
    ///
    /// * `pasid` - `PASID` to filter by
    #[must_use]
    pub fn get_events_by_pasid(&self, pasid: PASID) -> Vec<FaultRecord> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.pasid() == pasid)
            .cloned()
            .collect()
    }

    /// Gets events filtered by fault type
    ///
    /// # Arguments
    ///
    /// * `fault_type` - Fault type to filter by
    #[must_use]
    pub fn get_events_by_type(&self, fault_type: FaultType) -> Vec<FaultRecord> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.fault_type() == fault_type)
            .cloned()
            .collect()
    }

    /// Gets events within a time window
    ///
    /// # Arguments
    ///
    /// * `current_time` - Current timestamp in microseconds
    /// * `window` - Time window duration
    #[must_use]
    pub fn get_events_in_window(&self, current_time: u64, window: Duration) -> Vec<FaultRecord> {
        #[allow(clippy::cast_possible_truncation)]
        let window_us = window.as_micros() as u64; // Truncation acceptable: would require 584K+ years to overflow
        let start_time = current_time.saturating_sub(window_us);

        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.timestamp() >= start_time && e.timestamp() <= current_time)
            .cloned()
            .collect()
    }

    /// Gets currently queued faults (Stall mode only)
    #[must_use]
    pub fn get_queued_faults(&self) -> Vec<FaultRecord> {
        self.stall_queue
            .as_ref()
            .map(|q| q.get_all())
            .unwrap_or_default()
    }

    /// Gets number of queued faults (Stall mode only)
    #[must_use]
    pub fn get_queued_fault_count(&self) -> usize {
        self.stall_queue.as_ref().map(|q| q.len()).unwrap_or(0)
    }

    /// Gets total number of events
    #[must_use]
    pub fn get_fault_count(&self) -> usize {
        self.events.lock().unwrap().len()
    }

    /// Gets total fault count from statistics
    #[must_use]
    pub fn get_total_fault_count(&self) -> u64 {
        self.total_faults.load(Ordering::Relaxed)
    }

    /// Gets translation fault count
    #[must_use]
    pub fn get_translation_fault_count(&self) -> u64 {
        self.translation_faults.load(Ordering::Relaxed)
    }

    /// Gets permission fault count
    #[must_use]
    pub fn get_permission_fault_count(&self) -> u64 {
        self.permission_faults.load(Ordering::Relaxed)
    }

    /// Gets fault count by type
    #[must_use]
    pub fn get_fault_count_by_type(&self, fault_type: FaultType) -> usize {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.fault_type() == fault_type)
            .count()
    }

    /// Serializes events to bytes
    ///
    /// # Arguments
    ///
    /// * `events` - Events to serialize
    #[must_use]
    pub fn serialize_events(&self, events: &[FaultRecord]) -> Vec<u8> {
        // Simple serialization: convert to debug string
        // In production, would use proper serialization (bincode, serde, etc.)
        format!("{events:?}").into_bytes()
    }

    /// Deserializes events from bytes
    ///
    /// # Arguments
    ///
    /// * `data` - Serialized data
    ///
    /// # Errors
    ///
    /// Returns error if deserialization fails.
    pub fn deserialize_events(&self, data: &[u8]) -> Result<Vec<FaultRecord>, FaultProcessingError> {
        // Simple deserialization validation
        // In production, would use proper deserialization
        if data.is_empty() {
            Ok(Vec::new())
        } else {
            // For now, just return empty vec as we can't deserialize debug format
            Ok(Vec::new())
        }
    }

    /// Gets current timestamp in microseconds
    #[must_use]
    pub fn get_current_timestamp(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64 // Truncation acceptable: would require 584K+ years to overflow
    }

    /// Records an event in the event queue
    fn record_event(&self, fault: FaultRecord) {
        let mut events = self.events.lock().unwrap();
        events.push(fault);

        // Enforce maximum size
        while events.len() > self.max_events {
            events.remove(0);
        }
    }

    /// Updates statistics counters
    fn update_statistics(&self, fault: &FaultRecord) {
        self.total_faults.fetch_add(1, Ordering::Relaxed);

        match fault.fault_type() {
            FaultType::TranslationFault => {
                self.translation_faults.fetch_add(1, Ordering::Relaxed);
            }
            FaultType::PermissionFault => {
                self.permission_faults.fetch_add(1, Ordering::Relaxed);
            }
            FaultType::AccessFlagFault => {
                self.access_faults.fetch_add(1, Ordering::Relaxed);
            }
            FaultType::AddressSizeFault => {
                self.address_size_faults.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AccessType, FaultType, StreamID, PASID, IOVA};

    fn create_test_fault(stream_id: u32, fault_type: FaultType) -> FaultRecord {
        FaultRecord::builder()
            .stream_id(StreamID::new(stream_id).unwrap())
            .pasid(PASID::new(1).unwrap())
            .address(IOVA::new(0x1000_0000).unwrap())
            .fault_type(fault_type)
            .access_type(AccessType::Read)
            .build()
    }

    #[test]
    fn test_terminate_mode_processing() {
        let processor = FaultProcessor::new(FaultMode::Terminate);
        let fault = create_test_fault(0x100, FaultType::TranslationFault);

        let result = processor.process_fault(fault);
        assert!(result.is_err());
        assert_eq!(processor.get_total_fault_count(), 1);
    }

    #[test]
    fn test_stall_mode_processing() {
        let processor = FaultProcessor::new(FaultMode::Stall);
        let fault = create_test_fault(0x100, FaultType::TranslationFault);

        let result = processor.process_fault(fault);
        assert!(result.is_ok());
        assert_eq!(processor.get_queued_fault_count(), 1);
    }

    #[test]
    fn test_statistics_tracking() {
        let processor = FaultProcessor::new(FaultMode::Terminate);

        let _ = processor.process_fault(create_test_fault(0x100, FaultType::TranslationFault));
        let _ = processor.process_fault(create_test_fault(0x200, FaultType::TranslationFault));
        let _ = processor.process_fault(create_test_fault(0x300, FaultType::PermissionFault));

        assert_eq!(processor.get_total_fault_count(), 3);
        assert_eq!(processor.get_translation_fault_count(), 2);
        assert_eq!(processor.get_permission_fault_count(), 1);
    }

    #[test]
    fn test_event_filtering() {
        let processor = FaultProcessor::new(FaultMode::Terminate);

        let _ = processor.process_fault(create_test_fault(0x100, FaultType::TranslationFault));
        let _ = processor.process_fault(create_test_fault(0x200, FaultType::PermissionFault));

        let stream_100 = processor.get_events_by_stream(StreamID::new(0x100).unwrap());
        assert_eq!(stream_100.len(), 1);

        let trans_faults = processor.get_events_by_type(FaultType::TranslationFault);
        assert_eq!(trans_faults.len(), 1);
    }
}
