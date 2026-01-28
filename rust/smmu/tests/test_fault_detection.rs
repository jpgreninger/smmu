//! ARM SMMU v3 Section 6.1: Fault Detection and Classification Tests
//!
//! Comprehensive test suite for fault detection covering all 15 ARM SMMU v3 fault types
//! with detailed context capture, permission checking, and address validation.

use smmu::types::{
    AccessType, FaultContext, FaultRecord, FaultSeverity, FaultSyndrome, FaultType, IOVA, PASID,
    PagePermissions, SecurityState, StreamID, PA,
};

// Test helper to create StreamID safely
fn test_stream_id(value: u32) -> StreamID {
    StreamID::new(value).expect("Valid test StreamID")
}

// Test helper to create PASID safely
fn test_pasid(value: u32) -> PASID {
    PASID::new(value).expect("Valid test PASID")
}

// Test helper to create IOVA safely
fn test_iova(value: u64) -> IOVA {
    IOVA::new(value).expect("Valid test IOVA")
}

// Test helper to create PA safely
fn test_pa(value: u64) -> PA {
    PA::new(value).expect("Valid test PA")
}

// ============================================================================
// Test Suite 1: Translation Fault Detection with Context (5 hours)
// ============================================================================

#[test]
fn test_translation_fault_basic() {
    // Test basic translation fault creation with full context
    let fault = FaultRecord::builder()
        .stream_id(test_stream_id(42))
        .pasid(test_pasid(7))
        .address(test_iova(0x1000))
        .fault_type(FaultType::TranslationFault)
        .access_type(AccessType::Read)
        .security_state(SecurityState::NonSecure)
        .timestamp(12345)
        .build();

    assert_eq!(fault.stream_id(), test_stream_id(42));
    assert_eq!(fault.pasid(), test_pasid(7));
    assert_eq!(fault.address(), test_iova(0x1000));
    assert_eq!(fault.fault_type(), FaultType::TranslationFault);
    assert_eq!(fault.access_type(), AccessType::Read);
    assert_eq!(fault.security_state(), SecurityState::NonSecure);
    assert_eq!(fault.timestamp(), 12345);
}

#[test]
fn test_translation_fault_all_access_types() {
    // Test translation fault detection for Read, Write, Execute
    let access_types = [AccessType::Read, AccessType::Write, AccessType::Execute];

    for (idx, access_type) in access_types.iter().enumerate() {
        let fault = FaultRecord::builder()
            .stream_id(test_stream_id(100 + idx as u32))
            .pasid(test_pasid(0))
            .address(test_iova(0x1000 * (idx as u64 + 1)))
            .fault_type(FaultType::TranslationFault)
            .access_type(*access_type)
            .security_state(SecurityState::NonSecure)
            .build();

        assert_eq!(fault.access_type(), *access_type);
        assert_eq!(fault.fault_type(), FaultType::TranslationFault);
    }
}

#[test]
fn test_translation_fault_security_states() {
    // Test translation fault with different security states
    let security_states = [
        SecurityState::NonSecure,
        SecurityState::Secure,
        SecurityState::Realm,
    ];

    for (idx, security_state) in security_states.iter().enumerate() {
        let fault = FaultRecord::builder()
            .stream_id(test_stream_id(200 + idx as u32))
            .pasid(test_pasid(0))
            .address(test_iova(0x2000))
            .fault_type(FaultType::TranslationFault)
            .access_type(AccessType::Read)
            .security_state(*security_state)
            .build();

        assert_eq!(fault.security_state(), *security_state);
    }
}

#[test]
fn test_translation_fault_syndrome_generation() {
    // Test fault syndrome generation with complete ARM SMMU v3 details
    let syndrome = FaultSyndrome::builder()
        .syndrome_register(0x12345678)
        .fault_level(2)
        .write_not_read(true)
        .valid_syndrome(true)
        .context_descriptor_index(5)
        .build();

    let fault = FaultRecord::builder()
        .stream_id(test_stream_id(300))
        .pasid(test_pasid(1))
        .address(test_iova(0x3000))
        .fault_type(FaultType::TranslationFault)
        .access_type(AccessType::Write)
        .security_state(SecurityState::NonSecure)
        .syndrome(syndrome.clone())
        .timestamp(67890)
        .build();

    assert!(fault.syndrome().valid_syndrome());
    assert_eq!(fault.syndrome().syndrome_register(), 0x12345678);
    assert_eq!(fault.syndrome().fault_level(), 2);
    assert!(fault.syndrome().write_not_read());
    assert_eq!(fault.syndrome().context_descriptor_index(), 5);
}

#[test]
fn test_fault_classification_by_type() {
    // Test that FaultType enum provides correct classification
    assert!(FaultType::TranslationFault.is_translation_fault());
    assert!(!FaultType::PermissionFault.is_translation_fault());

    assert!(FaultType::PermissionFault.is_permission_fault());
    assert!(FaultType::AccessFlagFault.is_permission_fault());

    assert!(FaultType::BadStreamID.is_configuration_fault());
    assert!(FaultType::BadCD.is_configuration_fault());
    assert!(FaultType::BadSTE.is_configuration_fault());

    assert!(FaultType::AddressSizeFault.is_address_fault());
    assert!(FaultType::AlignmentFault.is_address_fault());

    assert!(FaultType::ExternalAbort.is_external_fault());
    assert!(FaultType::WalkEABT.is_external_fault());
}

#[test]
fn test_fault_severity_levels() {
    // Test fault severity classification
    assert_eq!(
        FaultType::BadSTE.severity(),
        FaultSeverity::Critical
    );
    assert_eq!(
        FaultType::BadCD.severity(),
        FaultSeverity::Critical
    );
    assert_eq!(
        FaultType::TranslationFault.severity(),
        FaultSeverity::Error
    );
    assert_eq!(
        FaultType::PermissionFault.severity(),
        FaultSeverity::Error
    );
    assert_eq!(
        FaultType::AccessFlagFault.severity(),
        FaultSeverity::Warning
    );
}

#[test]
fn test_fault_recoverability() {
    // Test fault recoverability classification
    assert!(FaultType::AccessFlagFault.is_recoverable());
    assert!(FaultType::TLBConflictAbort.is_recoverable());
    assert!(!FaultType::TranslationFault.is_recoverable());
    assert!(!FaultType::PermissionFault.is_recoverable());
}

// ============================================================================
// Test Suite 2: Permission Fault Checking (4 hours)
// ============================================================================

#[test]
fn test_permission_fault_read_violation() {
    // Test read permission violation
    let fault = FaultRecord::builder()
        .stream_id(test_stream_id(400))
        .pasid(test_pasid(2))
        .address(test_iova(0x4000))
        .fault_type(FaultType::PermissionFault)
        .access_type(AccessType::Read)
        .security_state(SecurityState::NonSecure)
        .build();

    assert_eq!(fault.fault_type(), FaultType::PermissionFault);
    assert_eq!(fault.access_type(), AccessType::Read);
}

#[test]
fn test_permission_fault_write_violation() {
    // Test write permission violation
    let fault = FaultRecord::builder()
        .stream_id(test_stream_id(401))
        .pasid(test_pasid(2))
        .address(test_iova(0x4000))
        .fault_type(FaultType::PermissionFault)
        .access_type(AccessType::Write)
        .security_state(SecurityState::NonSecure)
        .build();

    assert_eq!(fault.fault_type(), FaultType::PermissionFault);
    assert_eq!(fault.access_type(), AccessType::Write);
}

#[test]
fn test_permission_fault_execute_violation() {
    // Test execute permission violation
    let fault = FaultRecord::builder()
        .stream_id(test_stream_id(402))
        .pasid(test_pasid(2))
        .address(test_iova(0x4000))
        .fault_type(FaultType::PermissionFault)
        .access_type(AccessType::Execute)
        .security_state(SecurityState::NonSecure)
        .build();

    assert_eq!(fault.fault_type(), FaultType::PermissionFault);
    assert_eq!(fault.access_type(), AccessType::Execute);
}

#[test]
fn test_page_permissions_bitwise_checks() {
    // Test bitwise permission checking logic
    let no_perms = PagePermissions::new(false, false, false);
    assert!(!no_perms.read());
    assert!(!no_perms.write());
    assert!(!no_perms.execute());

    let read_only = PagePermissions::new(true, false, false);
    assert!(read_only.read());
    assert!(!read_only.write());
    assert!(!read_only.execute());

    let read_write = PagePermissions::new(true, true, false);
    assert!(read_write.read());
    assert!(read_write.write());
    assert!(!read_write.execute());

    let full_perms = PagePermissions::new(true, true, true);
    assert!(full_perms.read());
    assert!(full_perms.write());
    assert!(full_perms.execute());
}

#[test]
fn test_permission_context_capture() {
    // Test that permission fault captures full context
    let syndrome = FaultSyndrome::builder()
        .syndrome_register(0x04000000) // Permission fault syndrome
        .fault_level(3)
        .write_not_read(false) // Read access
        .valid_syndrome(true)
        .build();

    let fault = FaultRecord::builder()
        .stream_id(test_stream_id(403))
        .pasid(test_pasid(3))
        .address(test_iova(0x5000))
        .fault_type(FaultType::PermissionFault)
        .access_type(AccessType::Read)
        .security_state(SecurityState::Secure)
        .syndrome(syndrome)
        .timestamp(11111)
        .build();

    assert_eq!(fault.syndrome().syndrome_register(), 0x04000000);
    assert_eq!(fault.syndrome().fault_level(), 3);
    assert!(!fault.syndrome().write_not_read()); // Read access
}

// ============================================================================
// Test Suite 3: Address Range Validation (4 hours)
// ============================================================================

#[test]
fn test_address_size_fault_32bit() {
    // Test 32-bit address size violation
    let fault = FaultRecord::builder()
        .stream_id(test_stream_id(500))
        .pasid(test_pasid(0))
        .address(test_iova(0x100000000)) // Beyond 32-bit
        .fault_type(FaultType::AddressSizeFault)
        .access_type(AccessType::Read)
        .security_state(SecurityState::NonSecure)
        .build();

    assert_eq!(fault.fault_type(), FaultType::AddressSizeFault);
    assert!(fault.address().as_u64() > 0xFFFFFFFF);
}

#[test]
fn test_address_size_fault_48bit() {
    // Test 48-bit address size violation
    let addr_48bit_max = 0x0000_FFFF_FFFF_FFFF;
    let addr_beyond = addr_48bit_max + 1;

    let fault = FaultRecord::builder()
        .stream_id(test_stream_id(501))
        .pasid(test_pasid(0))
        .address(test_iova(addr_beyond))
        .fault_type(FaultType::AddressSizeFault)
        .access_type(AccessType::Read)
        .security_state(SecurityState::NonSecure)
        .build();

    assert_eq!(fault.fault_type(), FaultType::AddressSizeFault);
}

#[test]
fn test_alignment_fault_detection() {
    // Test alignment fault detection
    let unaligned_addresses = [0x1001, 0x2003, 0x3007, 0x400F];

    for (idx, &addr) in unaligned_addresses.iter().enumerate() {
        let fault = FaultRecord::builder()
            .stream_id(test_stream_id(510 + idx as u32))
            .pasid(test_pasid(0))
            .address(test_iova(addr))
            .fault_type(FaultType::AlignmentFault)
            .access_type(AccessType::Read)
            .security_state(SecurityState::NonSecure)
            .build();

        assert_eq!(fault.fault_type(), FaultType::AlignmentFault);
        assert!(fault.address().as_u64() & 0xFFF != 0); // Not page-aligned
    }
}

#[test]
fn test_output_address_range_fault() {
    // Test output address range fault (PA exceeds supported range)
    let fault = FaultRecord::builder()
        .stream_id(test_stream_id(520))
        .pasid(test_pasid(0))
        .address(test_iova(0x6000))
        .fault_type(FaultType::OutputAddressRangeFault)
        .access_type(AccessType::Read)
        .security_state(SecurityState::NonSecure)
        .build();

    assert_eq!(fault.fault_type(), FaultType::OutputAddressRangeFault);
}

#[test]
fn test_address_boundary_checking() {
    // Test address boundary validation
    let test_cases = [
        (0x0000_0000_0000_0000, true),  // Min valid
        (0x0000_0000_0000_1000, true),  // Page-aligned
        (0x0000_0FFF_FFFF_F000, true),  // Near 52-bit max
        (0x000F_FFFF_FFFF_FFFF, true),  // 52-bit max
    ];

    for (addr, is_valid) in test_cases {
        let iova = test_iova(addr);
        assert_eq!(iova.as_u64(), addr);

        if is_valid {
            // Valid addresses should be representable
            assert!(iova.as_u64() <= 0x000F_FFFF_FFFF_FFFF);
        }
    }
}

#[test]
fn test_address_validation_with_context() {
    // Test that address validation failures capture full context
    let syndrome = FaultSyndrome::builder()
        .syndrome_register(0x02000000) // Address size fault syndrome
        .fault_level(0)
        .write_not_read(false)
        .valid_syndrome(true)
        .build();

    let fault = FaultRecord::builder()
        .stream_id(test_stream_id(530))
        .pasid(test_pasid(5))
        .address(test_iova(0x100000000))
        .fault_type(FaultType::AddressSizeFault)
        .access_type(AccessType::Write)
        .security_state(SecurityState::NonSecure)
        .syndrome(syndrome)
        .build();

    assert!(fault.syndrome().valid_syndrome());
    assert_eq!(fault.syndrome().syndrome_register(), 0x02000000);
}

// ============================================================================
// Test Suite 4: Comprehensive Fault Categorization (4 hours)
// ============================================================================

#[test]
fn test_all_15_fault_types() {
    // Test all 15 ARM SMMU v3 fault types
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

    assert_eq!(fault_types.len(), 15);

    for (idx, fault_type) in fault_types.iter().enumerate() {
        let fault = FaultRecord::builder()
            .stream_id(test_stream_id(600 + idx as u32))
            .pasid(test_pasid(0))
            .address(test_iova(0x7000 + (idx as u64 * 0x1000)))
            .fault_type(*fault_type)
            .access_type(AccessType::Read)
            .security_state(SecurityState::NonSecure)
            .build();

        assert_eq!(fault.fault_type(), *fault_type);
        assert_eq!(fault.fault_type().code(), (idx + 1) as u8);
    }
}

#[test]
fn test_fault_type_codes() {
    // Test ARM SMMU v3 fault code mapping (0x01-0x0F)
    assert_eq!(FaultType::TranslationFault.code(), 0x01);
    assert_eq!(FaultType::AddressSizeFault.code(), 0x02);
    assert_eq!(FaultType::AccessFlagFault.code(), 0x03);
    assert_eq!(FaultType::PermissionFault.code(), 0x04);
    assert_eq!(FaultType::ExternalAbort.code(), 0x05);
    assert_eq!(FaultType::TLBConflictAbort.code(), 0x06);
    assert_eq!(FaultType::UnsupportedAtomicUpdate.code(), 0x07);
    assert_eq!(FaultType::AlignmentFault.code(), 0x08);
    assert_eq!(FaultType::OutputAddressRangeFault.code(), 0x09);
    assert_eq!(FaultType::BadStreamID.code(), 0x0A);
    assert_eq!(FaultType::CDFetchFault.code(), 0x0B);
    assert_eq!(FaultType::BadCD.code(), 0x0C);
    assert_eq!(FaultType::WalkEABT.code(), 0x0D);
    assert_eq!(FaultType::BadSTE.code(), 0x0E);
    assert_eq!(FaultType::STEFetchFault.code(), 0x0F);
}

#[test]
fn test_fault_stage_attribution() {
    // Test fault stage attribution (Stage 1 vs Stage 2)

    // Stage 1 faults
    assert!(FaultType::TranslationFault.can_occur_in_stage1());
    assert!(FaultType::PermissionFault.can_occur_in_stage1());
    assert!(FaultType::BadCD.can_occur_in_stage1());

    // Stage 2 faults
    assert!(FaultType::TranslationFault.can_occur_in_stage2());
    assert!(FaultType::PermissionFault.can_occur_in_stage2());

    // Stage-agnostic faults
    assert!(FaultType::BadStreamID.is_stage_agnostic());
    assert!(FaultType::BadSTE.is_stage_agnostic());

    // Cannot occur in Stage 2
    assert!(!FaultType::BadCD.can_occur_in_stage2());
    assert!(!FaultType::CDFetchFault.can_occur_in_stage2());
}

#[test]
fn test_fault_priority_ordering() {
    // Test fault priority ordering per ARM SMMU v3 spec
    // Critical faults (configuration) have highest priority
    let critical_faults = [
        FaultType::BadSTE,
        FaultType::BadCD,
        FaultType::BadStreamID,
    ];

    for fault_type in critical_faults {
        assert_eq!(fault_type.severity(), FaultSeverity::Critical);
    }

    // Error faults have medium priority
    let error_faults = [
        FaultType::TranslationFault,
        FaultType::PermissionFault,
        FaultType::AddressSizeFault,
    ];

    for fault_type in error_faults {
        assert_eq!(fault_type.severity(), FaultSeverity::Error);
    }

    // Warning faults have lowest priority
    let warning_faults = [
        FaultType::AccessFlagFault,
        FaultType::TLBConflictAbort,
    ];

    for fault_type in warning_faults {
        assert_eq!(fault_type.severity(), FaultSeverity::Warning);
    }
}

#[test]
fn test_fault_names_and_descriptions() {
    // Test that all fault types have proper names and descriptions
    let all_faults = [
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

    for fault_type in all_faults {
        let name = fault_type.name();
        let description = fault_type.description();

        assert!(!name.is_empty());
        assert!(!description.is_empty());
        assert!(description.len() > 10); // Meaningful description
    }
}

#[test]
fn test_fault_context_comprehensive() {
    // Test FaultContext with all details
    let context = FaultContext::new(
        FaultType::PermissionFault,
        test_stream_id(700),
        test_pasid(10),
        test_iova(0x8000),
    );

    assert_eq!(context.fault_type(), FaultType::PermissionFault);
    assert_eq!(context.stream_id(), test_stream_id(700));
    assert_eq!(context.pasid(), test_pasid(10));
    assert_eq!(context.address(), test_iova(0x8000));
}

// ============================================================================
// Integration Tests: Complete Fault Detection Scenarios
// ============================================================================

#[test]
fn test_complete_translation_fault_scenario() {
    // Complete scenario: Translation fault with full syndrome
    let syndrome = FaultSyndrome::builder()
        .syndrome_register(0x01000000)
        .fault_level(2)
        .write_not_read(false)
        .valid_syndrome(true)
        .context_descriptor_index(0)
        .build();

    let fault = FaultRecord::builder()
        .stream_id(test_stream_id(800))
        .pasid(test_pasid(15))
        .address(test_iova(0x9000))
        .fault_type(FaultType::TranslationFault)
        .access_type(AccessType::Read)
        .security_state(SecurityState::NonSecure)
        .syndrome(syndrome)
        .timestamp(99999)
        .build();

    // Verify all fault details
    assert_eq!(fault.stream_id(), test_stream_id(800));
    assert_eq!(fault.pasid(), test_pasid(15));
    assert_eq!(fault.address(), test_iova(0x9000));
    assert_eq!(fault.fault_type(), FaultType::TranslationFault);
    assert_eq!(fault.access_type(), AccessType::Read);
    assert!(fault.fault_type().is_translation_fault());
    assert_eq!(fault.fault_type().severity(), FaultSeverity::Error);
    assert!(!fault.fault_type().is_recoverable());
}

#[test]
fn test_complete_permission_fault_scenario() {
    // Complete scenario: Permission fault with write access
    let syndrome = FaultSyndrome::builder()
        .syndrome_register(0x04000000)
        .fault_level(3)
        .write_not_read(true) // Write access
        .valid_syndrome(true)
        .build();

    let fault = FaultRecord::builder()
        .stream_id(test_stream_id(801))
        .pasid(test_pasid(20))
        .address(test_iova(0xA000))
        .fault_type(FaultType::PermissionFault)
        .access_type(AccessType::Write)
        .security_state(SecurityState::Secure)
        .syndrome(syndrome)
        .timestamp(100000)
        .build();

    // Verify permission fault specifics
    assert!(fault.fault_type().is_permission_fault());
    assert!(fault.syndrome().write_not_read());
    assert_eq!(fault.security_state(), SecurityState::Secure);
}

#[test]
fn test_complete_configuration_fault_scenario() {
    // Complete scenario: Configuration fault (BadCD)
    let fault = FaultRecord::builder()
        .stream_id(test_stream_id(802))
        .pasid(test_pasid(0))
        .address(test_iova(0xB000))
        .fault_type(FaultType::BadCD)
        .access_type(AccessType::Read)
        .security_state(SecurityState::NonSecure)
        .build();

    // Verify configuration fault
    assert!(fault.fault_type().is_configuration_fault());
    assert_eq!(fault.fault_type().severity(), FaultSeverity::Critical);
    assert!(!fault.fault_type().is_recoverable());
}

#[test]
fn test_fault_record_default_creation() {
    // Test default fault record creation
    let fault = FaultRecord::default();

    assert_eq!(fault.stream_id(), test_stream_id(0));
    assert_eq!(fault.pasid(), test_pasid(0));
    assert_eq!(fault.address(), test_iova(0));
    assert_eq!(fault.fault_type(), FaultType::TranslationFault);
    assert_eq!(fault.access_type(), AccessType::Read);
    assert_eq!(fault.security_state(), SecurityState::NonSecure);
}

#[test]
fn test_fault_syndrome_default_creation() {
    // Test default fault syndrome creation
    let syndrome = FaultSyndrome::default();

    assert_eq!(syndrome.syndrome_register(), 0);
    assert_eq!(syndrome.fault_level(), 0);
    assert!(!syndrome.write_not_read());
    assert!(syndrome.valid_syndrome());
    assert_eq!(syndrome.context_descriptor_index(), 0);
}

#[test]
fn test_fault_type_from_code() {
    // Test creating fault types from codes
    for code in 0x01..=0x0F {
        let result = FaultType::from_code(code);
        assert!(result.is_ok());
        let fault_type = result.unwrap();
        assert_eq!(fault_type.code(), code);
    }

    // Test invalid codes
    assert!(FaultType::from_code(0x00).is_err());
    assert!(FaultType::from_code(0x10).is_err());
    assert!(FaultType::from_code(0xFF).is_err());
}
