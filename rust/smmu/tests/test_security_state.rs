//! Comprehensive tests for SecurityState enum
//!
//! Tests cover:
//! - SecurityState enum variants (Secure, NonSecure, Realm)
//! - State validation and transition methods
//! - ARM SMMU v3 security domain isolation
//! - State transition rules and validation
//! - Copy, Clone, Debug, PartialEq, Eq traits

use smmu::{SecurityState, ValidationError};

// ============================================================================
// Basic SecurityState Tests
// ============================================================================

#[test]
fn test_security_state_secure() {
    let state = SecurityState::Secure;
    assert!(state.is_secure());
    assert!(!state.is_non_secure());
    assert!(!state.is_realm());
}

#[test]
fn test_security_state_non_secure() {
    let state = SecurityState::NonSecure;
    assert!(!state.is_secure());
    assert!(state.is_non_secure());
    assert!(!state.is_realm());
}

#[test]
fn test_security_state_realm() {
    let state = SecurityState::Realm;
    assert!(!state.is_secure());
    assert!(!state.is_non_secure());
    assert!(state.is_realm());
}

// ============================================================================
// State Transition Tests
// ============================================================================

#[test]
fn test_transition_secure_to_non_secure_denied() {
    let state = SecurityState::Secure;
    let result = state.transition_to(SecurityState::NonSecure);

    // Direct transition from Secure to NonSecure not allowed
    assert!(result.is_err());
    assert!(matches!(
        result,
        Err(ValidationError::InvalidStateTransition { .. })
    ));
}

#[test]
fn test_transition_secure_to_secure_allowed() {
    let state = SecurityState::Secure;
    let result = state.transition_to(SecurityState::Secure);

    // Same-state transition is always allowed (no-op)
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), SecurityState::Secure);
}

#[test]
fn test_transition_non_secure_to_secure_denied() {
    let state = SecurityState::NonSecure;
    let result = state.transition_to(SecurityState::Secure);

    // NonSecure cannot transition to Secure
    assert!(result.is_err());
}

#[test]
fn test_transition_realm_to_secure_denied() {
    let state = SecurityState::Realm;
    let result = state.transition_to(SecurityState::Secure);

    // Realm cannot transition to Secure
    assert!(result.is_err());
}

#[test]
fn test_transition_realm_to_non_secure_denied() {
    let state = SecurityState::Realm;
    let result = state.transition_to(SecurityState::NonSecure);

    // Realm cannot transition to NonSecure directly
    assert!(result.is_err());
}

#[test]
fn test_all_same_state_transitions_allowed() {
    let states = [
        SecurityState::Secure,
        SecurityState::NonSecure,
        SecurityState::Realm,
    ];

    for state in &states {
        let result = state.transition_to(*state);
        assert!(result.is_ok(), "Same-state transition should always succeed");
        assert_eq!(result.unwrap(), *state);
    }
}

// ============================================================================
// State Isolation Tests
// ============================================================================

#[test]
fn test_can_access_secure_from_secure() {
    let accessing_state = SecurityState::Secure;
    let target_state = SecurityState::Secure;

    assert!(accessing_state.can_access(target_state));
}

#[test]
fn test_cannot_access_secure_from_non_secure() {
    let accessing_state = SecurityState::NonSecure;
    let target_state = SecurityState::Secure;

    assert!(!accessing_state.can_access(target_state));
}

#[test]
fn test_can_access_non_secure_from_secure() {
    let accessing_state = SecurityState::Secure;
    let target_state = SecurityState::NonSecure;

    // Secure world can access NonSecure (downgrade)
    assert!(accessing_state.can_access(target_state));
}

#[test]
fn test_can_access_non_secure_from_non_secure() {
    let accessing_state = SecurityState::NonSecure;
    let target_state = SecurityState::NonSecure;

    assert!(accessing_state.can_access(target_state));
}

#[test]
fn test_realm_isolation_from_secure() {
    let accessing_state = SecurityState::Secure;
    let target_state = SecurityState::Realm;

    // Secure cannot directly access Realm (isolated)
    assert!(!accessing_state.can_access(target_state));
}

#[test]
fn test_realm_isolation_from_non_secure() {
    let accessing_state = SecurityState::NonSecure;
    let target_state = SecurityState::Realm;

    // NonSecure cannot access Realm (isolated)
    assert!(!accessing_state.can_access(target_state));
}

#[test]
fn test_realm_can_access_itself() {
    let accessing_state = SecurityState::Realm;
    let target_state = SecurityState::Realm;

    assert!(accessing_state.can_access(target_state));
}

// ============================================================================
// ARM SMMU v3 Encoding Tests
// ============================================================================

#[test]
fn test_security_state_to_bits() {
    // ARM SMMU v3 security state encoding
    assert_eq!(SecurityState::Secure.to_bits(), 0b00);
    assert_eq!(SecurityState::NonSecure.to_bits(), 0b01);
    assert_eq!(SecurityState::Realm.to_bits(), 0b10);
}

#[test]
fn test_security_state_from_bits() {
    assert_eq!(
        SecurityState::from_bits(0b00),
        Ok(SecurityState::Secure)
    );
    assert_eq!(
        SecurityState::from_bits(0b01),
        Ok(SecurityState::NonSecure)
    );
    assert_eq!(
        SecurityState::from_bits(0b10),
        Ok(SecurityState::Realm)
    );
}

#[test]
fn test_security_state_from_bits_invalid() {
    let result = SecurityState::from_bits(0b11);
    assert!(result.is_err());
    assert!(matches!(
        result,
        Err(ValidationError::InvalidSecurityState { .. })
    ));
}

#[test]
fn test_security_state_from_bits_out_of_range() {
    let result = SecurityState::from_bits(0xFF);
    assert!(result.is_err());
}

// ============================================================================
// Trait Implementation Tests
// ============================================================================

#[test]
fn test_security_state_copy_clone() {
    let state1 = SecurityState::Secure;
    let state2 = state1; // Copy
    let state3 = state1.clone(); // Clone

    assert_eq!(state1, state2);
    assert_eq!(state1, state3);
}

#[test]
fn test_security_state_equality() {
    let secure1 = SecurityState::Secure;
    let secure2 = SecurityState::Secure;
    let non_secure = SecurityState::NonSecure;

    assert_eq!(secure1, secure2);
    assert_ne!(secure1, non_secure);
}

#[test]
fn test_security_state_debug() {
    let state = SecurityState::Secure;
    let debug = format!("{:?}", state);
    assert!(debug.contains("Secure"));
}

#[test]
fn test_security_state_display() {
    assert_eq!(format!("{}", SecurityState::Secure), "Secure");
    assert_eq!(format!("{}", SecurityState::NonSecure), "NonSecure");
    assert_eq!(format!("{}", SecurityState::Realm), "Realm");
}

// ============================================================================
// Const Methods Tests
// ============================================================================

#[test]
fn test_const_is_secure() {
    const STATE: SecurityState = SecurityState::Secure;
    const IS_SECURE: bool = STATE.const_is_secure();
    assert!(IS_SECURE);
}

#[test]
fn test_const_is_non_secure() {
    const STATE: SecurityState = SecurityState::NonSecure;
    const IS_NON_SECURE: bool = STATE.const_is_non_secure();
    assert!(IS_NON_SECURE);
}

#[test]
fn test_const_is_realm() {
    const STATE: SecurityState = SecurityState::Realm;
    const IS_REALM: bool = STATE.const_is_realm();
    assert!(IS_REALM);
}

#[test]
fn test_const_evaluation() {
    // Test compile-time evaluation
    const _STATE1: SecurityState = SecurityState::Secure;
    const _STATE2: SecurityState = SecurityState::NonSecure;
    const _IS_SECURE: bool = SecurityState::Secure.const_is_secure();

    // If this compiles, const evaluation works
}

// ============================================================================
// Validation Tests
// ============================================================================

#[test]
fn test_validate_access_secure_to_secure() {
    let from = SecurityState::Secure;
    let to = SecurityState::Secure;

    let result = from.validate_access(to);
    assert!(result.is_ok());
}

#[test]
fn test_validate_access_non_secure_to_secure_denied() {
    let from = SecurityState::NonSecure;
    let to = SecurityState::Secure;

    let result = from.validate_access(to);
    assert!(result.is_err());
    assert!(matches!(
        result,
        Err(ValidationError::SecurityViolation { .. })
    ));
}

#[test]
fn test_validate_access_realm_isolation() {
    let from = SecurityState::NonSecure;
    let to = SecurityState::Realm;

    let result = from.validate_access(to);
    assert!(result.is_err());
}

// ============================================================================
// ARM SMMU v3 Compliance Tests
// ============================================================================

#[test]
fn test_security_domain_isolation() {
    // ARM SMMU v3 requires strict isolation between security domains
    let domains = [
        SecurityState::Secure,
        SecurityState::NonSecure,
        SecurityState::Realm,
    ];

    for &from_domain in &domains {
        for &to_domain in &domains {
            let can_access = from_domain.can_access(to_domain);

            // Same domain always accessible
            if from_domain == to_domain {
                assert!(can_access, "{:?} should access {:?}", from_domain, to_domain);
            } else if from_domain == SecurityState::Secure
                && to_domain == SecurityState::NonSecure
            {
                // Secure can downgrade to NonSecure
                assert!(can_access);
            } else if to_domain == SecurityState::Realm {
                // Realm is isolated from all others
                assert!(!can_access);
            } else {
                // All other cross-domain access denied
                assert!(!can_access);
            }
        }
    }
}

#[test]
fn test_realm_cca_compliance() {
    // ARM Confidential Compute Architecture (CCA) Realm isolation
    let realm = SecurityState::Realm;

    // Realm is completely isolated
    assert!(!SecurityState::Secure.can_access(realm));
    assert!(!SecurityState::NonSecure.can_access(realm));
    assert!(realm.can_access(realm));
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_default_security_state() {
    let state = SecurityState::default();
    // Default should be NonSecure (least privileged)
    assert_eq!(state, SecurityState::NonSecure);
}

#[test]
fn test_all_security_states_unique() {
    let secure = SecurityState::Secure;
    let non_secure = SecurityState::NonSecure;
    let realm = SecurityState::Realm;

    // All states should be distinct
    assert_ne!(secure, non_secure);
    assert_ne!(secure, realm);
    assert_ne!(non_secure, realm);

    // All encodings should be distinct
    assert_ne!(secure.to_bits(), non_secure.to_bits());
    assert_ne!(secure.to_bits(), realm.to_bits());
    assert_ne!(non_secure.to_bits(), realm.to_bits());
}

// ============================================================================
// Performance Tests
// ============================================================================

#[test]
fn test_security_check_performance() {
    const ITERATIONS: usize = 100_000;
    let state = SecurityState::Secure;

    let start = std::time::Instant::now();
    for _ in 0..ITERATIONS {
        let _s = state.is_secure();
        let _n = state.is_non_secure();
        let _r = state.is_realm();
    }
    let duration = start.elapsed();

    // Security checks should be extremely fast
    assert!(
        duration.as_millis() < 10,
        "Security checks too slow: {:?}",
        duration
    );
}
