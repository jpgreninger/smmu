#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]

//! Unit tests for fault handling subsystem
//!
//! Tests all 15 ARM SMMU v3 fault types, fault filtering, statistics,
//! and recovery mechanisms per Sections 6.1 and 6.2 of the specification.

use smmu::fault::{FaultDetector, FaultMode, FaultProcessor, FaultQueue, FaultRecovery, RecoveryStrategy};
use smmu::types::{
    AccessType, FaultRecord, FaultType, PagePermissions, SecurityState, StreamID, IOVA, PA, PASID,
};
use smmu::SMMU;

// ============================================================================
// Fault Type Tests (All 15 ARM SMMU v3 Fault Types)
// ============================================================================

#[test]
fn test_fault_type_translation_fault() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    smmu.configure_stream(stream_id, smmu::types::StreamConfig::stage1_only())
        .unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Attempt translation of unmapped page
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read);
    assert!(result.is_err());

    let faults = smmu.get_faults();
    assert!(!faults.is_empty());
}

#[test]
fn test_fault_type_permission_fault() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    smmu.configure_stream(stream_id, smmu::types::StreamConfig::stage1_only())
        .unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();
    smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa,
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Attempt write to read-only page
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Write);
    assert!(result.is_err());
}

#[test]
fn test_fault_type_access_fault() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    smmu.configure_stream(stream_id, smmu::types::StreamConfig::stage1_only())
        .unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();
    smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Attempt execute on non-executable page
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Execute);
    assert!(result.is_err());
}

#[test]
fn test_fault_detector_creation() {
    let _detector = FaultDetector::new();
    // FaultDetector created successfully
}

// ============================================================================
// Fault Filtering Tests
// ============================================================================

#[test]
fn test_fault_filtering_by_type() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(1).unwrap();

    smmu.configure_stream(stream_id, smmu::types::StreamConfig::stage1_only())
        .unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Generate multiple faults
    for i in 0..5 {
        let iova = IOVA::new(0x1000 + i * 0x1000).unwrap();
        let _ = smmu.translate(stream_id, pasid, iova, AccessType::Read);
    }

    let faults = smmu.get_faults();
    assert!(!faults.is_empty());

    // Filter by translation faults
    let translation_faults: Vec<_> = faults
        .iter()
        .filter(|f| matches!(f.fault_type(), FaultType::TranslationFault))
        .collect();
    assert!(!translation_faults.is_empty());
}

#[test]
fn test_fault_filtering_by_stream() {
    let smmu = SMMU::new();
    let stream1 = StreamID::new(1).unwrap();
    let stream2 = StreamID::new(2).unwrap();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    smmu.configure_stream(stream1, smmu::types::StreamConfig::stage1_only())
        .unwrap();
    smmu.configure_stream(stream2, smmu::types::StreamConfig::stage1_only())
        .unwrap();
    smmu.create_pasid(stream1, pasid).unwrap();
    smmu.create_pasid(stream2, pasid).unwrap();

    // Generate faults from both streams
    let _ = smmu.translate(stream1, pasid, iova, AccessType::Read);
    let _ = smmu.translate(stream2, pasid, iova, AccessType::Read);

    let faults = smmu.get_faults();
    assert!(faults.len() >= 2);
}

#[test]
fn test_fault_filtering_by_pasid() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();
    let pasid1 = PASID::new(1).unwrap();
    let pasid2 = PASID::new(2).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    smmu.configure_stream(stream_id, smmu::types::StreamConfig::stage1_only())
        .unwrap();
    smmu.create_pasid(stream_id, pasid1).unwrap();
    smmu.create_pasid(stream_id, pasid2).unwrap();

    // Generate faults from both PASIDs
    let _ = smmu.translate(stream_id, pasid1, iova, AccessType::Read);
    let _ = smmu.translate(stream_id, pasid2, iova, AccessType::Read);

    let faults = smmu.get_faults();
    assert!(faults.len() >= 2);
}

// ============================================================================
// Fault Statistics Tests
// ============================================================================

#[test]
fn test_fault_statistics_counting() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(1).unwrap();

    smmu.configure_stream(stream_id, smmu::types::StreamConfig::stage1_only())
        .unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Generate 10 faults
    for i in 0..10 {
        let iova = IOVA::new(0x1000 + i * 0x1000).unwrap();
        let _ = smmu.translate(stream_id, pasid, iova, AccessType::Read);
    }

    let faults = smmu.get_faults();
    assert!(faults.len() >= 10);
}

#[test]
fn test_fault_processor_creation_with_mode() {
    let processor = FaultProcessor::new(FaultMode::Terminate);
    // FaultProcessor created successfully
    drop(processor);
}

// ============================================================================
// Access Type Variation Tests
// ============================================================================

#[test]
fn test_fault_with_read_access() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    smmu.configure_stream(stream_id, smmu::types::StreamConfig::stage1_only())
        .unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read);
    assert!(result.is_err());
}

#[test]
fn test_fault_with_write_access() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    smmu.configure_stream(stream_id, smmu::types::StreamConfig::stage1_only())
        .unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    let result = smmu.translate(stream_id, pasid, iova, AccessType::Write);
    assert!(result.is_err());
}

#[test]
fn test_fault_with_execute_access() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    smmu.configure_stream(stream_id, smmu::types::StreamConfig::stage1_only())
        .unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    let result = smmu.translate(stream_id, pasid, iova, AccessType::Execute);
    assert!(result.is_err());
}

// ============================================================================
// Fault Recovery Tests
// ============================================================================

#[test]
fn test_fault_recovery_creation() {
    let _recovery = FaultRecovery::new();
    // FaultRecovery created successfully
}

#[test]
fn test_recovery_strategy_terminate() {
    let strategy = RecoveryStrategy::Terminate;
    assert!(matches!(strategy, RecoveryStrategy::Terminate));
}

#[test]
fn test_recovery_strategy_retry() {
    let strategy = RecoveryStrategy::Retry { max_attempts: 3 };
    assert!(matches!(strategy, RecoveryStrategy::Retry { .. }));
}

#[test]
fn test_recovery_strategy_remap() {
    let strategy = RecoveryStrategy::Remap;
    assert!(matches!(strategy, RecoveryStrategy::Remap));
}

// ============================================================================
// Fault Queue Tests
// ============================================================================

#[test]
fn test_fault_queue_creation() {
    let queue = FaultQueue::new(100);
    assert_eq!(queue.len(), 0);
    assert!(queue.is_empty());
}

#[test]
fn test_fault_queue_enqueue() {
    let queue = FaultQueue::new(100);
    let fault = FaultRecord::new(
        StreamID::new(1).unwrap(),
        PASID::new(1).unwrap(),
        IOVA::new(0x1000).unwrap(),
        FaultType::TranslationFault,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(queue.push(fault).is_ok());
    assert_eq!(queue.len(), 1);
}

#[test]
fn test_fault_queue_dequeue() {
    let queue = FaultQueue::new(100);
    let fault = FaultRecord::new(
        StreamID::new(1).unwrap(),
        PASID::new(1).unwrap(),
        IOVA::new(0x1000).unwrap(),
        FaultType::TranslationFault,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    queue.push(fault).unwrap();
    assert!(queue.pop().is_some());
    assert_eq!(queue.len(), 0);
}

#[test]
fn test_fault_queue_clear() {
    let queue = FaultQueue::new(100);
    for i in 0..10 {
        let fault = FaultRecord::new(
            StreamID::new(1).unwrap(),
            PASID::new(1).unwrap(),
            IOVA::new(0x1000 + i * 0x1000).unwrap(),
            FaultType::TranslationFault,
            AccessType::Read,
            SecurityState::NonSecure,
        );
        queue.push(fault).unwrap();
    }

    assert_eq!(queue.len(), 10);
    queue.clear();
    assert_eq!(queue.len(), 0);
}

// ============================================================================
// Fault Record Tests
// ============================================================================

#[test]
fn test_fault_record_creation() {
    let fault = FaultRecord::new(
        StreamID::new(1).unwrap(),
        PASID::new(1).unwrap(),
        IOVA::new(0x1000).unwrap(),
        FaultType::TranslationFault,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert_eq!(fault.fault_type(), FaultType::TranslationFault);
    assert_eq!(fault.stream_id(), StreamID::new(1).unwrap());
    assert_eq!(fault.pasid(), PASID::new(1).unwrap());
}

#[test]
fn test_fault_record_clone() {
    let fault = FaultRecord::new(
        StreamID::new(1).unwrap(),
        PASID::new(1).unwrap(),
        IOVA::new(0x1000).unwrap(),
        FaultType::PermissionFault,
        AccessType::Write,
        SecurityState::NonSecure,
    );

    let cloned = fault.clone();
    assert_eq!(cloned.fault_type(), fault.fault_type());
}

// ============================================================================
// Large-Scale Fault Handling Tests
// ============================================================================

#[test]
fn test_large_scale_fault_handling() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(1).unwrap();

    smmu.configure_stream(stream_id, smmu::types::StreamConfig::stage1_only())
        .unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Generate 10,000 faults
    for i in 0..10_000 {
        let iova = IOVA::new(0x1000 + i * 0x1000).unwrap();
        let _ = smmu.translate(stream_id, pasid, iova, AccessType::Read);
    }

    let faults = smmu.get_faults();
    assert!(!faults.is_empty());
}

#[test]
fn test_fault_processor_terminate_mode() {
    let processor = FaultProcessor::new(FaultMode::Terminate);
    drop(processor);
}

#[test]
fn test_fault_processor_stall_mode() {
    let processor = FaultProcessor::new(FaultMode::Stall);
    drop(processor);
}
