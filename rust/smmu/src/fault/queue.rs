//! Thread-safe fault queue for ARM `SMMU` v3
//!
//! This module implements a thread-safe FIFO queue for fault records using VecDeque
//! with Mutex protection for concurrent access. Used in Stall mode to queue faults
//! for software intervention.
//!
//! # Thread Safety
//!
//! All operations are thread-safe and can be called from multiple threads concurrently.
//! The queue uses `std::sync::Mutex` for synchronization.
//!
//! # Examples
//!
//! ```
//! use smmu::fault::queue::FaultQueue;
//! use smmu::types::{FaultRecordBuilder, `FaultType`, `AccessType`, `StreamID`, `PASID`, `IOVA`};
//!
//! let queue = FaultQueue::new(100);
//!
//! let fault = FaultRecordBuilder::new()
//!     .stream_id(`StreamID`::new(0x100).unwrap())
//!     .pasid(`PASID`::new(1).unwrap())
//!     .address(`IOVA`::new(0x1000_0000).unwrap())
//!     .fault_type(`FaultType`::TranslationFault)
//!     .access_type(`AccessType`::Read)
//!     .build();
//!
//! queue.push(fault).unwrap();
//! assert_eq!(queue.len(), 1);
//!
//! let popped = queue.pop();
//! assert!(popped.is_some());
//! ```

use crate::types::FaultRecord;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Error type for fault queue operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FaultQueueError {
    /// Queue is full and cannot accept more entries
    QueueFull,
    /// Queue is empty, no faults to pop
    QueueEmpty,
    /// Lock poisoned due to panic in another thread
    LockPoisoned,
}

impl std::fmt::Display for FaultQueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::QueueFull => write!(f, "Fault queue is full"),
            Self::QueueEmpty => write!(f, "Fault queue is empty"),
            Self::LockPoisoned => write!(f, "Fault queue lock is poisoned"),
        }
    }
}

impl std::error::Error for FaultQueueError {}

/// Thread-safe FIFO queue for fault records
///
/// Implements efficient fault queuing using VecDeque with O(1) push/pop operations.
/// All methods are thread-safe and can be called concurrently.
#[derive(Debug)]
pub struct FaultQueue {
    /// Internal queue storage
    queue: Arc<Mutex<VecDeque<FaultRecord>>>,
    /// Maximum queue capacity
    capacity: usize,
}

impl FaultQueue {
    /// Creates a new fault queue with specified capacity
    ///
    /// # Arguments
    ///
    /// * `capacity` - Maximum number of faults the queue can hold
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::queue::FaultQueue;
    ///
    /// let queue = FaultQueue::new(1000);
    /// assert_eq!(queue.capacity(), 1000);
    /// ```
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
        }
    }

    /// Pushes a fault record onto the queue
    ///
    /// Returns an error if the queue is full.
    ///
    /// # Arguments
    ///
    /// * `fault` - Fault record to enqueue
    ///
    /// # Errors
    ///
    /// Returns `FaultQueueError::QueueFull` if queue is at capacity.
    /// Returns `FaultQueueError::LockPoisoned` if mutex is poisoned.
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::queue::FaultQueue;
    /// use smmu::types::{`FaultRecord`, `FaultType`, `AccessType`, `StreamID`, `PASID`, `IOVA`};
    ///
    /// let queue = FaultQueue::new(10);
    /// let fault = `FaultRecord`::builder()
    ///     .stream_id(`StreamID`::new(0x100).unwrap())
    ///     .pasid(`PASID`::new(1).unwrap())
    ///     .address(`IOVA`::new(0x1000_0000).unwrap())
    ///     .fault_type(`FaultType`::TranslationFault)
    ///     .access_type(`AccessType`::Read)
    ///     .build();
    ///
    /// assert!(queue.push(fault).is_ok());
    /// ```
    pub fn push(&self, fault: FaultRecord) -> Result<(), FaultQueueError> {
        let mut queue = self.queue.lock().map_err(|_| FaultQueueError::LockPoisoned)?;

        if queue.len() >= self.capacity {
            return Err(FaultQueueError::QueueFull);
        }

        queue.push_back(fault);
        Ok(())
    }

    /// Pops a fault record from the queue (FIFO order)
    ///
    /// Returns `None` if the queue is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::queue::FaultQueue;
    /// use smmu::types::{`FaultRecord`, `FaultType`, `AccessType`, `StreamID`, `PASID`, `IOVA`};
    ///
    /// let queue = FaultQueue::new(10);
    /// let fault = `FaultRecord`::builder()
    ///     .stream_id(`StreamID`::new(0x100).unwrap())
    ///     .pasid(`PASID`::new(1).unwrap())
    ///     .address(`IOVA`::new(0x1000_0000).unwrap())
    ///     .fault_type(`FaultType`::TranslationFault)
    ///     .access_type(`AccessType`::Read)
    ///     .build();
    ///
    /// queue.push(fault.clone()).unwrap();
    /// let popped = queue.pop();
    /// assert!(popped.is_some());
    /// assert_eq!(popped.unwrap().fault_type(), fault.fault_type());
    /// ```
    #[must_use]
    pub fn pop(&self) -> Option<FaultRecord> {
        self.queue.lock().ok()?.pop_front()
    }

    /// Returns the number of faults currently in the queue
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::queue::FaultQueue;
    ///
    /// let queue = FaultQueue::new(10);
    /// assert_eq!(queue.len(), 0);
    /// ```
    #[must_use]
    pub fn len(&self) -> usize {
        self.queue.lock().map(|q| q.len()).unwrap_or(0)
    }

    /// Returns true if the queue is empty
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::queue::FaultQueue;
    ///
    /// let queue = FaultQueue::new(10);
    /// assert!(queue.is_empty());
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns true if the queue is full
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::queue::FaultQueue;
    ///
    /// let queue = FaultQueue::new(1);
    /// assert!(!queue.is_full());
    /// ```
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.len() >= self.capacity
    }

    /// Returns the maximum capacity of the queue
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::queue::FaultQueue;
    ///
    /// let queue = FaultQueue::new(1000);
    /// assert_eq!(queue.capacity(), 1000);
    /// ```
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Clears all faults from the queue
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::queue::FaultQueue;
    /// use smmu::types::{`FaultRecord`, `FaultType`, `AccessType`, `StreamID`, `PASID`, `IOVA`};
    ///
    /// let queue = FaultQueue::new(10);
    /// let fault = `FaultRecord`::builder()
    ///     .stream_id(`StreamID`::new(0x100).unwrap())
    ///     .pasid(`PASID`::new(1).unwrap())
    ///     .address(`IOVA`::new(0x1000_0000).unwrap())
    ///     .fault_type(`FaultType`::TranslationFault)
    ///     .access_type(`AccessType`::Read)
    ///     .build();
    ///
    /// queue.push(fault).unwrap();
    /// assert_eq!(queue.len(), 1);
    ///
    /// queue.clear();
    /// assert!(queue.is_empty());
    /// ```
    pub fn clear(&self) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.clear();
        }
    }

    /// Peeks at the front fault without removing it
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::queue::FaultQueue;
    /// use smmu::types::{`FaultRecord`, `FaultType`, `AccessType`, `StreamID`, `PASID`, `IOVA`};
    ///
    /// let queue = FaultQueue::new(10);
    /// let fault = `FaultRecord`::builder()
    ///     .stream_id(`StreamID`::new(0x100).unwrap())
    ///     .pasid(`PASID`::new(1).unwrap())
    ///     .address(`IOVA`::new(0x1000_0000).unwrap())
    ///     .fault_type(`FaultType`::TranslationFault)
    ///     .access_type(`AccessType`::Read)
    ///     .build();
    ///
    /// queue.push(fault.clone()).unwrap();
    /// let peeked = queue.peek();
    /// assert!(peeked.is_some());
    /// assert_eq!(queue.len(), 1); // Still in queue
    /// ```
    #[must_use]
    pub fn peek(&self) -> Option<FaultRecord> {
        self.queue.lock().ok()?.front().cloned()
    }

    /// Gets all faults as a vector without removing them
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::queue::FaultQueue;
    /// use smmu::types::{`FaultRecord`, `FaultType`, `AccessType`, `StreamID`, `PASID`, `IOVA`};
    ///
    /// let queue = FaultQueue::new(10);
    /// let fault = `FaultRecord`::builder()
    ///     .stream_id(`StreamID`::new(0x100).unwrap())
    ///     .pasid(`PASID`::new(1).unwrap())
    ///     .address(`IOVA`::new(0x1000_0000).unwrap())
    ///     .fault_type(`FaultType`::TranslationFault)
    ///     .access_type(`AccessType`::Read)
    ///     .build();
    ///
    /// queue.push(fault).unwrap();
    /// let all = queue.get_all();
    /// assert_eq!(all.len(), 1);
    /// assert_eq!(queue.len(), 1); // Still in queue
    /// ```
    #[must_use]
    pub fn get_all(&self) -> Vec<FaultRecord> {
        self.queue
            .lock()
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default()
    }
}

impl Clone for FaultQueue {
    fn clone(&self) -> Self {
        let queue = self.queue.lock().unwrap();
        Self {
            queue: Arc::new(Mutex::new(queue.clone())),
            capacity: self.capacity,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AccessType, FaultType, StreamID, PASID, IOVA};

    fn create_test_fault(stream_id: u32) -> FaultRecord {
        FaultRecord::builder()
            .stream_id(StreamID::new(stream_id).unwrap())
            .pasid(PASID::new(1).unwrap())
            .address(IOVA::new(0x1000_0000).unwrap())
            .fault_type(FaultType::TranslationFault)
            .access_type(AccessType::Read)
            .build()
    }

    #[test]
    fn test_new_queue() {
        let queue = FaultQueue::new(100);
        assert_eq!(queue.capacity(), 100);
        assert_eq!(queue.len(), 0);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_push_pop() {
        let queue = FaultQueue::new(10);
        let fault = create_test_fault(0x100);

        assert!(queue.push(fault.clone()).is_ok());
        assert_eq!(queue.len(), 1);

        let popped = queue.pop();
        assert!(popped.is_some());
        assert_eq!(popped.unwrap().stream_id(), fault.stream_id());
        assert!(queue.is_empty());
    }

    #[test]
    fn test_capacity_limit() {
        let queue = FaultQueue::new(2);

        assert!(queue.push(create_test_fault(0x100)).is_ok());
        assert!(queue.push(create_test_fault(0x200)).is_ok());
        assert!(queue.is_full());

        // Should fail when full
        assert_eq!(
            queue.push(create_test_fault(0x300)),
            Err(FaultQueueError::QueueFull)
        );
    }

    #[test]
    fn test_fifo_order() {
        let queue = FaultQueue::new(10);

        queue.push(create_test_fault(0x100)).unwrap();
        queue.push(create_test_fault(0x200)).unwrap();
        queue.push(create_test_fault(0x300)).unwrap();

        assert_eq!(queue.pop().unwrap().stream_id().as_u32(), 0x100);
        assert_eq!(queue.pop().unwrap().stream_id().as_u32(), 0x200);
        assert_eq!(queue.pop().unwrap().stream_id().as_u32(), 0x300);
    }
}
