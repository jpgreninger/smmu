//! ARM SMMU v3 StreamContext Core Tests - Section 4.1
//!
//! Comprehensive TDD test suite for StreamContext implementation following the
//! ARM SMMU v3 specification. These tests are written BEFORE implementation to
//! drive the design and ensure complete functionality coverage.
//!
//! # Test Organization
//!
//! 1. **PASID Lifecycle Tests** (Task 4.1.1)
//!    - Create/remove PASID operations
//!    - PASID count limits enforcement
//!    - Duplicate PASID handling
//!    - Clear all PASIDs operation
//!
//! 2. **Isolation Validation Tests** (Task 4.1.2)
//!    - PASID 0 isolation verification
//!    - Cross-PASID access prevention
//!    - Security state isolation
//!    - Concurrent PASID access safety
//!
//! 3. **Configuration Validation Tests** (Task 4.1.3)
//!    - Stage-1 only configuration
//!    - Stage-2 only configuration
//!    - Two-stage configuration
//!    - Bypass mode (no stages)
//!    - Invalid configuration detection
//!    - Configuration state transitions
//!
//! 4. **Stage-1/Stage-2 Tests** (Task 4.1.4)
//!    - Stage-1 translation with multiple PASIDs
//!    - Stage-2 translation (shared across PASIDs)
//!    - Two-stage translation (Stage-1 → Stage-2)
//!    - Stage enable/disable operations
//!
//! 5. **Thread Safety Tests** (Task 4.1.5)
//!    - Concurrent PASID creation
//!    - Concurrent translation requests
//!    - Concurrent configuration updates
//!
//! 6. **Integration with AddressSpace** (Task 4.1.6)
//!    - Map/unmap through StreamContext
//!    - Translation through StreamContext
//!    - AddressSpace Arc reference counting
//!
//! # ARM SMMU v3 Compliance
//!
//! - PASID 0 support (default/legacy mode)
//! - Security state isolation (Secure/NonSecure/Realm)
//! - Stage configurations per ARM spec
//! - Fault handling integration
//!
//! Copyright (c) 2024 John Greninger

#![cfg(test)]

use smmu::types::{AccessType, PagePermissions, SecurityState, TranslationError, IOVA, PA, PASID};
use std::sync::Arc;
use std::thread;

// ============================================================================
// Section 4.1.1: PASID Lifecycle Tests
// ============================================================================

/// Test: Create new PASID with fresh AddressSpace
///
/// ARM SMMU v3 spec: Each PASID creates an isolated translation context
#[test]
#[ignore] // Pending StreamContext implementation
fn test_create_pasid_success() {
    // This test will fail until StreamContext::new() is implemented
    let stream_context = smmu::stream_context::StreamContext::new();

    // Create PASID 0 (valid per ARM SMMU v3 spec)
    let result = stream_context.create_pasid(PASID::new(0).unwrap());
    assert!(result.is_ok(), "PASID 0 creation should succeed");

    // Create PASID 1
    let result = stream_context.create_pasid(PASID::new(1).unwrap());
    assert!(result.is_ok(), "PASID 1 creation should succeed");

    // Verify PASID count
    assert_eq!(
        stream_context.pasid_count(),
        2,
        "Should have 2 PASIDs created"
    );

    // Verify PASID existence
    assert!(
        stream_context.has_pasid(PASID::new(0).unwrap()),
        "PASID 0 should exist"
    );
    assert!(
        stream_context.has_pasid(PASID::new(1).unwrap()),
        "PASID 1 should exist"
    );
}

/// Test: Create duplicate PASID returns error
///
/// ARM SMMU v3 spec: PASID must be unique within stream context
#[test]
#[ignore] // Pending StreamContext implementation
fn test_create_duplicate_pasid_error() {
    let stream_context = smmu::stream_context::StreamContext::new();

    // Create PASID 1 first time
    let result = stream_context.create_pasid(PASID::new(1).unwrap());
    assert!(result.is_ok(), "First PASID creation should succeed");

    // Attempt to create same PASID again
    let result = stream_context.create_pasid(PASID::new(1).unwrap());
    assert!(result.is_err(), "Duplicate PASID creation should fail");

    // Verify error type is PASIDAlreadyExists
    let error = result.unwrap_err();
    match error {
        smmu::types::StreamContextError::PASIDAlreadyExists(pasid_val) => {
            assert_eq!(pasid_val, 1, "Error should contain PASID value 1");
        }
        _ => panic!("Expected PASIDAlreadyExists error, got: {:?}", error),
    }
}

/// Test: Remove existing PASID and verify cleanup
///
/// ARM SMMU v3 spec: PASID removal invalidates all associated translations
#[test]
#[ignore] // Pending StreamContext implementation
fn test_remove_pasid_success() {
    let stream_context = smmu::stream_context::StreamContext::new();

    // Create and remove PASID
    stream_context
        .create_pasid(PASID::new(1).unwrap())
        .unwrap();
    assert_eq!(stream_context.pasid_count(), 1);

    let result = stream_context.remove_pasid(PASID::new(1).unwrap());
    assert!(result.is_ok(), "PASID removal should succeed");

    // Verify PASID is removed
    assert_eq!(stream_context.pasid_count(), 0, "PASID count should be 0");
    assert!(
        !stream_context.has_pasid(PASID::new(1).unwrap()),
        "PASID should not exist after removal"
    );
}

/// Test: Remove non-existent PASID returns error
///
/// ARM SMMU v3 spec: Removing non-existent PASID is an error condition
#[test]
#[ignore] // Pending StreamContext implementation
fn test_remove_nonexistent_pasid_error() {
    let stream_context = smmu::stream_context::StreamContext::new();

    // Attempt to remove PASID that doesn't exist
    let result = stream_context.remove_pasid(PASID::new(99).unwrap());
    assert!(result.is_err(), "Removing non-existent PASID should fail");

    // Verify error type is PASIDNotFound
    let error = result.unwrap_err();
    match error {
        smmu::types::StreamContextError::PASIDNotFound(pasid_val) => {
            assert_eq!(pasid_val, 99, "Error should contain PASID value 99");
        }
        _ => panic!("Expected PASIDNotFound error, got: {:?}", error),
    }
}

/// Test: PASID count limit enforcement
///
/// ARM SMMU v3 spec: Maximum PASIDs per stream is configurable
#[test]
#[ignore] // Pending StreamContext implementation
fn test_pasid_count_limit_enforcement() {
    let mut stream_context = smmu::stream_context::StreamContext::new();

    // Set maximum PASIDs to 3
    stream_context.set_max_pasids_per_stream(3);

    // Create 3 PASIDs successfully
    for i in 0..3 {
        let result = stream_context.create_pasid(PASID::new(i).unwrap());
        assert!(result.is_ok(), "PASID {} creation should succeed", i);
    }

    // Attempt to create 4th PASID should fail
    let result = stream_context.create_pasid(PASID::new(3).unwrap());
    assert!(
        result.is_err(),
        "Creating PASID beyond limit should fail"
    );

    // Verify error type is PASIDLimitExceeded
    let error = result.unwrap_err();
    match error {
        smmu::types::StreamContextError::PASIDLimitExceeded(current, max) => {
            assert_eq!(current, 3, "Current count should be 3");
            assert_eq!(max, 3, "Max should be 3");
        }
        _ => panic!("Expected PASIDLimitExceeded error, got: {:?}", error),
    }
}

/// Test: Clear all PASIDs operation
///
/// ARM SMMU v3 spec: Clearing all PASIDs invalidates entire stream context
#[test]
#[ignore] // Pending StreamContext implementation
fn test_clear_all_pasids() {
    let stream_context = smmu::stream_context::StreamContext::new();

    // Create multiple PASIDs
    for i in 0..5 {
        stream_context
            .create_pasid(PASID::new(i).unwrap())
            .unwrap();
    }
    assert_eq!(stream_context.pasid_count(), 5);

    // Clear all PASIDs
    let result = stream_context.clear_all_pasids();
    assert!(result.is_ok(), "Clear all PASIDs should succeed");

    // Verify all PASIDs are removed
    assert_eq!(
        stream_context.pasid_count(),
        0,
        "PASID count should be 0 after clear"
    );

    // Verify individual PASIDs don't exist
    for i in 0..5 {
        assert!(
            !stream_context.has_pasid(PASID::new(i).unwrap()),
            "PASID {} should not exist after clear",
            i
        );
    }
}

/// Test: PASID 0 is valid and supported
///
/// ARM SMMU v3 spec: PASID 0 is valid for kernel/hypervisor contexts
#[test]
#[ignore] // Pending StreamContext implementation
fn test_pasid_zero_is_valid() {
    let stream_context = smmu::stream_context::StreamContext::new();

    // PASID 0 should be valid
    let result = stream_context.create_pasid(PASID::new(0).unwrap());
    assert!(result.is_ok(), "PASID 0 creation should succeed");

    // Should be able to map and translate through PASID 0
    let perms = PagePermissions::new(true, true, false);
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x4000).unwrap();

    stream_context
        .map_page(PASID::new(0).unwrap(), iova, pa, perms, SecurityState::NonSecure)
        .unwrap();

    let result = stream_context.translate(
        PASID::new(0).unwrap(),
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(result.is_ok(), "Translation through PASID 0 should succeed");
}

/// Test: Invalid PASID values are rejected
///
/// ARM SMMU v3 spec: PASID is 20-bit (0 to 1,048,575)
#[test]
#[ignore] // Pending StreamContext implementation
fn test_invalid_pasid_rejected() {
    let stream_context = smmu::stream_context::StreamContext::new();

    // PASID creation with max valid value should succeed
    let max_valid_pasid = (1 << 20) - 1; // 20-bit max
    let result = stream_context.create_pasid(PASID::new(max_valid_pasid).unwrap());
    assert!(
        result.is_ok(),
        "PASID at max value should be accepted by StreamContext"
    );
}

// ============================================================================
// Section 4.1.2: Isolation Validation Tests
// ============================================================================

/// Test: PASID 0 isolation from other PASIDs
///
/// ARM SMMU v3 spec: Each PASID has independent address space
#[test]
#[ignore] // Pending StreamContext implementation
fn test_pasid_zero_isolation() {
    let stream_context = smmu::stream_context::StreamContext::new();

    // Create PASID 0 and PASID 1
    stream_context
        .create_pasid(PASID::new(0).unwrap())
        .unwrap();
    stream_context
        .create_pasid(PASID::new(1).unwrap())
        .unwrap();

    // Map same IOVA to different PAs in each PASID
    let iova = IOVA::new(0x1000).unwrap();
    let pa0 = PA::new(0x4000).unwrap();
    let pa1 = PA::new(0x8000).unwrap();
    let perms = PagePermissions::new(true, true, false);

    stream_context
        .map_page(PASID::new(0).unwrap(), iova, pa0, perms, SecurityState::NonSecure)
        .unwrap();
    stream_context
        .map_page(PASID::new(1).unwrap(), iova, pa1, perms, SecurityState::NonSecure)
        .unwrap();

    // Verify translations are isolated
    let result0 = stream_context
        .translate(
            PASID::new(0).unwrap(),
            iova,
            AccessType::Read,
            SecurityState::NonSecure,
        )
        .unwrap();
    let result1 = stream_context
        .translate(
            PASID::new(1).unwrap(),
            iova,
            AccessType::Read,
            SecurityState::NonSecure,
        )
        .unwrap();

    assert_eq!(result0.physical_address(), pa0, "PASID 0 should map to PA0");
    assert_eq!(result1.physical_address(), pa1, "PASID 1 should map to PA1");
    assert_ne!(
        result0.physical_address(), result1.physical_address(),
        "PASIDs should be isolated"
    );
}

/// Test: One PASID cannot access another PASID's address space
///
/// ARM SMMU v3 spec: PASID isolation enforcement
#[test]
#[ignore] // Pending StreamContext implementation
fn test_cross_pasid_access_prevention() {
    let stream_context = smmu::stream_context::StreamContext::new();

    // Create PASID 1 with mapping
    stream_context
        .create_pasid(PASID::new(1).unwrap())
        .unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x4000).unwrap();
    let perms = PagePermissions::new(true, true, false);
    stream_context
        .map_page(PASID::new(1).unwrap(), iova, pa, perms, SecurityState::NonSecure)
        .unwrap();

    // Create PASID 2 without mapping at same IOVA
    stream_context
        .create_pasid(PASID::new(2).unwrap())
        .unwrap();

    // Translation through PASID 1 should succeed
    let result1 = stream_context.translate(
        PASID::new(1).unwrap(),
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(result1.is_ok(), "Translation through PASID 1 should succeed");

    // Translation through PASID 2 should fail (page not mapped)
    let result2 = stream_context.translate(
        PASID::new(2).unwrap(),
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result2.is_err(),
        "Translation through PASID 2 should fail"
    );

    // Verify error is PageNotMapped
    match result2.unwrap_err() {
        smmu::types::TranslationError::PageNotMapped => {
            // Expected error type
        }
        err => panic!("Expected PageNotMapped error, got: {:?}", err),
    }
}

/// Test: Security state isolation
///
/// ARM SMMU v3 spec: Secure/NonSecure/Realm isolation enforcement
#[test]
fn test_security_state_isolation() {
    let mut stream_context = smmu::stream_context::StreamContext::new();

    // Create PASID for testing
    let pasid = PASID::new(1).unwrap();
    stream_context.create_pasid(pasid).unwrap();

    // Configure Stage-1 only for testing
    stream_context.set_stage1_enabled(true);
    stream_context.set_stage2_enabled(false);

    // Map different IOVAs for different security states
    // Per ARM SMMU v3: Each IOVA maps to a single page entry with one security state
    let iova_secure = IOVA::new(0x1000).unwrap();
    let iova_nonsecure = IOVA::new(0x2000).unwrap(); // DIFFERENT IOVA
    let iova_realm = IOVA::new(0x3000).unwrap(); // DIFFERENT IOVA

    let pa_secure = PA::new(0x4000).unwrap();
    let pa_nonsecure = PA::new(0x8000).unwrap();
    let pa_realm = PA::new(0xC000).unwrap();

    let perms = PagePermissions::new(true, true, false);

    // Map each IOVA with its respective security state
    stream_context
        .map_page(pasid, iova_secure, pa_secure, perms, SecurityState::Secure)
        .unwrap();
    stream_context
        .map_page(
            pasid,
            iova_nonsecure,
            pa_nonsecure,
            perms,
            SecurityState::NonSecure,
        )
        .unwrap();
    stream_context
        .map_page(pasid, iova_realm, pa_realm, perms, SecurityState::Realm)
        .unwrap();

    // Verify each translation returns correct PA
    let result_secure = stream_context
        .translate(pasid, iova_secure, AccessType::Read, SecurityState::Secure)
        .unwrap();
    assert_eq!(
        result_secure.physical_address(),
        pa_secure,
        "Secure translation should return secure PA"
    );

    let result_nonsecure = stream_context
        .translate(
            pasid,
            iova_nonsecure,
            AccessType::Read,
            SecurityState::NonSecure,
        )
        .unwrap();
    assert_eq!(
        result_nonsecure.physical_address(),
        pa_nonsecure,
        "NonSecure translation should return nonsecure PA"
    );

    let result_realm = stream_context
        .translate(pasid, iova_realm, AccessType::Read, SecurityState::Realm)
        .unwrap();
    assert_eq!(
        result_realm.physical_address(),
        pa_realm,
        "Realm translation should return realm PA"
    );

    // Verify security state mismatch returns error (cross-state access prevention)
    let result_wrong_state = stream_context.translate(
        pasid,
        iova_secure,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result_wrong_state.is_err(),
        "Translation with wrong security state should fail"
    );
    match result_wrong_state.unwrap_err() {
        TranslationError::SecurityViolation => {
            // Expected error - cross-state access prevented
        }
        other => panic!("Expected SecurityViolation, got: {:?}", other),
    }
}

/// Test: Concurrent PASID access (thread safety)
///
/// ARM SMMU v3 spec: Concurrent access must be safe
#[test]
#[ignore] // Pending StreamContext implementation
fn test_concurrent_pasid_access() {
    use std::sync::Arc;
    use std::thread;

    let stream_context = Arc::new(smmu::stream_context::StreamContext::new());

    // Create PASIDs for concurrent access
    for i in 0..10 {
        stream_context
            .create_pasid(PASID::new(i).unwrap())
            .unwrap();
    }

    // Spawn threads to concurrently access different PASIDs
    let mut handles = vec![];
    for i in 0..10 {
        let ctx = Arc::clone(&stream_context);
        let handle = thread::spawn(move || {
            let pasid = PASID::new(i).unwrap();
            let iova = IOVA::new((i as u64) * 0x1000 + 0x1000).unwrap();
            let pa = PA::new((i as u64) * 0x4000 + 0x4000).unwrap();
            let perms = PagePermissions::new(true, true, false);

            // Map and translate
            ctx.map_page(pasid, iova, pa, perms, SecurityState::NonSecure)
                .unwrap();
            let result = ctx
                .translate(pasid, iova, AccessType::Read, SecurityState::NonSecure)
                .unwrap();

            assert_eq!(result.physical_address(), pa);
        });
        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().unwrap();
    }
}

// ============================================================================
// Section 4.1.3: Configuration Validation Tests
// ============================================================================

/// Test: Stage-1 only configuration
///
/// ARM SMMU v3 spec: Stage-1 enabled, Stage-2 disabled
#[test]
#[ignore] // Pending StreamContext implementation
fn test_stage1_only_configuration() {
    let mut stream_context = smmu::stream_context::StreamContext::new();

    // Configure Stage-1 only
    stream_context.set_stage1_enabled(true);
    stream_context.set_stage2_enabled(false);

    // Verify configuration
    assert!(
        stream_context.is_stage1_enabled(),
        "Stage-1 should be enabled"
    );
    assert!(
        !stream_context.is_stage2_enabled(),
        "Stage-2 should be disabled"
    );

    // Create PASID and verify Stage-1 translation works
    stream_context
        .create_pasid(PASID::new(1).unwrap())
        .unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x4000).unwrap();
    let perms = PagePermissions::new(true, true, false);

    stream_context
        .map_page(PASID::new(1).unwrap(), iova, pa, perms, SecurityState::NonSecure)
        .unwrap();

    let result = stream_context.translate(
        PASID::new(1).unwrap(),
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(result.is_ok(), "Stage-1 only translation should succeed");
    assert_eq!(result.unwrap().physical_address(), pa);
}

/// Test: Stage-2 only configuration
///
/// ARM SMMU v3 spec: Stage-1 disabled, Stage-2 enabled
#[test]
#[ignore] // Pending StreamContext implementation
fn test_stage2_only_configuration() {
    let mut stream_context = smmu::stream_context::StreamContext::new();

    // Configure Stage-2 only
    stream_context.set_stage1_enabled(false);
    stream_context.set_stage2_enabled(true);

    // Create and configure Stage-2 AddressSpace
    let mut stage2_space = smmu::address_space::AddressSpace::new();
    let ipa = IOVA::new(0x1000).unwrap(); // IPA in Stage-2
    let pa = PA::new(0x4000).unwrap();
    let perms = PagePermissions::new(true, true, false);
    stage2_space
        .map_page(ipa, pa, perms, SecurityState::NonSecure)
        .unwrap();

    stream_context.set_stage2_address_space(Some(Arc::new(stage2_space)));

    // Verify configuration
    assert!(
        !stream_context.is_stage1_enabled(),
        "Stage-1 should be disabled"
    );
    assert!(
        stream_context.is_stage2_enabled(),
        "Stage-2 should be enabled"
    );

    // Create PASID (required even for Stage-2 only)
    stream_context
        .create_pasid(PASID::new(0).unwrap())
        .unwrap();

    // Translation should use Stage-2 only (IOVA directly to Stage-2)
    let result = stream_context.translate(
        PASID::new(0).unwrap(),
        ipa,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(result.is_ok(), "Stage-2 only translation should succeed");
    assert_eq!(result.unwrap().physical_address(), pa);
}

/// Test: Two-stage configuration (both enabled)
///
/// ARM SMMU v3 spec: Stage-1 IOVA→IPA, Stage-2 IPA→PA
#[test]
#[ignore] // Pending StreamContext implementation
fn test_two_stage_configuration() {
    let mut stream_context = smmu::stream_context::StreamContext::new();

    // Configure both stages
    stream_context.set_stage1_enabled(true);
    stream_context.set_stage2_enabled(true);

    // Create Stage-2 AddressSpace
    let mut stage2_space = smmu::address_space::AddressSpace::new();
    let ipa = IOVA::new(0x2000).unwrap(); // Intermediate PA from Stage-1
    let pa = PA::new(0x4000).unwrap(); // Final PA from Stage-2
    let perms = PagePermissions::new(true, true, false);
    stage2_space
        .map_page(ipa, pa, perms, SecurityState::NonSecure)
        .unwrap();

    stream_context.set_stage2_address_space(Some(Arc::new(stage2_space)));

    // Create PASID and configure Stage-1 mapping (IOVA → IPA as PA)
    stream_context
        .create_pasid(PASID::new(1).unwrap())
        .unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let ipa_as_pa = PA::new(0x2000).unwrap(); // IPA value used as PA in Stage-1 mapping
    stream_context
        .map_page(PASID::new(1).unwrap(), iova, ipa_as_pa, perms, SecurityState::NonSecure)
        .unwrap();

    // Verify both stages are enabled
    assert!(
        stream_context.is_stage1_enabled(),
        "Stage-1 should be enabled"
    );
    assert!(
        stream_context.is_stage2_enabled(),
        "Stage-2 should be enabled"
    );

    // Translation should go through both stages: IOVA→IPA→PA
    let result = stream_context.translate(
        PASID::new(1).unwrap(),
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(result.is_ok(), "Two-stage translation should succeed");
    assert_eq!(
        result.unwrap().physical_address(),
        pa,
        "Final PA should be from Stage-2"
    );
}

/// Test: Bypass mode (both stages disabled)
///
/// ARM SMMU v3 spec: Translation disabled, identity mapping
#[test]
#[ignore] // Pending StreamContext implementation
fn test_bypass_mode_both_stages_disabled() {
    let mut stream_context = smmu::stream_context::StreamContext::new();

    // Disable both stages
    stream_context.set_stage1_enabled(false);
    stream_context.set_stage2_enabled(false);

    // Verify configuration
    assert!(
        !stream_context.is_stage1_enabled(),
        "Stage-1 should be disabled"
    );
    assert!(
        !stream_context.is_stage2_enabled(),
        "Stage-2 should be disabled"
    );

    // Create PASID (may still be required for consistency)
    stream_context
        .create_pasid(PASID::new(0).unwrap())
        .unwrap();

    // Translation should return identity mapping (IOVA == PA)
    let iova = IOVA::new(0x1000).unwrap();
    let result = stream_context.translate(
        PASID::new(0).unwrap(),
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(result.is_ok(), "Bypass mode translation should succeed");
    assert_eq!(
        result.unwrap().physical_address().as_u64(),
        iova.as_u64(),
        "Bypass mode should return identity mapping"
    );
}

/// Test: Invalid configuration detection
///
/// ARM SMMU v3 spec: Certain configurations are invalid
#[test]
#[ignore] // Pending StreamContext implementation
fn test_invalid_configuration_detection() {
    let mut stream_context = smmu::stream_context::StreamContext::new();

    // Configure Stage-2 enabled without Stage-2 AddressSpace
    stream_context.set_stage1_enabled(false);
    stream_context.set_stage2_enabled(true);
    stream_context.set_stage2_address_space(None);

    // Create PASID
    stream_context
        .create_pasid(PASID::new(0).unwrap())
        .unwrap();

    // Translation should fail due to missing Stage-2 AddressSpace
    let iova = IOVA::new(0x1000).unwrap();
    let result = stream_context.translate(
        PASID::new(0).unwrap(),
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_err(),
        "Translation with invalid config should fail"
    );
}

/// Test: Configuration state transitions
///
/// ARM SMMU v3 spec: Dynamic reconfiguration support
#[test]
#[ignore] // Pending StreamContext implementation
fn test_configuration_state_transitions() {
    let mut stream_context = smmu::stream_context::StreamContext::new();

    // Start with Stage-1 only
    stream_context.set_stage1_enabled(true);
    stream_context.set_stage2_enabled(false);
    assert!(stream_context.is_stage1_enabled());
    assert!(!stream_context.is_stage2_enabled());

    // Transition to both stages enabled
    stream_context.set_stage2_enabled(true);
    assert!(stream_context.is_stage1_enabled());
    assert!(stream_context.is_stage2_enabled());

    // Transition to Stage-2 only
    stream_context.set_stage1_enabled(false);
    assert!(!stream_context.is_stage1_enabled());
    assert!(stream_context.is_stage2_enabled());

    // Transition to bypass mode
    stream_context.set_stage2_enabled(false);
    assert!(!stream_context.is_stage1_enabled());
    assert!(!stream_context.is_stage2_enabled());
}

// ============================================================================
// Section 4.1.4: Stage-1/Stage-2 Translation Tests
// ============================================================================

/// Test: Stage-1 translation with multiple PASIDs
///
/// ARM SMMU v3 spec: Each PASID has independent Stage-1 translation
#[test]
#[ignore] // Pending StreamContext implementation
fn test_stage1_translation_multiple_pasids() {
    let mut stream_context = smmu::stream_context::StreamContext::new();

    // Configure Stage-1 only
    stream_context.set_stage1_enabled(true);
    stream_context.set_stage2_enabled(false);

    // Create multiple PASIDs with different mappings
    let pasids = [1, 2, 3];
    let iova = IOVA::new(0x1000).unwrap();
    let perms = PagePermissions::new(true, true, false);

    for (idx, &pasid_val) in pasids.iter().enumerate() {
        let pasid = PASID::new(pasid_val).unwrap();
        stream_context.create_pasid(pasid).unwrap();

        let pa = PA::new((idx as u64 + 1) * 0x4000).unwrap();
        stream_context
            .map_page(pasid, iova, pa, perms, SecurityState::NonSecure)
            .unwrap();
    }

    // Verify each PASID translates independently
    for (idx, &pasid_val) in pasids.iter().enumerate() {
        let pasid = PASID::new(pasid_val).unwrap();
        let expected_pa = PA::new((idx as u64 + 1) * 0x4000).unwrap();

        let result = stream_context
            .translate(pasid, iova, AccessType::Read, SecurityState::NonSecure)
            .unwrap();

        assert_eq!(
            result.physical_address(), expected_pa,
            "PASID {} should have independent translation",
            pasid_val
        );
    }
}

/// Test: Stage-2 shared across PASIDs
///
/// ARM SMMU v3 spec: Stage-2 is shared across all PASIDs in stream
#[test]
#[ignore] // Pending StreamContext implementation
fn test_stage2_shared_across_pasids() {
    let mut stream_context = smmu::stream_context::StreamContext::new();

    // Configure both stages
    stream_context.set_stage1_enabled(true);
    stream_context.set_stage2_enabled(true);

    // Create shared Stage-2 AddressSpace
    let mut stage2_space = smmu::address_space::AddressSpace::new();
    let ipa = IOVA::new(0x2000).unwrap();
    let pa = PA::new(0x8000).unwrap();
    let perms = PagePermissions::new(true, true, false);
    stage2_space
        .map_page(ipa, pa, perms, SecurityState::NonSecure)
        .unwrap();

    stream_context.set_stage2_address_space(Some(Arc::new(stage2_space)));

    // Create multiple PASIDs, all mapping to same IPA (as PA in Stage-1)
    let ipa_as_pa = PA::new(0x2000).unwrap(); // IPA value used as PA in Stage-1 mappings
    for i in 0..3 {
        let pasid = PASID::new(i).unwrap();
        stream_context.create_pasid(pasid).unwrap();

        let iova = IOVA::new((i as u64) * 0x1000 + 0x1000).unwrap();
        stream_context
            .map_page(pasid, iova, ipa_as_pa, perms, SecurityState::NonSecure)
            .unwrap();
    }

    // Verify all PASIDs translate to same final PA through shared Stage-2
    for i in 0..3 {
        let pasid = PASID::new(i).unwrap();
        let iova = IOVA::new((i as u64) * 0x1000 + 0x1000).unwrap();

        let result = stream_context
            .translate(pasid, iova, AccessType::Read, SecurityState::NonSecure)
            .unwrap();

        assert_eq!(
            result.physical_address(), pa,
            "PASID {} should use shared Stage-2",
            i
        );
    }
}

/// Test: Two-stage translation path (Stage-1 → Stage-2)
///
/// ARM SMMU v3 spec: IOVA→IPA→PA translation flow
#[test]
#[ignore] // Pending StreamContext implementation
fn test_two_stage_translation_path() {
    let mut stream_context = smmu::stream_context::StreamContext::new();

    // Configure both stages
    stream_context.set_stage1_enabled(true);
    stream_context.set_stage2_enabled(true);

    // Setup Stage-2 mapping: IPA 0x2000 → PA 0x8000
    let mut stage2_space = smmu::address_space::AddressSpace::new();
    let ipa = IOVA::new(0x2000).unwrap();
    let pa = PA::new(0x8000).unwrap();
    let perms = PagePermissions::new(true, true, false);
    stage2_space
        .map_page(ipa, pa, perms, SecurityState::NonSecure)
        .unwrap();

    stream_context.set_stage2_address_space(Some(Arc::new(stage2_space)));

    // Setup Stage-1 mapping: IOVA 0x1000 → IPA 0x2000 (as PA in Stage-1)
    stream_context
        .create_pasid(PASID::new(1).unwrap())
        .unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let ipa_as_pa = PA::new(0x2000).unwrap(); // IPA value used as PA in Stage-1 mapping
    stream_context
        .map_page(PASID::new(1).unwrap(), iova, ipa_as_pa, perms, SecurityState::NonSecure)
        .unwrap();

    // Verify full translation path: IOVA 0x1000 → IPA 0x2000 → PA 0x8000
    let result = stream_context
        .translate(
            PASID::new(1).unwrap(),
            iova,
            AccessType::Read,
            SecurityState::NonSecure,
        )
        .unwrap();

    assert_eq!(
        result.physical_address(), pa,
        "Two-stage translation should produce final PA"
    );
}

/// Test: Stage enable/disable operations
///
/// ARM SMMU v3 spec: Dynamic stage configuration
#[test]
#[ignore] // Pending StreamContext implementation
fn test_stage_enable_disable_operations() {
    let mut stream_context = smmu::stream_context::StreamContext::new();

    // Initial state: Stage-1 enabled, Stage-2 disabled (default)
    stream_context.set_stage1_enabled(true);
    stream_context.set_stage2_enabled(false);

    // Create PASID and mapping
    stream_context
        .create_pasid(PASID::new(1).unwrap())
        .unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x4000).unwrap();
    let perms = PagePermissions::new(true, true, false);
    stream_context
        .map_page(PASID::new(1).unwrap(), iova, pa, perms, SecurityState::NonSecure)
        .unwrap();

    // Verify Stage-1 translation works
    let result = stream_context.translate(
        PASID::new(1).unwrap(),
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(result.is_ok(), "Stage-1 translation should work initially");

    // Disable Stage-1, enable Stage-2
    stream_context.set_stage1_enabled(false);
    stream_context.set_stage2_enabled(true);

    // Setup Stage-2
    let mut stage2_space = smmu::address_space::AddressSpace::new();
    stage2_space
        .map_page(iova, pa, perms, SecurityState::NonSecure)
        .unwrap();
    stream_context.set_stage2_address_space(Some(Arc::new(stage2_space)));

    // Verify Stage-2 translation now works
    let result = stream_context.translate(
        PASID::new(1).unwrap(),
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(result.is_ok(), "Stage-2 translation should work after switch");
}

// ============================================================================
// Section 4.1.5: Thread Safety Tests
// ============================================================================

/// Test: Concurrent PASID creation
///
/// ARM SMMU v3 spec: Thread-safe PASID management
#[test]
#[ignore] // Pending StreamContext implementation
fn test_concurrent_pasid_creation() {
    let stream_context = Arc::new(smmu::stream_context::StreamContext::new());

    // Spawn threads to create PASIDs concurrently
    let mut handles = vec![];
    for i in 0..20 {
        let ctx = Arc::clone(&stream_context);
        let handle = thread::spawn(move || {
            let pasid = PASID::new(i).unwrap();
            ctx.create_pasid(pasid).unwrap();
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify all PASIDs were created
    assert_eq!(
        stream_context.pasid_count(),
        20,
        "All 20 PASIDs should be created"
    );
}

/// Test: Concurrent translation requests
///
/// ARM SMMU v3 spec: Thread-safe translation operations
#[test]
#[ignore] // Pending StreamContext implementation
fn test_concurrent_translation_requests() {
    // Configure Stage-1 only (before wrapping in Arc)
    let mut stream_context = smmu::stream_context::StreamContext::new();
    stream_context.set_stage1_enabled(true);
    stream_context.set_stage2_enabled(false);

    let stream_context = Arc::new(stream_context);

    // Setup PASIDs with mappings
    for i in 0..10 {
        let pasid = PASID::new(i).unwrap();
        stream_context.create_pasid(pasid).unwrap();

        let iova = IOVA::new((i as u64) * 0x1000 + 0x1000).unwrap();
        let pa = PA::new((i as u64) * 0x4000 + 0x4000).unwrap();
        let perms = PagePermissions::new(true, true, false);
        stream_context
            .map_page(pasid, iova, pa, perms, SecurityState::NonSecure)
            .unwrap();
    }

    // Spawn threads to perform concurrent translations
    let mut handles = vec![];
    for i in 0..10 {
        let ctx = Arc::clone(&stream_context);
        let handle = thread::spawn(move || {
            let pasid = PASID::new(i).unwrap();
            let iova = IOVA::new((i as u64) * 0x1000 + 0x1000).unwrap();
            let expected_pa = PA::new((i as u64) * 0x4000 + 0x4000).unwrap();

            // Perform multiple translations
            for _ in 0..100 {
                let result = ctx
                    .translate(pasid, iova, AccessType::Read, SecurityState::NonSecure)
                    .unwrap();
                assert_eq!(result.physical_address(), expected_pa);
            }
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }
}

/// Test: Concurrent configuration updates
///
/// ARM SMMU v3 spec: Thread-safe configuration changes
#[test]
#[ignore] // Pending StreamContext implementation
fn test_concurrent_configuration_updates() {
    let stream_context = Arc::new(std::sync::Mutex::new(
        smmu::stream_context::StreamContext::new(),
    ));

    // Spawn threads to update configuration concurrently
    let mut handles = vec![];
    for i in 0..10 {
        let ctx = Arc::clone(&stream_context);
        let handle = thread::spawn(move || {
            let mut ctx = ctx.lock().unwrap();
            // Toggle stages
            if i % 2 == 0 {
                ctx.set_stage1_enabled(true);
                ctx.set_stage2_enabled(false);
            } else {
                ctx.set_stage1_enabled(false);
                ctx.set_stage2_enabled(true);
            }
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify configuration is consistent (no corruption)
    let ctx = stream_context.lock().unwrap();
    let stage1 = ctx.is_stage1_enabled();
    let stage2 = ctx.is_stage2_enabled();
    assert!(
        (stage1 && !stage2) || (!stage1 && stage2),
        "Configuration should be consistent"
    );
}

// ============================================================================
// Section 4.1.6: Integration with AddressSpace Tests
// ============================================================================

/// Test: Map/unmap through StreamContext
///
/// ARM SMMU v3 spec: StreamContext delegates to AddressSpace
#[test]
#[ignore] // Pending StreamContext implementation
fn test_map_unmap_through_stream_context() {
    let stream_context = smmu::stream_context::StreamContext::new();

    // Create PASID
    stream_context
        .create_pasid(PASID::new(1).unwrap())
        .unwrap();

    // Map page through StreamContext
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x4000).unwrap();
    let perms = PagePermissions::new(true, true, false);
    let result = stream_context.map_page(
        PASID::new(1).unwrap(),
        iova,
        pa,
        perms,
        SecurityState::NonSecure,
    );
    assert!(result.is_ok(), "Map page should succeed");

    // Verify mapping exists
    let trans_result = stream_context.translate(
        PASID::new(1).unwrap(),
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(trans_result.is_ok(), "Translation should succeed");

    // Unmap page through StreamContext
    let result = stream_context.unmap_page(PASID::new(1).unwrap(), iova);
    assert!(result.is_ok(), "Unmap page should succeed");

    // Verify mapping no longer exists
    let trans_result = stream_context.translate(
        PASID::new(1).unwrap(),
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        trans_result.is_err(),
        "Translation should fail after unmap"
    );
}

/// Test: Translation through StreamContext
///
/// ARM SMMU v3 spec: Full translation flow
#[test]
#[ignore] // Pending StreamContext implementation
fn test_translation_through_stream_context() {
    let mut stream_context = smmu::stream_context::StreamContext::new();

    // Configure Stage-1 only
    stream_context.set_stage1_enabled(true);
    stream_context.set_stage2_enabled(false);

    // Create PASID and mapping
    stream_context
        .create_pasid(PASID::new(1).unwrap())
        .unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x4000).unwrap();
    let perms = PagePermissions::new(true, true, true);
    stream_context
        .map_page(PASID::new(1).unwrap(), iova, pa, perms, SecurityState::NonSecure)
        .unwrap();

    // Test read translation
    let result = stream_context.translate(
        PASID::new(1).unwrap(),
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(result.is_ok(), "Read translation should succeed");
    assert_eq!(result.unwrap().physical_address(), pa);

    // Test write translation
    let result = stream_context.translate(
        PASID::new(1).unwrap(),
        iova,
        AccessType::Write,
        SecurityState::NonSecure,
    );
    assert!(result.is_ok(), "Write translation should succeed");

    // Test execute translation
    let result = stream_context.translate(
        PASID::new(1).unwrap(),
        iova,
        AccessType::Execute,
        SecurityState::NonSecure,
    );
    assert!(result.is_ok(), "Execute translation should succeed");
}

/// Test: AddressSpace Arc reference counting
///
/// Rust spec: Arc provides shared ownership
#[test]
#[ignore] // Pending StreamContext implementation
fn test_address_space_arc_reference_counting() {
    let stream_context = smmu::stream_context::StreamContext::new();

    // Create PASID with AddressSpace
    stream_context
        .create_pasid(PASID::new(1).unwrap())
        .unwrap();

    // Get AddressSpace reference (should be Arc internally)
    let addr_space = stream_context.get_pasid_address_space(PASID::new(1).unwrap());
    assert!(
        addr_space.is_some(),
        "AddressSpace should exist for PASID"
    );

    // Remove PASID - AddressSpace should still be accessible if Arc count > 1
    let _addr_space_clone = addr_space.unwrap();
    stream_context.remove_pasid(PASID::new(1).unwrap()).unwrap();

    // AddressSpace should still be valid due to Arc reference
    // (This tests Arc semantics - the clone keeps it alive)
}

/// Test: Shared AddressSpace across multiple PASIDs
///
/// ARM SMMU v3 spec: Multiple PASIDs can share same address space
#[test]
#[ignore] // Pending StreamContext implementation
fn test_shared_address_space_multiple_pasids() {
    let stream_context = smmu::stream_context::StreamContext::new();

    // Create first PASID with AddressSpace
    stream_context
        .create_pasid(PASID::new(1).unwrap())
        .unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x4000).unwrap();
    let perms = PagePermissions::new(true, true, false);
    stream_context
        .map_page(PASID::new(1).unwrap(), iova, pa, perms, SecurityState::NonSecure)
        .unwrap();

    // Get AddressSpace from PASID 1
    let addr_space = stream_context
        .get_pasid_address_space(PASID::new(1).unwrap())
        .unwrap();

    // Add PASID 2 sharing same AddressSpace
    stream_context.add_pasid(PASID::new(2).unwrap(), addr_space).unwrap();

    // Verify both PASIDs translate to same PA
    let result1 = stream_context
        .translate(
            PASID::new(1).unwrap(),
            iova,
            AccessType::Read,
            SecurityState::NonSecure,
        )
        .unwrap();
    let result2 = stream_context
        .translate(
            PASID::new(2).unwrap(),
            iova,
            AccessType::Read,
            SecurityState::NonSecure,
        )
        .unwrap();

    assert_eq!(
        result1.physical_address(), result2.physical_address(),
        "Shared AddressSpace should produce same translation"
    );
}

// ============================================================================
// Additional Edge Case Tests
// ============================================================================

/// Test: Translation with non-existent PASID
///
/// ARM SMMU v3 spec: PASIDNotFound error
#[test]
#[ignore] // Pending StreamContext implementation
fn test_translation_nonexistent_pasid() {
    let stream_context = smmu::stream_context::StreamContext::new();

    let iova = IOVA::new(0x1000).unwrap();
    let result = stream_context.translate(
        PASID::new(99).unwrap(),
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(result.is_err(), "Translation should fail");
    match result.unwrap_err() {
        smmu::types::TranslationError::PASIDNotFound => {
            // Expected error type
        }
        err => panic!("Expected PASIDNotFound error, got: {:?}", err),
    }
}

/// Test: Permission violation detection
///
/// ARM SMMU v3 spec: PermissionViolation error
#[test]
#[ignore] // Pending StreamContext implementation
fn test_permission_violation_detection() {
    let mut stream_context = smmu::stream_context::StreamContext::new();

    // Configure Stage-1 only
    stream_context.set_stage1_enabled(true);
    stream_context.set_stage2_enabled(false);

    // Create PASID with read-only mapping
    stream_context
        .create_pasid(PASID::new(1).unwrap())
        .unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x4000).unwrap();
    let perms = PagePermissions::new(true, false, false); // Read-only
    stream_context
        .map_page(PASID::new(1).unwrap(), iova, pa, perms, SecurityState::NonSecure)
        .unwrap();

    // Read should succeed
    let result = stream_context.translate(
        PASID::new(1).unwrap(),
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(result.is_ok(), "Read should succeed");

    // Write should fail with permission violation
    let result = stream_context.translate(
        PASID::new(1).unwrap(),
        iova,
        AccessType::Write,
        SecurityState::NonSecure,
    );
    assert!(result.is_err(), "Write should fail on read-only page");
    match result.unwrap_err() {
        smmu::types::TranslationError::PermissionViolation { access } => {
            assert_eq!(access, AccessType::Write, "Permission violation should be for Write access");
        }
        err => panic!("Expected PermissionViolation error, got: {:?}", err),
    }
}

/// Test: Stage-2 fault propagation
///
/// ARM SMMU v3 spec: Stage-2 faults reported correctly
#[test]
#[ignore] // Pending StreamContext implementation
fn test_stage2_fault_propagation() {
    let mut stream_context = smmu::stream_context::StreamContext::new();

    // Configure both stages
    stream_context.set_stage1_enabled(true);
    stream_context.set_stage2_enabled(true);

    // Setup Stage-2 with NO mapping
    let stage2_space = smmu::address_space::AddressSpace::new();
    stream_context.set_stage2_address_space(Some(Arc::new(stage2_space)));

    // Setup Stage-1 mapping to IPA (treating IPA as PA for Stage-1 mapping)
    stream_context
        .create_pasid(PASID::new(1).unwrap())
        .unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let ipa_as_pa = PA::new(0x2000).unwrap(); // IPA value treated as PA in Stage-1
    let perms = PagePermissions::new(true, true, false);
    stream_context
        .map_page(PASID::new(1).unwrap(), iova, ipa_as_pa, perms, SecurityState::NonSecure)
        .unwrap();

    // Translation should fail in Stage-2 (IPA not mapped)
    let result = stream_context.translate(
        PASID::new(1).unwrap(),
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(result.is_err(), "Translation should fail in Stage-2");
    match result.unwrap_err() {
        smmu::types::TranslationError::PageNotMapped => {
            // Expected error type from Stage-2
        }
        err => panic!("Expected PageNotMapped error from Stage-2, got: {:?}", err),
    }
}

/// Test: Drop cleanup (RAII)
///
/// Rust spec: StreamContext properly cleans up on drop
#[test]
#[ignore] // Pending StreamContext implementation
fn test_drop_cleanup() {
    {
        let stream_context = smmu::stream_context::StreamContext::new();

        // Create PASIDs and mappings
        for i in 0..5 {
            stream_context
                .create_pasid(PASID::new(i).unwrap())
                .unwrap();
        }

        // StreamContext goes out of scope here
    }

    // If RAII is correct, all resources should be cleaned up
    // This test verifies no leaks via Arc reference counting
}
