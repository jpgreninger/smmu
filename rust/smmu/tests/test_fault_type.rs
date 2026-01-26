//! Comprehensive tests for FaultType enum
//!
//! Tests cover:
//! - All 15 ARM SMMU v3 fault types
//! - Fault classification and categorization
//! - Fault severity levels
//! - From trait for fault classification
//! - Detailed ARM SMMU v3 specification compliance
//! - Copy, Clone, Debug, PartialEq, Eq traits

use smmu::{FaultType, ValidationError, FaultSeverity, FaultContext, StreamID, PASID, IOVA};

// ============================================================================
// Basic FaultType Tests - All 15 ARM SMMU v3 Fault Types
// ============================================================================

#[test]
fn test_fault_type_translation_fault() {
    let fault = FaultType::TranslationFault;
    assert_eq!(fault.code(), 0x01);
    assert_eq!(fault.name(), "Translation Fault");
    assert!(fault.is_translation_fault());
}

#[test]
fn test_fault_type_address_size_fault() {
    let fault = FaultType::AddressSizeFault;
    assert_eq!(fault.code(), 0x02);
    assert_eq!(fault.name(), "Address Size Fault");
    assert!(fault.is_address_fault());
}

#[test]
fn test_fault_type_access_flag_fault() {
    let fault = FaultType::AccessFlagFault;
    assert_eq!(fault.code(), 0x03);
    assert_eq!(fault.name(), "Access Flag Fault");
}

#[test]
fn test_fault_type_permission_fault() {
    let fault = FaultType::PermissionFault;
    assert_eq!(fault.code(), 0x04);
    assert_eq!(fault.name(), "Permission Fault");
    assert!(fault.is_permission_fault());
}

#[test]
fn test_fault_type_external_abort() {
    let fault = FaultType::ExternalAbort;
    assert_eq!(fault.code(), 0x05);
    assert_eq!(fault.name(), "External Abort");
    assert!(fault.is_external_fault());
}

#[test]
fn test_fault_type_tlb_conflict() {
    let fault = FaultType::TLBConflictAbort;
    assert_eq!(fault.code(), 0x06);
    assert_eq!(fault.name(), "TLB Conflict Abort");
}

#[test]
fn test_fault_type_unsupported_atomic() {
    let fault = FaultType::UnsupportedAtomicUpdate;
    assert_eq!(fault.code(), 0x07);
    assert_eq!(fault.name(), "Unsupported Atomic Update");
}

#[test]
fn test_fault_type_alignment_fault() {
    let fault = FaultType::AlignmentFault;
    assert_eq!(fault.code(), 0x08);
    assert_eq!(fault.name(), "Alignment Fault");
    assert!(fault.is_address_fault());
}

#[test]
fn test_fault_type_output_address_range() {
    let fault = FaultType::OutputAddressRangeFault;
    assert_eq!(fault.code(), 0x09);
    assert_eq!(fault.name(), "Output Address Range Fault");
}

#[test]
fn test_fault_type_bad_streamid() {
    let fault = FaultType::BadStreamID;
    assert_eq!(fault.code(), 0x0A);
    assert_eq!(fault.name(), "Bad StreamID");
    assert!(fault.is_configuration_fault());
}

#[test]
fn test_fault_type_cd_fetch() {
    let fault = FaultType::CDFetchFault;
    assert_eq!(fault.code(), 0x0B);
    assert_eq!(fault.name(), "Context Descriptor Fetch Fault");
    assert!(fault.is_configuration_fault());
}

#[test]
fn test_fault_type_bad_cd() {
    let fault = FaultType::BadCD;
    assert_eq!(fault.code(), 0x0C);
    assert_eq!(fault.name(), "Bad Context Descriptor");
    assert!(fault.is_configuration_fault());
}

#[test]
fn test_fault_type_walk_eabt() {
    let fault = FaultType::WalkEABT;
    assert_eq!(fault.code(), 0x0D);
    assert_eq!(fault.name(), "Page Table Walk External Abort");
    assert!(fault.is_external_fault());
}

#[test]
fn test_fault_type_bad_ste() {
    let fault = FaultType::BadSTE;
    assert_eq!(fault.code(), 0x0E);
    assert_eq!(fault.name(), "Bad Stream Table Entry");
    assert!(fault.is_configuration_fault());
}

#[test]
fn test_fault_type_ste_fetch() {
    let fault = FaultType::STEFetchFault;
    assert_eq!(fault.code(), 0x0F);
    assert_eq!(fault.name(), "Stream Table Entry Fetch Fault");
    assert!(fault.is_configuration_fault());
}

// ============================================================================
// Fault Classification Tests
// ============================================================================

#[test]
fn test_all_translation_faults() {
    let translation_faults = [
        FaultType::TranslationFault,
    ];

    for fault in &translation_faults {
        assert!(fault.is_translation_fault());
        assert!(!fault.is_permission_fault());
        assert!(!fault.is_external_fault());
    }
}

#[test]
fn test_all_permission_faults() {
    let permission_faults = [
        FaultType::PermissionFault,
        FaultType::AccessFlagFault,
    ];

    for fault in &permission_faults {
        assert!(fault.is_permission_fault() || fault.code() == 0x03);
        assert!(!fault.is_external_fault());
    }
}

#[test]
fn test_all_configuration_faults() {
    let config_faults = [
        FaultType::BadStreamID,
        FaultType::CDFetchFault,
        FaultType::BadCD,
        FaultType::BadSTE,
        FaultType::STEFetchFault,
    ];

    for fault in &config_faults {
        assert!(fault.is_configuration_fault());
        assert!(!fault.is_translation_fault());
    }
}

#[test]
fn test_all_address_faults() {
    let address_faults = [
        FaultType::AddressSizeFault,
        FaultType::AlignmentFault,
        FaultType::OutputAddressRangeFault,
    ];

    for fault in &address_faults {
        assert!(fault.is_address_fault());
    }
}

#[test]
fn test_all_external_faults() {
    let external_faults = [
        FaultType::ExternalAbort,
        FaultType::WalkEABT,
    ];

    for fault in &external_faults {
        assert!(fault.is_external_fault());
    }
}

// ============================================================================
// Fault Severity Tests
// ============================================================================

#[test]
fn test_fault_severity_critical() {
    let critical_faults = [
        FaultType::BadSTE,
        FaultType::BadCD,
        FaultType::BadStreamID,
    ];

    for fault in &critical_faults {
        assert_eq!(fault.severity(), FaultSeverity::Critical);
    }
}

#[test]
fn test_fault_severity_error() {
    let error_faults = [
        FaultType::TranslationFault,
        FaultType::PermissionFault,
        FaultType::ExternalAbort,
    ];

    for fault in &error_faults {
        assert_eq!(fault.severity(), FaultSeverity::Error);
    }
}

#[test]
fn test_fault_severity_warning() {
    let warning_faults = [
        FaultType::AccessFlagFault,
        FaultType::TLBConflictAbort,
    ];

    for fault in &warning_faults {
        assert_eq!(fault.severity(), FaultSeverity::Warning);
    }
}

// ============================================================================
// Fault Recoverability Tests
// ============================================================================

#[test]
fn test_recoverable_faults() {
    let recoverable = [
        FaultType::AccessFlagFault,  // Can set access flag
        FaultType::TLBConflictAbort, // Can invalidate TLB
    ];

    for fault in &recoverable {
        assert!(fault.is_recoverable());
    }
}

#[test]
fn test_non_recoverable_faults() {
    let non_recoverable = [
        FaultType::TranslationFault,
        FaultType::PermissionFault,
        FaultType::BadStreamID,
        FaultType::BadCD,
        FaultType::BadSTE,
    ];

    for fault in &non_recoverable {
        assert!(!fault.is_recoverable());
    }
}

// ============================================================================
// ARM SMMU v3 Encoding Tests
// ============================================================================

#[test]
fn test_fault_type_codes_unique() {
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

    let mut codes = std::collections::HashSet::new();
    for fault in &all_faults {
        let code = fault.code();
        assert!(codes.insert(code), "Duplicate fault code: {:#x}", code);
    }

    assert_eq!(codes.len(), 15, "Should have exactly 15 unique fault codes");
}

#[test]
fn test_fault_type_from_code() {
    assert_eq!(FaultType::from_code(0x01), Ok(FaultType::TranslationFault));
    assert_eq!(FaultType::from_code(0x04), Ok(FaultType::PermissionFault));
    assert_eq!(FaultType::from_code(0x0A), Ok(FaultType::BadStreamID));
    assert_eq!(FaultType::from_code(0x0F), Ok(FaultType::STEFetchFault));
}

#[test]
fn test_fault_type_from_code_invalid() {
    let result = FaultType::from_code(0x00);
    assert!(result.is_err());

    let result = FaultType::from_code(0xFF);
    assert!(result.is_err());
}

#[test]
fn test_fault_type_round_trip() {
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

    for fault in &all_faults {
        let code = fault.code();
        let decoded = FaultType::from_code(code).unwrap();
        assert_eq!(decoded, *fault);
    }
}

// ============================================================================
// Trait Implementation Tests
// ============================================================================

#[test]
fn test_fault_type_copy_clone() {
    let fault1 = FaultType::TranslationFault;
    let fault2 = fault1; // Copy
    let fault3 = fault1.clone(); // Clone

    assert_eq!(fault1, fault2);
    assert_eq!(fault1, fault3);
}

#[test]
fn test_fault_type_equality() {
    let fault1 = FaultType::TranslationFault;
    let fault2 = FaultType::TranslationFault;
    let fault3 = FaultType::PermissionFault;

    assert_eq!(fault1, fault2);
    assert_ne!(fault1, fault3);
}

#[test]
fn test_fault_type_debug() {
    let fault = FaultType::TranslationFault;
    let debug = format!("{:?}", fault);
    assert!(debug.contains("Translation") || debug.contains("Fault"));
}

#[test]
fn test_fault_type_display() {
    let fault = FaultType::PermissionFault;
    let display = format!("{}", fault);
    assert_eq!(display, "Permission Fault");
}

// ============================================================================
// Fault Description Tests
// ============================================================================

#[test]
fn test_fault_descriptions() {
    let fault = FaultType::TranslationFault;
    let desc = fault.description();
    assert!(!desc.is_empty());
    assert!(desc.contains("translation") || desc.contains("Translation"));
}

#[test]
fn test_all_faults_have_descriptions() {
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

    for fault in &all_faults {
        let desc = fault.description();
        assert!(!desc.is_empty(), "Fault {:?} missing description", fault);
    }
}

// ============================================================================
// Fault Stage Attribution Tests
// ============================================================================

#[test]
fn test_stage_1_faults() {
    let stage1_faults = [
        FaultType::TranslationFault,
        FaultType::PermissionFault,
        FaultType::AccessFlagFault,
    ];

    for fault in &stage1_faults {
        assert!(fault.can_occur_in_stage1());
    }
}

#[test]
fn test_stage_2_faults() {
    let stage2_faults = [
        FaultType::TranslationFault,
        FaultType::PermissionFault,
        FaultType::AccessFlagFault,
    ];

    for fault in &stage2_faults {
        assert!(fault.can_occur_in_stage2());
    }
}

#[test]
fn test_stage_agnostic_faults() {
    let agnostic_faults = [
        FaultType::BadStreamID,
        FaultType::BadSTE,
        FaultType::ExternalAbort,
    ];

    for fault in &agnostic_faults {
        assert!(fault.is_stage_agnostic());
    }
}

// ============================================================================
// Validation Tests
// ============================================================================

#[test]
fn test_validate_fault_context() {
    use smmu::{FaultContext, StreamID, PASID, IOVA};

    let fault = FaultType::TranslationFault;
    let stream_id = StreamID::new(42).unwrap();
    let pasid = PASID::new(1).unwrap();
    let addr = IOVA::new(0x1000).unwrap();

    let context = FaultContext::new(fault, stream_id, pasid, addr);
    assert_eq!(context.fault_type(), fault);
    assert_eq!(context.stream_id(), stream_id);
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_all_fault_codes_in_range() {
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

    for fault in &all_faults {
        let code = fault.code();
        assert!(code >= 0x01 && code <= 0x0F, "Fault code out of range: {:#x}", code);
    }
}

// ============================================================================
// Performance Tests
// ============================================================================

#[test]
fn test_fault_classification_performance() {
    const ITERATIONS: usize = 100_000;
    let fault = FaultType::TranslationFault;

    let start = std::time::Instant::now();
    for _ in 0..ITERATIONS {
        let _t = fault.is_translation_fault();
        let _p = fault.is_permission_fault();
        let _c = fault.is_configuration_fault();
    }
    let duration = start.elapsed();

    assert!(
        duration.as_millis() < 10,
        "Fault classification too slow: {:?}",
        duration
    );
}
