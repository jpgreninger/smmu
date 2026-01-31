//! Comprehensive tests for ValidationError type
//!
//! This test suite achieves 100% coverage for types/validation_error.rs by testing:
//! - All 11 error variant constructors
//! - Display implementation for each variant
//! - Generic error constructor (backward compatibility)
//! - Feature-gated std::error::Error trait implementation
//! - Error message formatting consistency

use smmu::ValidationError;

// ============================================================================
// Display Implementation Tests - OutOfRange
// ============================================================================

#[test]
fn test_validation_error_out_of_range_display() {
    let error = ValidationError::OutOfRange {
        field: "PASID".to_string(),
        value: 1048576,
        max: 1048575,
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Validation error for PASID: value 1048576 exceeds maximum 1048575"
    );
}

#[test]
fn test_validation_error_out_of_range_stream_id() {
    let error = ValidationError::OutOfRange {
        field: "StreamID".to_string(),
        value: 65536,
        max: 65535,
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Validation error for StreamID: value 65536 exceeds maximum 65535"
    );
}

// ============================================================================
// Display Implementation Tests - InvalidAlignment
// ============================================================================

#[test]
fn test_validation_error_invalid_alignment_display() {
    let error = ValidationError::InvalidAlignment {
        address: 0x1001,
        required_alignment: 0x1000,
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Address 0x1001 is not aligned to 0x1000"
    );
}

#[test]
fn test_validation_error_invalid_alignment_64kb() {
    let error = ValidationError::InvalidAlignment {
        address: 0x20001,
        required_alignment: 0x10000,
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Address 0x20001 is not aligned to 0x10000"
    );
}

// ============================================================================
// Display Implementation Tests - InvalidAccessType
// ============================================================================

#[test]
fn test_validation_error_invalid_access_type_display() {
    let error = ValidationError::InvalidAccessType {
        bits: 0b11111111,
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Invalid access type bit pattern: 0b11111111"
    );
}

#[test]
fn test_validation_error_invalid_access_type_zero() {
    let error = ValidationError::InvalidAccessType {
        bits: 0,
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Invalid access type bit pattern: 0b0"
    );
}

// ============================================================================
// Display Implementation Tests - InvalidSecurityState
// ============================================================================

#[test]
fn test_validation_error_invalid_security_state_display() {
    let error = ValidationError::InvalidSecurityState {
        bits: 0xFF,
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Invalid security state encoding: 0b11111111"
    );
}

#[test]
fn test_validation_error_invalid_security_state_three() {
    let error = ValidationError::InvalidSecurityState {
        bits: 3,
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Invalid security state encoding: 0b11"
    );
}

// ============================================================================
// Display Implementation Tests - InvalidTranslationStage
// ============================================================================

#[test]
fn test_validation_error_invalid_translation_stage_display() {
    let error = ValidationError::InvalidTranslationStage {
        bits: 0xFF,
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Invalid translation stage configuration: 0b11111111"
    );
}

#[test]
fn test_validation_error_invalid_translation_stage_four() {
    let error = ValidationError::InvalidTranslationStage {
        bits: 4,
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Invalid translation stage configuration: 0b100"
    );
}

// ============================================================================
// Display Implementation Tests - InvalidFaultType
// ============================================================================

#[test]
fn test_validation_error_invalid_fault_type_display() {
    let error = ValidationError::InvalidFaultType {
        code: 0xFF,
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Invalid fault type code: 0xff"
    );
}

#[test]
fn test_validation_error_invalid_fault_type_zero() {
    let error = ValidationError::InvalidFaultType {
        code: 0,
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Invalid fault type code: 0x0"
    );
}

// ============================================================================
// Display Implementation Tests - InvalidStateTransition
// ============================================================================

#[test]
fn test_validation_error_invalid_state_transition_display() {
    let error = ValidationError::InvalidStateTransition {
        from: "Idle".to_string(),
        to: "Active".to_string(),
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Invalid state transition from Idle to Active"
    );
}

#[test]
fn test_validation_error_invalid_state_transition_disabled() {
    let error = ValidationError::InvalidStateTransition {
        from: "Disabled".to_string(),
        to: "Bypassed".to_string(),
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Invalid state transition from Disabled to Bypassed"
    );
}

// ============================================================================
// Display Implementation Tests - PermissionDenied
// ============================================================================

#[test]
fn test_validation_error_permission_denied_display() {
    let error = ValidationError::PermissionDenied {
        requested: "Read+Write".to_string(),
        available: "Read".to_string(),
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Permission denied: requested Read+Write but only Read available"
    );
}

#[test]
fn test_validation_error_permission_denied_execute() {
    let error = ValidationError::PermissionDenied {
        requested: "Execute".to_string(),
        available: "None".to_string(),
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Permission denied: requested Execute but only None available"
    );
}

// ============================================================================
// Display Implementation Tests - SecurityViolation
// ============================================================================

#[test]
fn test_validation_error_security_violation_display() {
    let error = ValidationError::SecurityViolation {
        from_state: "NonSecure".to_string(),
        to_state: "Secure".to_string(),
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Security violation: NonSecure cannot access Secure"
    );
}

#[test]
fn test_validation_error_security_violation_realm() {
    let error = ValidationError::SecurityViolation {
        from_state: "Realm".to_string(),
        to_state: "Root".to_string(),
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Security violation: Realm cannot access Root"
    );
}

// ============================================================================
// Display Implementation Tests - InvalidConfiguration
// ============================================================================

#[test]
fn test_validation_error_invalid_configuration_display() {
    let error = ValidationError::InvalidConfiguration {
        reason: "Stage 1 and Stage 2 cannot both be disabled".to_string(),
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Invalid configuration: Stage 1 and Stage 2 cannot both be disabled"
    );
}

#[test]
fn test_validation_error_invalid_configuration_cache() {
    let error = ValidationError::InvalidConfiguration {
        reason: "Cache size must be power of 2".to_string(),
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Invalid configuration: Cache size must be power of 2"
    );
}

// ============================================================================
// Display Implementation Tests - InvalidPASID
// ============================================================================

#[test]
fn test_validation_error_invalid_pasid_display() {
    let error = ValidationError::InvalidPASID {
        value: 1048576,
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Invalid PASID value: 1048576"
    );
}

#[test]
fn test_validation_error_invalid_pasid_max() {
    let error = ValidationError::InvalidPASID {
        value: u32::MAX,
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        format!("Invalid PASID value: {}", u32::MAX)
    );
}

// ============================================================================
// Display Implementation Tests - Generic (Backward Compatibility)
// ============================================================================

#[test]
fn test_validation_error_generic_display() {
    let error = ValidationError::Generic {
        field: "PageSize".to_string(),
        value: "8192".to_string(),
        constraint: "must be 4096 or 65536".to_string(),
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Validation error for PageSize: value '8192' must be 4096 or 65536"
    );
}

#[test]
fn test_validation_error_generic_empty_values() {
    let error = ValidationError::Generic {
        field: "Field".to_string(),
        value: "".to_string(),
        constraint: "must not be empty".to_string(),
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Validation error for Field: value '' must not be empty"
    );
}

// ============================================================================
// Error Construction Tests
// ============================================================================

#[test]
fn test_validation_error_new_constructor() {
    let error = ValidationError::new(
        "TestField",
        "invalid_value",
        "must be positive"
    );

    match error {
        ValidationError::Generic { field, value, constraint } => {
            assert_eq!(field, "TestField");
            assert_eq!(value, "invalid_value");
            assert_eq!(constraint, "must be positive");
        }
        _ => panic!("Expected Generic variant"),
    }
}

#[test]
fn test_validation_error_new_display() {
    let error = ValidationError::new(
        "Count",
        "0",
        "must be greater than 0"
    );

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Validation error for Count: value '0' must be greater than 0"
    );
}

#[test]
fn test_validation_error_construct_all_variants() {
    // Test construction of all variants to ensure they can be created

    let _out_of_range = ValidationError::OutOfRange {
        field: "test".to_string(),
        value: 100,
        max: 99,
    };

    let _invalid_alignment = ValidationError::InvalidAlignment {
        address: 0x1234,
        required_alignment: 0x1000,
    };

    let _invalid_access = ValidationError::InvalidAccessType {
        bits: 0xFF,
    };

    let _invalid_security = ValidationError::InvalidSecurityState {
        bits: 0x10,
    };

    let _invalid_stage = ValidationError::InvalidTranslationStage {
        bits: 0x0F,
    };

    let _invalid_fault = ValidationError::InvalidFaultType {
        code: 0xAB,
    };

    let _invalid_transition = ValidationError::InvalidStateTransition {
        from: "A".to_string(),
        to: "B".to_string(),
    };

    let _permission_denied = ValidationError::PermissionDenied {
        requested: "RW".to_string(),
        available: "R".to_string(),
    };

    let _security_violation = ValidationError::SecurityViolation {
        from_state: "NS".to_string(),
        to_state: "S".to_string(),
    };

    let _invalid_config = ValidationError::InvalidConfiguration {
        reason: "test".to_string(),
    };

    let _invalid_pasid = ValidationError::InvalidPASID {
        value: 1000000,
    };

    let _generic = ValidationError::Generic {
        field: "test".to_string(),
        value: "val".to_string(),
        constraint: "constraint".to_string(),
    };
}

// ============================================================================
// Clone and PartialEq Tests
// ============================================================================

#[test]
fn test_validation_error_clone() {
    let error1 = ValidationError::InvalidPASID { value: 12345 };
    let error2 = error1.clone();

    assert_eq!(error1, error2);
}

#[test]
fn test_validation_error_equality() {
    let error1 = ValidationError::OutOfRange {
        field: "test".to_string(),
        value: 100,
        max: 50,
    };

    let error2 = ValidationError::OutOfRange {
        field: "test".to_string(),
        value: 100,
        max: 50,
    };

    assert_eq!(error1, error2);
}

#[test]
fn test_validation_error_inequality() {
    let error1 = ValidationError::InvalidPASID { value: 100 };
    let error2 = ValidationError::InvalidPASID { value: 200 };

    assert_ne!(error1, error2);
}

// ============================================================================
// Debug Trait Tests
// ============================================================================

#[test]
fn test_validation_error_debug() {
    let error = ValidationError::InvalidPASID { value: 999 };
    let debug_str = format!("{error:?}");

    // Debug should show the variant and fields
    assert!(debug_str.contains("InvalidPASID"));
    assert!(debug_str.contains("999"));
}

#[test]
fn test_validation_error_debug_generic() {
    let error = ValidationError::Generic {
        field: "test".to_string(),
        value: "val".to_string(),
        constraint: "constraint".to_string(),
    };
    let debug_str = format!("{error:?}");

    assert!(debug_str.contains("Generic"));
    assert!(debug_str.contains("test"));
    assert!(debug_str.contains("val"));
    assert!(debug_str.contains("constraint"));
}

// ============================================================================
// Feature-Gated std::error::Error Trait Tests
// ============================================================================

#[cfg(feature = "std")]
#[test]
fn test_validation_error_implements_std_error() {
    use std::error::Error;

    let error = ValidationError::InvalidPASID { value: 12345 };

    // Test that it implements Error trait
    let _: &dyn Error = &error;

    // Test Display through Error trait
    let display = format!("{}", error);
    assert_eq!(display, "Invalid PASID value: 12345");
}

#[cfg(feature = "std")]
#[test]
fn test_validation_error_source_is_none() {
    use std::error::Error;

    let error = ValidationError::InvalidConfiguration {
        reason: "Test error".to_string(),
    };

    // ValidationError doesn't chain errors, so source should be None
    assert!(error.source().is_none());
}

#[cfg(feature = "std")]
#[test]
fn test_validation_error_downcast() {
    use std::error::Error;

    let error: Box<dyn Error> = Box::new(ValidationError::InvalidPASID { value: 100 });

    // Test that we can downcast back to ValidationError
    let downcast = error.downcast::<ValidationError>();
    assert!(downcast.is_ok());

    if let Ok(boxed_error) = downcast {
        assert_eq!(*boxed_error, ValidationError::InvalidPASID { value: 100 });
    }
}

// ============================================================================
// Edge Cases and Message Consistency Tests
// ============================================================================

#[test]
fn test_validation_error_large_values() {
    let error = ValidationError::OutOfRange {
        field: "LargeValue".to_string(),
        value: u64::MAX,
        max: u64::MAX - 1,
    };

    let display = format!("{}", error);
    assert!(display.contains(&format!("{}", u64::MAX)));
}

#[test]
fn test_validation_error_zero_values() {
    let error = ValidationError::OutOfRange {
        field: "ZeroValue".to_string(),
        value: 0,
        max: 0,
    };

    let display = format!("{}", error);
    assert_eq!(
        display,
        "Validation error for ZeroValue: value 0 exceeds maximum 0"
    );
}

#[test]
fn test_validation_error_message_format_consistency() {
    // Test that all error messages follow consistent format patterns

    let errors = vec![
        (
            ValidationError::OutOfRange {
                field: "F".to_string(),
                value: 1,
                max: 0,
            },
            "Validation error for F: value 1 exceeds maximum 0",
        ),
        (
            ValidationError::InvalidAlignment {
                address: 0x100,
                required_alignment: 0x10,
            },
            "Address 0x100 is not aligned to 0x10",
        ),
        (
            ValidationError::InvalidAccessType { bits: 8 },
            "Invalid access type bit pattern: 0b1000",
        ),
        (
            ValidationError::InvalidSecurityState { bits: 2 },
            "Invalid security state encoding: 0b10",
        ),
        (
            ValidationError::InvalidTranslationStage { bits: 7 },
            "Invalid translation stage configuration: 0b111",
        ),
        (
            ValidationError::InvalidFaultType { code: 0x42 },
            "Invalid fault type code: 0x42",
        ),
        (
            ValidationError::InvalidStateTransition {
                from: "A".to_string(),
                to: "B".to_string(),
            },
            "Invalid state transition from A to B",
        ),
        (
            ValidationError::PermissionDenied {
                requested: "X".to_string(),
                available: "Y".to_string(),
            },
            "Permission denied: requested X but only Y available",
        ),
        (
            ValidationError::SecurityViolation {
                from_state: "NS".to_string(),
                to_state: "S".to_string(),
            },
            "Security violation: NS cannot access S",
        ),
        (
            ValidationError::InvalidConfiguration {
                reason: "reason".to_string(),
            },
            "Invalid configuration: reason",
        ),
        (
            ValidationError::InvalidPASID { value: 42 },
            "Invalid PASID value: 42",
        ),
    ];

    for (error, expected) in errors {
        assert_eq!(format!("{}", error), expected);
    }
}

#[test]
fn test_validation_error_unicode_support() {
    let error = ValidationError::Generic {
        field: "テスト".to_string(),
        value: "値".to_string(),
        constraint: "制約".to_string(),
    };

    let display = format!("{}", error);
    assert_eq!(display, "Validation error for テスト: value '値' 制約");
}

#[test]
fn test_validation_error_special_characters() {
    let error = ValidationError::InvalidConfiguration {
        reason: "Value contains special chars: !@#$%^&*()".to_string(),
    };

    let display = format!("{}", error);
    assert_eq!(display, "Invalid configuration: Value contains special chars: !@#$%^&*()");
}
