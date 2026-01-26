//! Translation stage definitions for ARM SMMU v3

use crate::types::ValidationError;
use crate::types::fault_type::{AddressType, TranslationStep};
use core::fmt;

/// ARM SMMU v3 Translation Stage Configuration
///
/// Defines which translation stages are active:
/// - Bypass: No translation
/// - Stage1: IOVA → PA (process/application address space)
/// - Stage2: IPA → PA (guest/VM address space)
/// - Stage1And2: IOVA → IPA → PA (nested virtualization)
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TranslationStage {
    /// No translation - direct passthrough
    Bypass = 0b00,

    /// Stage 1 only (IOVA → PA)
    Stage1 = 0b01,

    /// Stage 2 only (IPA → PA)
    Stage2 = 0b10,

    /// Two-stage translation (IOVA → IPA → PA)
    Stage1And2 = 0b11,
}

impl TranslationStage {
    /// Check if Stage 1 translation is used
    #[inline]
    #[must_use]
    pub const fn uses_stage1(self) -> bool {
        matches!(self, Self::Stage1 | Self::Stage1And2)
    }

    /// Check if Stage 2 translation is used
    #[inline]
    #[must_use]
    pub const fn uses_stage2(self) -> bool {
        matches!(self, Self::Stage2 | Self::Stage1And2)
    }

    /// Check if this is two-stage translation
    #[inline]
    #[must_use]
    pub const fn is_two_stage(self) -> bool {
        matches!(self, Self::Stage1And2)
    }

    /// Check if this is bypass mode
    #[inline]
    #[must_use]
    pub const fn is_bypass(self) -> bool {
        matches!(self, Self::Bypass)
    }

    /// Const versions for compile-time evaluation
    #[inline]
    #[must_use]
    pub const fn const_uses_stage1(self) -> bool {
        self.uses_stage1()
    }

    #[inline]
    #[must_use]
    pub const fn const_uses_stage2(self) -> bool {
        self.uses_stage2()
    }

    #[inline]
    #[must_use]
    pub const fn const_is_two_stage(self) -> bool {
        self.is_two_stage()
    }

    /// Get input address type
    #[must_use]
    pub const fn input_address_type(self) -> AddressType {
        match self {
            Self::Bypass => AddressType::PA,
            Self::Stage1 | Self::Stage1And2 => AddressType::IOVA,
            Self::Stage2 => AddressType::IPA,
        }
    }

    /// Get output address type
    #[must_use]
    pub const fn output_address_type(self) -> AddressType {
        AddressType::PA
    }

    /// Get intermediate address type (for two-stage)
    #[must_use]
    pub const fn intermediate_address_type(self) -> Option<AddressType> {
        match self {
            Self::Stage1And2 => Some(AddressType::IPA),
            _ => None,
        }
    }

    /// Validate stage configuration
    pub const fn validate_configuration(self, stage1_enabled: bool, stage2_enabled: bool) -> Result<(), ValidationError> {
        match self {
            Self::Bypass => Ok(()),
            Self::Stage1 => {
                if stage1_enabled {
                    Ok(())
                } else {
                    Err(ValidationError::InvalidTranslationStage { bits: self as u8 })
                }
            }
            Self::Stage2 => {
                if stage2_enabled {
                    Ok(())
                } else {
                    Err(ValidationError::InvalidTranslationStage { bits: self as u8 })
                }
            }
            Self::Stage1And2 => {
                if stage1_enabled && stage2_enabled {
                    Ok(())
                } else {
                    Err(ValidationError::InvalidTranslationStage { bits: self as u8 })
                }
            }
        }
    }

    /// Get translation sequence
    #[must_use]
    pub fn translation_sequence(self) -> Vec<TranslationStep> {
        match self {
            Self::Bypass => vec![],
            Self::Stage1 => vec![TranslationStep::Stage1],
            Self::Stage2 => vec![TranslationStep::Stage2],
            Self::Stage1And2 => vec![TranslationStep::Stage1, TranslationStep::Stage2],
        }
    }

    /// Check if PASID is supported at translation level
    #[inline]
    #[must_use]
    pub const fn supports_pasid(self) -> bool {
        matches!(self, Self::Stage1 | Self::Stage1And2)
    }

    /// Check if page tables are required
    #[inline]
    #[must_use]
    pub const fn requires_page_tables(self) -> bool {
        !matches!(self, Self::Bypass)
    }

    /// Check if page size is supported
    #[inline]
    #[must_use]
    pub const fn supports_page_size(self, _page_size: u64) -> bool {
        // All non-bypass modes support standard page sizes
        !matches!(self, Self::Bypass)
    }

    /// Check if transition to target stage is valid
    #[must_use]
    pub const fn can_transition_to(self, target: Self) -> bool {
        match (self, target) {
            // Same stage is always valid
            _ if self as u8 == target as u8 => true,
            // Any stage can transition to bypass
            (_, Self::Bypass) => true,
            // Bypass can transition to any stage
            (Self::Bypass, _) => true,
            // Stage1 can upgrade to two-stage
            (Self::Stage1, Self::Stage1And2) => true,
            // Stage2 cannot directly transition to Stage1 (different address spaces)
            (Self::Stage2, Self::Stage1) => false,
            // Other combinations need re-configuration
            _ => false,
        }
    }

    /// Check if this represents process address space
    #[inline]
    #[must_use]
    pub const fn is_process_address_space(self) -> bool {
        matches!(self, Self::Stage1 | Self::Stage1And2)
    }

    /// Check if this represents guest/VM address space
    #[inline]
    #[must_use]
    pub const fn is_guest_address_space(self) -> bool {
        matches!(self, Self::Stage2 | Self::Stage1And2)
    }

    /// Convert to ARM SMMU v3 bit encoding
    #[inline]
    #[must_use]
    pub const fn to_bits(self) -> u8 {
        self as u8
    }

    /// Create from ARM SMMU v3 bit encoding
    pub const fn from_bits(bits: u8) -> Result<Self, ValidationError> {
        match bits {
            0b00 => Ok(Self::Bypass),
            0b01 => Ok(Self::Stage1),
            0b10 => Ok(Self::Stage2),
            0b11 => Ok(Self::Stage1And2),
            _ => Err(ValidationError::InvalidTranslationStage { bits }),
        }
    }

    /// Validate for stream configuration (stub for now)
    pub fn validate_for_config(self, _config: &StreamConfig) -> Result<(), ValidationError> {
        // TODO: Implement once StreamConfig is defined
        Ok(())
    }
}

impl fmt::Display for TranslationStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Bypass => "Bypass",
            Self::Stage1 => "Stage 1",
            Self::Stage2 => "Stage 2",
            Self::Stage1And2 => "Stage 1+2",
        };
        write!(f, "{}", s)
    }
}

impl Default for TranslationStage {
    /// Default to bypass (safest, no translation)
    fn default() -> Self {
        Self::Bypass
    }
}

/// Stream configuration (stub for now)
#[derive(Debug, Default)]
pub struct StreamConfig {
    // TODO: Add fields as needed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stage_flags() {
        assert!(!TranslationStage::Bypass.uses_stage1());
        assert!(!TranslationStage::Bypass.uses_stage2());

        assert!(TranslationStage::Stage1.uses_stage1());
        assert!(!TranslationStage::Stage1.uses_stage2());

        assert!(!TranslationStage::Stage2.uses_stage1());
        assert!(TranslationStage::Stage2.uses_stage2());

        assert!(TranslationStage::Stage1And2.uses_stage1());
        assert!(TranslationStage::Stage1And2.uses_stage2());
        assert!(TranslationStage::Stage1And2.is_two_stage());
    }

    #[test]
    fn test_encoding() {
        assert_eq!(TranslationStage::Bypass.to_bits(), 0b00);
        assert_eq!(TranslationStage::Stage1.to_bits(), 0b01);
        assert_eq!(TranslationStage::Stage2.to_bits(), 0b10);
        assert_eq!(TranslationStage::Stage1And2.to_bits(), 0b11);
    }
}
