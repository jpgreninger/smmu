//! TDD failing tests for BUG-AUDIT-36, BUG-AUDIT-37, BUG-AUDIT-38,
//! BUG-AUDIT-39, and BUG-AUDIT-40 (Rust).
//!
//! Each test is written to FAIL with the current code (red) and PASS only after
//! the corresponding fix is applied (green), EXCEPT where explicitly marked as a
//! regression/baseline test.
//!
//! # BUG-AUDIT-36 (§5.2 SteIllegal): S2HD not validated against HTTU
//!
//! ARM IHI0070G.b §5.2 line 8354:
//! `if STE.S2HD == '1' && SMMU_IDR0.HTTU == '01' then return TRUE`
//!
//! HTTU is fixed at 0b01 (access flag only; dirty state NOT supported).
//! When `stage2_enabled=true` AND `s2hd=true`, `configure_stream()` must emit
//! `C_BAD_STE` and return `Err`.
//!
//! BEFORE FIX: `configure_stream()` returns `Ok` silently → FAILS.
//! AFTER FIX:  `C_BAD_STE` emitted, `Err` returned → PASSES.
//!
//! # BUG-AUDIT-37 (§6.3.4): IDR3.HAD not gated on S1P
//!
//! ARM IHI0070G.b §6.3.4 line 11425:
//! "When SMMU_IDR0.S1P == 0, SMMU_IDR3.HAD == 0"
//!
//! `get_idr3()` must return bit 2 == 0 when S1P=0.
//!
//! BEFORE FIX: bit 2 is hardcoded `(1u32 << 2)` regardless of S1P → FAILS.
//! AFTER FIX:  bit 2 == 0 when S1P=0 → PASSES.
//!
//! # BUG-AUDIT-38 (§9.1.5): GATOS reserved faultcodes
//!
//! ARM IHI0070G.b §9.1.5: `EventType::FUut` (0x01), `FBadAtsTreq` (0x05),
//! and `FTranslForbidden` (0x07) are RESERVED in the GATOS faultcode table and
//! must map to 0xFD (INTERNAL_ERR).
//!
//! The mapping is tested indirectly by verifying that recognised non-reserved
//! GATOS faultcodes (C_BAD_STREAMID → 0x02, F_TRANSLATION → 0x10) still work.
//!
//! # BUG-AUDIT-39 (§6.3.37/§9.1): GATOS does not check SMMUEN
//!
//! ARM IHI0070G.b §6.3.37/§9.1: GATOS operations require SMMU_CR0.SMMUEN==1.
//! When SMMUEN==0, `gatos_translate()` must return a fault PAR with
//! `FAULT=1` and `FAULTCODE=0xFD` (INTERNAL_ERR).
//!
//! BEFORE FIX: GATOS proceeds even when SMMUEN=0 → FAILS.
//! AFTER FIX:  PAR bit 0 == 1, faultcode == 0xFD → PASSES.
//!
//! # BUG-AUDIT-40 (§5.5 CdIllegal): CD.HD not validated against HTTU
//!
//! ARM IHI0070G.b §5.5 line 9797:
//! `if CD.HD == '1' && SMMU_IDR0.HTTU == '01' then return TRUE`
//!
//! HTTU is fixed at 0b01 (access flag only). When `stage1_enabled=true` AND
//! `hd=true`, `configure_stream()` must emit `C_BAD_CD` and return `Err`.
//!
//! BEFORE FIX: `configure_stream()` returns `Ok` silently → FAILS.
//! AFTER FIX:  `C_BAD_CD` emitted, `Err` returned → PASSES.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{
    AccessType, EventType, FaultMode, PagePermissions, SecurityState, StreamConfig, StreamID, IOVA,
    PA, PASID,
};
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
// BUG-AUDIT-36: S2HD must be rejected when stage2_enabled (HTTU==0b01)
// ============================================================================

/// BUG-AUDIT-36 test 1: `stage2_enabled=true` + `s2hd=true` → `C_BAD_STE` event + `Err`.
///
/// ARM §5.2 line 8354: `STE.S2HD=='1' && HTTU=='01'` → `SteIllegal` → `C_BAD_STE`.
/// HTTU is fixed at `0b01` (access flag only).
///
/// BEFORE FIX: `configure_stream()` returns `Ok`, no event emitted → FAILS.
/// AFTER FIX:  `configure_stream()` returns `Err`, `C_BAD_STE` in event queue → PASSES.
#[test]
fn s2hd_raises_c_bad_ste_when_stage2_enabled() {
    let smmu = make_smmu();

    let mut cfg = StreamConfig::stage2_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s2hd = true;

    let result = smmu.configure_stream(sid(1), cfg);

    assert!(
        result.is_err(),
        "BUG-AUDIT-36: configure_stream() must return Err when stage2_enabled=true AND s2hd=true \
         (ARM §5.2 SteIllegal HTTU==0b01)"
    );
    assert!(
        has_event(&smmu, EventType::CBadSte),
        "BUG-AUDIT-36: C_BAD_STE event must be emitted when s2hd=true with stage2_enabled=true"
    );
}

/// BUG-AUDIT-36 test 2: `s2hd=true` but `stage2_enabled=false` (bypass) → accepted.
///
/// S2HD is only validated when stage-2 translation is active.  A bypass
/// stream with `s2hd=true` must be accepted silently.
///
/// BEFORE / AFTER FIX: This must always pass (regression guard).
#[test]
fn s2hd_accepted_when_stage2_disabled() {
    let smmu = make_smmu();

    let mut cfg = StreamConfig::bypass();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s2hd = true;

    let result = smmu.configure_stream(sid(2), cfg);
    assert!(
        result.is_ok(),
        "BUG-AUDIT-36 baseline: s2hd=true on a bypass (stage2_enabled=false) stream must be accepted"
    );
    assert!(
        !has_event(&smmu, EventType::CBadSte),
        "BUG-AUDIT-36 baseline: no C_BAD_STE expected for bypass stream with s2hd=true"
    );
}

/// BUG-AUDIT-36 test 3: `s2hd=false` + `stage2_enabled=true` → accepted.
///
/// S2HD=0 is always legal regardless of HTTU value.
///
/// BEFORE / AFTER FIX: This must always pass (regression guard).
#[test]
fn s2hd_false_stage2_enabled_accepted() {
    let smmu = make_smmu();

    let mut cfg = StreamConfig::stage2_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s2hd = false;

    let result = smmu.configure_stream(sid(3), cfg);
    assert!(
        result.is_ok(),
        "BUG-AUDIT-36 baseline: s2hd=false with stage2_enabled=true must be accepted"
    );
}

// ============================================================================
// BUG-AUDIT-37: IDR3.HAD must be RES0 when S1P==0
// ============================================================================

/// BUG-AUDIT-37 test 1: `set_s1p_supported(false)` → IDR3 bit 2 (HAD) == 0.
///
/// ARM §6.3.4 line 11425: "When SMMU_IDR0.S1P == 0, SMMU_IDR3.HAD == 0."
///
/// BEFORE FIX: bit 2 is hardcoded `(1u32 << 2)` → FAILS.
/// AFTER FIX:  bit 2 == 0 when S1P=0 → PASSES.
#[test]
fn idr3_had_res0_when_s1p_disabled() {
    let smmu = make_smmu();
    smmu.set_s1p_supported(false);

    let idr3 = smmu.get_idr3();
    assert_eq!(
        idr3 & (1u32 << 2),
        0,
        "BUG-AUDIT-37: IDR3.HAD (bit 2) must be RES0 when IDR0.S1P==0 (ARM §6.3.4 line 11425)"
    );
}

/// BUG-AUDIT-37 test 2: `set_s1p_supported(true)` → IDR3 bit 2 (HAD) != 0.
///
/// When S1P=1 the HAD bit should be set (default behavior after fix).
///
/// BEFORE / AFTER FIX: This must always pass when S1P=true (regression guard).
#[test]
fn idr3_had_set_when_s1p_enabled() {
    let smmu = make_smmu();
    smmu.set_s1p_supported(true);

    let idr3 = smmu.get_idr3();
    assert_ne!(
        idr3 & (1u32 << 2),
        0,
        "BUG-AUDIT-37: IDR3.HAD (bit 2) must be set when IDR0.S1P==1"
    );
}

/// BUG-AUDIT-37 test 3: fresh `SMMU::new()` defaults S1P=true → IDR3.HAD should be set.
///
/// The default `s1p_supported = AtomicBool::new(true)` means IDR3.HAD should be
/// non-zero on a freshly constructed SMMU.
///
/// BEFORE / AFTER FIX: This must pass (regression guard — S1P defaults to true).
#[test]
fn idr3_had_set_by_default() {
    // Fresh SMMU: s1p_supported defaults to true.
    let smmu = SMMU::new();

    let idr3 = smmu.get_idr3();
    assert_ne!(
        idr3 & (1u32 << 2),
        0,
        "BUG-AUDIT-37 baseline: IDR3.HAD must be set on fresh SMMU (S1P defaults to true)"
    );
}

// ============================================================================
// BUG-AUDIT-38: GATOS faultcodes — verify known-good codes still work
// ============================================================================

/// BUG-AUDIT-38 test 1 (updated by BUG-AUDIT-64): GATOS on a non-existent stream →
/// FAULT=1 + FAULTCODE == 0xFE (INV_STAGE).
///
/// BUG-AUDIT-64 fix: ARM §9.1.3 line 27699 — a GATOS request for a stream that is
/// absent from the stream table (no STE configured) must return INV_STAGE: FAULT=1,
/// FAULTCODE=0xFE. The previous behavior returned C_BAD_STE (0x04) by letting
/// translate() run and reporting the event; the pre-check now intercepts absent
/// streams before translate() is called.
#[test]
fn gatos_unknown_stream_returns_c_bad_ste_faultcode() {
    let smmu = make_smmu();

    // StreamID 99 does not exist — absent stream must return INV_STAGE per §9.1.3.
    let par = smmu.gatos_translate(
        sid(99),
        PASID::new(0).unwrap(),
        IOVA::new(0x1000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    // bit 0 must be 1 (FAULT)
    assert_eq!(par & 1, 1, "BUG-AUDIT-64: PAR.FAULT (bit 0) must be 1 for absent stream");

    // BUG-AUDIT-64 fix: FAULTCODE in bits[11:4] must be 0xFE (INV_STAGE) per ARM §9.1.3.
    let faultcode = (par >> 4) & 0xFF;
    assert_eq!(
        faultcode,
        0xFE,
        "BUG-AUDIT-64: PAR FAULTCODE must be 0xFE (INV_STAGE) for absent stream per ARM §9.1.3; got 0x{faultcode:02X}"
    );
}

/// BUG-AUDIT-38 test 2: GATOS on a fully configured stream with a valid mapping →
/// PAR.FAULT == 0 and physical address is returned correctly.
///
/// Verifies that non-reserved event faultcodes (and the success path) work
/// correctly after the reserved-faultcode fix is applied.
///
/// BEFORE / AFTER FIX: This must always pass (regression guard for success path).
#[test]
fn gatos_valid_mapping_returns_success() {
    let smmu = make_smmu();

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.fault_mode = FaultMode::Terminate;
    smmu.configure_stream(sid(5), cfg).unwrap();
    smmu.create_pasid(sid(5), PASID::new(0).unwrap()).unwrap();
    smmu.map_page(
        sid(5),
        PASID::new(0).unwrap(),
        IOVA::new(0x5000).unwrap(),
        PA::new(0x0005_0000u64).unwrap(),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let par = smmu.gatos_translate(
        sid(5),
        PASID::new(0).unwrap(),
        IOVA::new(0x5000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert_eq!(
        par & 1,
        0,
        "BUG-AUDIT-38: PAR.FAULT must be 0 for a valid mapping; got PAR=0x{par:016X}"
    );
}

// ============================================================================
// BUG-AUDIT-39: GATOS must check SMMUEN before proceeding
// ============================================================================

/// BUG-AUDIT-39 test 1: `gatos_translate()` when SMMUEN==0 → PAR FAULT=1, FAULTCODE=0xFD.
///
/// ARM §6.3.37/§9.1: GATOS requires SMMU_CR0.SMMUEN==1.  When SMMUEN=0 the
/// PAR must indicate a fault with FAULTCODE=0xFD (INTERNAL_ERR).
///
/// BEFORE FIX: GATOS proceeds as if enabled → different error or success → FAILS.
/// AFTER FIX:  PAR bit 0 == 1, faultcode == 0xFD → PASSES.
#[test]
fn gatos_returns_fault_when_smmuen_0() {
    // Deliberately create an SMMU WITHOUT SMMUEN.
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    // Verify SMMUEN is indeed 0.
    assert_eq!(
        smmu.get_cr0() & SMMU::CR0_SMMUEN,
        0,
        "Test setup: SMMUEN must be 0 for this test"
    );

    let par = smmu.gatos_translate(
        sid(1),
        PASID::new(0).unwrap(),
        IOVA::new(0x1000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert_eq!(
        par & 1,
        1,
        "BUG-AUDIT-39: PAR.FAULT (bit 0) must be 1 when SMMUEN==0"
    );

    let faultcode = (par >> 4) & 0xFF;
    assert_eq!(
        faultcode,
        0xFD,
        "BUG-AUDIT-39: PAR FAULTCODE must be 0xFD (INTERNAL_ERR) when SMMUEN==0; got 0x{faultcode:02X}"
    );
}

/// BUG-AUDIT-39 test 2: `gatos_translate()` with SMMUEN==1 and valid mapping → PAR FAULT=0.
///
/// Regression guard: GATOS succeeds normally when SMMUEN=1.
///
/// BEFORE / AFTER FIX: This must always pass (regression guard).
#[test]
fn gatos_succeeds_when_smmuen_1() {
    let smmu = make_smmu();

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.fault_mode = FaultMode::Terminate;
    smmu.configure_stream(sid(10), cfg).unwrap();

    smmu.create_pasid(sid(10), PASID::new(0).unwrap()).unwrap();
    smmu.map_page(
        sid(10),
        PASID::new(0).unwrap(),
        IOVA::new(0x8000).unwrap(),
        PA::new(0x0000_8000u64).unwrap(),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let par = smmu.gatos_translate(
        sid(10),
        PASID::new(0).unwrap(),
        IOVA::new(0x8000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert_eq!(
        par & 1,
        0,
        "BUG-AUDIT-39 baseline: PAR.FAULT must be 0 when SMMUEN=1 and mapping is valid; got PAR=0x{par:016X}"
    );
}

// ============================================================================
// BUG-AUDIT-40: CD.HD must be rejected when stage1_enabled (HTTU==0b01)
// ============================================================================

/// BUG-AUDIT-40 test 1: `stage1_enabled=true` + `hd=true` → `C_BAD_CD` event + `Err`.
///
/// ARM §5.5 line 9797: `CD.HD=='1' && HTTU=='01'` → `CdIllegal` → `C_BAD_CD`.
/// HTTU is fixed at `0b01` (access flag only; dirty state NOT supported).
///
/// BEFORE FIX: `configure_stream()` returns `Ok`, no event emitted → FAILS.
/// AFTER FIX:  `configure_stream()` returns `Err`, `C_BAD_CD` in event queue → PASSES.
#[test]
fn hd_raises_c_bad_cd_when_stage1_enabled() {
    let smmu = make_smmu();

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.fault_mode = FaultMode::Terminate;
    cfg.hd = true;

    let result = smmu.configure_stream(sid(20), cfg);

    assert!(
        result.is_err(),
        "BUG-AUDIT-40: configure_stream() must return Err when stage1_enabled=true AND hd=true \
         (ARM §5.5 CdIllegal HTTU==0b01)"
    );
    assert!(
        has_event(&smmu, EventType::CBadCd),
        "BUG-AUDIT-40: C_BAD_CD event must be emitted when hd=true with stage1_enabled=true"
    );
}

/// BUG-AUDIT-40 test 2: `hd=true` but `stage1_enabled=false` (stage2-only) → accepted.
///
/// CD.HD is only meaningful when stage-1 translation is active.  A stage-2-only
/// stream with `hd=true` must be accepted silently.
///
/// BEFORE / AFTER FIX: This must always pass (regression guard).
#[test]
fn hd_accepted_when_stage1_disabled() {
    let smmu = make_smmu();

    let mut cfg = StreamConfig::stage2_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.hd = true;

    let result = smmu.configure_stream(sid(21), cfg);
    assert!(
        result.is_ok(),
        "BUG-AUDIT-40 baseline: hd=true on a stage2-only (stage1_enabled=false) stream must be accepted"
    );
    assert!(
        !has_event(&smmu, EventType::CBadCd),
        "BUG-AUDIT-40 baseline: no C_BAD_CD expected for stage2-only stream with hd=true"
    );
}

/// BUG-AUDIT-40 test 3: `hd=false` + `stage1_enabled=true` → accepted.
///
/// CD.HD=0 is always legal regardless of HTTU value.
///
/// BEFORE / AFTER FIX: This must always pass (regression guard).
#[test]
fn hd_false_stage1_enabled_accepted() {
    let smmu = make_smmu();

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.fault_mode = FaultMode::Terminate;
    cfg.hd = false;

    let result = smmu.configure_stream(sid(22), cfg);
    assert!(
        result.is_ok(),
        "BUG-AUDIT-40 baseline: hd=false with stage1_enabled=true must always be accepted"
    );
}
