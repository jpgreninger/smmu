//! TDD failing tests for BUG-AUDIT-S2-XNX and BUG-AUDIT-S2-OAS.
//!
//! Each test is designed to FAIL before the corresponding fix is applied,
//! documenting the exact observable bug behaviour.
//!
//! # BUG-AUDIT-S2-XNX — IDR3.XNX=1 advertised without S2UXN enforcement
//!
//! ARM IHI0070G.b §2.3: IDR3.XNX (bit 4) advertises FEAT_XNX (stage-2
//! unprivileged execute-never). Advertising XNX without actually enforcing
//! S2UXN in the PagePermissions model is incorrect — the SMMU must not
//! claim to support a feature it does not implement.
//!
//! Fix direction: Set IDR3.XNX=0 (bit 4 clear) unconditionally, regardless
//! of S2P, since S2UXN is not enforced by the PagePermissions model.
//!
//! BEFORE FIX: `get_idr3()` sets bit 4 when S2P=1 → FAILS.
//! AFTER FIX:  `get_idr3()` always returns bit 4 == 0 → PASSES.
//!
//! # BUG-AUDIT-S2-OAS — IDR5.OAS=5 (48-bit) inconsistent with 52-bit PA limit
//!
//! ARM IHI0070G.b §2.3 / §6.3.6: IDR5.OAS must reflect the actual maximum
//! output address size implemented. The default configuration uses
//! `DEFAULT_PA_BITS=52` (from `src/types/config.rs`), so `get_idr5()` must
//! encode OAS=6 (52-bit) in bits[2:0].
//!
//! OAS encoding (ARM §6.3.6):
//!   0 = 32-bit, 1 = 36-bit, 2 = 40-bit, 3 = 42-bit,
//!   4 = 44-bit, 5 = 48-bit, 6 = 52-bit
//!
//! Fix direction: derive OAS from `max_pa_bits` at runtime instead of
//! hardcoding 5.
//!
//! BEFORE FIX: `get_idr5()` returns OAS=5 (48-bit) even though max_pa_bits=52 → FAILS.
//! AFTER FIX:  `get_idr5()` returns OAS=6 (52-bit) for default configuration → PASSES.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::SMMU;

// ============================================================================
// BUG-AUDIT-S2-XNX — IDR3.XNX must be 0 because S2UXN is not implemented
// ============================================================================

/// BUG-AUDIT-S2-XNX: `get_idr3()` must return bit 4 (XNX) == 0 even when S2P
/// is enabled, because the stage-2 unprivileged execute-never (S2UXN) feature
/// is not enforced by the PagePermissions model.
///
/// ARM §2.3: advertising FEAT_XNX via IDR3.XNX=1 without enforcing S2UXN is
/// incorrect. The correct behaviour is to leave IDR3.XNX=0 (RES0) until
/// S2UXN is actually implemented.
///
/// BEFORE FIX: `get_idr3()` sets bit 4 when `s2p_supported=true` (default) → FAILS.
/// AFTER FIX:  `get_idr3()` always returns bit 4 == 0 → PASSES.
#[test]
fn bug_audit_s2_xnx_idr3_xnx_bit_must_be_zero_when_s2uxn_not_implemented() {
    // Default SMMU::new() has s2p_supported=true, so the current code sets bit 4.
    let smmu = SMMU::new();

    let idr3 = smmu.get_idr3();
    let xnx_bit = (idr3 >> 4) & 1;

    assert_eq!(
        xnx_bit,
        0,
        "BUG-AUDIT-S2-XNX: IDR3.XNX (bit 4) must be 0 because S2UXN is not \
         enforced in the PagePermissions model — advertising FEAT_XNX without \
         implementing it violates ARM §2.3. \
         Current get_idr3() sets bit 4 whenever S2P=1: idr3=0x{:08x}",
        idr3,
    );
}

/// BUG-AUDIT-S2-XNX / BUG-IDR3-FWB / BUG-IDR3-STT regression baseline: IDR3
/// capability bits must reflect the correct implemented/not-implemented state
/// after all IDR3 fixes.
///
/// - FWB (bit 8): must be 0 — STE.S2FWB and combine_attrs_fwb() are not
///   implemented (BUG-IDR3-FWB fix).
/// - STT (bit 9): must be 1 — S2T0SZ is accepted up to 48, the STT=1 limit
///   (BUG-IDR3-STT fix).
/// - RIL (bit 10): must remain 1 — range-based invalidation is implemented.
/// - BBML[0] (bit 11): must remain 1 — BBML level 1 is implemented.
#[test]
fn bug_audit_s2_xnx_idr3_other_bits_unaffected() {
    let smmu = SMMU::new();
    let idr3 = smmu.get_idr3();

    // FWB (bit 8) must be 0 — FEAT_S2FWB is not implemented (BUG-IDR3-FWB fix).
    assert_eq!(
        idr3 & (1 << 8),
        0,
        "IDR3.FWB (bit 8) must be 0 — STE.S2FWB / combine_attrs_fwb() not implemented \
         (BUG-IDR3-FWB): idr3=0x{:08x}",
        idr3,
    );
    // STT (bit 9) must be 1 — S2T0SZ accepted up to 48 (BUG-IDR3-STT fix).
    assert_ne!(
        idr3 & (1 << 9),
        0,
        "IDR3.STT (bit 9) must be 1 — S2T0SZ accepted up to 48 (BUG-IDR3-STT): \
         idr3=0x{:08x}",
        idr3,
    );
    // RIL (bit 10) must remain set.
    assert_ne!(
        idr3 & (1 << 10),
        0,
        "IDR3.RIL (bit 10) must remain set after IDR3 fixes: idr3=0x{:08x}",
        idr3,
    );
    // BBML[0] (bit 11) must remain set.
    assert_ne!(
        idr3 & (1 << 11),
        0,
        "IDR3.BBML[0] (bit 11) must remain set after IDR3 fixes: idr3=0x{:08x}",
        idr3,
    );
}

// ============================================================================
// BUG-AUDIT-S2-OAS — IDR5.OAS must match max_pa_bits (52 → OAS=6)
// ============================================================================

/// BUG-AUDIT-S2-OAS: `get_idr5()` must return OAS=6 (52-bit) in bits[2:0]
/// when the SMMU is created with default settings, because `DEFAULT_PA_BITS=52`
/// in `src/types/config.rs`.
///
/// ARM §6.3.6 OAS encoding:
///   0=32-bit, 1=36-bit, 2=40-bit, 3=42-bit, 4=44-bit, 5=48-bit, 6=52-bit
///
/// The current code hardcodes `5u32` (48-bit) in `get_idr5()`, which is
/// inconsistent with the 52-bit default PA configuration.
///
/// BEFORE FIX: `get_idr5() & 0b111` == 5 → FAILS (expected 6).
/// AFTER FIX:  `get_idr5() & 0b111` == 6 → PASSES.
#[test]
fn bug_audit_s2_oas_idr5_oas_reflects_52bit_default_pa_config() {
    // SMMU::new() uses the default SMMUConfig, which sets max_pa_bits=52
    // (AddressConfig::DEFAULT_PA_BITS). IDR5.OAS (bits[2:0]) must encode 6.
    let smmu = SMMU::new();

    let idr5 = smmu.get_idr5();
    let oas = idr5 & 0b111;

    assert_eq!(
        oas,
        6,
        "BUG-AUDIT-S2-OAS: IDR5.OAS (bits[2:0]) must be 6 (52-bit) when \
         max_pa_bits=52 (DEFAULT_PA_BITS). \
         Current get_idr5() hardcodes OAS=5 (48-bit) regardless of max_pa_bits. \
         ARM §6.3.6: OAS encoding 6=52-bit. Got OAS={} (idr5=0x{:08x})",
        oas,
        idr5,
    );
}

// ============================================================================
// BUG-IDR3-FWB — IDR3.FWB must be 0 because S2FWB / combine_attrs_fwb() is
//                not implemented in this software model
// ============================================================================

/// BUG-IDR3-FWB: `get_idr3()` must return bit 8 (FWB) == 0.
///
/// ARM IHI0070G.b §2.3 / §6.3.4: IDR3.FWB (bit 8) advertises support for
/// FEAT_S2FWB — stage-2 force write-back attribute override via STE.S2FWB.
/// The field `STE.S2FWB` does not exist in this model's `StreamContext`, and
/// there is no `combine_attrs_fwb()` implementation. Advertising FWB=1 without
/// the corresponding enforcement is spec-non-compliant.
///
/// Fix direction: clear IDR3.FWB (bit 8) — honest advertisement that FWB is
/// not implemented in this software model.
///
/// BEFORE FIX: `get_idr3()` unconditionally sets `(1u32 << 8)` → FAILS.
/// AFTER FIX:  `get_idr3()` returns bit 8 == 0 → PASSES.
#[test]
fn bug_idr3_fwb_bit_must_be_zero_when_s2fwb_not_implemented() {
    // Default SMMU::new() — get_idr3() currently ORs in (1u32 << 8) always.
    let smmu = SMMU::new();

    let idr3 = smmu.get_idr3();
    let fwb_bit = (idr3 >> 8) & 1;

    assert_eq!(
        fwb_bit,
        0,
        "BUG-IDR3-FWB: IDR3.FWB (bit 8) must be 0 because STE.S2FWB and \
         combine_attrs_fwb() are not implemented in this software model — \
         advertising FEAT_S2FWB without implementing it violates ARM §2.3 / §6.3.4. \
         Current get_idr3() always sets bit 8: idr3=0x{:08x}",
        idr3,
    );
}

// ============================================================================
// BUG-IDR3-STT — IDR3.STT must be 1 because S2T0SZ is accepted up to 48
//                (the STT=1 maximum), so IDR3 must advertise that truthfully
// ============================================================================

/// BUG-IDR3-STT: `get_idr3()` must return bit 9 (STT) == 1.
///
/// ARM IHI0070G.b §6.3.4: IDR3.STT (bit 9) indicates whether the SMMU
/// supports S2T0SZ values beyond 39 (i.e. up to 48). When STT=0 the maximum
/// valid S2T0SZ is 39; when STT=1 the maximum is 48.
///
/// The implementation in `src/types/config.rs` and `check_ste_illegal()` already
/// accepts S2T0SZ up to 48 — the STT=1 limit — yet `get_idr3()` does not set
/// bit 9, creating an inconsistency between the advertised capability and the
/// enforced limit. The correct fix is to set IDR3.STT=1.
///
/// Fix direction: OR `(1u32 << 9)` into the value returned by `get_idr3()`.
///
/// BEFORE FIX: `get_idr3()` does not set bit 9 → FAILS.
/// AFTER FIX:  `get_idr3()` returns bit 9 == 1 → PASSES.
#[test]
fn bug_idr3_stt_bit_must_be_one_when_s2t0sz_accepts_up_to_48() {
    // Default SMMU::new() — get_idr3() currently does not set bit 9.
    let smmu = SMMU::new();

    let idr3 = smmu.get_idr3();
    let stt_bit = (idr3 >> 9) & 1;

    assert_ne!(
        stt_bit,
        0,
        "BUG-IDR3-STT: IDR3.STT (bit 9) must be 1 because the implementation \
         accepts S2T0SZ values up to 48 (the STT=1 maximum). ARM §6.3.4 requires \
         IDR3.STT to truthfully advertise this limit. \
         Current get_idr3() leaves bit 9 clear: idr3=0x{:08x}",
        idr3,
    );
}

// ============================================================================
// BUG-AUDIT-S2-OAS — IDR5.OAS must match max_pa_bits (52 → OAS=6)
// ============================================================================

/// BUG-AUDIT-S2-OAS regression: other IDR5 bits (GRAN4K, STALL_MAX) must be
/// unaffected by deriving OAS at runtime.
///
/// This test is expected to PASS before and after the fix.
#[test]
fn bug_audit_s2_oas_idr5_other_bits_unaffected() {
    let smmu = SMMU::new();
    let idr5 = smmu.get_idr5();

    // GRAN4K (bit 4) must be set — only 4KB granule is implemented.
    assert_ne!(
        idr5 & (1 << 4),
        0,
        "IDR5.GRAN4K (bit 4) must remain set after OAS fix: idr5=0x{:08x}",
        idr5,
    );

    // GRAN16K (bit 5) and GRAN64K (bit 6) must remain clear (BUG-AUDIT-52 fix).
    assert_eq!(
        idr5 & (1 << 5),
        0,
        "IDR5.GRAN16K (bit 5) must remain 0 — not implemented: idr5=0x{:08x}",
        idr5,
    );
    assert_eq!(
        idr5 & (1 << 6),
        0,
        "IDR5.GRAN64K (bit 6) must remain 0 — not implemented: idr5=0x{:08x}",
        idr5,
    );

    // STALL_MAX (bits[31:16]) must be 64 for the default (non-terminate-only) model.
    let stall_max = idr5 >> 16;
    assert_eq!(
        stall_max,
        64,
        "IDR5.STALL_MAX (bits[31:16]) must be 64 for default stall model after OAS fix: \
         idr5=0x{:08x}",
        idr5,
    );
}
