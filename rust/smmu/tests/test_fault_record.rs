//! Comprehensive tests for types/fault_record.rs
//!
//! Tests cover:
//! - FaultSyndrome construction (new, builder, default)
//! - FaultSyndrome field access (5 getters)
//! - FaultSyndromeBuilder pattern (5 setters + build)
//! - FaultRecord construction (new, with_syndrome, builder, default)
//! - FaultRecord field access (9 getters)
//! - FaultRecordBuilder pattern (8 setters + build)
//! - Trait implementations (Debug, Clone, PartialEq, Eq, Default)
//! - Const correctness
//! - ARM SMMU v3 fault reporting compliance
//! - Builder validation and error handling

use smmu::types::{AccessType, FaultRecord, FaultSyndrome, FaultType, SecurityState, StreamID, IOVA, PASID};

// ============================================================================
// FaultSyndrome Construction Tests
// ============================================================================

#[test]
fn test_fault_syndrome_new() {
    let syndrome = FaultSyndrome::new();

    assert_eq!(syndrome.syndrome_register(), 0);
    assert_eq!(syndrome.fault_level(), 0);
    assert_eq!(syndrome.write_not_read(), false);
    assert_eq!(syndrome.valid_syndrome(), true);
    assert_eq!(syndrome.context_descriptor_index(), 0);
}

#[test]
fn test_fault_syndrome_default() {
    let syndrome = FaultSyndrome::default();

    assert_eq!(syndrome.syndrome_register(), 0);
    assert_eq!(syndrome.fault_level(), 0);
    assert_eq!(syndrome.write_not_read(), false);
    assert_eq!(syndrome.valid_syndrome(), true);
    assert_eq!(syndrome.context_descriptor_index(), 0);
}

#[test]
fn test_fault_syndrome_builder_basic() {
    let syndrome = FaultSyndrome::builder().build();

    assert_eq!(syndrome.syndrome_register(), 0);
    assert_eq!(syndrome.valid_syndrome(), true);
}

#[test]
fn test_fault_syndrome_builder_all_fields() {
    let syndrome = FaultSyndrome::builder()
        .syndrome_register(0x12345678)
        .fault_level(2)
        .write_not_read(true)
        .valid_syndrome(false)
        .context_descriptor_index(42)
        .build();

    assert_eq!(syndrome.syndrome_register(), 0x12345678);
    assert_eq!(syndrome.fault_level(), 2);
    assert_eq!(syndrome.write_not_read(), true);
    assert_eq!(syndrome.valid_syndrome(), false);
    assert_eq!(syndrome.context_descriptor_index(), 42);
}

#[test]
fn test_fault_syndrome_builder_partial_fields() {
    let syndrome = FaultSyndrome::builder().syndrome_register(0xABCD).fault_level(3).build();

    assert_eq!(syndrome.syndrome_register(), 0xABCD);
    assert_eq!(syndrome.fault_level(), 3);
    assert_eq!(syndrome.write_not_read(), false); // Default
    assert_eq!(syndrome.valid_syndrome(), true); // Default
}

// ============================================================================
// FaultSyndrome Getter Tests
// ============================================================================

#[test]
fn test_fault_syndrome_syndrome_register() {
    let syndrome = FaultSyndrome::builder().syndrome_register(0xDEADBEEF).build();

    assert_eq!(syndrome.syndrome_register(), 0xDEADBEEF);
}

#[test]
fn test_fault_syndrome_fault_level() {
    for level in 0..=3 {
        let syndrome = FaultSyndrome::builder().fault_level(level).build();
        assert_eq!(syndrome.fault_level(), level);
    }
}

#[test]
fn test_fault_syndrome_write_not_read_flags() {
    let read_syndrome = FaultSyndrome::builder().write_not_read(false).build();
    assert_eq!(read_syndrome.write_not_read(), false);

    let write_syndrome = FaultSyndrome::builder().write_not_read(true).build();
    assert_eq!(write_syndrome.write_not_read(), true);
}

#[test]
fn test_fault_syndrome_valid_syndrome_flag() {
    let valid = FaultSyndrome::builder().valid_syndrome(true).build();
    assert_eq!(valid.valid_syndrome(), true);

    let invalid = FaultSyndrome::builder().valid_syndrome(false).build();
    assert_eq!(invalid.valid_syndrome(), false);
}

#[test]
fn test_fault_syndrome_context_descriptor_index() {
    let syndrome = FaultSyndrome::builder().context_descriptor_index(1234).build();

    assert_eq!(syndrome.context_descriptor_index(), 1234);
}

// ============================================================================
// FaultSyndrome Trait Tests
// ============================================================================

#[test]
fn test_fault_syndrome_clone() {
    let syndrome1 = FaultSyndrome::builder().syndrome_register(0x1234).fault_level(2).build();

    let syndrome2 = syndrome1.clone();

    assert_eq!(syndrome1, syndrome2);
}

#[test]
fn test_fault_syndrome_equality() {
    let syndrome1 = FaultSyndrome::builder().syndrome_register(0x1234).fault_level(2).build();

    let syndrome2 = FaultSyndrome::builder().syndrome_register(0x1234).fault_level(2).build();

    let syndrome3 = FaultSyndrome::builder().syndrome_register(0x5678).fault_level(2).build();

    assert_eq!(syndrome1, syndrome2);
    assert_ne!(syndrome1, syndrome3);
}

#[test]
fn test_fault_syndrome_debug() {
    let syndrome = FaultSyndrome::builder().syndrome_register(0x1234).build();

    let debug_str = format!("{syndrome:?}");
    assert!(debug_str.contains("FaultSyndrome"));
}

// ============================================================================
// FaultRecord Construction Tests
// ============================================================================

#[test]
fn test_fault_record_new() {
    let record = FaultRecord::new(
        StreamID::new(10).unwrap(),
        PASID::new(5).unwrap(),
        IOVA::new(0x1000).unwrap(),
        FaultType::TranslationFault,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert_eq!(record.stream_id(), StreamID::new(10).unwrap());
    assert_eq!(record.pasid(), PASID::new(5).unwrap());
    assert_eq!(record.address(), IOVA::new(0x1000).unwrap());
    assert_eq!(record.fault_type(), FaultType::TranslationFault);
    assert_eq!(record.access_type(), AccessType::Read);
    assert_eq!(record.security_state(), SecurityState::NonSecure);
    assert_eq!(record.timestamp(), 0);
}

#[test]
fn test_fault_record_with_syndrome() {
    let syndrome = FaultSyndrome::builder().syndrome_register(0xABCD).fault_level(2).build();

    let record = FaultRecord::with_syndrome(
        StreamID::new(1).unwrap(),
        PASID::new(2).unwrap(),
        IOVA::new(0x2000).unwrap(),
        FaultType::PermissionFault,
        AccessType::Write,
        SecurityState::Secure,
        syndrome,
    );

    assert_eq!(record.syndrome().syndrome_register(), 0xABCD);
    assert_eq!(record.syndrome().fault_level(), 2);
}

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

#[test]
fn test_fault_record_builder_minimal() {
    let record = FaultRecord::builder()
        .stream_id(StreamID::new(42).unwrap())
        .pasid(PASID::new(7).unwrap())
        .address(IOVA::new(0x1000).unwrap())
        .build();

    assert_eq!(record.stream_id(), StreamID::new(42).unwrap());
    assert_eq!(record.pasid(), PASID::new(7).unwrap());
    assert_eq!(record.address(), IOVA::new(0x1000).unwrap());
    // Defaults
    assert_eq!(record.fault_type(), FaultType::TranslationFault);
    assert_eq!(record.access_type(), AccessType::Read);
    assert_eq!(record.security_state(), SecurityState::NonSecure);
}

#[test]
fn test_fault_record_builder_all_fields() {
    let syndrome = FaultSyndrome::builder().syndrome_register(0x9876).build();

    let record = FaultRecord::builder()
        .stream_id(StreamID::new(100).unwrap())
        .pasid(PASID::new(50).unwrap())
        .address(IOVA::new(0x5000).unwrap())
        .fault_type(FaultType::AddressSizeFault)
        .access_type(AccessType::Execute)
        .security_state(SecurityState::Realm)
        .syndrome(syndrome)
        .timestamp(12345)
        .build();

    assert_eq!(record.stream_id(), StreamID::new(100).unwrap());
    assert_eq!(record.pasid(), PASID::new(50).unwrap());
    assert_eq!(record.address(), IOVA::new(0x5000).unwrap());
    assert_eq!(record.fault_type(), FaultType::AddressSizeFault);
    assert_eq!(record.access_type(), AccessType::Execute);
    assert_eq!(record.security_state(), SecurityState::Realm);
    assert_eq!(record.syndrome().syndrome_register(), 0x9876);
    assert_eq!(record.timestamp(), 12345);
}

// ============================================================================
// FaultRecord Getter Tests
// ============================================================================

#[test]
fn test_fault_record_all_getters() {
    let syndrome = FaultSyndrome::builder().syndrome_register(0x1111).build();

    let record = FaultRecord::builder()
        .stream_id(StreamID::new(5).unwrap())
        .pasid(PASID::new(10).unwrap())
        .address(IOVA::new(0x3000).unwrap())
        .fault_type(FaultType::PermissionFault)
        .access_type(AccessType::Write)
        .security_state(SecurityState::Secure)
        .syndrome(syndrome)
        .timestamp(999)
        .build();

    assert_eq!(record.stream_id(), StreamID::new(5).unwrap());
    assert_eq!(record.pasid(), PASID::new(10).unwrap());
    assert_eq!(record.address(), IOVA::new(0x3000).unwrap());
    assert_eq!(record.fault_type(), FaultType::PermissionFault);
    assert_eq!(record.access_type(), AccessType::Write);
    assert_eq!(record.security_state(), SecurityState::Secure);
    assert_eq!(record.syndrome().syndrome_register(), 0x1111);
    assert_eq!(record.timestamp(), 999);
}

#[test]
fn test_fault_record_iova_alias() {
    let record = FaultRecord::new(
        StreamID::new(1).unwrap(),
        PASID::new(0).unwrap(),
        IOVA::new(0x4000).unwrap(),
        FaultType::TranslationFault,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    // iova() should be an alias for address()
    assert_eq!(record.iova(), record.address());
    assert_eq!(record.iova(), IOVA::new(0x4000).unwrap());
}

#[test]
fn test_fault_record_stage() {
    let record = FaultRecord::default();

    // Currently returns Stage2 as placeholder
    use smmu::types::TranslationStage;
    assert_eq!(record.stage(), TranslationStage::Stage2);
}

// ============================================================================
// FaultRecord Trait Tests
// ============================================================================

#[test]
fn test_fault_record_clone() {
    let record1 = FaultRecord::new(
        StreamID::new(1).unwrap(),
        PASID::new(2).unwrap(),
        IOVA::new(0x1000).unwrap(),
        FaultType::TranslationFault,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let record2 = record1.clone();

    assert_eq!(record1, record2);
}

#[test]
fn test_fault_record_equality() {
    let record1 = FaultRecord::new(
        StreamID::new(1).unwrap(),
        PASID::new(2).unwrap(),
        IOVA::new(0x1000).unwrap(),
        FaultType::TranslationFault,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let record2 = FaultRecord::new(
        StreamID::new(1).unwrap(),
        PASID::new(2).unwrap(),
        IOVA::new(0x1000).unwrap(),
        FaultType::TranslationFault,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let record3 = FaultRecord::new(
        StreamID::new(2).unwrap(), // Different stream_id
        PASID::new(2).unwrap(),
        IOVA::new(0x1000).unwrap(),
        FaultType::TranslationFault,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert_eq!(record1, record2);
    assert_ne!(record1, record3);
}

#[test]
fn test_fault_record_debug() {
    let record = FaultRecord::default();
    let debug_str = format!("{record:?}");

    assert!(debug_str.contains("FaultRecord"));
}

// ============================================================================
// Builder Validation Tests
// ============================================================================

#[test]
#[should_panic(expected = "stream_id must be set")]
fn test_fault_record_builder_missing_stream_id() {
    FaultRecord::builder()
        .pasid(PASID::new(1).unwrap())
        .address(IOVA::new(0x1000).unwrap())
        .build();
}

#[test]
#[should_panic(expected = "pasid must be set")]
fn test_fault_record_builder_missing_pasid() {
    FaultRecord::builder()
        .stream_id(StreamID::new(1).unwrap())
        .address(IOVA::new(0x1000).unwrap())
        .build();
}

#[test]
#[should_panic(expected = "address must be set")]
fn test_fault_record_builder_missing_address() {
    FaultRecord::builder()
        .stream_id(StreamID::new(1).unwrap())
        .pasid(PASID::new(1).unwrap())
        .build();
}

// ============================================================================
// ARM SMMU v3 Fault Scenarios
// ============================================================================

#[test]
fn test_translation_fault_scenario() {
    // ARM SMMU v3: Translation fault during page table walk
    let record = FaultRecord::new(
        StreamID::new(10).unwrap(),
        PASID::new(5).unwrap(),
        IOVA::new(0x1000).unwrap(),
        FaultType::TranslationFault,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert_eq!(record.fault_type(), FaultType::TranslationFault);
    assert_eq!(record.access_type(), AccessType::Read);
}

#[test]
fn test_permission_fault_scenario() {
    // ARM SMMU v3: Permission fault (page present but wrong permissions)
    let syndrome = FaultSyndrome::builder().write_not_read(true).fault_level(3).build();

    let record = FaultRecord::with_syndrome(
        StreamID::new(20).unwrap(),
        PASID::new(10).unwrap(),
        IOVA::new(0x2000).unwrap(),
        FaultType::PermissionFault,
        AccessType::Write,
        SecurityState::Secure,
        syndrome,
    );

    assert_eq!(record.fault_type(), FaultType::PermissionFault);
    assert_eq!(record.syndrome().write_not_read(), true);
}

#[test]
fn test_address_size_fault_scenario() {
    // ARM SMMU v3: Address exceeds supported address size
    let record = FaultRecord::builder()
        .stream_id(StreamID::new(30).unwrap())
        .pasid(PASID::new(15).unwrap())
        .address(IOVA::new(0xFFFF_FFFF_FFFF_F000).unwrap())
        .fault_type(FaultType::AddressSizeFault)
        .access_type(AccessType::Read)
        .security_state(SecurityState::NonSecure)
        .build();

    assert_eq!(record.fault_type(), FaultType::AddressSizeFault);
    assert_eq!(record.address().as_u64(), 0xFFFF_FFFF_FFFF_F000);
}

#[test]
fn test_access_flag_fault_scenario() {
    // ARM SMMU v3: Access flag not set in page table entry
    let syndrome = FaultSyndrome::builder().fault_level(2).valid_syndrome(true).build();

    let record = FaultRecord::with_syndrome(
        StreamID::new(40).unwrap(),
        PASID::new(20).unwrap(),
        IOVA::new(0x4000).unwrap(),
        FaultType::AccessFlagFault,
        AccessType::Read,
        SecurityState::NonSecure,
        syndrome,
    );

    assert_eq!(record.fault_type(), FaultType::AccessFlagFault);
    assert_eq!(record.syndrome().fault_level(), 2);
}

// ============================================================================
// Security State Scenarios
// ============================================================================

#[test]
fn test_secure_fault_record() {
    let record = FaultRecord::builder()
        .stream_id(StreamID::new(1).unwrap())
        .pasid(PASID::new(0).unwrap())
        .address(IOVA::new(0x1000).unwrap())
        .security_state(SecurityState::Secure)
        .build();

    assert_eq!(record.security_state(), SecurityState::Secure);
}

#[test]
fn test_nonsecure_fault_record() {
    let record = FaultRecord::builder()
        .stream_id(StreamID::new(1).unwrap())
        .pasid(PASID::new(0).unwrap())
        .address(IOVA::new(0x1000).unwrap())
        .security_state(SecurityState::NonSecure)
        .build();

    assert_eq!(record.security_state(), SecurityState::NonSecure);
}

#[test]
fn test_realm_fault_record() {
    let record = FaultRecord::builder()
        .stream_id(StreamID::new(1).unwrap())
        .pasid(PASID::new(0).unwrap())
        .address(IOVA::new(0x1000).unwrap())
        .security_state(SecurityState::Realm)
        .build();

    assert_eq!(record.security_state(), SecurityState::Realm);
}

// ============================================================================
// Timestamp Ordering Tests
// ============================================================================

#[test]
fn test_fault_record_timestamp_ordering() {
    let record1 = FaultRecord::builder()
        .stream_id(StreamID::new(1).unwrap())
        .pasid(PASID::new(0).unwrap())
        .address(IOVA::new(0x1000).unwrap())
        .timestamp(100)
        .build();

    let record2 = FaultRecord::builder()
        .stream_id(StreamID::new(2).unwrap())
        .pasid(PASID::new(0).unwrap())
        .address(IOVA::new(0x2000).unwrap())
        .timestamp(200)
        .build();

    let record3 = FaultRecord::builder()
        .stream_id(StreamID::new(3).unwrap())
        .pasid(PASID::new(0).unwrap())
        .address(IOVA::new(0x3000).unwrap())
        .timestamp(50)
        .build();

    let mut records = vec![record1, record2, record3];
    records.sort_by_key(|r| r.timestamp());

    assert_eq!(records[0].timestamp(), 50);
    assert_eq!(records[1].timestamp(), 100);
    assert_eq!(records[2].timestamp(), 200);
}

// ============================================================================
// Collection Tests
// ============================================================================

#[test]
fn test_fault_record_vec_operations() {
    let mut records = Vec::new();

    records.push(FaultRecord::new(
        StreamID::new(1).unwrap(),
        PASID::new(0).unwrap(),
        IOVA::new(0x1000).unwrap(),
        FaultType::TranslationFault,
        AccessType::Read,
        SecurityState::NonSecure,
    ));

    records.push(FaultRecord::new(
        StreamID::new(2).unwrap(),
        PASID::new(0).unwrap(),
        IOVA::new(0x2000).unwrap(),
        FaultType::PermissionFault,
        AccessType::Write,
        SecurityState::NonSecure,
    ));

    assert_eq!(records.len(), 2);
    assert_eq!(records[0].fault_type(), FaultType::TranslationFault);
    assert_eq!(records[1].fault_type(), FaultType::PermissionFault);
}

#[test]
fn test_fault_record_filtering() {
    let records = vec![
        FaultRecord::builder()
            .stream_id(StreamID::new(1).unwrap())
            .pasid(PASID::new(0).unwrap())
            .address(IOVA::new(0x1000).unwrap())
            .fault_type(FaultType::TranslationFault)
            .build(),
        FaultRecord::builder()
            .stream_id(StreamID::new(2).unwrap())
            .pasid(PASID::new(0).unwrap())
            .address(IOVA::new(0x2000).unwrap())
            .fault_type(FaultType::PermissionFault)
            .build(),
        FaultRecord::builder()
            .stream_id(StreamID::new(3).unwrap())
            .pasid(PASID::new(0).unwrap())
            .address(IOVA::new(0x3000).unwrap())
            .fault_type(FaultType::TranslationFault)
            .build(),
    ];

    let translation_faults: Vec<_> = records
        .iter()
        .filter(|r| r.fault_type() == FaultType::TranslationFault)
        .collect();

    assert_eq!(translation_faults.len(), 2);
}

// ============================================================================
// Comprehensive Builder Pattern Tests
// ============================================================================

#[test]
fn test_builder_fluent_interface() {
    // Test that builder methods can be chained fluently
    let record = FaultRecord::builder()
        .stream_id(StreamID::new(1).unwrap())
        .pasid(PASID::new(2).unwrap())
        .address(IOVA::new(0x1000).unwrap())
        .fault_type(FaultType::TranslationFault)
        .access_type(AccessType::Read)
        .security_state(SecurityState::NonSecure)
        .timestamp(123)
        .build();

    assert_eq!(record.stream_id(), StreamID::new(1).unwrap());
    assert_eq!(record.timestamp(), 123);
}

#[test]
fn test_syndrome_builder_fluent_interface() {
    // Test FaultSyndrome builder chaining
    let syndrome = FaultSyndrome::builder()
        .syndrome_register(0x1234)
        .fault_level(2)
        .write_not_read(true)
        .valid_syndrome(true)
        .context_descriptor_index(99)
        .build();

    assert_eq!(syndrome.syndrome_register(), 0x1234);
    assert_eq!(syndrome.context_descriptor_index(), 99);
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_fault_syndrome_maximum_values() {
    let syndrome = FaultSyndrome::builder()
        .syndrome_register(u32::MAX)
        .fault_level(u8::MAX)
        .context_descriptor_index(u16::MAX)
        .build();

    assert_eq!(syndrome.syndrome_register(), u32::MAX);
    assert_eq!(syndrome.fault_level(), u8::MAX);
    assert_eq!(syndrome.context_descriptor_index(), u16::MAX);
}

#[test]
fn test_fault_record_maximum_timestamp() {
    let record = FaultRecord::builder()
        .stream_id(StreamID::new(1).unwrap())
        .pasid(PASID::new(0).unwrap())
        .address(IOVA::new(0x1000).unwrap())
        .timestamp(u64::MAX)
        .build();

    assert_eq!(record.timestamp(), u64::MAX);
}
