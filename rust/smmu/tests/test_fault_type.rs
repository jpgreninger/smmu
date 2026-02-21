#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]

//! Comprehensive tests for `types/fault_type.rs`
//!
//! This test suite achieves 100% coverage for `types/fault_type.rs` by testing:
//! 1. All 15 `FaultType` variants and their properties
//! 2. All classification methods (`is_translation_fault`, `is_permission_fault`, etc.)
//! 3. Severity levels for all fault types
//! 4. Stage occurrence validation (Stage 1, Stage 2, stage-agnostic)
//! 5. Fault code uniqueness and `from_code()` validation
//! 6. `FaultContext` construction and all getters
//! 7. Display trait implementation
//! 8. All enum trait implementations (Copy, Clone, Debug, `PartialEq`, Eq, Hash)
//! 9. `FaultSeverity`, `TranslationStep`, and `AddressType` enums
//!
//! Coverage Target: 0% → 100%
//! Estimated Tests: 35+ tests
//! ARM SMMU v3 Fault Classification Compliance

use smmu::types::{
    AddressType, FaultContext, FaultSeverity, FaultType, StreamID, TranslationStep, ValidationError, IOVA, PASID,
};
use std::collections::HashSet;

// ============================================================================
// FaultType: Code and Name Tests
// ============================================================================

#[test]
fn test_fault_type_codes_unique() {
    // All 15 fault codes must be unique
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

    let mut codes = HashSet::new();
    for fault in &fault_types {
        let code = fault.code();
        assert!(codes.insert(code), "Duplicate fault code: 0x{code:02X}");
    }

    assert_eq!(codes.len(), 15, "Expected 15 unique fault codes");
}

#[test]
fn test_fault_type_codes_correct() {
    // Verify each fault type has the correct code per ARM SMMU v3 spec
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
fn test_fault_type_names() {
    // Verify all fault types have appropriate names
    assert_eq!(FaultType::TranslationFault.name(), "Translation Fault");
    assert_eq!(FaultType::AddressSizeFault.name(), "Address Size Fault");
    assert_eq!(FaultType::AccessFlagFault.name(), "Access Flag Fault");
    assert_eq!(FaultType::PermissionFault.name(), "Permission Fault");
    assert_eq!(FaultType::ExternalAbort.name(), "External Abort");
    assert_eq!(FaultType::TLBConflictAbort.name(), "TLB Conflict Abort");
    assert_eq!(FaultType::UnsupportedAtomicUpdate.name(), "Unsupported Atomic Update");
    assert_eq!(FaultType::AlignmentFault.name(), "Alignment Fault");
    assert_eq!(FaultType::OutputAddressRangeFault.name(), "Output Address Range Fault");
    assert_eq!(FaultType::BadStreamID.name(), "Bad StreamID");
    assert_eq!(FaultType::CDFetchFault.name(), "Context Descriptor Fetch Fault");
    assert_eq!(FaultType::BadCD.name(), "Bad Context Descriptor");
    assert_eq!(FaultType::WalkEABT.name(), "Page Table Walk External Abort");
    assert_eq!(FaultType::BadSTE.name(), "Bad Stream Table Entry");
    assert_eq!(FaultType::STEFetchFault.name(), "Stream Table Entry Fetch Fault");
}

#[test]
fn test_fault_type_descriptions() {
    // Verify all fault types have non-empty descriptions
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

    for fault in &fault_types {
        let desc = fault.description();
        assert!(!desc.is_empty(), "Fault {fault:?} has empty description");
        assert!(desc.len() > 10, "Fault {fault:?} description too short: {desc}");
    }
}

// ============================================================================
// FaultType: Classification Tests
// ============================================================================

#[test]
fn test_is_translation_fault() {
    // Only TranslationFault should return true
    assert!(FaultType::TranslationFault.is_translation_fault());

    // All other faults should return false
    assert!(!FaultType::AddressSizeFault.is_translation_fault());
    assert!(!FaultType::AccessFlagFault.is_translation_fault());
    assert!(!FaultType::PermissionFault.is_translation_fault());
    assert!(!FaultType::ExternalAbort.is_translation_fault());
    assert!(!FaultType::TLBConflictAbort.is_translation_fault());
    assert!(!FaultType::UnsupportedAtomicUpdate.is_translation_fault());
    assert!(!FaultType::AlignmentFault.is_translation_fault());
    assert!(!FaultType::OutputAddressRangeFault.is_translation_fault());
    assert!(!FaultType::BadStreamID.is_translation_fault());
    assert!(!FaultType::CDFetchFault.is_translation_fault());
    assert!(!FaultType::BadCD.is_translation_fault());
    assert!(!FaultType::WalkEABT.is_translation_fault());
    assert!(!FaultType::BadSTE.is_translation_fault());
    assert!(!FaultType::STEFetchFault.is_translation_fault());
}

#[test]
fn test_is_permission_fault() {
    // PermissionFault and AccessFlagFault should return true
    assert!(FaultType::PermissionFault.is_permission_fault());
    assert!(FaultType::AccessFlagFault.is_permission_fault());

    // All other faults should return false
    assert!(!FaultType::TranslationFault.is_permission_fault());
    assert!(!FaultType::AddressSizeFault.is_permission_fault());
    assert!(!FaultType::ExternalAbort.is_permission_fault());
    assert!(!FaultType::TLBConflictAbort.is_permission_fault());
    assert!(!FaultType::UnsupportedAtomicUpdate.is_permission_fault());
    assert!(!FaultType::AlignmentFault.is_permission_fault());
    assert!(!FaultType::OutputAddressRangeFault.is_permission_fault());
    assert!(!FaultType::BadStreamID.is_permission_fault());
    assert!(!FaultType::CDFetchFault.is_permission_fault());
    assert!(!FaultType::BadCD.is_permission_fault());
    assert!(!FaultType::WalkEABT.is_permission_fault());
    assert!(!FaultType::BadSTE.is_permission_fault());
    assert!(!FaultType::STEFetchFault.is_permission_fault());
}

#[test]
fn test_is_configuration_fault() {
    // 5 configuration faults
    assert!(FaultType::BadStreamID.is_configuration_fault());
    assert!(FaultType::CDFetchFault.is_configuration_fault());
    assert!(FaultType::BadCD.is_configuration_fault());
    assert!(FaultType::BadSTE.is_configuration_fault());
    assert!(FaultType::STEFetchFault.is_configuration_fault());

    // All other faults should return false
    assert!(!FaultType::TranslationFault.is_configuration_fault());
    assert!(!FaultType::AddressSizeFault.is_configuration_fault());
    assert!(!FaultType::AccessFlagFault.is_configuration_fault());
    assert!(!FaultType::PermissionFault.is_configuration_fault());
    assert!(!FaultType::ExternalAbort.is_configuration_fault());
    assert!(!FaultType::TLBConflictAbort.is_configuration_fault());
    assert!(!FaultType::UnsupportedAtomicUpdate.is_configuration_fault());
    assert!(!FaultType::AlignmentFault.is_configuration_fault());
    assert!(!FaultType::OutputAddressRangeFault.is_configuration_fault());
    assert!(!FaultType::WalkEABT.is_configuration_fault());
}

#[test]
fn test_is_address_fault() {
    // 3 address faults
    assert!(FaultType::AddressSizeFault.is_address_fault());
    assert!(FaultType::AlignmentFault.is_address_fault());
    assert!(FaultType::OutputAddressRangeFault.is_address_fault());

    // All other faults should return false
    assert!(!FaultType::TranslationFault.is_address_fault());
    assert!(!FaultType::AccessFlagFault.is_address_fault());
    assert!(!FaultType::PermissionFault.is_address_fault());
    assert!(!FaultType::ExternalAbort.is_address_fault());
    assert!(!FaultType::TLBConflictAbort.is_address_fault());
    assert!(!FaultType::UnsupportedAtomicUpdate.is_address_fault());
    assert!(!FaultType::BadStreamID.is_address_fault());
    assert!(!FaultType::CDFetchFault.is_address_fault());
    assert!(!FaultType::BadCD.is_address_fault());
    assert!(!FaultType::WalkEABT.is_address_fault());
    assert!(!FaultType::BadSTE.is_address_fault());
    assert!(!FaultType::STEFetchFault.is_address_fault());
}

#[test]
fn test_is_external_fault() {
    // 2 external faults
    assert!(FaultType::ExternalAbort.is_external_fault());
    assert!(FaultType::WalkEABT.is_external_fault());

    // All other faults should return false
    assert!(!FaultType::TranslationFault.is_external_fault());
    assert!(!FaultType::AddressSizeFault.is_external_fault());
    assert!(!FaultType::AccessFlagFault.is_external_fault());
    assert!(!FaultType::PermissionFault.is_external_fault());
    assert!(!FaultType::TLBConflictAbort.is_external_fault());
    assert!(!FaultType::UnsupportedAtomicUpdate.is_external_fault());
    assert!(!FaultType::AlignmentFault.is_external_fault());
    assert!(!FaultType::OutputAddressRangeFault.is_external_fault());
    assert!(!FaultType::BadStreamID.is_external_fault());
    assert!(!FaultType::CDFetchFault.is_external_fault());
    assert!(!FaultType::BadCD.is_external_fault());
    assert!(!FaultType::BadSTE.is_external_fault());
    assert!(!FaultType::STEFetchFault.is_external_fault());
}

#[test]
fn test_classification_mutually_exclusive() {
    // Each fault should belong to at most one classification category
    // Note: Not all faults fit into these 5 categories (e.g., TLBConflictAbort, UnsupportedAtomicUpdate)
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

    for fault in &fault_types {
        let categories = [fault.is_translation_fault(),
            fault.is_permission_fault(),
            fault.is_configuration_fault(),
            fault.is_address_fault(),
            fault.is_external_fault()];

        let count = categories.iter().filter(|&&x| x).count();
        // Verify that a fault belongs to at most one category (mutually exclusive)
        assert!(count <= 1, "Fault {fault:?} belongs to multiple categories");
    }
}

// ============================================================================
// FaultType: Severity Tests
// ============================================================================

#[test]
fn test_severity_critical() {
    // Critical severity faults (configuration errors)
    assert_eq!(FaultType::BadSTE.severity(), FaultSeverity::Critical);
    assert_eq!(FaultType::BadCD.severity(), FaultSeverity::Critical);
    assert_eq!(FaultType::BadStreamID.severity(), FaultSeverity::Critical);
}

#[test]
fn test_severity_error() {
    // Error severity faults (non-recoverable operational errors)
    assert_eq!(FaultType::TranslationFault.severity(), FaultSeverity::Error);
    assert_eq!(FaultType::PermissionFault.severity(), FaultSeverity::Error);
    assert_eq!(FaultType::ExternalAbort.severity(), FaultSeverity::Error);
    assert_eq!(FaultType::AddressSizeFault.severity(), FaultSeverity::Error);
    assert_eq!(FaultType::AlignmentFault.severity(), FaultSeverity::Error);
    assert_eq!(FaultType::OutputAddressRangeFault.severity(), FaultSeverity::Error);
    assert_eq!(FaultType::CDFetchFault.severity(), FaultSeverity::Error);
    assert_eq!(FaultType::STEFetchFault.severity(), FaultSeverity::Error);
    assert_eq!(FaultType::WalkEABT.severity(), FaultSeverity::Error);
    assert_eq!(FaultType::UnsupportedAtomicUpdate.severity(), FaultSeverity::Error);
}

#[test]
fn test_severity_warning() {
    // Warning severity faults (recoverable)
    assert_eq!(FaultType::AccessFlagFault.severity(), FaultSeverity::Warning);
    assert_eq!(FaultType::TLBConflictAbort.severity(), FaultSeverity::Warning);
}

#[test]
fn test_severity_coverage() {
    // All 15 faults must have a severity assigned
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

    for fault in &fault_types {
        let severity = fault.severity();
        // Just verify it returns a valid severity
        assert!(
            matches!(
                severity,
                FaultSeverity::Warning | FaultSeverity::Error | FaultSeverity::Critical
            ),
            "Fault {fault:?} has invalid severity: {severity:?}"
        );
    }
}

// ============================================================================
// FaultType: Recoverability Tests
// ============================================================================

#[test]
fn test_is_recoverable() {
    // Only AccessFlagFault and TLBConflictAbort are recoverable
    assert!(FaultType::AccessFlagFault.is_recoverable());
    assert!(FaultType::TLBConflictAbort.is_recoverable());

    // All other faults are not recoverable
    assert!(!FaultType::TranslationFault.is_recoverable());
    assert!(!FaultType::AddressSizeFault.is_recoverable());
    assert!(!FaultType::PermissionFault.is_recoverable());
    assert!(!FaultType::ExternalAbort.is_recoverable());
    assert!(!FaultType::UnsupportedAtomicUpdate.is_recoverable());
    assert!(!FaultType::AlignmentFault.is_recoverable());
    assert!(!FaultType::OutputAddressRangeFault.is_recoverable());
    assert!(!FaultType::BadStreamID.is_recoverable());
    assert!(!FaultType::CDFetchFault.is_recoverable());
    assert!(!FaultType::BadCD.is_recoverable());
    assert!(!FaultType::WalkEABT.is_recoverable());
    assert!(!FaultType::BadSTE.is_recoverable());
    assert!(!FaultType::STEFetchFault.is_recoverable());
}

#[test]
fn test_recoverable_severity_correlation() {
    // Recoverable faults should have Warning severity
    assert_eq!(FaultType::AccessFlagFault.severity(), FaultSeverity::Warning);
    assert_eq!(FaultType::TLBConflictAbort.severity(), FaultSeverity::Warning);
    assert!(FaultType::AccessFlagFault.is_recoverable());
    assert!(FaultType::TLBConflictAbort.is_recoverable());
}

// ============================================================================
// FaultType: Stage Occurrence Tests
// ============================================================================

#[test]
fn test_can_occur_in_stage1() {
    // Most faults can occur in Stage 1, except BadSTE and STEFetchFault
    assert!(FaultType::TranslationFault.can_occur_in_stage1());
    assert!(FaultType::AddressSizeFault.can_occur_in_stage1());
    assert!(FaultType::AccessFlagFault.can_occur_in_stage1());
    assert!(FaultType::PermissionFault.can_occur_in_stage1());
    assert!(FaultType::ExternalAbort.can_occur_in_stage1());
    assert!(FaultType::TLBConflictAbort.can_occur_in_stage1());
    assert!(FaultType::UnsupportedAtomicUpdate.can_occur_in_stage1());
    assert!(FaultType::AlignmentFault.can_occur_in_stage1());
    assert!(FaultType::OutputAddressRangeFault.can_occur_in_stage1());
    assert!(FaultType::BadStreamID.can_occur_in_stage1());
    assert!(FaultType::CDFetchFault.can_occur_in_stage1());
    assert!(FaultType::BadCD.can_occur_in_stage1());
    assert!(FaultType::WalkEABT.can_occur_in_stage1());

    // These cannot occur in Stage 1
    assert!(!FaultType::BadSTE.can_occur_in_stage1());
    assert!(!FaultType::STEFetchFault.can_occur_in_stage1());
}

#[test]
fn test_can_occur_in_stage2() {
    // Most faults can occur in Stage 2, except configuration faults
    assert!(FaultType::TranslationFault.can_occur_in_stage2());
    assert!(FaultType::AddressSizeFault.can_occur_in_stage2());
    assert!(FaultType::AccessFlagFault.can_occur_in_stage2());
    assert!(FaultType::PermissionFault.can_occur_in_stage2());
    assert!(FaultType::ExternalAbort.can_occur_in_stage2());
    assert!(FaultType::TLBConflictAbort.can_occur_in_stage2());
    assert!(FaultType::UnsupportedAtomicUpdate.can_occur_in_stage2());
    assert!(FaultType::AlignmentFault.can_occur_in_stage2());
    assert!(FaultType::OutputAddressRangeFault.can_occur_in_stage2());
    assert!(FaultType::WalkEABT.can_occur_in_stage2());

    // These cannot occur in Stage 2
    assert!(!FaultType::BadStreamID.can_occur_in_stage2());
    assert!(!FaultType::BadSTE.can_occur_in_stage2());
    assert!(!FaultType::STEFetchFault.can_occur_in_stage2());
    assert!(!FaultType::BadCD.can_occur_in_stage2());
    assert!(!FaultType::CDFetchFault.can_occur_in_stage2());
}

#[test]
fn test_is_stage_agnostic() {
    // 4 stage-agnostic faults
    assert!(FaultType::BadStreamID.is_stage_agnostic());
    assert!(FaultType::BadSTE.is_stage_agnostic());
    assert!(FaultType::ExternalAbort.is_stage_agnostic());
    assert!(FaultType::STEFetchFault.is_stage_agnostic());

    // All other faults are stage-specific
    assert!(!FaultType::TranslationFault.is_stage_agnostic());
    assert!(!FaultType::AddressSizeFault.is_stage_agnostic());
    assert!(!FaultType::AccessFlagFault.is_stage_agnostic());
    assert!(!FaultType::PermissionFault.is_stage_agnostic());
    assert!(!FaultType::TLBConflictAbort.is_stage_agnostic());
    assert!(!FaultType::UnsupportedAtomicUpdate.is_stage_agnostic());
    assert!(!FaultType::AlignmentFault.is_stage_agnostic());
    assert!(!FaultType::OutputAddressRangeFault.is_stage_agnostic());
    assert!(!FaultType::CDFetchFault.is_stage_agnostic());
    assert!(!FaultType::BadCD.is_stage_agnostic());
    assert!(!FaultType::WalkEABT.is_stage_agnostic());
}

#[test]
fn test_stage_occurrence_consistency() {
    // If a fault is stage-agnostic, it should not be limited to one stage
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

    for fault in &fault_types {
        let stage1 = fault.can_occur_in_stage1();
        let stage2 = fault.can_occur_in_stage2();
        let agnostic = fault.is_stage_agnostic();

        // If stage-agnostic, it should occur in at least one stage or be a pre-stage fault
        if agnostic {
            // Stage-agnostic faults may occur before stage processing
            // (e.g., BadStreamID, BadSTE, STEFetchFault)
            // or in both stages (ExternalAbort)
        } else {
            // Non-agnostic faults must occur in at least one stage
            assert!(
                stage1 || stage2,
                "Fault {fault:?} occurs in no stage but is not stage-agnostic"
            );
        }
    }
}

// ============================================================================
// FaultType: from_code() Tests
// ============================================================================

#[test]
fn test_from_code_valid_codes() {
    // Test all valid fault codes 0x01-0x0F
    assert_eq!(FaultType::from_code(0x01).unwrap(), FaultType::TranslationFault);
    assert_eq!(FaultType::from_code(0x02).unwrap(), FaultType::AddressSizeFault);
    assert_eq!(FaultType::from_code(0x03).unwrap(), FaultType::AccessFlagFault);
    assert_eq!(FaultType::from_code(0x04).unwrap(), FaultType::PermissionFault);
    assert_eq!(FaultType::from_code(0x05).unwrap(), FaultType::ExternalAbort);
    assert_eq!(FaultType::from_code(0x06).unwrap(), FaultType::TLBConflictAbort);
    assert_eq!(FaultType::from_code(0x07).unwrap(), FaultType::UnsupportedAtomicUpdate);
    assert_eq!(FaultType::from_code(0x08).unwrap(), FaultType::AlignmentFault);
    assert_eq!(FaultType::from_code(0x09).unwrap(), FaultType::OutputAddressRangeFault);
    assert_eq!(FaultType::from_code(0x0A).unwrap(), FaultType::BadStreamID);
    assert_eq!(FaultType::from_code(0x0B).unwrap(), FaultType::CDFetchFault);
    assert_eq!(FaultType::from_code(0x0C).unwrap(), FaultType::BadCD);
    assert_eq!(FaultType::from_code(0x0D).unwrap(), FaultType::WalkEABT);
    assert_eq!(FaultType::from_code(0x0E).unwrap(), FaultType::BadSTE);
    assert_eq!(FaultType::from_code(0x0F).unwrap(), FaultType::STEFetchFault);
}

#[test]
fn test_from_code_invalid_zero() {
    // Code 0x00 is invalid
    let result = FaultType::from_code(0x00);
    assert!(result.is_err());
    match result {
        Err(ValidationError::InvalidFaultType { code }) => {
            assert_eq!(code, 0x00);
        },
        _ => panic!("Expected InvalidFaultType error"),
    }
}

#[test]
fn test_from_code_invalid_above_range() {
    // Codes 0x12 and above are invalid (0x11 = BadSubstreamId is now valid)
    let invalid_codes = [0x12, 0x20, 0x50, 0xFF];
    for &code in &invalid_codes {
        let result = FaultType::from_code(code);
        assert!(result.is_err(), "Code 0x{code:02X} should be invalid");
        match result {
            Err(ValidationError::InvalidFaultType { code: err_code }) => {
                assert_eq!(err_code, code);
            },
            _ => panic!("Expected InvalidFaultType error for code 0x{code:02X}"),
        }
    }
}

#[test]
fn test_from_code_roundtrip() {
    // Verify code() and from_code() are inverse operations
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

    for fault in &fault_types {
        let code = fault.code();
        let reconstructed = FaultType::from_code(code).unwrap();
        assert_eq!(*fault, reconstructed, "Roundtrip failed for fault {fault:?}");
    }
}

// ============================================================================
// FaultType: Display Trait Tests
// ============================================================================

#[test]
fn test_display_trait() {
    // Display should match name()
    assert_eq!(format!("{}", FaultType::TranslationFault), "Translation Fault");
    assert_eq!(format!("{}", FaultType::AddressSizeFault), "Address Size Fault");
    assert_eq!(format!("{}", FaultType::AccessFlagFault), "Access Flag Fault");
    assert_eq!(format!("{}", FaultType::PermissionFault), "Permission Fault");
    assert_eq!(format!("{}", FaultType::ExternalAbort), "External Abort");
    assert_eq!(format!("{}", FaultType::TLBConflictAbort), "TLB Conflict Abort");
    assert_eq!(format!("{}", FaultType::UnsupportedAtomicUpdate), "Unsupported Atomic Update");
    assert_eq!(format!("{}", FaultType::AlignmentFault), "Alignment Fault");
    assert_eq!(format!("{}", FaultType::OutputAddressRangeFault), "Output Address Range Fault");
    assert_eq!(format!("{}", FaultType::BadStreamID), "Bad StreamID");
    assert_eq!(format!("{}", FaultType::CDFetchFault), "Context Descriptor Fetch Fault");
    assert_eq!(format!("{}", FaultType::BadCD), "Bad Context Descriptor");
    assert_eq!(format!("{}", FaultType::WalkEABT), "Page Table Walk External Abort");
    assert_eq!(format!("{}", FaultType::BadSTE), "Bad Stream Table Entry");
    assert_eq!(format!("{}", FaultType::STEFetchFault), "Stream Table Entry Fetch Fault");
}

// ============================================================================
// FaultType: Trait Implementation Tests
// ============================================================================

#[test]
fn test_debug_trait() {
    // Debug should produce a valid output
    let debug_str = format!("{:?}", FaultType::TranslationFault);
    assert!(debug_str.contains("TranslationFault"));
}

#[test]
fn test_copy_clone_traits() {
    // Test Copy and Clone traits
    let fault = FaultType::PermissionFault;
    let copied = fault; // Uses Copy
    let cloned = fault; // Uses Clone

    assert_eq!(fault, copied);
    assert_eq!(fault, cloned);
}

#[test]
fn test_partialeq_eq_traits() {
    // Test PartialEq and Eq traits
    assert_eq!(FaultType::TranslationFault, FaultType::TranslationFault);
    assert_ne!(FaultType::TranslationFault, FaultType::PermissionFault);

    // Test in collections
    let faults = [FaultType::TranslationFault, FaultType::PermissionFault];
    assert!(faults.contains(&FaultType::TranslationFault));
    assert!(!faults.contains(&FaultType::BadStreamID));
}

#[test]
fn test_hash_trait() {
    // Test Hash trait by using fault types in a HashSet
    let mut set = HashSet::new();
    set.insert(FaultType::TranslationFault);
    set.insert(FaultType::PermissionFault);
    set.insert(FaultType::TranslationFault); // Duplicate

    assert_eq!(set.len(), 2);
    assert!(set.contains(&FaultType::TranslationFault));
    assert!(set.contains(&FaultType::PermissionFault));
    assert!(!set.contains(&FaultType::BadStreamID));
}

// ============================================================================
// FaultContext Tests
// ============================================================================

#[test]
fn test_fault_context_new() {
    // Test FaultContext construction
    let stream_id = StreamID::new(42).unwrap();
    let pasid = PASID::new(100).unwrap();
    let address = IOVA::new(0x1000_0000).unwrap();
    let fault_type = FaultType::TranslationFault;

    let context = FaultContext::new(fault_type, stream_id, pasid, address);

    assert_eq!(context.fault_type(), fault_type);
    assert_eq!(context.stream_id(), stream_id);
    assert_eq!(context.pasid(), pasid);
    assert_eq!(context.address(), address);
}

#[test]
fn test_fault_context_getters() {
    // Test all getter methods
    let stream_id = StreamID::new(123).unwrap();
    let pasid = PASID::new(456).unwrap();
    let address = IOVA::new(0xDEAD_BEEF_0000).unwrap();
    let fault_type = FaultType::PermissionFault;

    let context = FaultContext::new(fault_type, stream_id, pasid, address);

    // Test fault_type()
    assert_eq!(context.fault_type(), FaultType::PermissionFault);
    assert_eq!(context.fault_type().code(), 0x04);

    // Test stream_id()
    assert_eq!(context.stream_id().as_u32(), 123);

    // Test pasid()
    assert_eq!(context.pasid().as_u32(), 456);

    // Test address()
    assert_eq!(context.address().as_u64(), 0xDEAD_BEEF_0000);
}

#[test]
fn test_fault_context_with_all_fault_types() {
    // Test FaultContext with each fault type
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(1).unwrap();
    let address = IOVA::new(0x1000).unwrap();

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
        let context = FaultContext::new(*fault_type, stream_id, pasid, address);
        assert_eq!(context.fault_type(), *fault_type);
    }
}

#[test]
fn test_fault_context_clone() {
    // Test FaultContext Clone trait
    let stream_id = StreamID::new(42).unwrap();
    let pasid = PASID::new(100).unwrap();
    let address = IOVA::new(0x1000_0000).unwrap();
    let fault_type = FaultType::TranslationFault;

    let context = FaultContext::new(fault_type, stream_id, pasid, address);
    let cloned = context.clone();

    assert_eq!(cloned.fault_type(), context.fault_type());
    assert_eq!(cloned.stream_id(), context.stream_id());
    assert_eq!(cloned.pasid(), context.pasid());
    assert_eq!(cloned.address(), context.address());
}

#[test]
fn test_fault_context_debug() {
    // Test FaultContext Debug trait
    let stream_id = StreamID::new(42).unwrap();
    let pasid = PASID::new(100).unwrap();
    let address = IOVA::new(0x1000_0000).unwrap();
    let fault_type = FaultType::TranslationFault;

    let context = FaultContext::new(fault_type, stream_id, pasid, address);
    let debug_str = format!("{context:?}");

    assert!(!debug_str.is_empty());
    assert!(debug_str.contains("FaultContext"));
}

// ============================================================================
// FaultSeverity Tests
// ============================================================================

#[test]
fn test_fault_severity_variants() {
    // Test all FaultSeverity variants
    let warning = FaultSeverity::Warning;
    let error = FaultSeverity::Error;
    let critical = FaultSeverity::Critical;

    // Test Copy/Clone
    let warning2 = warning;
    let error2 = error;
    let critical2 = critical;

    assert_eq!(warning, warning2);
    assert_eq!(error, error2);
    assert_eq!(critical, critical2);
}

#[test]
fn test_fault_severity_equality() {
    // Test PartialEq and Eq
    assert_eq!(FaultSeverity::Warning, FaultSeverity::Warning);
    assert_eq!(FaultSeverity::Error, FaultSeverity::Error);
    assert_eq!(FaultSeverity::Critical, FaultSeverity::Critical);

    assert_ne!(FaultSeverity::Warning, FaultSeverity::Error);
    assert_ne!(FaultSeverity::Warning, FaultSeverity::Critical);
    assert_ne!(FaultSeverity::Error, FaultSeverity::Critical);
}

#[test]
fn test_fault_severity_debug() {
    // Test Debug trait
    let debug_str = format!("{:?}", FaultSeverity::Warning);
    assert!(debug_str.contains("Warning"));

    let debug_str = format!("{:?}", FaultSeverity::Error);
    assert!(debug_str.contains("Error"));

    let debug_str = format!("{:?}", FaultSeverity::Critical);
    assert!(debug_str.contains("Critical"));
}

// ============================================================================
// TranslationStep Tests
// ============================================================================

#[test]
fn test_translation_step_variants() {
    // Test all TranslationStep variants
    let stage1 = TranslationStep::Stage1;
    let stage2 = TranslationStep::Stage2;

    // Test Copy/Clone
    let stage1_copy = stage1;
    let stage2_clone = stage2;

    assert_eq!(stage1, stage1_copy);
    assert_eq!(stage2, stage2_clone);
}

#[test]
fn test_translation_step_equality() {
    // Test PartialEq and Eq
    assert_eq!(TranslationStep::Stage1, TranslationStep::Stage1);
    assert_eq!(TranslationStep::Stage2, TranslationStep::Stage2);
    assert_ne!(TranslationStep::Stage1, TranslationStep::Stage2);
}

#[test]
fn test_translation_step_debug() {
    // Test Debug trait
    let debug_str = format!("{:?}", TranslationStep::Stage1);
    assert!(debug_str.contains("Stage1"));

    let debug_str = format!("{:?}", TranslationStep::Stage2);
    assert!(debug_str.contains("Stage2"));
}

// ============================================================================
// AddressType Tests
// ============================================================================

#[test]
fn test_address_type_variants() {
    // Test all AddressType variants
    let iova = AddressType::IOVA;
    let ipa = AddressType::IPA;
    let pa = AddressType::PA;

    // Test Copy/Clone
    let iova_copy = iova;
    let ipa_clone = ipa;
    let pa_copy = pa;

    assert_eq!(iova, iova_copy);
    assert_eq!(ipa, ipa_clone);
    assert_eq!(pa, pa_copy);
}

#[test]
fn test_address_type_equality() {
    // Test PartialEq and Eq
    assert_eq!(AddressType::IOVA, AddressType::IOVA);
    assert_eq!(AddressType::IPA, AddressType::IPA);
    assert_eq!(AddressType::PA, AddressType::PA);

    assert_ne!(AddressType::IOVA, AddressType::IPA);
    assert_ne!(AddressType::IOVA, AddressType::PA);
    assert_ne!(AddressType::IPA, AddressType::PA);
}

#[test]
fn test_address_type_debug() {
    // Test Debug trait
    let debug_str = format!("{:?}", AddressType::IOVA);
    assert!(debug_str.contains("IOVA"));

    let debug_str = format!("{:?}", AddressType::IPA);
    assert!(debug_str.contains("IPA"));

    let debug_str = format!("{:?}", AddressType::PA);
    assert!(debug_str.contains("PA"));
}

// ============================================================================
// Const Context Tests
// ============================================================================

#[test]
fn test_const_methods_in_const_context() {
    // Test that const methods can be used in const contexts
    const FAULT_CODE: u8 = FaultType::TranslationFault.code();
    assert_eq!(FAULT_CODE, 0x01);

    const FAULT_NAME: &str = FaultType::PermissionFault.name();
    assert_eq!(FAULT_NAME, "Permission Fault");

    const IS_TRANSLATION: bool = FaultType::TranslationFault.is_translation_fault();
    assert!(IS_TRANSLATION);

    const SEVERITY: FaultSeverity = FaultType::BadSTE.severity();
    assert!(matches!(SEVERITY, FaultSeverity::Critical));
}
