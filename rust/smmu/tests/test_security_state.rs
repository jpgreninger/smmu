#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]

//! Comprehensive tests for `SecurityState` type
//!
//! This test suite achieves 100% coverage for `types/security_state.rs` by testing:
//! - State transition validation (9 combinations)
//! - Realm world state handling
//! - Security violation detection
//! - Display formatting for all states
//! - Const methods for compile-time evaluation
//! - Bit encoding/decoding
//! - Access control rules

use smmu::types::SecurityState;
use smmu::ValidationError;
use std::collections::HashSet;

// ============================================================================
// Basic State Checks
// ============================================================================

#[test]
fn test_security_state_is_secure() {
    let state = SecurityState::Secure;
    assert!(state.is_secure());
    assert!(!state.is_non_secure());
    assert!(!state.is_realm());
}

#[test]
fn test_security_state_is_non_secure() {
    let state = SecurityState::NonSecure;
    assert!(!state.is_secure());
    assert!(state.is_non_secure());
    assert!(!state.is_realm());
}

#[test]
fn test_security_state_is_realm() {
    let state = SecurityState::Realm;
    assert!(!state.is_secure());
    assert!(!state.is_non_secure());
    assert!(state.is_realm());
}

// ============================================================================
// Const Methods for Compile-Time Evaluation
// ============================================================================

#[test]
fn test_security_state_const_is_secure() {
    let state = SecurityState::Secure;
    assert!(state.const_is_secure());
    assert!(!SecurityState::NonSecure.const_is_secure());
    assert!(!SecurityState::Realm.const_is_secure());
}

#[test]
fn test_security_state_const_is_non_secure() {
    let state = SecurityState::NonSecure;
    assert!(state.const_is_non_secure());
    assert!(!SecurityState::Secure.const_is_non_secure());
    assert!(!SecurityState::Realm.const_is_non_secure());
}

#[test]
fn test_security_state_const_is_realm() {
    let state = SecurityState::Realm;
    assert!(state.const_is_realm());
    assert!(!SecurityState::Secure.const_is_realm());
    assert!(!SecurityState::NonSecure.const_is_realm());
}

#[test]
fn test_security_state_const_methods_in_const_context() {
    const fn check_secure(state: SecurityState) -> bool {
        state.const_is_secure()
    }

    const fn check_non_secure(state: SecurityState) -> bool {
        state.const_is_non_secure()
    }

    const fn check_realm(state: SecurityState) -> bool {
        state.const_is_realm()
    }

    assert!(check_secure(SecurityState::Secure));
    assert!(check_non_secure(SecurityState::NonSecure));
    assert!(check_realm(SecurityState::Realm));
}

// ============================================================================
// State Transition Tests (9 Combinations)
// ============================================================================

#[test]
fn test_transition_secure_to_secure() {
    let state = SecurityState::Secure;
    let result = state.transition_to(SecurityState::Secure);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), SecurityState::Secure);
}

#[test]
fn test_transition_nonsecure_to_nonsecure() {
    let state = SecurityState::NonSecure;
    let result = state.transition_to(SecurityState::NonSecure);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), SecurityState::NonSecure);
}

#[test]
fn test_transition_realm_to_realm() {
    let state = SecurityState::Realm;
    let result = state.transition_to(SecurityState::Realm);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), SecurityState::Realm);
}

#[test]
fn test_transition_secure_to_nonsecure_requires_reconfiguration() {
    let state = SecurityState::Secure;
    let result = state.transition_to(SecurityState::NonSecure);
    assert!(result.is_err());

    if let Err(ValidationError::InvalidStateTransition { from, to }) = result {
        assert_eq!(from, "Secure");
        assert_eq!(to, "NonSecure");
    } else {
        panic!("Expected InvalidStateTransition error");
    }
}

#[test]
fn test_transition_secure_to_realm_requires_reconfiguration() {
    let state = SecurityState::Secure;
    let result = state.transition_to(SecurityState::Realm);
    assert!(result.is_err());

    if let Err(ValidationError::InvalidStateTransition { from, to }) = result {
        assert_eq!(from, "Secure");
        assert_eq!(to, "Realm");
    } else {
        panic!("Expected InvalidStateTransition error");
    }
}

#[test]
fn test_transition_nonsecure_to_secure_requires_reconfiguration() {
    let state = SecurityState::NonSecure;
    let result = state.transition_to(SecurityState::Secure);
    assert!(result.is_err());

    if let Err(ValidationError::InvalidStateTransition { from, to }) = result {
        assert_eq!(from, "NonSecure");
        assert_eq!(to, "Secure");
    } else {
        panic!("Expected InvalidStateTransition error");
    }
}

#[test]
fn test_transition_nonsecure_to_realm_requires_reconfiguration() {
    let state = SecurityState::NonSecure;
    let result = state.transition_to(SecurityState::Realm);
    assert!(result.is_err());

    if let Err(ValidationError::InvalidStateTransition { from, to }) = result {
        assert_eq!(from, "NonSecure");
        assert_eq!(to, "Realm");
    } else {
        panic!("Expected InvalidStateTransition error");
    }
}

#[test]
fn test_transition_realm_to_secure_requires_reconfiguration() {
    let state = SecurityState::Realm;
    let result = state.transition_to(SecurityState::Secure);
    assert!(result.is_err());

    if let Err(ValidationError::InvalidStateTransition { from, to }) = result {
        assert_eq!(from, "Realm");
        assert_eq!(to, "Secure");
    } else {
        panic!("Expected InvalidStateTransition error");
    }
}

#[test]
fn test_transition_realm_to_nonsecure_requires_reconfiguration() {
    let state = SecurityState::Realm;
    let result = state.transition_to(SecurityState::NonSecure);
    assert!(result.is_err());

    if let Err(ValidationError::InvalidStateTransition { from, to }) = result {
        assert_eq!(from, "Realm");
        assert_eq!(to, "NonSecure");
    } else {
        panic!("Expected InvalidStateTransition error");
    }
}

// ============================================================================
// Access Control Rules Tests
// ============================================================================

#[test]
fn test_can_access_same_state_secure() {
    assert!(SecurityState::Secure.can_access(SecurityState::Secure));
}

#[test]
fn test_can_access_same_state_nonsecure() {
    assert!(SecurityState::NonSecure.can_access(SecurityState::NonSecure));
}

#[test]
fn test_can_access_same_state_realm() {
    assert!(SecurityState::Realm.can_access(SecurityState::Realm));
}

#[test]
fn test_can_access_secure_to_nonsecure() {
    // Secure can access NonSecure (downgrade)
    assert!(SecurityState::Secure.can_access(SecurityState::NonSecure));
}

#[test]
fn test_cannot_access_nonsecure_to_secure() {
    // NonSecure cannot access Secure
    assert!(!SecurityState::NonSecure.can_access(SecurityState::Secure));
}

#[test]
fn test_cannot_access_secure_to_realm() {
    // Realm is isolated from Secure
    assert!(!SecurityState::Secure.can_access(SecurityState::Realm));
}

#[test]
fn test_cannot_access_nonsecure_to_realm() {
    // Realm is isolated from NonSecure
    assert!(!SecurityState::NonSecure.can_access(SecurityState::Realm));
}

#[test]
fn test_cannot_access_realm_to_secure() {
    // Realm is isolated from Secure
    assert!(!SecurityState::Realm.can_access(SecurityState::Secure));
}

#[test]
fn test_cannot_access_realm_to_nonsecure() {
    // Realm is isolated from NonSecure
    assert!(!SecurityState::Realm.can_access(SecurityState::NonSecure));
}

// ============================================================================
// Validate Access Tests (Security Violation Detection)
// ============================================================================

#[test]
fn test_validate_access_same_state_secure() {
    let result = SecurityState::Secure.validate_access(SecurityState::Secure);
    assert!(result.is_ok());
}

#[test]
fn test_validate_access_same_state_nonsecure() {
    let result = SecurityState::NonSecure.validate_access(SecurityState::NonSecure);
    assert!(result.is_ok());
}

#[test]
fn test_validate_access_same_state_realm() {
    let result = SecurityState::Realm.validate_access(SecurityState::Realm);
    assert!(result.is_ok());
}

#[test]
fn test_validate_access_secure_to_nonsecure() {
    let result = SecurityState::Secure.validate_access(SecurityState::NonSecure);
    assert!(result.is_ok());
}

#[test]
fn test_validate_access_nonsecure_to_secure_violation() {
    let result = SecurityState::NonSecure.validate_access(SecurityState::Secure);
    assert!(result.is_err());

    if let Err(ValidationError::SecurityViolation { from_state, to_state }) = result {
        assert_eq!(from_state, "NonSecure");
        assert_eq!(to_state, "Secure");
    } else {
        panic!("Expected SecurityViolation error");
    }
}

#[test]
fn test_validate_access_secure_to_realm_violation() {
    let result = SecurityState::Secure.validate_access(SecurityState::Realm);
    assert!(result.is_err());

    if let Err(ValidationError::SecurityViolation { from_state, to_state }) = result {
        assert_eq!(from_state, "Secure");
        assert_eq!(to_state, "Realm");
    } else {
        panic!("Expected SecurityViolation error");
    }
}

#[test]
fn test_validate_access_nonsecure_to_realm_violation() {
    let result = SecurityState::NonSecure.validate_access(SecurityState::Realm);
    assert!(result.is_err());

    if let Err(ValidationError::SecurityViolation { from_state, to_state }) = result {
        assert_eq!(from_state, "NonSecure");
        assert_eq!(to_state, "Realm");
    } else {
        panic!("Expected SecurityViolation error");
    }
}

#[test]
fn test_validate_access_realm_to_secure_violation() {
    let result = SecurityState::Realm.validate_access(SecurityState::Secure);
    assert!(result.is_err());

    if let Err(ValidationError::SecurityViolation { from_state, to_state }) = result {
        assert_eq!(from_state, "Realm");
        assert_eq!(to_state, "Secure");
    } else {
        panic!("Expected SecurityViolation error");
    }
}

#[test]
fn test_validate_access_realm_to_nonsecure_violation() {
    let result = SecurityState::Realm.validate_access(SecurityState::NonSecure);
    assert!(result.is_err());

    if let Err(ValidationError::SecurityViolation { from_state, to_state }) = result {
        assert_eq!(from_state, "Realm");
        assert_eq!(to_state, "NonSecure");
    } else {
        panic!("Expected SecurityViolation error");
    }
}

// ============================================================================
// Bit Encoding/Decoding Tests
// ============================================================================

#[test]
fn test_to_bits_secure() {
    assert_eq!(SecurityState::Secure.to_bits(), 0b00);
}

#[test]
fn test_to_bits_nonsecure() {
    assert_eq!(SecurityState::NonSecure.to_bits(), 0b01);
}

#[test]
fn test_to_bits_realm() {
    assert_eq!(SecurityState::Realm.to_bits(), 0b10);
}

#[test]
fn test_from_bits_secure() {
    let result = SecurityState::from_bits(0b00);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), SecurityState::Secure);
}

#[test]
fn test_from_bits_nonsecure() {
    let result = SecurityState::from_bits(0b01);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), SecurityState::NonSecure);
}

#[test]
fn test_from_bits_realm() {
    let result = SecurityState::from_bits(0b10);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), SecurityState::Realm);
}

#[test]
fn test_from_bits_invalid() {
    let result = SecurityState::from_bits(0b11);
    assert!(result.is_err());

    if let Err(ValidationError::InvalidSecurityState { bits }) = result {
        assert_eq!(bits, 0b11);
    } else {
        panic!("Expected InvalidSecurityState error");
    }
}

#[test]
fn test_from_bits_invalid_large_value() {
    let result = SecurityState::from_bits(0xFF);
    assert!(result.is_err());

    if let Err(ValidationError::InvalidSecurityState { bits }) = result {
        assert_eq!(bits, 0xFF);
    } else {
        panic!("Expected InvalidSecurityState error");
    }
}

#[test]
fn test_from_bits_roundtrip() {
    let states = [SecurityState::Secure, SecurityState::NonSecure, SecurityState::Realm];

    for state in &states {
        let bits = state.to_bits();
        let decoded = SecurityState::from_bits(bits).unwrap();
        assert_eq!(*state, decoded);
    }
}

// ============================================================================
// Display Trait Tests
// ============================================================================

#[test]
fn test_display_secure() {
    let state = SecurityState::Secure;
    assert_eq!(format!("{state}"), "Secure");
}

#[test]
fn test_display_nonsecure() {
    let state = SecurityState::NonSecure;
    assert_eq!(format!("{state}"), "NonSecure");
}

#[test]
fn test_display_realm() {
    let state = SecurityState::Realm;
    assert_eq!(format!("{state}"), "Realm");
}

#[test]
fn test_display_all_states() {
    let states = [
        (SecurityState::Secure, "Secure"),
        (SecurityState::NonSecure, "NonSecure"),
        (SecurityState::Realm, "Realm"),
    ];

    for (state, expected) in &states {
        assert_eq!(format!("{state}"), *expected);
    }
}

// ============================================================================
// Default Trait Test
// ============================================================================

#[test]
fn test_default_is_nonsecure() {
    let state = SecurityState::default();
    assert_eq!(state, SecurityState::NonSecure);
    assert!(state.is_non_secure());
    assert!(!state.is_secure());
    assert!(!state.is_realm());
}

// ============================================================================
// Hash Trait Tests
// ============================================================================

#[test]
fn test_hash_in_hashset() {
    let mut set = HashSet::new();

    set.insert(SecurityState::Secure);
    set.insert(SecurityState::NonSecure);
    set.insert(SecurityState::Realm);
    set.insert(SecurityState::Secure); // Duplicate

    assert_eq!(set.len(), 3); // Only 3 unique states
    assert!(set.contains(&SecurityState::Secure));
    assert!(set.contains(&SecurityState::NonSecure));
    assert!(set.contains(&SecurityState::Realm));
}

#[test]
fn test_hash_consistency() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let state = SecurityState::Realm;

    let mut hasher1 = DefaultHasher::new();
    state.hash(&mut hasher1);
    let hash1 = hasher1.finish();

    let mut hasher2 = DefaultHasher::new();
    state.hash(&mut hasher2);
    let hash2 = hasher2.finish();

    assert_eq!(hash1, hash2);
}

// ============================================================================
// Copy and Clone Trait Tests
// ============================================================================

#[test]
fn test_copy() {
    let state1 = SecurityState::Secure;
    let state2 = state1; // Copy

    assert_eq!(state1, state2);
    assert!(state1.is_secure()); // Original still valid
}

#[test]
fn test_clone() {
    let state1 = SecurityState::Realm;
    let state2 = state1;

    assert_eq!(state1, state2);
}

// ============================================================================
// Equality Tests
// ============================================================================

#[test]
fn test_equality() {
    assert_eq!(SecurityState::Secure, SecurityState::Secure);
    assert_eq!(SecurityState::NonSecure, SecurityState::NonSecure);
    assert_eq!(SecurityState::Realm, SecurityState::Realm);
}

#[test]
fn test_inequality() {
    assert_ne!(SecurityState::Secure, SecurityState::NonSecure);
    assert_ne!(SecurityState::Secure, SecurityState::Realm);
    assert_ne!(SecurityState::NonSecure, SecurityState::Realm);
}

// ============================================================================
// Realm World State Isolation Tests (ARM CCA)
// ============================================================================

#[test]
fn test_realm_isolation_from_secure() {
    // Realm cannot access Secure
    assert!(!SecurityState::Realm.can_access(SecurityState::Secure));

    // Secure cannot access Realm
    assert!(!SecurityState::Secure.can_access(SecurityState::Realm));

    // Validate access should fail in both directions
    assert!(SecurityState::Realm.validate_access(SecurityState::Secure).is_err());
    assert!(SecurityState::Secure.validate_access(SecurityState::Realm).is_err());
}

#[test]
fn test_realm_isolation_from_nonsecure() {
    // Realm cannot access NonSecure
    assert!(!SecurityState::Realm.can_access(SecurityState::NonSecure));

    // NonSecure cannot access Realm
    assert!(!SecurityState::NonSecure.can_access(SecurityState::Realm));

    // Validate access should fail in both directions
    assert!(SecurityState::Realm.validate_access(SecurityState::NonSecure).is_err());
    assert!(SecurityState::NonSecure.validate_access(SecurityState::Realm).is_err());
}

#[test]
fn test_realm_self_access_only() {
    // Realm can only access itself
    assert!(SecurityState::Realm.can_access(SecurityState::Realm));
    assert!(SecurityState::Realm.validate_access(SecurityState::Realm).is_ok());

    // All other access is forbidden
    assert!(!SecurityState::Realm.can_access(SecurityState::Secure));
    assert!(!SecurityState::Realm.can_access(SecurityState::NonSecure));
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_security_downgrade_allowed() {
    // Secure can downgrade to NonSecure (TrustZone model)
    let secure = SecurityState::Secure;
    assert!(secure.can_access(SecurityState::NonSecure));
    assert!(secure.validate_access(SecurityState::NonSecure).is_ok());
}

#[test]
fn test_security_upgrade_forbidden() {
    // NonSecure cannot upgrade to Secure (TrustZone model)
    let nonsecure = SecurityState::NonSecure;
    assert!(!nonsecure.can_access(SecurityState::Secure));
    assert!(nonsecure.validate_access(SecurityState::Secure).is_err());
}

#[test]
fn test_arm_cca_realm_complete_isolation() {
    // ARM Confidential Compute Architecture (CCA) Realm world is completely isolated
    let realm = SecurityState::Realm;

    // Can only access itself
    assert!(realm.can_access(SecurityState::Realm));

    // Cannot access any other world
    assert!(!realm.can_access(SecurityState::Secure));
    assert!(!realm.can_access(SecurityState::NonSecure));

    // No other world can access Realm
    assert!(!SecurityState::Secure.can_access(realm));
    assert!(!SecurityState::NonSecure.can_access(realm));
}

#[test]
fn test_all_state_combinations_access_matrix() {
    // Complete access matrix test (3x3 = 9 combinations)
    let _states = [SecurityState::Secure, SecurityState::NonSecure, SecurityState::Realm];

    let expected_access = [
        // From Secure:
        (SecurityState::Secure, SecurityState::Secure, true),
        (SecurityState::Secure, SecurityState::NonSecure, true),
        (SecurityState::Secure, SecurityState::Realm, false),
        // From NonSecure:
        (SecurityState::NonSecure, SecurityState::Secure, false),
        (SecurityState::NonSecure, SecurityState::NonSecure, true),
        (SecurityState::NonSecure, SecurityState::Realm, false),
        // From Realm:
        (SecurityState::Realm, SecurityState::Secure, false),
        (SecurityState::Realm, SecurityState::NonSecure, false),
        (SecurityState::Realm, SecurityState::Realm, true),
    ];

    for (from, to, should_allow) in &expected_access {
        assert_eq!(
            from.can_access(*to),
            *should_allow,
            "Access from {} to {} should be {}",
            from,
            to,
            if *should_allow { "allowed" } else { "denied" }
        );
    }
}
