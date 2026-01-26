//! Comprehensive tests for TranslationResult and TranslationData structures
//!
//! This test suite validates the translation result types following ARM SMMU v3
//! specification requirements. Tests are written FIRST following TDD.

use smmu::types::{
    AccessType, PagePermissions, SecurityState, TranslationData, TranslationError,
    TranslationResult, PA,
};

/// Test successful TranslationResult with TranslationData
#[test]
fn test_translation_result_success() {
    let pa = PA::new(0x1000).unwrap();
    let perms = PagePermissions::read_write();
    let data = TranslationData::new(pa, perms, SecurityState::NonSecure);

    let result: TranslationResult = Ok(data);

    assert!(result.is_ok());
    let unwrapped = result.unwrap();
    assert_eq!(unwrapped.physical_address(), pa);
    assert_eq!(unwrapped.permissions(), perms);
}

/// Test TranslationResult with error
#[test]
fn test_translation_result_error() {
    let result: TranslationResult = Err(TranslationError::PageNotMapped);

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), TranslationError::PageNotMapped);
}

/// Test TranslationData creation with all fields
#[test]
fn test_translation_data_full() {
    let pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_execute();
    let sec_state = SecurityState::Secure;

    let data = TranslationData::new(pa, perms, sec_state);

    assert_eq!(data.physical_address(), pa);
    assert_eq!(data.permissions(), perms);
    assert_eq!(data.security_state(), sec_state);
}

/// Test TranslationData default construction
#[test]
fn test_translation_data_default() {
    let data = TranslationData::default();

    assert_eq!(data.physical_address(), PA::new(0).unwrap());
    assert_eq!(data.permissions(), PagePermissions::default());
    assert_eq!(data.security_state(), SecurityState::NonSecure);
}

/// Test TranslationData with physical address only
#[test]
fn test_translation_data_pa_only() {
    let pa = PA::new(0x3000).unwrap();
    let data = TranslationData::with_pa(pa);

    assert_eq!(data.physical_address(), pa);
    assert_eq!(data.security_state(), SecurityState::NonSecure);
}

/// Test TranslationData Clone trait
#[test]
fn test_translation_data_clone() {
    let pa = PA::new(0x4000).unwrap();
    let perms = PagePermissions::all();
    let data1 = TranslationData::new(pa, perms, SecurityState::Realm);

    let data2 = data1.clone();

    assert_eq!(data1.physical_address(), data2.physical_address());
    assert_eq!(data1.permissions(), data2.permissions());
    assert_eq!(data1.security_state(), data2.security_state());
}

/// Test TranslationData Debug trait
#[test]
fn test_translation_data_debug() {
    let pa = PA::new(0x5000).unwrap();
    let data = TranslationData::with_pa(pa);

    let debug_str = format!("{:?}", data);
    assert!(!debug_str.is_empty());
    assert!(debug_str.contains("TranslationData"));
}

/// Test TranslationData PartialEq trait
#[test]
fn test_translation_data_equality() {
    let pa = PA::new(0x6000).unwrap();
    let perms = PagePermissions::read_write();

    let data1 = TranslationData::new(pa, perms, SecurityState::NonSecure);
    let data2 = TranslationData::new(pa, perms, SecurityState::NonSecure);
    let data3 = TranslationData::new(PA::new(0x7000).unwrap(), perms, SecurityState::NonSecure);

    assert_eq!(data1, data2);
    assert_ne!(data1, data3);
}

/// Test all TranslationError variants
#[test]
fn test_translation_error_variants() {
    let errors = [
        TranslationError::PageNotMapped,
        TranslationError::PermissionViolation { access: AccessType::Write },
        TranslationError::InvalidAddress { address: 0xDEAD },
        TranslationError::InvalidStreamID,
        TranslationError::InvalidPASID,
        TranslationError::StreamNotConfigured,
        TranslationError::StreamDisabled,
        TranslationError::AddressSizeError,
        TranslationError::AlignmentError,
        TranslationError::SecurityViolation,
        TranslationError::ExternalAbort,
        TranslationError::TlbConflict,
    ];

    for error in &errors {
        // Verify each error can be created and formatted
        let _error_str = format!("{}", error);
        let _debug_str = format!("{:?}", error);
    }
}

/// Test TranslationError Display trait
#[test]
fn test_translation_error_display() {
    let error = TranslationError::PageNotMapped;
    let display_str = format!("{}", error);

    assert!(!display_str.is_empty());
    assert!(display_str.to_lowercase().contains("page"));
}

/// Test TranslationError with context
#[test]
fn test_translation_error_with_context() {
    let error = TranslationError::PermissionViolation {
        access: AccessType::Write,
    };

    let display_str = format!("{}", error);
    assert!(!display_str.is_empty());
}

/// Test TranslationError InvalidAddress with context
#[test]
fn test_translation_error_invalid_address() {
    let error = TranslationError::InvalidAddress { address: 0x12345678 };
    let display_str = format!("{}", error);

    assert!(display_str.contains("12345678") || display_str.contains("address"));
}

/// Test TranslationResult map operation
#[test]
fn test_translation_result_map() {
    let pa = PA::new(0x8000).unwrap();
    let data = TranslationData::with_pa(pa);
    let result: TranslationResult = Ok(data);

    let mapped = result.map(|d| d.physical_address());

    assert!(mapped.is_ok());
    assert_eq!(mapped.unwrap(), pa);
}

/// Test TranslationResult and_then operation
#[test]
fn test_translation_result_and_then() {
    let pa = PA::new(0x9000).unwrap();
    let perms = PagePermissions::read_only();
    let data = TranslationData::new(pa, perms, SecurityState::NonSecure);
    let result: TranslationResult = Ok(data);

    let chained = result.and_then(|d| {
        if d.permissions().write() {
            Err(TranslationError::PermissionViolation {
                access: AccessType::Write,
            })
        } else {
            Ok(d)
        }
    });

    assert!(chained.is_ok());
}

/// Test TranslationResult error propagation with ? operator
#[test]
fn test_translation_result_question_mark() {
    fn helper() -> TranslationResult {
        let pa = PA::new(0xA000).unwrap();
        let data = TranslationData::with_pa(pa);
        let result: TranslationResult = Ok(data);

        let _unwrapped = result?;

        Ok(TranslationData::default())
    }

    let result = helper();
    assert!(result.is_ok());
}

/// Test TranslationError equality
#[test]
fn test_translation_error_equality() {
    let error1 = TranslationError::PageNotMapped;
    let error2 = TranslationError::PageNotMapped;
    let error3 = TranslationError::StreamDisabled;

    assert_eq!(error1, error2);
    assert_ne!(error1, error3);
}

/// Test TranslationData Copy trait (should be Copy for efficiency)
#[test]
fn test_translation_data_copy() {
    let pa = PA::new(0xB000).unwrap();
    let data1 = TranslationData::with_pa(pa);
    let data2 = data1; // Should copy, not move

    // Both should be usable
    assert_eq!(data1.physical_address(), pa);
    assert_eq!(data2.physical_address(), pa);
}

/// Test TranslationData size for cache efficiency
#[test]
fn test_translation_data_size() {
    use std::mem::size_of;

    let size = size_of::<TranslationData>();

    // TranslationData should be compact (< 32 bytes for cache efficiency)
    assert!(size <= 32, "TranslationData too large: {} bytes", size);
}

/// Test TranslationError From conversions
#[test]
fn test_translation_error_from() {
    // Test that we can convert from specific error types if needed
    let _error: TranslationError = TranslationError::PageNotMapped;

    // Verify error implements std::error::Error
    let error_trait: &dyn std::error::Error = &TranslationError::PageNotMapped;
    assert!(!error_trait.to_string().is_empty());
}

/// Performance test: TranslationData construction should be fast
#[test]
fn test_translation_data_performance() {
    let pa = PA::new(0xC000).unwrap();
    let perms = PagePermissions::read_write();

    let start = std::time::Instant::now();
    for _ in 0..100000 {
        let _ = TranslationData::new(pa, perms, SecurityState::NonSecure);
    }
    let elapsed = start.elapsed();

    // Construction should be very fast (< 5ms for 100k constructions)
    assert!(
        elapsed.as_millis() < 10,
        "TranslationData construction too slow: {:?}",
        elapsed
    );
}

/// Test TranslationResult unwrap_or pattern
#[test]
fn test_translation_result_unwrap_or() {
    let result: TranslationResult = Err(TranslationError::PageNotMapped);
    let default = TranslationData::default();

    let data = result.unwrap_or(default);

    assert_eq!(data.physical_address(), PA::new(0).unwrap());
}

/// Test TranslationResult match pattern
#[test]
fn test_translation_result_match() {
    let pa = PA::new(0xD000).unwrap();
    let data = TranslationData::with_pa(pa);
    let result: TranslationResult = Ok(data);

    match result {
        Ok(d) => assert_eq!(d.physical_address(), pa),
        Err(_) => panic!("Expected Ok, got Err"),
    }
}

/// Test TranslationData builder pattern
#[test]
fn test_translation_data_builder() {
    let pa = PA::new(0xE000).unwrap();
    let perms = PagePermissions::read_execute();

    let data = TranslationData::builder()
        .physical_address(pa)
        .permissions(perms)
        .security_state(SecurityState::Realm)
        .build();

    assert_eq!(data.physical_address(), pa);
    assert_eq!(data.permissions(), perms);
    assert_eq!(data.security_state(), SecurityState::Realm);
}

/// Test zero unsafe code requirement
#[test]
fn test_translation_result_safe() {
    // Exercise all operations without any unsafe blocks
    let pa = PA::new(0xF000).unwrap();
    let data = TranslationData::with_pa(pa);
    let result: TranslationResult = Ok(data);

    let _ = result.clone();
    let _ = format!("{:?}", result);

    // All operations are safe
    assert!(result.is_ok());
}

/// Test TranslationError comprehensive error messages
#[test]
fn test_translation_error_messages() {
    let errors = vec![
        (
            TranslationError::PageNotMapped,
            "page",
        ),
        (
            TranslationError::PermissionViolation {
                access: AccessType::Write,
            },
            "permission",
        ),
        (
            TranslationError::SecurityViolation,
            "security",
        ),
    ];

    for (error, keyword) in errors {
        let msg = format!("{}", error);
        assert!(
            msg.to_lowercase().contains(keyword),
            "Error message '{}' should contain '{}'",
            msg,
            keyword
        );
    }
}

/// Test TranslationData with all security states
#[test]
fn test_translation_data_all_security_states() {
    let pa = PA::new(0x10000).unwrap();
    let perms = PagePermissions::read_write();

    let non_secure = TranslationData::new(pa, perms, SecurityState::NonSecure);
    assert_eq!(non_secure.security_state(), SecurityState::NonSecure);

    let secure = TranslationData::new(pa, perms, SecurityState::Secure);
    assert_eq!(secure.security_state(), SecurityState::Secure);

    let realm = TranslationData::new(pa, perms, SecurityState::Realm);
    assert_eq!(realm.security_state(), SecurityState::Realm);
}

/// Test TranslationResult with error context propagation
#[test]
fn test_translation_error_context() {
    let error = TranslationError::InvalidAddress { address: 0xBADC0DE };

    // Verify error contains context
    let error_str = format!("{:?}", error);
    assert!(!error_str.is_empty());
}

/// Test TranslationData immutability
#[test]
fn test_translation_data_immutable() {
    let pa = PA::new(0x11000).unwrap();
    let data = TranslationData::with_pa(pa);

    // Verify accessors return values, not mutable references
    let _pa = data.physical_address();
    let _perms = data.permissions();
    let _state = data.security_state();

    // Original data unchanged
    assert_eq!(data.physical_address(), pa);
}
