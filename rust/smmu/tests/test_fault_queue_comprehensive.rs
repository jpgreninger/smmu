#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]

//! Comprehensive test coverage for fault/queue.rs
//!
//! This test suite achieves 100% coverage of the `FaultQueue` implementation,
//! covering all edge cases, error conditions, and FIFO ordering guarantees.

use smmu::fault::queue::{FaultQueue, FaultQueueError};
use smmu::types::{AccessType, FaultRecord, FaultType, StreamID, IOVA, PASID};

fn create_test_fault(stream_id: u32, pasid_val: u32) -> FaultRecord {
    FaultRecord::builder()
        .stream_id(StreamID::new(stream_id).unwrap())
        .pasid(PASID::new(pasid_val).unwrap())
        .address(IOVA::new(0x1000_0000 + (u64::from(stream_id) * 0x1000)).unwrap())
        .fault_type(FaultType::TranslationFault)
        .access_type(AccessType::Read)
        .build()
}

// ============================================================================
// Basic Queue Operations
// ============================================================================

#[test]
fn test_new_queue_empty() {
    let queue = FaultQueue::new(100);
    assert_eq!(queue.capacity(), 100);
    assert_eq!(queue.len(), 0);
    assert!(queue.is_empty());
    assert!(!queue.is_full());
}

#[test]
fn test_new_queue_zero_capacity() {
    let queue = FaultQueue::new(0);
    assert_eq!(queue.capacity(), 0);
    assert!(queue.is_empty());
    assert!(queue.is_full());
}

#[test]
fn test_new_queue_large_capacity() {
    let queue = FaultQueue::new(1_000_000);
    assert_eq!(queue.capacity(), 1_000_000);
    assert!(queue.is_empty());
}

// ============================================================================
// Push/Pop Operations
// ============================================================================

#[test]
fn test_push_single_fault() {
    let queue = FaultQueue::new(10);
    let fault = create_test_fault(0x100, 1);

    assert!(queue.push(fault).is_ok());
    assert_eq!(queue.len(), 1);
    assert!(!queue.is_empty());
    assert!(!queue.is_full());
}

#[test]
fn test_pop_single_fault() {
    let queue = FaultQueue::new(10);
    let fault = create_test_fault(0x100, 1);

    queue.push(fault.clone()).unwrap();
    let popped = queue.pop();

    assert!(popped.is_some());
    let popped = popped.unwrap();
    assert_eq!(popped.stream_id(), fault.stream_id());
    assert_eq!(popped.pasid(), fault.pasid());
    assert_eq!(popped.fault_type(), fault.fault_type());
    assert!(queue.is_empty());
}

#[test]
fn test_pop_empty_queue() {
    let queue = FaultQueue::new(10);
    let popped = queue.pop();
    assert!(popped.is_none());
}

#[test]
fn test_pop_multiple_times_empty() {
    let queue = FaultQueue::new(10);
    assert!(queue.pop().is_none());
    assert!(queue.pop().is_none());
    assert!(queue.pop().is_none());
}

// ============================================================================
// Queue Full Handling
// ============================================================================

#[test]
fn test_push_until_full() {
    let capacity = 5;
    let queue = FaultQueue::new(capacity);

    // Fill the queue
    for i in 0..capacity {
        let fault = create_test_fault(0x100 + i as u32, 1);
        assert!(queue.push(fault).is_ok());
    }

    assert_eq!(queue.len(), capacity);
    assert!(queue.is_full());
    assert!(!queue.is_empty());
}

#[test]
fn test_push_when_full_returns_error() {
    let capacity = 3;
    let queue = FaultQueue::new(capacity);

    // Fill the queue
    for i in 0..capacity {
        queue.push(create_test_fault(0x100 + i as u32, 1)).unwrap();
    }

    // Next push should fail
    let result = queue.push(create_test_fault(0x999, 1));
    assert_eq!(result, Err(FaultQueueError::QueueFull));
    assert_eq!(queue.len(), capacity);
}

#[test]
fn test_push_to_zero_capacity_queue() {
    let queue = FaultQueue::new(0);
    let result = queue.push(create_test_fault(0x100, 1));
    assert_eq!(result, Err(FaultQueueError::QueueFull));
}

#[test]
fn test_queue_full_after_exact_capacity() {
    let queue = FaultQueue::new(1);
    assert!(!queue.is_full());

    queue.push(create_test_fault(0x100, 1)).unwrap();
    assert!(queue.is_full());
}

// ============================================================================
// FIFO Ordering Guarantees
// ============================================================================

#[test]
fn test_fifo_order_simple() {
    let queue = FaultQueue::new(10);

    // Push faults with different stream IDs
    queue.push(create_test_fault(0x100, 1)).unwrap();
    queue.push(create_test_fault(0x200, 2)).unwrap();
    queue.push(create_test_fault(0x300, 3)).unwrap();

    // Pop and verify FIFO order
    assert_eq!(queue.pop().unwrap().stream_id().as_u32(), 0x100);
    assert_eq!(queue.pop().unwrap().stream_id().as_u32(), 0x200);
    assert_eq!(queue.pop().unwrap().stream_id().as_u32(), 0x300);
    assert!(queue.pop().is_none());
}

#[test]
fn test_fifo_order_with_refill() {
    let queue = FaultQueue::new(5);

    // First batch
    queue.push(create_test_fault(0x100, 1)).unwrap();
    queue.push(create_test_fault(0x200, 2)).unwrap();

    // Pop first
    assert_eq!(queue.pop().unwrap().stream_id().as_u32(), 0x100);

    // Add more
    queue.push(create_test_fault(0x300, 3)).unwrap();
    queue.push(create_test_fault(0x400, 4)).unwrap();

    // Verify order
    assert_eq!(queue.pop().unwrap().stream_id().as_u32(), 0x200);
    assert_eq!(queue.pop().unwrap().stream_id().as_u32(), 0x300);
    assert_eq!(queue.pop().unwrap().stream_id().as_u32(), 0x400);
}

#[test]
fn test_fifo_order_large_sequence() {
    let queue = FaultQueue::new(100);
    let count = 50;

    // Push sequence
    for i in 0..count {
        queue.push(create_test_fault(0x1000 + i, 1)).unwrap();
    }

    // Pop and verify exact order
    for i in 0..count {
        let popped = queue.pop().unwrap();
        assert_eq!(popped.stream_id().as_u32(), 0x1000 + i);
    }

    assert!(queue.is_empty());
}

// ============================================================================
// Peek Operation
// ============================================================================

#[test]
fn test_peek_empty_queue() {
    let queue = FaultQueue::new(10);
    assert!(queue.peek().is_none());
}

#[test]
fn test_peek_does_not_remove() {
    let queue = FaultQueue::new(10);
    let fault = create_test_fault(0x100, 1);

    queue.push(fault.clone()).unwrap();

    // Peek multiple times
    let peeked1 = queue.peek();
    assert!(peeked1.is_some());
    assert_eq!(peeked1.as_ref().unwrap().stream_id(), fault.stream_id());

    let peeked2 = queue.peek();
    assert!(peeked2.is_some());
    assert_eq!(peeked2.as_ref().unwrap().stream_id(), fault.stream_id());

    // Queue still has the item
    assert_eq!(queue.len(), 1);
}

#[test]
fn test_peek_returns_front_item() {
    let queue = FaultQueue::new(10);

    queue.push(create_test_fault(0x100, 1)).unwrap();
    queue.push(create_test_fault(0x200, 2)).unwrap();
    queue.push(create_test_fault(0x300, 3)).unwrap();

    // Peek should return first item
    let peeked = queue.peek().unwrap();
    assert_eq!(peeked.stream_id().as_u32(), 0x100);

    // Pop and verify peek returns next
    queue.pop();
    let peeked = queue.peek().unwrap();
    assert_eq!(peeked.stream_id().as_u32(), 0x200);
}

// ============================================================================
// Clear Operation
// ============================================================================

#[test]
fn test_clear_empty_queue() {
    let queue = FaultQueue::new(10);
    queue.clear();
    assert!(queue.is_empty());
    assert_eq!(queue.len(), 0);
}

#[test]
fn test_clear_non_empty_queue() {
    let queue = FaultQueue::new(10);

    queue.push(create_test_fault(0x100, 1)).unwrap();
    queue.push(create_test_fault(0x200, 2)).unwrap();
    queue.push(create_test_fault(0x300, 3)).unwrap();

    assert_eq!(queue.len(), 3);

    queue.clear();

    assert_eq!(queue.len(), 0);
    assert!(queue.is_empty());
    assert!(!queue.is_full());
}

#[test]
fn test_clear_and_reuse() {
    let queue = FaultQueue::new(5);

    // Fill queue
    for i in 0..5 {
        queue.push(create_test_fault(0x100 + i, 1)).unwrap();
    }

    queue.clear();

    // Can push again
    queue.push(create_test_fault(0x999, 1)).unwrap();
    assert_eq!(queue.len(), 1);
    assert_eq!(queue.pop().unwrap().stream_id().as_u32(), 0x999);
}

// ============================================================================
// Get All Operation
// ============================================================================

#[test]
fn test_get_all_empty_queue() {
    let queue = FaultQueue::new(10);
    let all = queue.get_all();
    assert!(all.is_empty());
}

#[test]
fn test_get_all_returns_all_faults() {
    let queue = FaultQueue::new(10);

    queue.push(create_test_fault(0x100, 1)).unwrap();
    queue.push(create_test_fault(0x200, 2)).unwrap();
    queue.push(create_test_fault(0x300, 3)).unwrap();

    let all = queue.get_all();

    assert_eq!(all.len(), 3);
    assert_eq!(all[0].stream_id().as_u32(), 0x100);
    assert_eq!(all[1].stream_id().as_u32(), 0x200);
    assert_eq!(all[2].stream_id().as_u32(), 0x300);
}

#[test]
fn test_get_all_does_not_remove_faults() {
    let queue = FaultQueue::new(10);

    queue.push(create_test_fault(0x100, 1)).unwrap();
    queue.push(create_test_fault(0x200, 2)).unwrap();

    let all = queue.get_all();
    assert_eq!(all.len(), 2);

    // Queue still has the faults
    assert_eq!(queue.len(), 2);
    assert!(!queue.is_empty());
}

// ============================================================================
// Clone Operation
// ============================================================================

#[test]
fn test_clone_empty_queue() {
    let queue = FaultQueue::new(10);
    let cloned = queue.clone();

    assert_eq!(cloned.capacity(), queue.capacity());
    assert_eq!(cloned.len(), queue.len());
    assert!(cloned.is_empty());
}

#[test]
fn test_clone_non_empty_queue() {
    let queue = FaultQueue::new(10);

    queue.push(create_test_fault(0x100, 1)).unwrap();
    queue.push(create_test_fault(0x200, 2)).unwrap();

    let cloned = queue.clone();

    assert_eq!(cloned.capacity(), queue.capacity());
    assert_eq!(cloned.len(), queue.len());

    // Verify contents match
    let orig_all = queue.get_all();
    let clone_all = cloned.get_all();

    assert_eq!(orig_all.len(), clone_all.len());
    for (i, fault) in orig_all.iter().enumerate() {
        assert_eq!(fault.stream_id(), clone_all[i].stream_id());
    }
}

#[test]
fn test_clone_independence() {
    let queue = FaultQueue::new(10);
    queue.push(create_test_fault(0x100, 1)).unwrap();

    let cloned = queue.clone();

    // Modify original
    queue.push(create_test_fault(0x200, 2)).unwrap();

    // Clone should be unchanged
    assert_eq!(queue.len(), 2);
    assert_eq!(cloned.len(), 1);
}

// ============================================================================
// Error Display
// ============================================================================

#[test]
fn test_queue_full_error_display() {
    let error = FaultQueueError::QueueFull;
    let msg = format!("{error}");
    assert_eq!(msg, "Fault queue is full");
}

#[test]
fn test_queue_empty_error_display() {
    let error = FaultQueueError::QueueEmpty;
    let msg = format!("{error}");
    assert_eq!(msg, "Fault queue is empty");
}

#[test]
fn test_lock_poisoned_error_display() {
    let error = FaultQueueError::LockPoisoned;
    let msg = format!("{error}");
    assert_eq!(msg, "Fault queue lock is poisoned");
}

#[test]
fn test_error_equality() {
    assert_eq!(FaultQueueError::QueueFull, FaultQueueError::QueueFull);
    assert_eq!(FaultQueueError::QueueEmpty, FaultQueueError::QueueEmpty);
    assert_eq!(FaultQueueError::LockPoisoned, FaultQueueError::LockPoisoned);

    assert_ne!(FaultQueueError::QueueFull, FaultQueueError::QueueEmpty);
    assert_ne!(FaultQueueError::QueueFull, FaultQueueError::LockPoisoned);
}

#[test]
fn test_error_debug() {
    let error = FaultQueueError::QueueFull;
    let debug = format!("{error:?}");
    assert!(debug.contains("QueueFull"));
}

#[test]
fn test_error_clone() {
    let error = FaultQueueError::QueueFull;
    let cloned = error.clone();
    assert_eq!(error, cloned);
}

// ============================================================================
// Edge Cases and Stress Tests
// ============================================================================

#[test]
fn test_alternating_push_pop() {
    let queue = FaultQueue::new(10);

    for i in 0..100 {
        queue.push(create_test_fault(0x100 + i, 1)).unwrap();
        let popped = queue.pop().unwrap();
        assert_eq!(popped.stream_id().as_u32(), 0x100 + i);
        assert!(queue.is_empty());
    }
}

#[test]
fn test_fill_empty_cycle() {
    let capacity = 5;
    let queue = FaultQueue::new(capacity);

    for _cycle in 0..10 {
        // Fill
        for i in 0..capacity {
            queue.push(create_test_fault(0x100 + i as u32, 1)).unwrap();
        }
        assert!(queue.is_full());

        // Empty
        for _i in 0..capacity {
            assert!(queue.pop().is_some());
        }
        assert!(queue.is_empty());
    }
}

#[test]
fn test_capacity_boundary_minus_one() {
    let queue = FaultQueue::new(3);

    // Push capacity - 1
    queue.push(create_test_fault(0x100, 1)).unwrap();
    queue.push(create_test_fault(0x200, 2)).unwrap();

    assert_eq!(queue.len(), 2);
    assert!(!queue.is_full());

    // One more should succeed
    queue.push(create_test_fault(0x300, 3)).unwrap();
    assert!(queue.is_full());
}

#[test]
fn test_different_fault_types() {
    let queue = FaultQueue::new(10);

    let fault_types = [
        FaultType::TranslationFault,
        FaultType::PermissionFault,
        FaultType::AddressSizeFault,
    ];

    for (i, fault_type) in fault_types.iter().enumerate() {
        let fault = FaultRecord::builder()
            .stream_id(StreamID::new(0x100 + i as u32).unwrap())
            .pasid(PASID::new(1).unwrap())
            .address(IOVA::new(0x1000).unwrap())
            .fault_type(*fault_type)
            .access_type(AccessType::Read)
            .build();

        queue.push(fault).unwrap();
    }

    assert_eq!(queue.len(), 3);

    for fault_type in &fault_types {
        let popped = queue.pop().unwrap();
        assert_eq!(popped.fault_type(), *fault_type);
    }
}

#[test]
fn test_different_access_types() {
    let queue = FaultQueue::new(10);

    let access_types = [AccessType::Read, AccessType::Write, AccessType::Execute, AccessType::ReadWrite];

    for (i, access_type) in access_types.iter().enumerate() {
        let fault = FaultRecord::builder()
            .stream_id(StreamID::new(0x100 + i as u32).unwrap())
            .pasid(PASID::new(1).unwrap())
            .address(IOVA::new(0x1000).unwrap())
            .fault_type(FaultType::TranslationFault)
            .access_type(*access_type)
            .build();

        queue.push(fault).unwrap();
    }

    assert_eq!(queue.len(), 4);
}

// ============================================================================
// Concurrent Access Patterns (Sequential Tests)
// ============================================================================

#[test]
fn test_multiple_operations_sequence() {
    let queue = FaultQueue::new(10);

    // Mix of operations
    queue.push(create_test_fault(0x100, 1)).unwrap();
    assert_eq!(queue.len(), 1);

    let _peeked = queue.peek();
    assert_eq!(queue.len(), 1);

    queue.push(create_test_fault(0x200, 2)).unwrap();
    assert_eq!(queue.len(), 2);

    let _all = queue.get_all();
    assert_eq!(queue.len(), 2);

    queue.pop();
    assert_eq!(queue.len(), 1);

    queue.clear();
    assert!(queue.is_empty());
}

#[test]
fn test_capacity_one_special_case() {
    let queue = FaultQueue::new(1);

    assert!(!queue.is_full());
    assert!(queue.is_empty());

    queue.push(create_test_fault(0x100, 1)).unwrap();
    assert!(queue.is_full());
    assert!(!queue.is_empty());

    assert_eq!(queue.push(create_test_fault(0x200, 2)), Err(FaultQueueError::QueueFull));

    queue.pop();
    assert!(queue.is_empty());
    assert!(!queue.is_full());
}

#[test]
fn test_large_capacity_queue() {
    let queue = FaultQueue::new(10_000);

    // Push many faults
    for i in 0..1000 {
        queue.push(create_test_fault(i, 1)).unwrap();
    }

    assert_eq!(queue.len(), 1000);
    assert!(!queue.is_full());

    // Verify order
    for i in 0..1000 {
        let popped = queue.pop().unwrap();
        assert_eq!(popped.stream_id().as_u32(), i);
    }
}
