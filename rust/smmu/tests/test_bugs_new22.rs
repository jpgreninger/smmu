//! TDD failing tests for BUG-AUDIT-41, BUG-AUDIT-42, and BUG-AUDIT-43 (Rust).
//!
//! Each test is written to FAIL with the current code (red) and PASS only after
//! the corresponding fix is applied (green), EXCEPT where explicitly marked as a
//! regression/baseline test.
//!
//! # BUG-AUDIT-41 (§6.3.1): IDR0.Hyp missing S1P gate
//!
//! ARM IHI0070G.b §6.3.1:
//! IDR0.Hyp (bit 9) must be RES0 when S1P==0 OR S2P==0.
//! Current code gates on `hyp_supported && s2p_supported` but NOT on `s1p_supported`.
//!
//! BEFORE FIX: hyp=true, s2p=true, s1p=false → bit 9 is 1 → FAILS.
//! AFTER FIX:  bit 9 == 0 when s1p=false → PASSES.
//!
//! # BUG-AUDIT-42 (§5.2 SteIllegal): VMSAv8-32 not validated against TTF
//!
//! ARM IHI0070G.b §5.2 SteIllegal():
//! `if STE.S2AA64=='0' && TTF[0]=='0' then return TRUE`
//! TTF is hardcoded 0b10 (AArch64-only), so TTF[0]==0 always.
//! When `stage2_enabled=true` AND `s2_aa64=false`, `configure_stream()` must emit
//! `C_BAD_STE` and return `Err`.
//!
//! BEFORE FIX: `configure_stream()` returns `Ok` silently → FAILS.
//! AFTER FIX:  `C_BAD_STE` emitted, `Err` returned → PASSES.
//!
//! # BUG-AUDIT-43 (§5.2 SteIllegal): S1P/S2P capability not checked against Config
//!
//! ARM IHI0070G.b §5.2 SteIllegal():
//! `if Config=='1x1' && S1P=='0' then return TRUE`  (stage1 enabled but S1P absent)
//! `if Config=='11x' && S2P=='0' then return TRUE`  (stage2 enabled but S2P absent)
//!
//! BEFORE FIX: capability-vs-config guards absent → silently accepted → FAILS.
//! AFTER FIX:  `C_BAD_STE` emitted for each missing-capability case → PASSES.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{EventType, FaultMode, SecurityState, StreamConfig, StreamID};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

/// Build an SMMU with all queues and global enable active.
fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    smmu
}

/// Returns true when the event queue contains at least one event of the given type.
fn has_event(smmu: &SMMU, event_type: EventType) -> bool {
    smmu.get_events().iter().any(|e| e.event_type == event_type)
}

// ============================================================================
// BUG-AUDIT-41: IDR0.Hyp must be RES0 when S1P==0 (missing gate)
// ============================================================================

/// BUG-AUDIT-41 test 1: hyp=true, s2p=true, s1p=false → IDR0 bit 9 (Hyp) == 0.
///
/// ARM §6.3.1: IDR0.Hyp is RES0 when S1P==0 OR S2P==0.
/// Current code gates on `hyp_supported && s2p_supported` but NOT `s1p_supported`.
///
/// BEFORE FIX: bit 9 is set (1) despite s1p=false → FAILS.
/// AFTER FIX:  bit 9 == 0 when s1p=false → PASSES.
#[test]
fn idr0_hyp_res0_when_s1p_disabled() {
    let smmu = make_smmu();
    smmu.set_hyp_supported(true);
    smmu.set_s2p_supported(true);
    smmu.set_s1p_supported(false);

    let idr0 = smmu.get_idr0();
    assert_eq!(
        idr0 & (1u32 << 9),
        0,
        "BUG-AUDIT-41: IDR0.Hyp (bit 9) must be RES0 when IDR0.S1P==0 (ARM §6.3.1); \
         got idr0=0x{idr0:08X}"
    );
}

/// BUG-AUDIT-41 test 2: hyp=true, s2p=true, s1p=true → IDR0 bit 9 (Hyp) != 0.
///
/// All three capability gates enabled — Hyp should be reported.
///
/// BEFORE / AFTER FIX: This must always pass (regression guard).
#[test]
fn idr0_hyp_set_when_all_capabilities_enabled() {
    let smmu = make_smmu();
    smmu.set_hyp_supported(true);
    smmu.set_s2p_supported(true);
    smmu.set_s1p_supported(true);

    let idr0 = smmu.get_idr0();
    assert_ne!(
        idr0 & (1u32 << 9),
        0,
        "BUG-AUDIT-41: IDR0.Hyp (bit 9) must be 1 when hyp=true, s1p=true, s2p=true (ARM §6.3.1)"
    );
}

/// BUG-AUDIT-41 test 3: hyp=false → IDR0 bit 9 == 0 regardless of S1P/S2P.
///
/// This is the existing `hyp_supported` gate (regression guard).
///
/// BEFORE / AFTER FIX: This must always pass.
#[test]
fn idr0_hyp_res0_when_hyp_disabled() {
    let smmu = make_smmu();
    smmu.set_hyp_supported(false);
    smmu.set_s1p_supported(true);
    smmu.set_s2p_supported(true);

    let idr0 = smmu.get_idr0();
    assert_eq!(
        idr0 & (1u32 << 9),
        0,
        "BUG-AUDIT-41: IDR0.Hyp must be 0 when hyp_supported=false (existing gate, regression guard)"
    );
}

/// BUG-AUDIT-41 test 4: s2p=false → IDR0 bit 9 == 0 regardless of hyp/s1p.
///
/// This is the existing `s2p_supported` gate (regression guard).
///
/// BEFORE / AFTER FIX: This must always pass.
#[test]
fn idr0_hyp_res0_when_s2p_disabled() {
    let smmu = make_smmu();
    smmu.set_hyp_supported(true);
    smmu.set_s1p_supported(true);
    smmu.set_s2p_supported(false);

    let idr0 = smmu.get_idr0();
    assert_eq!(
        idr0 & (1u32 << 9),
        0,
        "BUG-AUDIT-41: IDR0.Hyp must be 0 when s2p_supported=false (existing gate, regression guard)"
    );
}

// ============================================================================
// BUG-AUDIT-42: S2AA64==0 must be rejected (TTF[0]==0 always per hardcoded TTF=0b10)
// ============================================================================

/// BUG-AUDIT-42 test 1: `stage2_enabled=true` + `s2_aa64=false` → `C_BAD_STE` + `Err`.
///
/// ARM §5.2 SteIllegal(): `STE.S2AA64=='0' && TTF[0]=='0'` → `SteIllegal`.
/// TTF is hardcoded to 0b10 (AArch64-only), so TTF[0]==0 always.
/// Therefore s2_aa64=false with stage-2 enabled must always be rejected.
///
/// BEFORE FIX: `configure_stream()` returns `Ok`, no event → FAILS.
/// AFTER FIX:  `configure_stream()` returns `Err`, `C_BAD_STE` emitted → PASSES.
#[test]
fn s2_aa64_false_raises_c_bad_ste_when_stage2_enabled() {
    let smmu = make_smmu();
    smmu.set_s2p_supported(true);

    // Build a stage2-only config with s2_aa64=false.
    let mut cfg = StreamConfig::stage2_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s2_aa64 = false;

    let result = smmu.configure_stream(sid(50), cfg);
    assert!(
        result.is_err(),
        "BUG-AUDIT-42: configure_stream() must return Err when stage2_enabled=true AND s2_aa64=false \
         (ARM §5.2 SteIllegal: S2AA64==0 && TTF[0]==0)"
    );
    assert!(
        has_event(&smmu, EventType::CBadSte),
        "BUG-AUDIT-42: C_BAD_STE event must be emitted when s2_aa64=false with stage2_enabled=true"
    );
}

/// BUG-AUDIT-42 test 2: `stage2_enabled=true` + `s2_aa64=true` → accepted.
///
/// AArch64 stage-2 tables are always supported (TTF=0b10 includes AArch64 S1+S2).
///
/// BEFORE / AFTER FIX: This must always pass (regression guard).
#[test]
fn s2_aa64_true_is_accepted_when_stage2_enabled() {
    let smmu = make_smmu();
    smmu.set_s2p_supported(true);

    let mut cfg = StreamConfig::stage2_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s2_aa64 = true;

    let result = smmu.configure_stream(sid(51), cfg);
    assert!(
        result.is_ok(),
        "BUG-AUDIT-42: stage2_enabled=true + s2_aa64=true must be accepted \
         (AArch64 stage-2 is always supported)"
    );
}

/// BUG-AUDIT-42 test 3: `stage2_enabled=false` + `s2_aa64=false` → accepted.
///
/// S2AA64 is irrelevant when stage-2 is disabled; no C_BAD_STE should fire.
///
/// BEFORE / AFTER FIX: This must always pass (regression guard).
#[test]
fn s2_aa64_false_is_accepted_when_stage2_disabled() {
    let smmu = make_smmu();
    smmu.set_s1p_supported(true);

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.fault_mode = FaultMode::Terminate;
    cfg.s2_aa64 = false;  // irrelevant when stage-2 disabled

    let result = smmu.configure_stream(sid(52), cfg);
    assert!(
        result.is_ok(),
        "BUG-AUDIT-42: s2_aa64=false with stage2_enabled=false must be accepted \
         (S2AA64 is irrelevant when stage-2 is disabled)"
    );
}

// ============================================================================
// BUG-AUDIT-43: S1P/S2P capability must be checked against STE Config
// ============================================================================

/// BUG-AUDIT-43 test 1: s1p_supported=false + stage1_enabled=true → `C_BAD_STE` + `Err`.
///
/// ARM §5.2 SteIllegal(): `Config=='1x1' && S1P=='0'` → `SteIllegal`.
/// (Config='1x1' encodes stage1 enabled.)
///
/// BEFORE FIX: `configure_stream()` returns `Ok`, no event → FAILS.
/// AFTER FIX:  `configure_stream()` returns `Err`, `C_BAD_STE` emitted → PASSES.
#[test]
fn s1p_absent_stage1_enabled_raises_c_bad_ste() {
    let smmu = make_smmu();
    smmu.set_s1p_supported(false);

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.fault_mode = FaultMode::Terminate;

    let result = smmu.configure_stream(sid(60), cfg);
    assert!(
        result.is_err(),
        "BUG-AUDIT-43: stage1_enabled=true + S1P==0 must be rejected \
         (ARM §5.2 SteIllegal Config=='1x1' && S1P=='0')"
    );
    assert!(
        has_event(&smmu, EventType::CBadSte),
        "BUG-AUDIT-43: C_BAD_STE must be emitted when S1P==0 and stage1_enabled=true"
    );
}

/// BUG-AUDIT-43 test 2: s2p_supported=false + stage2_enabled=true → `C_BAD_STE` + `Err`.
///
/// ARM §5.2 SteIllegal(): `Config=='11x' && S2P=='0'` → `SteIllegal`.
/// (Config='11x' encodes stage2 enabled.)
///
/// BEFORE FIX: `configure_stream()` returns `Ok`, no event → FAILS.
/// AFTER FIX:  `configure_stream()` returns `Err`, `C_BAD_STE` emitted → PASSES.
#[test]
fn s2p_absent_stage2_enabled_raises_c_bad_ste() {
    let smmu = make_smmu();
    smmu.set_s2p_supported(false);

    let mut cfg = StreamConfig::stage2_only();
    cfg.security_state = SecurityState::NonSecure;

    let result = smmu.configure_stream(sid(61), cfg);
    assert!(
        result.is_err(),
        "BUG-AUDIT-43: stage2_enabled=true + S2P==0 must be rejected \
         (ARM §5.2 SteIllegal Config=='11x' && S2P=='0')"
    );
    assert!(
        has_event(&smmu, EventType::CBadSte),
        "BUG-AUDIT-43: C_BAD_STE must be emitted when S2P==0 and stage2_enabled=true"
    );
}

/// BUG-AUDIT-43 test 3: s1p_supported=false + stage1_enabled=false → accepted (no false positive).
///
/// When stage-1 is not requested, the S1P capability is irrelevant.
///
/// BEFORE / AFTER FIX: This must always pass (regression guard).
#[test]
fn s1p_absent_stage1_disabled_is_accepted() {
    let smmu = make_smmu();
    smmu.set_s1p_supported(false);
    smmu.set_s2p_supported(true);

    let mut cfg = StreamConfig::stage2_only();
    cfg.security_state = SecurityState::NonSecure;

    let result = smmu.configure_stream(sid(62), cfg);
    assert!(
        result.is_ok(),
        "BUG-AUDIT-43: S1P==0 with stage1_enabled=false must be accepted \
         (S1P capability irrelevant when stage-1 not configured)"
    );
}

/// BUG-AUDIT-43 test 4: s2p_supported=false + stage2_enabled=false → accepted (no false positive).
///
/// When stage-2 is not requested, the S2P capability is irrelevant.
///
/// BEFORE / AFTER FIX: This must always pass (regression guard).
#[test]
fn s2p_absent_stage2_disabled_is_accepted() {
    let smmu = make_smmu();
    smmu.set_s2p_supported(false);
    smmu.set_s1p_supported(true);

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.fault_mode = FaultMode::Terminate;

    let result = smmu.configure_stream(sid(63), cfg);
    assert!(
        result.is_ok(),
        "BUG-AUDIT-43: S2P==0 with stage2_enabled=false must be accepted \
         (S2P capability irrelevant when stage-2 not configured)"
    );
}
