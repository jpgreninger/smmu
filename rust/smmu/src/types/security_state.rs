//! Security state definitions for ARM `SMMU` v3
//!
//! This module implements ARM `SMMU` v3 security domains including support for
//! ARM Confidential Compute Architecture (CCA) Realm.

use crate::types::ValidationError;
use core::fmt;

/// Security state for ARM `SMMU` v3
///
/// ARM `SMMU` v3 supports three security domains:
/// - Secure: Trusted execution environment
/// - NonSecure: Normal world
/// - Realm: ARM CCA confidential compute (isolated from both Secure and NonSecure)
///
/// # Encoding
///
/// ARM `SMMU` v3 uses 2-bit encoding:
/// - 0b00: Secure
/// - 0b01: NonSecure
/// - 0b10: Realm
///
/// # Example
///
/// ```
/// use smmu::`SecurityState`;
///
/// let state = `SecurityState`::Secure;
/// assert!(state.is_secure());
/// assert!(!state.is_non_secure());
///
/// // Secure can access NonSecure (downgrade)
/// assert!(state.can_access(`SecurityState`::NonSecure));
///
/// // NonSecure cannot access Secure
/// assert!(!`SecurityState`::NonSecure.can_access(`SecurityState`::Secure));
/// ```
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SecurityState {
    /// Secure world (TrustZone Secure state)
    Secure = 0b00,

    /// Non-secure world (TrustZone Non-secure state)
    NonSecure = 0b01,

    /// Realm world (ARM CCA Confidential Compute)
    Realm = 0b10,
}

impl SecurityState {
    /// Check if this is the Secure state
    #[inline]
    #[must_use]
    pub const fn is_secure(self) -> bool {
        matches!(self, Self::Secure)
    }

    /// Check if this is the NonSecure state
    #[inline]
    #[must_use]
    pub const fn is_non_secure(self) -> bool {
        matches!(self, Self::NonSecure)
    }

    /// Check if this is the Realm state
    #[inline]
    #[must_use]
    pub const fn is_realm(self) -> bool {
        matches!(self, Self::Realm)
    }

    /// Const versions for compile-time evaluation
    #[inline]
    #[must_use]
    pub const fn const_is_secure(self) -> bool {
        self.is_secure()
    }

    /// Check if the security state is non-secure (const version)
    ///
    /// This is a const-compatible version of `is_non_secure()` for use in const contexts.
    #[inline]
    #[must_use]
    pub const fn const_is_non_secure(self) -> bool {
        self.is_non_secure()
    }

    /// Check if the security state is realm (const version)
    ///
    /// This is a const-compatible version of `is_realm()` for use in const contexts.
    #[inline]
    #[must_use]
    pub const fn const_is_realm(self) -> bool {
        self.is_realm()
    }

    /// Check if a state transition is valid
    ///
    /// # ARM `SMMU` v3 Rules
    ///
    /// - Same-state transitions are always allowed (no-op)
    /// - All cross-state transitions require reconfiguration and are not allowed as simple transitions
    /// - Security state changes require explicit reconfiguration, not just state transition
    ///
    /// Note: This is different from `can_access()` which checks if one state can access
    /// memory in another state. Transitions are configuration changes, access is runtime checking.
    ///
    /// # Errors
    ///
    /// Returns an error if the transition is not allowed.
    pub fn transition_to(self, target: Self) -> Result<Self, ValidationError> {
        if self == target {
            // Same-state transition is always allowed (no-op)
            return Ok(target);
        }

        // All cross-state transitions require reconfiguration
        Err(ValidationError::InvalidStateTransition {
            from: format!("{self}"),
            to: format!("{target}"),
        })
    }

    /// Check if this state can access the target state
    ///
    /// # ARM `SMMU` v3 Access Rules
    ///
    /// - Same state can always access itself
    /// - Secure can access NonSecure (downgrade)
    /// - NonSecure cannot access Secure
    /// - Realm is completely isolated from both Secure and NonSecure
    #[inline]
    #[must_use]
    pub const fn can_access(self, target: Self) -> bool {
        matches!((self, target),
            // Same state can always access itself
            (Self::Secure, Self::Secure) | (Self::NonSecure, Self::NonSecure) | (Self::Realm, Self::Realm) |
            // Secure can access NonSecure (downgrade)
            (Self::Secure, Self::NonSecure)
        )
    }

    /// Validate access from this state to target state
    ///
    /// # Errors
    ///
    /// Returns `Err` if access is not permitted by ARM `SMMU` v3 security rules.
    pub fn validate_access(self, target: Self) -> Result<(), ValidationError> {
        if self.can_access(target) {
            Ok(())
        } else {
            Err(ValidationError::SecurityViolation {
                from_state: format!("{self}"),
                to_state: format!("{target}"),
            })
        }
    }

    /// Convert to ARM `SMMU` v3 bit encoding
    #[inline]
    #[must_use]
    pub const fn to_bits(self) -> u8 {
        self as u8
    }

    /// Create from ARM `SMMU` v3 bit encoding
    ///
    /// # Errors
    ///
    /// Returns `Err` if bits are invalid (>= 0b11).
    pub const fn from_bits(bits: u8) -> Result<Self, ValidationError> {
        match bits {
            0b00 => Ok(Self::Secure),
            0b01 => Ok(Self::NonSecure),
            0b10 => Ok(Self::Realm),
            _ => Err(ValidationError::InvalidSecurityState { bits }),
        }
    }
}

impl fmt::Display for SecurityState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Secure => "Secure",
            Self::NonSecure => "NonSecure",
            Self::Realm => "Realm",
        };
        write!(f, "{s}")
    }
}

impl Default for SecurityState {
    /// Default to NonSecure (least privileged)
    fn default() -> Self {
        Self::NonSecure
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_states() {
        assert!(SecurityState::Secure.is_secure());
        assert!(SecurityState::NonSecure.is_non_secure());
        assert!(SecurityState::Realm.is_realm());
    }

    #[test]
    fn test_access_rules() {
        // Same state can access itself
        assert!(SecurityState::Secure.can_access(SecurityState::Secure));
        assert!(SecurityState::NonSecure.can_access(SecurityState::NonSecure));
        assert!(SecurityState::Realm.can_access(SecurityState::Realm));

        // Secure can access NonSecure
        assert!(SecurityState::Secure.can_access(SecurityState::NonSecure));

        // NonSecure cannot access Secure
        assert!(!SecurityState::NonSecure.can_access(SecurityState::Secure));

        // Realm is isolated
        assert!(!SecurityState::Secure.can_access(SecurityState::Realm));
        assert!(!SecurityState::NonSecure.can_access(SecurityState::Realm));
    }

    #[test]
    fn test_encoding() {
        assert_eq!(SecurityState::Secure.to_bits(), 0b00);
        assert_eq!(SecurityState::NonSecure.to_bits(), 0b01);
        assert_eq!(SecurityState::Realm.to_bits(), 0b10);
    }
}
