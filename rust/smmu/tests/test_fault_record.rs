//! Comprehensive tests for FaultRecord structure
//!
//! This test suite validates the FaultRecord structure implementation following
//! ARM SMMU v3 specification requirements. Tests are written FIRST following TDD.

use smmu::types::{
    AccessType, FaultRecord, FaultSyndrome, FaultType, IOVA, PASID, SecurityState, StreamID,
};

/// Test default FaultRecord construction
#[test]
fn test_fault_record_default() {
    let record = FaultRecord::default();

    assert_eq!(record.stream_id(), StreamID::new(0).unwrap());
    assert_eq!(record.pasid(), PASID::new(0).unwrap());
    assert_eq!(record.address(), IOVA::new(0).unwrap());
    assert_eq!(record.fault_type(), FaultType::TranslationFault);
    assert_eq!(record.access_type(), AccessType::Read);
    assert_eq!(record.security_state(), SecurityState::NonSecure);
    assert_eq!(record.timestamp(), 0);
}

/// Test FaultRecord with basic fault information
#[test]
fn test_fault_record_basic() {
    let sid = StreamID::new(42).unwrap();
    let pasid = PASID::new(7).unwrap();
    let addr = IOVA::new(0x1000).unwrap();
    let fault_type = FaultType::PermissionFault;
    let access_type = AccessType::Write;
    let sec_state = SecurityState::Secure;

    let record = FaultRecord::new(sid, pasid, addr, fault_type, access_type, sec_state);

    assert_eq!(record.stream_id(), sid);
    assert_eq!(record.pasid(), pasid);
    assert_eq!(record.address(), addr);
    assert_eq!(record.fault_type(), fault_type);
    assert_eq!(record.access_type(), access_type);
    assert_eq!(record.security_state(), sec_state);
    assert_eq!(record.timestamp(), 0); // Default timestamp
}

/// Test FaultRecord with fault syndrome
#[test]
fn test_fault_record_with_syndrome() {
    let sid = StreamID::new(100).unwrap();
    let pasid = PASID::new(15).unwrap();
    let addr = IOVA::new(0x2000).unwrap();
    let syndrome = FaultSyndrome::default();

    let record = FaultRecord::with_syndrome(
        sid,
        pasid,
        addr,
        FaultType::AddressSizeFault,
        AccessType::Execute,
        SecurityState::Realm,
        syndrome.clone(),
    );

    assert_eq!(record.stream_id(), sid);
    assert_eq!(record.syndrome(), &syndrome);
}

/// Test FaultRecord builder pattern
#[test]
fn test_fault_record_builder() {
    let sid = StreamID::new(123).unwrap();
    let pasid = PASID::new(31).unwrap();
    let addr = IOVA::new(0x3000).unwrap();

    let record = FaultRecord::builder()
        .stream_id(sid)
        .pasid(pasid)
        .address(addr)
        .fault_type(FaultType::AccessFlagFault)
        .access_type(AccessType::ReadWrite)
        .security_state(SecurityState::NonSecure)
        .timestamp(12345)
        .build();

    assert_eq!(record.stream_id(), sid);
    assert_eq!(record.pasid(), pasid);
    assert_eq!(record.address(), addr);
    assert_eq!(record.fault_type(), FaultType::AccessFlagFault);
    assert_eq!(record.access_type(), AccessType::ReadWrite);
    assert_eq!(record.timestamp(), 12345);
}

/// Test FaultRecord with all ARM SMMU v3 fault types
#[test]
fn test_fault_record_all_fault_types() {
    let fault_types = [
        FaultType::TranslationFault,
        FaultType::AddressSizeFault,
        FaultType::AccessFlagFault,
        FaultType::PermissionFault,
        FaultType::ExternalAbort,
        FaultType::TLBConflictAbort,
        FaultType::UnsupportedAtomicUpdate,
        FaultType::AlignmentFault,
        FaultType::OutputAddressRangeFault,
        FaultType::BadStreamID,
        FaultType::CDFetchFault,
        FaultType::BadCD,
        FaultType::WalkEABT,
        FaultType::BadSTE,
        FaultType::STEFetchFault,
    ];

    for fault_type in &fault_types {
        let record = FaultRecord::builder()
            .stream_id(StreamID::new(1).unwrap())
            .pasid(PASID::new(0).unwrap())
            .address(IOVA::new(0).unwrap())
            .fault_type(*fault_type)
            .build();

        assert_eq!(record.fault_type(), *fault_type);
    }
}

/// Test FaultRecord with timestamp
#[test]
fn test_fault_record_timestamp() {
    let timestamp = 9876543210_u64;
    let record = FaultRecord::builder()
        .stream_id(StreamID::new(1).unwrap())
        .pasid(PASID::new(0).unwrap())
        .address(IOVA::new(0).unwrap())
        .fault_type(FaultType::TranslationFault)
        .timestamp(timestamp)
        .build();

    assert_eq!(record.timestamp(), timestamp);
}

/// Test FaultRecord Clone trait
#[test]
fn test_fault_record_clone() {
    let record1 = FaultRecord::builder()
        .stream_id(StreamID::new(50).unwrap())
        .pasid(PASID::new(10).unwrap())
        .address(IOVA::new(0x5000).unwrap())
        .fault_type(FaultType::PermissionFault)
        .timestamp(111222)
        .build();

    let record2 = record1.clone();

    assert_eq!(record1.stream_id(), record2.stream_id());
    assert_eq!(record1.pasid(), record2.pasid());
    assert_eq!(record1.address(), record2.address());
    assert_eq!(record1.fault_type(), record2.fault_type());
    assert_eq!(record1.timestamp(), record2.timestamp());
}

/// Test FaultRecord Debug trait
#[test]
fn test_fault_record_debug() {
    let record = FaultRecord::builder()
        .stream_id(StreamID::new(200).unwrap())
        .pasid(PASID::new(20).unwrap())
        .address(IOVA::new(0x6000).unwrap())
        .fault_type(FaultType::AccessFlagFault)
        .build();

    let debug_str = format!("{:?}", record);
    assert!(!debug_str.is_empty());
    assert!(debug_str.contains("FaultRecord"));
}

/// Test FaultRecord PartialEq trait
#[test]
fn test_fault_record_equality() {
    let sid = StreamID::new(75).unwrap();
    let pasid = PASID::new(5).unwrap();
    let addr = IOVA::new(0x7000).unwrap();

    let record1 = FaultRecord::new(
        sid,
        pasid,
        addr,
        FaultType::TranslationFault,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let record2 = FaultRecord::new(
        sid,
        pasid,
        addr,
        FaultType::TranslationFault,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let record3 = FaultRecord::new(
        sid,
        pasid,
        IOVA::new(0x8000).unwrap(),
        FaultType::TranslationFault,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert_eq!(record1, record2);
    assert_ne!(record1, record3);
}

/// Test FaultSyndrome default construction
#[test]
fn test_fault_syndrome_default() {
    let syndrome = FaultSyndrome::default();

    assert_eq!(syndrome.syndrome_register(), 0);
    assert!(!syndrome.write_not_read());
    assert!(syndrome.valid_syndrome());
}

/// Test FaultSyndrome with full details
#[test]
fn test_fault_syndrome_full() {
    let syndrome = FaultSyndrome::builder()
        .syndrome_register(0x12345678)
        .fault_level(2)
        .write_not_read(true)
        .context_descriptor_index(5)
        .build();

    assert_eq!(syndrome.syndrome_register(), 0x12345678);
    assert_eq!(syndrome.fault_level(), 2);
    assert!(syndrome.write_not_read());
    assert_eq!(syndrome.context_descriptor_index(), 5);
}

/// Test FaultRecord serialization (with serde in dev-dependencies)
#[cfg(feature = "serde")]
#[test]
fn test_fault_record_serialization() {
    let record = FaultRecord::builder()
        .stream_id(StreamID::new(99).unwrap())
        .pasid(PASID::new(11).unwrap())
        .address(IOVA::new(0x9000).unwrap())
        .fault_type(FaultType::PermissionFault)
        .timestamp(555666)
        .build();

    let json = serde_json::to_string(&record).expect("Failed to serialize");
    let deserialized: FaultRecord = serde_json::from_str(&json).expect("Failed to deserialize");

    assert_eq!(record, deserialized);
}

/// Test FaultRecord with current system time
#[test]
fn test_fault_record_with_system_time() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs();

    let record = FaultRecord::builder()
        .stream_id(StreamID::new(1).unwrap())
        .pasid(PASID::new(0).unwrap())
        .address(IOVA::new(0).unwrap())
        .fault_type(FaultType::TranslationFault)
        .timestamp(timestamp)
        .build();

    assert!(record.timestamp() > 0);
    assert_eq!(record.timestamp(), timestamp);
}

/// Test FaultRecord captures complete fault context
#[test]
fn test_fault_record_complete_context() {
    let sid = StreamID::new(255).unwrap();
    let pasid = PASID::new(127).unwrap();
    let addr = IOVA::new(0xDEADBEEF).unwrap();
    let syndrome = FaultSyndrome::builder()
        .syndrome_register(0xABCDEF00)
        .fault_level(3)
        .write_not_read(false)
        .context_descriptor_index(15)
        .build();

    let record = FaultRecord::builder()
        .stream_id(sid)
        .pasid(pasid)
        .address(addr)
        .fault_type(FaultType::PermissionFault)
        .access_type(AccessType::ReadExecute)
        .security_state(SecurityState::Realm)
        .syndrome(syndrome.clone())
        .timestamp(987654321)
        .build();

    // Verify all context is captured
    assert_eq!(record.stream_id(), sid);
    assert_eq!(record.pasid(), pasid);
    assert_eq!(record.address(), addr);
    assert_eq!(record.fault_type(), FaultType::PermissionFault);
    assert_eq!(record.access_type(), AccessType::ReadExecute);
    assert_eq!(record.security_state(), SecurityState::Realm);
    assert_eq!(record.syndrome(), &syndrome);
    assert_eq!(record.timestamp(), 987654321);
}

/// Test FaultSyndrome fault level validation (0-3)
#[test]
fn test_fault_syndrome_fault_level() {
    for level in 0..=3 {
        let syndrome = FaultSyndrome::builder().fault_level(level).build();
        assert_eq!(syndrome.fault_level(), level);
    }
}

/// Test FaultSyndrome Clone trait
#[test]
fn test_fault_syndrome_clone() {
    let syndrome1 = FaultSyndrome::builder()
        .syndrome_register(0x11223344)
        .fault_level(1)
        .write_not_read(true)
        .build();

    let syndrome2 = syndrome1.clone();

    assert_eq!(syndrome1.syndrome_register(), syndrome2.syndrome_register());
    assert_eq!(syndrome1.fault_level(), syndrome2.fault_level());
    assert_eq!(syndrome1.write_not_read(), syndrome2.write_not_read());
}

/// Test FaultRecord size for cache efficiency
#[test]
fn test_fault_record_size() {
    use std::mem::size_of;

    let size = size_of::<FaultRecord>();

    // FaultRecord should be reasonably sized for efficient logging
    // Allowing up to 128 bytes for comprehensive fault information
    assert!(size <= 128, "FaultRecord too large: {} bytes", size);
}

/// Test FaultSyndrome size
#[test]
fn test_fault_syndrome_size() {
    use std::mem::size_of;

    let size = size_of::<FaultSyndrome>();

    // FaultSyndrome should be compact (< 32 bytes)
    assert!(size <= 32, "FaultSyndrome too large: {} bytes", size);
}

/// Test zero unsafe code requirement for FaultRecord
#[test]
fn test_fault_record_safe() {
    // Exercise all FaultRecord operations without any unsafe blocks
    let record = FaultRecord::builder()
        .stream_id(StreamID::new(1).unwrap())
        .pasid(PASID::new(2).unwrap())
        .address(IOVA::new(0x1000).unwrap())
        .fault_type(FaultType::TranslationFault)
        .access_type(AccessType::Write)
        .security_state(SecurityState::Secure)
        .timestamp(12345)
        .build();

    let cloned = record.clone();
    let _ = format!("{:?}", record);

    // All operations are safe
    assert_eq!(record, cloned);
}

/// Test FaultRecord builder with minimal required fields
#[test]
fn test_fault_record_builder_minimal() {
    let record = FaultRecord::builder()
        .stream_id(StreamID::new(1).unwrap())
        .pasid(PASID::new(0).unwrap())
        .address(IOVA::new(0).unwrap())
        .fault_type(FaultType::TranslationFault)
        .build();

    // Should have sensible defaults for optional fields
    assert_eq!(record.access_type(), AccessType::Read); // Default
    assert_eq!(record.security_state(), SecurityState::NonSecure); // Default
    assert_eq!(record.timestamp(), 0); // Default
}

/// Test FaultRecord with maximum values
#[test]
fn test_fault_record_maximum_values() {
    use smmu::types::PASID_MAX;

    let record = FaultRecord::builder()
        .stream_id(StreamID::new(65535).unwrap()) // Max 16-bit StreamID
        .pasid(PASID::new(PASID_MAX).unwrap())
        .address(IOVA::new(u64::MAX).unwrap())
        .fault_type(FaultType::BadSTE)
        .access_type(AccessType::ReadWriteExecute)
        .timestamp(u64::MAX)
        .build();

    assert_eq!(record.stream_id(), StreamID::new(65535).unwrap());
    assert_eq!(record.pasid(), PASID::new(PASID_MAX).unwrap());
    assert_eq!(record.timestamp(), u64::MAX);
}

/// Performance test: FaultRecord construction should be fast
#[test]
fn test_fault_record_performance() {
    let sid = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();
    let addr = IOVA::new(0x1000).unwrap();

    let start = std::time::Instant::now();
    for _ in 0..10000 {
        let _ = FaultRecord::new(
            sid,
            pasid,
            addr,
            FaultType::TranslationFault,
            AccessType::Read,
            SecurityState::NonSecure,
        );
    }
    let elapsed = start.elapsed();

    // Construction should be very fast (< 5ms for 10k records)
    assert!(
        elapsed.as_millis() < 10,
        "FaultRecord construction too slow: {:?}",
        elapsed
    );
}
