//! Security state definitions for ARM SMMU v3
//!
//! This module implements ARM SMMU v3 security domains including support for
//! ARM Confidential Compute Architecture (CCA) Realm and SMMUv3.3 RME Root state.

use crate::types::ValidationError;
use core::fmt;

/// Security state for ARM SMMU v3
///
/// ARM SMMU v3.3 (RME) supports four security domains per §3.10:
/// - Secure: Trusted execution environment
/// - NonSecure: Normal world
/// - Realm: ARM CCA confidential compute (isolated from both Secure and NonSecure)
/// - Root: Highest privilege (RME), can access all PA spaces
///
/// # Encoding
///
/// Internal bit encoding used for serialization (matches PA-space identifiers
/// from ARM §3.10, not the SEC_SID input field — §3.10.1 line 2511 designates
/// SEC_SID=0b11 as Reserved for client transactions; Root=0b11 is the SMMU's
/// own privileged PA space, not a valid incoming SEC_SID):
/// - 0b00: NonSecure
/// - 0b01: Secure
/// - 0b10: Realm
/// - 0b11: Root (SMMUv3.3 RME — SMMU PA space, not a SEC_SID input value)
///
/// # Example
///
/// ```
/// use smmu::SecurityState;
///
/// let state = SecurityState::Secure;
/// assert!(state.is_secure());
/// assert!(!state.is_non_secure());
///
/// // Secure can access NonSecure (downgrade)
/// assert!(state.can_access(SecurityState::NonSecure));
///
/// // NonSecure cannot access Secure
/// assert!(!SecurityState::NonSecure.can_access(SecurityState::Secure));
///
/// // Root can access all states
/// assert!(SecurityState::Root.can_access(SecurityState::Secure));
/// assert!(SecurityState::Root.can_access(SecurityState::NonSecure));
/// assert!(SecurityState::Root.can_access(SecurityState::Realm));
/// assert!(SecurityState::Root.can_access(SecurityState::Root));
///
/// // No other state can access Root
/// assert!(!SecurityState::Secure.can_access(SecurityState::Root));
/// ```
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SecurityState {
    /// Non-secure world (TrustZone Non-secure state) — SEC_SID=0b00
    NonSecure = 0b00,

    /// Secure world (TrustZone Secure state) — SEC_SID=0b01
    Secure = 0b01,

    /// Realm world (ARM CCA Confidential Compute)
    Realm = 0b10,

    /// Root world (SMMUv3.3 RME — highest privilege, can access all PA spaces)
    Root = 0b11,
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

    /// Check if this is the Root state (SMMUv3.3 RME)
    #[inline]
    #[must_use]
    pub const fn is_root(self) -> bool {
        matches!(self, Self::Root)
    }

    /// Check if the security state is root (const version)
    ///
    /// This is a const-compatible version of `is_root()` for use in const contexts.
    #[inline]
    #[must_use]
    pub const fn const_is_root(self) -> bool {
        self.is_root()
    }

    /// Check if a state transition is valid
    ///
    /// # ARM SMMU v3 Rules
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
    /// # ARM SMMU v3 Access Rules (§3.10)
    ///
    /// - Same state can always access itself
    /// - Secure can access NonSecure (downgrade)
    /// - NonSecure cannot access Secure
    /// - Realm is completely isolated from both Secure and NonSecure
    /// - Root (SMMUv3.3 RME) can access all PA spaces (Secure, NonSecure, Realm, Root)
    /// - No other state can access Root PA space
    #[inline]
    #[must_use]
    pub const fn can_access(self, target: Self) -> bool {
        matches!(
            (self, target),
            // Root can access all PA spaces
            (Self::Root, _)
                // Secure can access Secure and NonSecure (downgrade)
                | (Self::Secure, Self::Secure | Self::NonSecure)
                // NonSecure can only access NonSecure
                | (Self::NonSecure, Self::NonSecure)
                // Realm can only access Realm
                | (Self::Realm, Self::Realm)
        )
    }

    /// Validate access from this state to target state
    ///
    /// # Errors
    ///
    /// Returns `Err` if access is not permitted by ARM SMMU v3 security rules.
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

    /// Convert to ARM SMMU v3 bit encoding
    #[inline]
    #[must_use]
    pub const fn to_bits(self) -> u8 {
        self as u8
    }

    /// Create from ARM SMMU v3 bit encoding
    ///
    /// # Errors
    ///
    /// Returns `Err` if bits are invalid (> 0b11).
    pub const fn from_bits(bits: u8) -> Result<Self, ValidationError> {
        match bits {
            0b00 => Ok(Self::NonSecure),
            0b01 => Ok(Self::Secure),
            0b10 => Ok(Self::Realm),
            0b11 => Ok(Self::Root),
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
            Self::Root => "Root",
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
        // ARM §3.10 SEC_SID: NonSecure=0b00, Secure=0b01, Realm=0b10, Root=0b11
        assert_eq!(SecurityState::NonSecure.to_bits(), 0b00);
        assert_eq!(SecurityState::Secure.to_bits(), 0b01);
        assert_eq!(SecurityState::Realm.to_bits(), 0b10);
    }
}
