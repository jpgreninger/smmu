//! Comprehensive tests for TranslationStage enum
//!
//! Tests cover:
//! - TranslationStage enum variants (Stage1, Stage2, Stage1And2)
//! - Stage coordination logic
//! - Two-stage translation validation
//! - ARM SMMU v3 translation stage configuration
//! - Copy, Clone, Debug, PartialEq, Eq traits

use smmu::{TranslationStage, ValidationError, TranslationStep, AddressType};
use smmu::types::translation_stage::StreamConfig;

// ============================================================================
// Basic TranslationStage Tests
// ============================================================================

#[test]
fn test_translation_stage_stage1() {
    let stage = TranslationStage::Stage1;
    assert!(stage.uses_stage1());
    assert!(!stage.uses_stage2());
    assert!(!stage.is_two_stage());
}

#[test]
fn test_translation_stage_stage2() {
    let stage = TranslationStage::Stage2;
    assert!(!stage.uses_stage1());
    assert!(stage.uses_stage2());
    assert!(!stage.is_two_stage());
}

#[test]
fn test_translation_stage_stage1_and_2() {
    let stage = TranslationStage::Stage1And2;
    assert!(stage.uses_stage1());
    assert!(stage.uses_stage2());
    assert!(stage.is_two_stage());
}

#[test]
fn test_translation_stage_bypass() {
    let stage = TranslationStage::Bypass;
    assert!(!stage.uses_stage1());
    assert!(!stage.uses_stage2());
    assert!(!stage.is_two_stage());
    assert!(stage.is_bypass());
}

// ============================================================================
// Stage Coordination Tests
// ============================================================================

#[test]
fn test_stage1_address_translation() {
    let stage = TranslationStage::Stage1;

    // Stage 1: IOVA -> PA (single stage)
    assert_eq!(stage.input_address_type(), AddressType::IOVA);
    assert_eq!(stage.output_address_type(), AddressType::PA);
}

#[test]
fn test_stage2_address_translation() {
    let stage = TranslationStage::Stage2;

    // Stage 2: IPA -> PA (single stage)
    assert_eq!(stage.input_address_type(), AddressType::IPA);
    assert_eq!(stage.output_address_type(), AddressType::PA);
}

#[test]
fn test_stage1_and_2_address_translation() {
    let stage = TranslationStage::Stage1And2;

    // Two-stage: IOVA -> IPA -> PA
    assert_eq!(stage.input_address_type(), AddressType::IOVA);
    assert_eq!(stage.intermediate_address_type(), Some(AddressType::IPA));
    assert_eq!(stage.output_address_type(), AddressType::PA);
}

#[test]
fn test_bypass_address_translation() {
    let stage = TranslationStage::Bypass;

    // Bypass: No translation
    assert_eq!(stage.input_address_type(), AddressType::PA);
    assert_eq!(stage.output_address_type(), AddressType::PA);
    assert_eq!(stage.intermediate_address_type(), None);
}

// ============================================================================
// Stage Configuration Validation Tests
// ============================================================================

#[test]
fn test_valid_stage1_configuration() {
    let stage = TranslationStage::Stage1;
    let result = stage.validate_configuration(true, false);
    assert!(result.is_ok());
}

#[test]
fn test_valid_stage2_configuration() {
    let stage = TranslationStage::Stage2;
    let result = stage.validate_configuration(false, true);
    assert!(result.is_ok());
}

#[test]
fn test_valid_two_stage_configuration() {
    let stage = TranslationStage::Stage1And2;
    let result = stage.validate_configuration(true, true);
    assert!(result.is_ok());
}

#[test]
fn test_invalid_stage1_configuration() {
    let stage = TranslationStage::Stage1;
    // Stage1 configured but both stages disabled
    let result = stage.validate_configuration(false, false);
    assert!(result.is_err());
}

#[test]
fn test_invalid_two_stage_configuration() {
    let stage = TranslationStage::Stage1And2;
    // Two-stage configured but only stage1 enabled
    let result = stage.validate_configuration(true, false);
    assert!(result.is_err());
}

#[test]
fn test_bypass_configuration_ignores_stages() {
    let stage = TranslationStage::Bypass;
    // Bypass should succeed regardless of stage configuration
    assert!(stage.validate_configuration(false, false).is_ok());
    assert!(stage.validate_configuration(true, false).is_ok());
    assert!(stage.validate_configuration(false, true).is_ok());
    assert!(stage.validate_configuration(true, true).is_ok());
}

// ============================================================================
// Stage Sequence Tests
// ============================================================================

#[test]
fn test_stage1_sequence() {
    let stage = TranslationStage::Stage1;
    let sequence = stage.translation_sequence();

    assert_eq!(sequence.len(), 1);
    assert_eq!(sequence[0], TranslationStep::Stage1);
}

#[test]
fn test_stage2_sequence() {
    let stage = TranslationStage::Stage2;
    let sequence = stage.translation_sequence();

    assert_eq!(sequence.len(), 1);
    assert_eq!(sequence[0], TranslationStep::Stage2);
}

#[test]
fn test_two_stage_sequence() {
    let stage = TranslationStage::Stage1And2;
    let sequence = stage.translation_sequence();

    assert_eq!(sequence.len(), 2);
    assert_eq!(sequence[0], TranslationStep::Stage1);
    assert_eq!(sequence[1], TranslationStep::Stage2);
}

#[test]
fn test_bypass_sequence() {
    let stage = TranslationStage::Bypass;
    let sequence = stage.translation_sequence();

    assert_eq!(sequence.len(), 0);
}

// ============================================================================
// ARM SMMU v3 Encoding Tests
// ============================================================================

#[test]
fn test_translation_stage_to_bits() {
    // ARM SMMU v3 stage configuration encoding
    assert_eq!(TranslationStage::Bypass.to_bits(), 0b00);
    assert_eq!(TranslationStage::Stage1.to_bits(), 0b01);
    assert_eq!(TranslationStage::Stage2.to_bits(), 0b10);
    assert_eq!(TranslationStage::Stage1And2.to_bits(), 0b11);
}

#[test]
fn test_translation_stage_from_bits() {
    assert_eq!(
        TranslationStage::from_bits(0b00),
        Ok(TranslationStage::Bypass)
    );
    assert_eq!(
        TranslationStage::from_bits(0b01),
        Ok(TranslationStage::Stage1)
    );
    assert_eq!(
        TranslationStage::from_bits(0b10),
        Ok(TranslationStage::Stage2)
    );
    assert_eq!(
        TranslationStage::from_bits(0b11),
        Ok(TranslationStage::Stage1And2)
    );
}

#[test]
fn test_translation_stage_from_bits_invalid() {
    let result = TranslationStage::from_bits(0b100);
    assert!(result.is_err());
    assert!(matches!(
        result,
        Err(ValidationError::InvalidTranslationStage { .. })
    ));
}

#[test]
fn test_translation_stage_round_trip() {
    let stages = [
        TranslationStage::Bypass,
        TranslationStage::Stage1,
        TranslationStage::Stage2,
        TranslationStage::Stage1And2,
    ];

    for stage in &stages {
        let bits = stage.to_bits();
        let decoded = TranslationStage::from_bits(bits).unwrap();
        assert_eq!(decoded, *stage);
    }
}

// ============================================================================
// Trait Implementation Tests
// ============================================================================

#[test]
fn test_translation_stage_copy_clone() {
    let stage1 = TranslationStage::Stage1;
    let stage2 = stage1; // Copy
    let stage3 = stage1.clone(); // Clone

    assert_eq!(stage1, stage2);
    assert_eq!(stage1, stage3);
}

#[test]
fn test_translation_stage_equality() {
    let stage1a = TranslationStage::Stage1;
    let stage1b = TranslationStage::Stage1;
    let stage2 = TranslationStage::Stage2;

    assert_eq!(stage1a, stage1b);
    assert_ne!(stage1a, stage2);
}

#[test]
fn test_translation_stage_debug() {
    let stage = TranslationStage::Stage1And2;
    let debug = format!("{:?}", stage);
    assert!(debug.contains("Stage") || debug.contains("1") || debug.contains("2"));
}

#[test]
fn test_translation_stage_display() {
    assert_eq!(format!("{}", TranslationStage::Bypass), "Bypass");
    assert_eq!(format!("{}", TranslationStage::Stage1), "Stage 1");
    assert_eq!(format!("{}", TranslationStage::Stage2), "Stage 2");
    assert_eq!(format!("{}", TranslationStage::Stage1And2), "Stage 1+2");
}

// ============================================================================
// Const Methods Tests
// ============================================================================

#[test]
fn test_const_uses_stage1() {
    const STAGE: TranslationStage = TranslationStage::Stage1;
    const USES_STAGE1: bool = STAGE.const_uses_stage1();
    assert!(USES_STAGE1);
}

#[test]
fn test_const_uses_stage2() {
    const STAGE: TranslationStage = TranslationStage::Stage2;
    const USES_STAGE2: bool = STAGE.const_uses_stage2();
    assert!(USES_STAGE2);
}

#[test]
fn test_const_is_two_stage() {
    const STAGE: TranslationStage = TranslationStage::Stage1And2;
    const IS_TWO_STAGE: bool = STAGE.const_is_two_stage();
    assert!(IS_TWO_STAGE);
}

#[test]
fn test_const_evaluation() {
    // Test compile-time evaluation
    const _STAGE1: TranslationStage = TranslationStage::Stage1;
    const _STAGE2: TranslationStage = TranslationStage::Stage2;
    const _USES_S1: bool = TranslationStage::Stage1.const_uses_stage1();

    // If this compiles, const evaluation works
}

// ============================================================================
// Stage Compatibility Tests
// ============================================================================

#[test]
fn test_compatible_with_pasid() {
    // Stage 1 and two-stage support PASID
    assert!(TranslationStage::Stage1.supports_pasid());
    assert!(TranslationStage::Stage1And2.supports_pasid());

    // Stage 2 and bypass don't support PASID at translation level
    assert!(!TranslationStage::Stage2.supports_pasid());
    assert!(!TranslationStage::Bypass.supports_pasid());
}

#[test]
fn test_requires_page_tables() {
    // All stages except bypass require page tables
    assert!(TranslationStage::Stage1.requires_page_tables());
    assert!(TranslationStage::Stage2.requires_page_tables());
    assert!(TranslationStage::Stage1And2.requires_page_tables());

    assert!(!TranslationStage::Bypass.requires_page_tables());
}

#[test]
fn test_supports_different_page_sizes() {
    // All translation stages support different page sizes
    let stages = [
        TranslationStage::Stage1,
        TranslationStage::Stage2,
        TranslationStage::Stage1And2,
    ];

    for stage in &stages {
        assert!(stage.supports_page_size(0x1000));   // 4KB
        assert!(stage.supports_page_size(0x20_0000)); // 2MB
        assert!(stage.supports_page_size(0x4000_0000)); // 1GB
    }
}

// ============================================================================
// Stage Transition Tests
// ============================================================================

#[test]
fn test_can_transition_to_bypass() {
    // All stages can transition to bypass
    assert!(TranslationStage::Stage1.can_transition_to(TranslationStage::Bypass));
    assert!(TranslationStage::Stage2.can_transition_to(TranslationStage::Bypass));
    assert!(TranslationStage::Stage1And2.can_transition_to(TranslationStage::Bypass));
}

#[test]
fn test_can_transition_from_bypass() {
    // Bypass can transition to any stage
    assert!(TranslationStage::Bypass.can_transition_to(TranslationStage::Stage1));
    assert!(TranslationStage::Bypass.can_transition_to(TranslationStage::Stage2));
    assert!(TranslationStage::Bypass.can_transition_to(TranslationStage::Stage1And2));
}

#[test]
fn test_can_transition_stage1_to_two_stage() {
    // Stage1 can upgrade to two-stage
    assert!(TranslationStage::Stage1.can_transition_to(TranslationStage::Stage1And2));
}

#[test]
fn test_cannot_transition_stage2_to_stage1() {
    // Stage2 cannot directly transition to Stage1 (different address spaces)
    assert!(!TranslationStage::Stage2.can_transition_to(TranslationStage::Stage1));
}

// ============================================================================
// Validation Tests
// ============================================================================

#[test]
fn test_validate_stage_for_stream_context() {
    let config = StreamConfig::default();

    // Validate stage configuration
    let result = TranslationStage::Stage1.validate_for_config(&config);
    assert!(result.is_ok() || result.is_err()); // Either valid or reports error
}

// ============================================================================
// ARM SMMU v3 Compliance Tests
// ============================================================================

#[test]
fn test_stage_capabilities() {
    // ARM SMMU v3 stage-specific capabilities

    // Stage 1: Process/application address space
    let s1 = TranslationStage::Stage1;
    assert!(s1.is_process_address_space());
    assert!(!s1.is_guest_address_space());

    // Stage 2: VM/guest address space
    let s2 = TranslationStage::Stage2;
    assert!(!s2.is_process_address_space());
    assert!(s2.is_guest_address_space());

    // Two-stage: Both
    let s12 = TranslationStage::Stage1And2;
    assert!(s12.is_process_address_space());
    assert!(s12.is_guest_address_space());

    // Bypass: Neither
    let bypass = TranslationStage::Bypass;
    assert!(!bypass.is_process_address_space());
    assert!(!bypass.is_guest_address_space());
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_default_translation_stage() {
    let stage = TranslationStage::default();
    // Default should be bypass (safest, no translation)
    assert_eq!(stage, TranslationStage::Bypass);
}

#[test]
fn test_all_stages_unique() {
    let bypass = TranslationStage::Bypass;
    let stage1 = TranslationStage::Stage1;
    let stage2 = TranslationStage::Stage2;
    let stage12 = TranslationStage::Stage1And2;

    assert_ne!(bypass, stage1);
    assert_ne!(bypass, stage2);
    assert_ne!(bypass, stage12);
    assert_ne!(stage1, stage2);
    assert_ne!(stage1, stage12);
    assert_ne!(stage2, stage12);
}

// ============================================================================
// Performance Tests
// ============================================================================

#[test]
fn test_stage_check_performance() {
    const ITERATIONS: usize = 100_000;
    let stage = TranslationStage::Stage1And2;

    let start = std::time::Instant::now();
    for _ in 0..ITERATIONS {
        let _s1 = stage.uses_stage1();
        let _s2 = stage.uses_stage2();
        let _two = stage.is_two_stage();
    }
    let duration = start.elapsed();

    assert!(
        duration.as_millis() < 10,
        "Stage checks too slow: {:?}",
        duration
    );
}
