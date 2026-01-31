//! Comprehensive test coverage for types/translation_result.rs
//!
//! This test suite achieves 100% coverage of TranslationResult, TranslationData,
//! TranslationError, and TranslationDataBuilder, covering all variants, conversions,
//! and display formatting.

use smmu::types::{
    AccessType, PagePermissions, SecurityState, TranslationData, TranslationError,
    TranslationResult, PA,
};

// ============================================================================
// TranslationData - Basic Construction
// ============================================================================

#[test]
fn test_translation_data_new() {
    let pa = PA::new(0x1000).unwrap();
    let perms = PagePermissions::read_write();
    let security = SecurityState::NonSecure;

    let data = TranslationData::new(pa, perms, security);

    assert_eq!(data.physical_address(), pa);
    assert_eq!(data.permissions(), perms);
    assert_eq!(data.security_state(), security);
}

#[test]
fn test_translation_data_with_pa() {
    let pa = PA::new(0x5000).unwrap();
    let data = TranslationData::with_pa(pa);

    assert_eq!(data.physical_address(), pa);
    assert_eq!(data.permissions(), PagePermissions::none());
    assert_eq!(data.security_state(), SecurityState::NonSecure);
}

#[test]
fn test_translation_data_default() {
    let data = TranslationData::default();

    assert_eq!(data.physical_address(), PA::new(0).unwrap());
    assert_eq!(data.permissions(), PagePermissions::default());
    assert_eq!(data.security_state(), SecurityState::NonSecure);
}

// ============================================================================
// TranslationData - All Security States
// ============================================================================

#[test]
fn test_translation_data_secure_state() {
    let pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_only();
    let data = TranslationData::new(pa, perms, SecurityState::Secure);

    assert_eq!(data.security_state(), SecurityState::Secure);
}

#[test]
fn test_translation_data_non_secure_state() {
    let pa = PA::new(0x3000).unwrap();
    let perms = PagePermissions::read_write();
    let data = TranslationData::new(pa, perms, SecurityState::NonSecure);

    assert_eq!(data.security_state(), SecurityState::NonSecure);
}

#[test]
fn test_translation_data_realm_state() {
    let pa = PA::new(0x4000).unwrap();
    let perms = PagePermissions::read_execute();
    let data = TranslationData::new(pa, perms, SecurityState::Realm);

    assert_eq!(data.security_state(), SecurityState::Realm);
}

// Note: SecurityState only has Secure, NonSecure, and Realm variants
// Root is not part of the ARM SMMU v3 spec implementation

// ============================================================================
// TranslationData - All Permission Types
// ============================================================================

#[test]
fn test_translation_data_read_only_permissions() {
    let pa = PA::new(0x1000).unwrap();
    let perms = PagePermissions::read_only();
    let data = TranslationData::new(pa, perms, SecurityState::NonSecure);

    assert_eq!(data.permissions(), perms);
}

#[test]
fn test_translation_data_write_only_permissions() {
    let pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::write_only();
    let data = TranslationData::new(pa, perms, SecurityState::NonSecure);

    assert_eq!(data.permissions(), perms);
}

#[test]
fn test_translation_data_read_write_permissions() {
    let pa = PA::new(0x3000).unwrap();
    let perms = PagePermissions::read_write();
    let data = TranslationData::new(pa, perms, SecurityState::NonSecure);

    assert_eq!(data.permissions(), perms);
}

#[test]
fn test_translation_data_execute_only_permissions() {
    let pa = PA::new(0x4000).unwrap();
    let perms = PagePermissions::execute_only();
    let data = TranslationData::new(pa, perms, SecurityState::NonSecure);

    assert_eq!(data.permissions(), perms);
}

#[test]
fn test_translation_data_read_execute_permissions() {
    let pa = PA::new(0x5000).unwrap();
    let perms = PagePermissions::read_execute();
    let data = TranslationData::new(pa, perms, SecurityState::NonSecure);

    assert_eq!(data.permissions(), perms);
}

#[test]
fn test_translation_data_all_permissions() {
    let pa = PA::new(0x6000).unwrap();
    let perms = PagePermissions::all();
    let data = TranslationData::new(pa, perms, SecurityState::NonSecure);

    assert_eq!(data.permissions(), perms);
}

#[test]
fn test_translation_data_none_permissions() {
    let pa = PA::new(0x7000).unwrap();
    let perms = PagePermissions::none();
    let data = TranslationData::new(pa, perms, SecurityState::NonSecure);

    assert_eq!(data.permissions(), perms);
}

// ============================================================================
// TranslationDataBuilder - Basic Usage
// ============================================================================

#[test]
fn test_builder_basic() {
    let pa = PA::new(0x1000).unwrap();
    let data = TranslationData::builder().physical_address(pa).build();

    assert_eq!(data.physical_address(), pa);
    assert_eq!(data.permissions(), PagePermissions::none());
    assert_eq!(data.security_state(), SecurityState::NonSecure);
}

#[test]
fn test_builder_full_configuration() {
    let pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_write();
    let security = SecurityState::Secure;

    let data = TranslationData::builder()
        .physical_address(pa)
        .permissions(perms)
        .security_state(security)
        .build();

    assert_eq!(data.physical_address(), pa);
    assert_eq!(data.permissions(), perms);
    assert_eq!(data.security_state(), security);
}

#[test]
fn test_builder_chaining() {
    let pa = PA::new(0x3000).unwrap();
    let perms = PagePermissions::read_execute();
    let security = SecurityState::Realm;

    let data = TranslationData::builder()
        .physical_address(pa)
        .permissions(perms)
        .security_state(security)
        .build();

    assert_eq!(data.physical_address(), pa);
    assert_eq!(data.permissions(), perms);
    assert_eq!(data.security_state(), security);
}

#[test]
fn test_builder_partial_configuration() {
    let pa = PA::new(0x4000).unwrap();

    let data = TranslationData::builder()
        .physical_address(pa)
        .permissions(PagePermissions::read_write())
        .build();

    assert_eq!(data.physical_address(), pa);
    assert_eq!(data.permissions(), PagePermissions::read_write());
    assert_eq!(data.security_state(), SecurityState::NonSecure);
}

#[test]
#[should_panic(expected = "Physical address must be set")]
fn test_builder_without_physical_address_panics() {
    let _data = TranslationData::builder()
        .permissions(PagePermissions::read_only())
        .build();
}

#[test]
fn test_builder_different_order() {
    let pa = PA::new(0x5000).unwrap();
    let perms = PagePermissions::read_write();
    let security = SecurityState::Realm;

    let data = TranslationData::builder()
        .security_state(security)
        .permissions(perms)
        .physical_address(pa)
        .build();

    assert_eq!(data.physical_address(), pa);
    assert_eq!(data.permissions(), perms);
    assert_eq!(data.security_state(), security);
}

// ============================================================================
// TranslationError - All Variants
// ============================================================================

#[test]
fn test_error_page_not_mapped() {
    let error = TranslationError::PageNotMapped;
    let msg = format!("{}", error);
    assert_eq!(msg, "Page not mapped in address space");
}

#[test]
fn test_error_permission_violation() {
    let error = TranslationError::PermissionViolation {
        access: AccessType::Write,
    };
    let msg = format!("{}", error);
    assert!(msg.contains("Permission violation"));
    assert!(msg.contains("Write"));
}

#[test]
fn test_error_permission_violation_read() {
    let error = TranslationError::PermissionViolation {
        access: AccessType::Read,
    };
    let msg = format!("{}", error);
    assert!(msg.contains("Read"));
}

#[test]
fn test_error_permission_violation_execute() {
    let error = TranslationError::PermissionViolation {
        access: AccessType::Execute,
    };
    let msg = format!("{}", error);
    assert!(msg.contains("Execute"));
}

#[test]
fn test_error_invalid_address() {
    let error = TranslationError::InvalidAddress { address: 0xDEADBEEF };
    let msg = format!("{}", error);
    assert!(msg.contains("Invalid address"));
    assert!(msg.contains("deadbeef"));
}

#[test]
fn test_error_invalid_stream_id() {
    let error = TranslationError::InvalidStreamID;
    let msg = format!("{}", error);
    assert_eq!(msg, "Invalid StreamID");
}

#[test]
fn test_error_invalid_pasid() {
    let error = TranslationError::InvalidPASID;
    let msg = format!("{}", error);
    assert_eq!(msg, "Invalid PASID");
}

#[test]
fn test_error_pasid_not_found() {
    let error = TranslationError::PASIDNotFound;
    let msg = format!("{}", error);
    assert_eq!(msg, "PASID not found");
}

#[test]
fn test_error_stream_not_configured() {
    let error = TranslationError::StreamNotConfigured;
    let msg = format!("{}", error);
    assert_eq!(msg, "Stream not configured");
}

#[test]
fn test_error_stream_disabled() {
    let error = TranslationError::StreamDisabled;
    let msg = format!("{}", error);
    assert_eq!(msg, "Stream disabled");
}

#[test]
fn test_error_address_size_error() {
    let error = TranslationError::AddressSizeError;
    let msg = format!("{}", error);
    assert!(msg.contains("Address size error"));
}

#[test]
fn test_error_alignment_error() {
    let error = TranslationError::AlignmentError;
    let msg = format!("{}", error);
    assert!(msg.contains("alignment error"));
}

#[test]
fn test_error_security_violation() {
    let error = TranslationError::SecurityViolation;
    let msg = format!("{}", error);
    assert!(msg.contains("Security"));
}

#[test]
fn test_error_external_abort() {
    let error = TranslationError::ExternalAbort;
    let msg = format!("{}", error);
    assert!(msg.contains("External abort"));
}

#[test]
fn test_error_tlb_conflict() {
    let error = TranslationError::TlbConflict;
    let msg = format!("{}", error);
    assert!(msg.contains("TLB conflict"));
}

// ============================================================================
// TranslationError - Equality and Cloning
// ============================================================================

#[test]
fn test_error_equality() {
    let err1 = TranslationError::PageNotMapped;
    let err2 = TranslationError::PageNotMapped;
    assert_eq!(err1, err2);

    let err3 = TranslationError::InvalidStreamID;
    assert_ne!(err1, err3);
}

#[test]
fn test_error_equality_with_fields() {
    let err1 = TranslationError::PermissionViolation {
        access: AccessType::Write,
    };
    let err2 = TranslationError::PermissionViolation {
        access: AccessType::Write,
    };
    assert_eq!(err1, err2);

    let err3 = TranslationError::PermissionViolation {
        access: AccessType::Read,
    };
    assert_ne!(err1, err3);
}

#[test]
fn test_error_clone() {
    let error = TranslationError::InvalidAddress { address: 0x12345 };
    let cloned = error.clone();
    assert_eq!(error, cloned);
}

#[test]
fn test_error_debug() {
    let error = TranslationError::PageNotMapped;
    let debug = format!("{:?}", error);
    assert!(debug.contains("PageNotMapped"));
}

// ============================================================================
// TranslationResult - Success Cases
// ============================================================================

#[test]
fn test_result_ok_basic() {
    let pa = PA::new(0x1000).unwrap();
    let data = TranslationData::with_pa(pa);
    let result: TranslationResult = Ok(data);

    assert!(result.is_ok());
    assert_eq!(result.unwrap().physical_address(), pa);
}

#[test]
fn test_result_ok_full_data() {
    let pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_write();
    let security = SecurityState::Secure;
    let data = TranslationData::new(pa, perms, security);
    let result: TranslationResult = Ok(data);

    assert!(result.is_ok());
    let unwrapped = result.unwrap();
    assert_eq!(unwrapped.physical_address(), pa);
    assert_eq!(unwrapped.permissions(), perms);
    assert_eq!(unwrapped.security_state(), security);
}

// ============================================================================
// TranslationResult - Error Cases
// ============================================================================

#[test]
fn test_result_err_page_not_mapped() {
    let result: TranslationResult = Err(TranslationError::PageNotMapped);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), TranslationError::PageNotMapped);
}

#[test]
fn test_result_err_permission_violation() {
    let result: TranslationResult = Err(TranslationError::PermissionViolation {
        access: AccessType::Execute,
    });
    assert!(result.is_err());
}

#[test]
fn test_result_err_invalid_address() {
    let result: TranslationResult = Err(TranslationError::InvalidAddress { address: 0x99999 });
    assert!(result.is_err());
}

#[test]
fn test_result_err_all_variants() {
    let errors = vec![
        TranslationError::PageNotMapped,
        TranslationError::PermissionViolation {
            access: AccessType::Write,
        },
        TranslationError::InvalidAddress { address: 0x1000 },
        TranslationError::InvalidStreamID,
        TranslationError::InvalidPASID,
        TranslationError::PASIDNotFound,
        TranslationError::StreamNotConfigured,
        TranslationError::StreamDisabled,
        TranslationError::AddressSizeError,
        TranslationError::AlignmentError,
        TranslationError::SecurityViolation,
        TranslationError::ExternalAbort,
        TranslationError::TlbConflict,
    ];

    for error in errors {
        let result: TranslationResult = Err(error.clone());
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), error);
    }
}

// ============================================================================
// TranslationData - Copy and Equality
// ============================================================================

#[test]
fn test_translation_data_copy() {
    let pa = PA::new(0x1000).unwrap();
    let perms = PagePermissions::read_write();
    let data = TranslationData::new(pa, perms, SecurityState::NonSecure);

    let copied = data;
    assert_eq!(data.physical_address(), copied.physical_address());
    assert_eq!(data.permissions(), copied.permissions());
    assert_eq!(data.security_state(), copied.security_state());
}

#[test]
fn test_translation_data_equality() {
    let pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_execute();

    let data1 = TranslationData::new(pa, perms, SecurityState::Secure);
    let data2 = TranslationData::new(pa, perms, SecurityState::Secure);

    assert_eq!(data1, data2);
}

#[test]
fn test_translation_data_inequality_address() {
    let pa1 = PA::new(0x1000).unwrap();
    let pa2 = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_write();

    let data1 = TranslationData::new(pa1, perms, SecurityState::NonSecure);
    let data2 = TranslationData::new(pa2, perms, SecurityState::NonSecure);

    assert_ne!(data1, data2);
}

#[test]
fn test_translation_data_inequality_permissions() {
    let pa = PA::new(0x1000).unwrap();
    let perms1 = PagePermissions::read_only();
    let perms2 = PagePermissions::read_write();

    let data1 = TranslationData::new(pa, perms1, SecurityState::NonSecure);
    let data2 = TranslationData::new(pa, perms2, SecurityState::NonSecure);

    assert_ne!(data1, data2);
}

#[test]
fn test_translation_data_inequality_security() {
    let pa = PA::new(0x1000).unwrap();
    let perms = PagePermissions::read_write();

    let data1 = TranslationData::new(pa, perms, SecurityState::Secure);
    let data2 = TranslationData::new(pa, perms, SecurityState::NonSecure);

    assert_ne!(data1, data2);
}

#[test]
fn test_translation_data_debug() {
    let pa = PA::new(0x1000).unwrap();
    let perms = PagePermissions::read_write();
    let data = TranslationData::new(pa, perms, SecurityState::NonSecure);

    let debug = format!("{:?}", data);
    assert!(!debug.is_empty());
}

// ============================================================================
// Edge Cases and Boundary Values
// ============================================================================

#[test]
fn test_translation_data_zero_address() {
    let pa = PA::new(0).unwrap();
    let data = TranslationData::with_pa(pa);
    assert_eq!(data.physical_address(), pa);
}

#[test]
fn test_translation_data_max_address() {
    let pa = PA::new(0xFFFF_FFFF_FFFF).unwrap();
    let data = TranslationData::with_pa(pa);
    assert_eq!(data.physical_address(), pa);
}

#[test]
fn test_error_invalid_address_zero() {
    let error = TranslationError::InvalidAddress { address: 0 };
    let msg = format!("{}", error);
    assert!(msg.contains("0x0"));
}

#[test]
fn test_error_invalid_address_max() {
    let error = TranslationError::InvalidAddress {
        address: 0xFFFF_FFFF_FFFF_FFFF,
    };
    let msg = format!("{}", error);
    assert!(msg.contains("Invalid address"));
}

// ============================================================================
// Builder Clone and Debug
// ============================================================================

#[test]
fn test_builder_clone() {
    let pa = PA::new(0x1000).unwrap();
    let builder = TranslationData::builder().physical_address(pa);

    let cloned = builder.clone();
    let data1 = builder.build();
    let data2 = cloned.build();

    assert_eq!(data1.physical_address(), data2.physical_address());
}

#[test]
fn test_builder_debug() {
    let builder = TranslationData::builder();
    let debug = format!("{:?}", builder);
    assert!(!debug.is_empty());
}

// ============================================================================
// Comprehensive Combinations
// ============================================================================

#[test]
fn test_all_security_permission_combinations() {
    let pa = PA::new(0x1000).unwrap();
    let security_states = [
        SecurityState::NonSecure,
        SecurityState::Secure,
        SecurityState::Realm,
    ];
    let permissions = [
        PagePermissions::none(),
        PagePermissions::read_only(),
        PagePermissions::write_only(),
        PagePermissions::read_write(),
        PagePermissions::execute_only(),
        PagePermissions::read_execute(),
        PagePermissions::all(),
    ];

    for security in &security_states {
        for perm in &permissions {
            let data = TranslationData::new(pa, *perm, *security);
            assert_eq!(data.security_state(), *security);
            assert_eq!(data.permissions(), *perm);
        }
    }
}

#[test]
fn test_result_pattern_matching() {
    let success: TranslationResult = Ok(TranslationData::with_pa(PA::new(0x1000).unwrap()));
    let failure: TranslationResult = Err(TranslationError::PageNotMapped);

    match success {
        Ok(_data) => assert!(true),
        Err(_) => panic!("Should be Ok"),
    }

    match failure {
        Ok(_) => panic!("Should be Err"),
        Err(_error) => assert!(true),
    }
}

#[test]
fn test_result_unwrap_or() {
    let pa = PA::new(0x1000).unwrap();
    let default_data = TranslationData::with_pa(PA::new(0).unwrap());

    let success: TranslationResult = Ok(TranslationData::with_pa(pa));
    let result = success.unwrap_or(default_data);
    assert_eq!(result.physical_address(), pa);

    let failure: TranslationResult = Err(TranslationError::PageNotMapped);
    let result = failure.unwrap_or(default_data);
    assert_eq!(result.physical_address(), PA::new(0).unwrap());
}
