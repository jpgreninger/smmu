#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

//! TDD conformance tests for ARM SMMU v3 §6.3.14 SMMU_GBPA field normalization and
//! ALLOCCFG semantics.
//!
//! Bugs covered:
//! - BUG-GBPA-01: INSTCFG reserved encoding 0b01 must be normalized to 0b00 on write.
//! - BUG-GBPA-02: PRIVCFG reserved encoding 0b01 must be normalized to 0b00 on write.
//! - BUG-GBPA-03: ALLOCCFG 0b0xxx (bit 3 == 0) means "use incoming" → alloc_hint == 0.
//! - BUG-GBPA-04: Field width masking: alloc_cfg masked to 4 bits, others to 2 bits.
//!
//! All tests were written BEFORE the corresponding fix was applied (red → green TDD).

use smmu::types::{AccessType, SecurityState, StreamConfig, StreamID};
use smmu::{GbpaConfig, SMMU};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

fn make_bypass_smmu() -> SMMU {
    // SMMU disabled (SMMUEN=0) + GBPA.ABORT=false → bypass path
    let smmu = SMMU::new();
    // Configure stream so translate() can resolve a stream context.
    smmu.configure_stream(sid(1), StreamConfig::bypass()).unwrap();
    smmu
}

fn bypass_translate(smmu: &SMMU) -> smmu::types::TranslationData {
    smmu.translate(
        sid(1),
        smmu::types::PASID::new(0).unwrap(),
        smmu::types::IOVA::new(0x1000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    )
    .expect("bypass translate must succeed when GBPA.ABORT=false and SMMU disabled")
}

// ============================================================================
// BUG-GBPA-01: INSTCFG reserved encoding 0b01 normalized to 0b00
// ============================================================================

/// §6.3.14 INSTCFG: encoding 0b01 is reserved and behaves as 0b00.
/// Writing inst_cfg=1 must be normalized to 0 on storage.
#[test]
fn test_gbpa_instcfg_reserved_01_normalized_to_00() {
    let smmu = SMMU::new();
    smmu.set_gbpa_config(GbpaConfig {
        inst_cfg: 1, // reserved encoding
        ..GbpaConfig::default()
    });
    let got = smmu.get_gbpa_config();
    assert_eq!(
        got.inst_cfg, 0,
        "INSTCFG=0b01 is reserved and must be normalized to 0b00 per §6.3.14"
    );
}

// ============================================================================
// BUG-GBPA-02: PRIVCFG reserved encoding 0b01 normalized to 0b00
// ============================================================================

/// §6.3.14 PRIVCFG: encoding 0b01 is reserved and behaves as 0b00.
/// Writing priv_cfg=1 must be normalized to 0 on storage.
#[test]
fn test_gbpa_privcfg_reserved_01_normalized_to_00() {
    let smmu = SMMU::new();
    smmu.set_gbpa_config(GbpaConfig {
        priv_cfg: 1, // reserved encoding
        ..GbpaConfig::default()
    });
    let got = smmu.get_gbpa_config();
    assert_eq!(
        got.priv_cfg, 0,
        "PRIVCFG=0b01 is reserved and must be normalized to 0b00 per §6.3.14"
    );
}

// ============================================================================
// BUG-GBPA-01/02: Valid (non-reserved) values must be preserved
// ============================================================================

/// §6.3.14 INSTCFG valid encodings: 0b00, 0b10, 0b11 are not reserved and
/// must be stored verbatim.
#[test]
fn test_gbpa_instcfg_valid_values_preserved() {
    for valid in [0u8, 2, 3] {
        let smmu = SMMU::new();
        smmu.set_gbpa_config(GbpaConfig {
            inst_cfg: valid,
            ..GbpaConfig::default()
        });
        let got = smmu.get_gbpa_config();
        assert_eq!(
            got.inst_cfg, valid,
            "INSTCFG={valid} is a valid encoding and must be preserved verbatim per §6.3.14"
        );
    }
}

/// §6.3.14 PRIVCFG valid encodings: 0b00, 0b10, 0b11 are not reserved and
/// must be stored verbatim.
#[test]
fn test_gbpa_privcfg_valid_values_preserved() {
    for valid in [0u8, 2, 3] {
        let smmu = SMMU::new();
        smmu.set_gbpa_config(GbpaConfig {
            priv_cfg: valid,
            ..GbpaConfig::default()
        });
        let got = smmu.get_gbpa_config();
        assert_eq!(
            got.priv_cfg, valid,
            "PRIVCFG={valid} is a valid encoding and must be preserved verbatim per §6.3.14"
        );
    }
}

// ============================================================================
// BUG-GBPA-03: ALLOCCFG 0b0xxx → alloc_hint == 0 in bypass output
// ============================================================================

/// §6.3.14 ALLOCCFG: when bit 3 == 0 (0b0xxx) the field means "use incoming
/// allocation hints". The software model represents "use incoming" as 0 in
/// `alloc_hint` of the `TranslationData` result.
#[test]
fn test_gbpa_alloccfg_0xxx_outputs_zero_alloc_hint() {
    let smmu = make_bypass_smmu();
    smmu.set_gbpa_config(GbpaConfig {
        alloc_cfg: 0b0101, // bit 3 == 0 → "use incoming"
        abort: false,
        ..GbpaConfig::default()
    });
    let data = bypass_translate(&smmu);
    assert_eq!(
        data.alloc_hint(),
        0,
        "ALLOCCFG=0b0101 (bit3=0) means 'use incoming'; alloc_hint in result must be 0 per §6.3.14"
    );
}

/// §6.3.14 ALLOCCFG: when bit 3 == 1 (0b1xxx) the field overrides allocation
/// hints. The `alloc_hint` in the result must equal the configured alloc_cfg value.
#[test]
fn test_gbpa_alloccfg_1xxx_outputs_override() {
    let smmu = make_bypass_smmu();
    smmu.set_gbpa_config(GbpaConfig {
        alloc_cfg: 0b1010, // bit 3 == 1 → override
        abort: false,
        ..GbpaConfig::default()
    });
    let data = bypass_translate(&smmu);
    assert_eq!(
        data.alloc_hint(),
        0b1010,
        "ALLOCCFG=0b1010 (bit3=1) overrides allocation hints; alloc_hint must equal 0b1010 per §6.3.14"
    );
}

// ============================================================================
// BUG-GBPA-04: Field width masking
// ============================================================================

/// §6.3.14 field widths: inst_cfg is 2 bits → values above 3 must be masked.
/// Writing 0xFF (oversized) must read back as 0x03 (lower 2 bits).
/// Note: 0xFF & 0x03 == 0x03, and 0x03 == 0b11 which is a valid (non-reserved)
/// encoding, so no additional normalization applies.
#[test]
fn test_gbpa_field_width_masking() {
    let smmu = SMMU::new();
    smmu.set_gbpa_config(GbpaConfig {
        inst_cfg: 0xFF,  // oversized — must be masked to 2 bits → 0x03
        priv_cfg: 0xFF,  // oversized — must be masked to 2 bits → 0x03
        sh_cfg: 0xFF,    // oversized — must be masked to 2 bits → 0x03
        mem_attr: 0xFF,  // oversized — must be masked to 4 bits → 0x0F
        alloc_cfg: 0xFF, // oversized — must be masked to 4 bits → 0x0F
        ..GbpaConfig::default()
    });
    let got = smmu.get_gbpa_config();
    assert_eq!(got.inst_cfg, 0x03, "inst_cfg must be masked to 2 bits");
    assert_eq!(got.priv_cfg, 0x03, "priv_cfg must be masked to 2 bits");
    assert_eq!(got.sh_cfg, 0x03, "sh_cfg must be masked to 2 bits");
    assert_eq!(got.mem_attr, 0x0F, "mem_attr must be masked to 4 bits");
    assert_eq!(got.alloc_cfg, 0x0F, "alloc_cfg must be masked to 4 bits");
}
