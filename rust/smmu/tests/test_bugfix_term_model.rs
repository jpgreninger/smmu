//! Test for Bug 2: IDR0.TERM_MODEL (bit 26) must be 0.
//!
//! ARM IHI0070G.b §6.3.1: TERM_MODEL=1 claims "RAZ/WI termination NOT supported;
//! CD.A must be 1". When TERM_MODEL=1, C_BAD_CD must fire if CD.A=0.
//!
//! The model does NOT validate CD.A=1, so claiming TERM_MODEL=1 is a spec violation.
//! The correct fix: set TERM_MODEL=0 (RAZ/WI IS supported — matching actual behavior).
//!
//! BEFORE FIX: get_idr0() returns bit 26 set → test FAILS.
//! AFTER FIX:  get_idr0() returns bit 26 clear → test PASSES.
#![allow(missing_docs)]
#![allow(clippy::doc_markdown)]

use smmu::SMMU;

/// Bug 2: IDR0.TERM_MODEL (bit 26) must be 0.
///
/// When TERM_MODEL=0, RAZ/WI termination IS supported, matching the model's
/// actual behavior (it does not validate CD.A=1 before permitting termination).
///
/// BEFORE FIX: bit 26 is set → assertion fails.
/// AFTER FIX:  bit 26 is clear → assertion passes.
#[test]
fn bug2_idr0_term_model_bit26_must_be_zero() {
    let smmu = SMMU::new();
    let idr0 = smmu.get_idr0();
    assert_eq!(
        idr0 & (1u32 << 26),
        0,
        "Bug 2: IDR0.TERM_MODEL (bit 26) must be 0 — the model supports RAZ/WI \
         termination and does NOT validate CD.A=1; claiming TERM_MODEL=1 is a spec \
         violation (ARM IHI0070G.b §6.3.1). Got IDR0=0x{idr0:08X}"
    );
}

/// Regression: IDR0.S1P (bit 1) and IDR0.S2P (bit 0) remain set after fix.
///
/// Clearing TERM_MODEL must not disturb other IDR0 bits.
#[test]
fn bug2_idr0_other_bits_unchanged_after_term_model_fix() {
    let smmu = SMMU::new();
    let idr0 = smmu.get_idr0();

    // S2P (bit 0) and S1P (bit 1): both stages supported — must stay set.
    assert_ne!(idr0 & (1u32 << 0), 0, "IDR0.S2P (bit 0) must remain set");
    assert_ne!(idr0 & (1u32 << 1), 0, "IDR0.S1P (bit 1) must remain set");

    // ST_LEVEL[0] (bit 27): 2-level stream table supported — must stay set.
    assert_ne!(idr0 & (1u32 << 27), 0, "IDR0.ST_LEVEL[0] (bit 27) must remain set");

    // ATOS (bit 15): GATOS implemented — must stay set.
    assert_ne!(idr0 & (1u32 << 15), 0, "IDR0.ATOS (bit 15) must remain set");
}
