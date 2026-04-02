//! TDD failing tests for BUG-AUDIT-63 and BUG-AUDIT-64.
//!
//! **BUG-AUDIT-63** (Rust §6.3.24/§6.3.25): STRTAB_BASE setters not guarded by SMMUEN.
//!   Spec §6.3.24/§6.3.25 line 13807: SMMU_STRTAB_BASE and SMMU_STRTAB_BASE_CFG are
//!   guarded by CR0.SMMUEN — writes while SMMUEN==1 must be silently IGNORED.
//!   `set_strtab_format()`, `set_strtab_split()`, and `set_strtab_log2size()` perform
//!   unconditional stores with no SMMUEN check.
//!   BEFORE FIX: writes succeed while SMMUEN=1 → values change → tests FAIL.
//!   AFTER FIX:  writes are silently ignored while SMMUEN=1 → values unchanged → tests PASS.
//!
//! **BUG-AUDIT-64** (Rust §9.1.3/§9.1.5): gatos_translate() missing INV_STAGE for
//!   bypass/disabled streams.
//!   Spec §9.1.3 line 27699: A GATOS request for a stream with bypass (Config=0b100) or
//!   disabled/abort (Config=0b0xx) must result in INV_STAGE — FAULT=1, FAULTCODE=0xFE.
//!   BEFORE FIX: bypass streams return success (PA == IOVA), disabled streams return wrong
//!   fault code → tests FAIL.
//!   AFTER FIX:  FAULT=1 + FAULTCODE=0xFE (INV_STAGE) returned for bypass and abort → tests PASS.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{AccessType, SecurityState, StreamConfig, StreamID, IOVA, PA, PASID};
use smmu::{StreamTableFormat, SMMU};

// ============================================================================
// Helpers
// ============================================================================

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

fn pasid0() -> PASID {
    PASID::new(0).unwrap()
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

/// Build a fresh SMMU with SMMUEN=1, CMDQEN=1, EVENTQEN=1.
fn make_enabled_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_s1p_supported(true);
    smmu.set_s2p_supported(true);
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    smmu
}

// ============================================================================
// BUG-AUDIT-63: STRTAB_BASE/STRTAB_BASE_CFG setters must be ignored when SMMUEN=1
// ============================================================================

/// BUG-AUDIT-63 test 1: set_strtab_format() must be ignored when SMMUEN=1.
///
/// ARM §6.3.25 line 13807: STRTAB_BASE_CFG is RO when SMMUEN=1.
/// Set format to Linear before enable. Enable SMMU. Attempt to set TwoLevel → must remain Linear.
///
/// BEFORE FIX: get_strtab_format() returns TwoLevel (write applied) → FAILS.
/// AFTER FIX:  get_strtab_format() returns Linear (write ignored) → PASSES.
#[test]
fn bug_audit_63_set_strtab_format_ignored_when_enabled() {
    let smmu = SMMU::new();

    // Set Linear format before enable.
    smmu.set_strtab_format(StreamTableFormat::Linear);
    assert_eq!(
        smmu.get_strtab_format(),
        StreamTableFormat::Linear,
        "pre-condition: format must be Linear before enable"
    );

    // Enable the SMMU.
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    // Attempt to change format while SMMUEN=1 — must be silently ignored.
    smmu.set_strtab_format(StreamTableFormat::TwoLevel);

    let fmt = smmu.get_strtab_format();
    assert_eq!(
        fmt,
        StreamTableFormat::Linear,
        "BUG-AUDIT-63: set_strtab_format(TwoLevel) must be silently ignored when \
         SMMUEN=1. ARM §6.3.25 line 13807: STRTAB_BASE_CFG is RO when SMMUEN=1. \
         Current code applies the write unconditionally. \
         got={:?}",
        fmt
    );
}

/// BUG-AUDIT-63 test 2: set_strtab_split() must be ignored when SMMUEN=1.
///
/// ARM §6.3.25 line 13807: STRTAB_BASE_CFG.SPLIT is RO when SMMUEN=1.
/// Set split=8 before enable. Enable SMMU. Try to change to 10 → must remain 8.
///
/// BEFORE FIX: get_strtab_split() returns 10 (write applied) → FAILS.
/// AFTER FIX:  get_strtab_split() returns 8 (write ignored) → PASSES.
#[test]
fn bug_audit_63_set_strtab_split_ignored_when_enabled() {
    let smmu = SMMU::new();

    // Set split=8 before enable.
    smmu.set_strtab_split(8);
    assert_eq!(
        smmu.get_strtab_split(),
        8,
        "pre-condition: split must be 8 before enable"
    );

    // Enable the SMMU.
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    // Attempt to change split while SMMUEN=1 — must be silently ignored.
    smmu.set_strtab_split(10);

    let split = smmu.get_strtab_split();
    assert_eq!(
        split, 8,
        "BUG-AUDIT-63: set_strtab_split(10) must be silently ignored when \
         SMMUEN=1. ARM §6.3.25 line 13807: STRTAB_BASE_CFG is RO when SMMUEN=1. \
         Current code applies the write unconditionally. \
         got={}",
        split
    );
}

/// BUG-AUDIT-63 test 3: set_strtab_log2size() must be ignored when SMMUEN=1.
///
/// ARM §6.3.24/§6.3.25 line 13807: STRTAB_BASE (LOG2SIZE) is RO when SMMUEN=1.
/// Set log2size=16 before enable. Enable SMMU. Try to change to 20 → must remain 16.
///
/// BEFORE FIX: get_strtab_log2size() returns 20 (write applied) → FAILS.
/// AFTER FIX:  get_strtab_log2size() returns 16 (write ignored) → PASSES.
#[test]
fn bug_audit_63_set_strtab_log2size_ignored_when_enabled() {
    let smmu = SMMU::new();

    // Set log2size=16 before enable.
    smmu.set_strtab_log2size(16);
    assert_eq!(
        smmu.get_strtab_log2size(),
        16,
        "pre-condition: log2size must be 16 before enable"
    );

    // Enable the SMMU.
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    // Attempt to change log2size while SMMUEN=1 — must be silently ignored.
    smmu.set_strtab_log2size(20);

    let log2size = smmu.get_strtab_log2size();
    assert_eq!(
        log2size, 16,
        "BUG-AUDIT-63: set_strtab_log2size(20) must be silently ignored when \
         SMMUEN=1. ARM §6.3.24/§6.3.25 line 13807: STRTAB_BASE is RO when SMMUEN=1. \
         Current code applies the write unconditionally. \
         got={}",
        log2size
    );
}

// ============================================================================
// BUG-AUDIT-64: gatos_translate() must return INV_STAGE for bypass/disabled streams
// ============================================================================

/// BUG-AUDIT-64 test 1: gatos_translate() for a bypass stream must return INV_STAGE.
///
/// ARM §9.1.3 line 27699: Config=0b100 (bypass) → INV_STAGE: FAULT=1, FAULTCODE=0xFE.
///
/// BEFORE FIX: bypass translate() returns Ok (identity PA), gatos_translate returns
///             FAULT=0 (no fault) → FAILS.
/// AFTER FIX:  gatos_translate returns FAULT=1 + FAULTCODE=0xFE → PASSES.
#[test]
fn bug_audit_64_gatos_bypass_stream_returns_inv_stage() {
    let smmu = make_enabled_smmu();

    // Configure stream as bypass (STE.Config=0b100).
    smmu.configure_stream(sid(700), StreamConfig::bypass()).unwrap();

    let par = smmu.gatos_translate(
        sid(700),
        pasid0(),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let fault = par & 0x1;
    let faultcode = (par >> 4) & 0xFF;

    assert_eq!(
        fault, 1,
        "BUG-AUDIT-64: gatos_translate() for bypass stream must set FAULT=1. \
         ARM §9.1.3 line 27699: Config=0b100 (bypass) → INV_STAGE. \
         Current code returns success (identity PA) for bypass streams. \
         par=0x{:016X}",
        par
    );

    assert_eq!(
        faultcode, 0xFE,
        "BUG-AUDIT-64: gatos_translate() for bypass stream must return FAULTCODE=0xFE \
         (INV_STAGE). ARM §9.1.3 line 27699. \
         Current code returns FAULTCODE=0x{:02X}. par=0x{:016X}",
        faultcode, par
    );
}

/// BUG-AUDIT-64 test 2: gatos_translate() for an abort/disabled stream must return INV_STAGE.
///
/// ARM §9.1.3 line 27699: Config=0b0xx (disabled/abort) → INV_STAGE: FAULT=1, FAULTCODE=0xFE.
///
/// BEFORE FIX: abort translate() returns Err with a different fault code → FAILS.
/// AFTER FIX:  gatos_translate returns FAULT=1 + FAULTCODE=0xFE → PASSES.
#[test]
fn bug_audit_64_gatos_abort_stream_returns_inv_stage() {
    let smmu = make_enabled_smmu();

    // Configure stream as abort mode (STE.Config=0b000).
    let abort_cfg = StreamConfig::builder()
        .translation_enabled(false)
        .build()
        .unwrap();
    smmu.configure_stream(sid(701), abort_cfg).unwrap();

    let par = smmu.gatos_translate(
        sid(701),
        pasid0(),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let fault = par & 0x1;
    let faultcode = (par >> 4) & 0xFF;

    assert_eq!(
        fault, 1,
        "BUG-AUDIT-64: gatos_translate() for abort/disabled stream must set FAULT=1. \
         ARM §9.1.3 line 27699: Config=0b0xx (abort) → INV_STAGE. \
         par=0x{:016X}",
        par
    );

    assert_eq!(
        faultcode, 0xFE,
        "BUG-AUDIT-64: gatos_translate() for abort/disabled stream must return \
         FAULTCODE=0xFE (INV_STAGE). ARM §9.1.3 line 27699. \
         Current code returns FAULTCODE=0x{:02X}. par=0x{:016X}",
        faultcode, par
    );
}

/// BUG-AUDIT-64/82 test 3: gatos_translate() for an unconfigured StreamID returns C_BAD_STE.
///
/// BUG-AUDIT-82 updated the absent stream behavior: an unconfigured stream has STE.V=0,
/// which causes C_BAD_STE (0x04) rather than INV_STAGE (0xFE). INV_STAGE is returned only
/// for bypass/disabled streams that have a valid STE but no translation stage enabled.
///
/// AFTER BUG-AUDIT-82 FIX: gatos_translate returns FAULT=1 + FAULTCODE=0x04 (C_BAD_STE) → PASSES.
#[test]
fn bug_audit_64_gatos_absent_stream_returns_inv_stage() {
    let smmu = make_enabled_smmu();
    // No streams configured.

    let par = smmu.gatos_translate(
        sid(702),
        pasid0(),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let fault = par & 0x1;
    let faultcode = (par >> 4) & 0xFF;

    assert_eq!(
        fault, 1,
        "BUG-AUDIT-64/82: gatos_translate() for absent/unconfigured stream must set FAULT=1. \
         par=0x{:016X}",
        par
    );

    assert_ne!(
        faultcode, 0xFE,
        "BUG-AUDIT-64/82: gatos_translate() for absent stream must NOT return INV_STAGE (0xFE); \
         unconfigured stream has STE.V=0 → C_BAD_STE. par=0x{:016X}",
        par
    );
    assert_eq!(
        faultcode, 0x04,
        "BUG-AUDIT-64/82: gatos_translate() for absent/unconfigured stream must return \
         FAULTCODE=0x04 (C_BAD_STE, STE.V=0). ARM §9.1.3. \
         Current code returns FAULTCODE=0x{:02X}. par=0x{:016X}",
        faultcode, par
    );
}

/// BUG-AUDIT-64 test 4 (regression): gatos_translate() for a stage-1 stream must NOT
/// return INV_STAGE — it should return a valid PA or a different fault code.
///
/// BEFORE / AFTER FIX: stage1 stream with a mapped page → FAULT=0, valid PA → PASSES.
/// This verifies the pre-check does not incorrectly block real translations.
#[test]
fn bug_audit_64_gatos_stage1_stream_proceeds() {
    let smmu = make_enabled_smmu();

    // Configure a stage-1-only stream.
    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(sid(703), cfg).unwrap();

    // Initialize PASID 0 address space before mapping a page.
    smmu.create_pasid(sid(703), pasid0()).unwrap();

    // Map a page so the translation succeeds.
    smmu.map_page(
        sid(703),
        pasid0(),
        iova(0x1000),
        pa(0x2000),
        smmu::types::PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let par = smmu.gatos_translate(
        sid(703),
        pasid0(),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let fault = par & 0x1;
    let faultcode = (par >> 4) & 0xFF;

    assert_eq!(
        fault, 0,
        "BUG-AUDIT-64 regression: gatos_translate() for a stage1 stream with a valid \
         mapping must NOT return FAULT=1. The pre-check must only apply to bypass/disabled \
         streams (both stage1_enabled=false and stage2_enabled=false). \
         par=0x{:016X}",
        par
    );

    assert_ne!(
        faultcode, 0xFE,
        "BUG-AUDIT-64 regression: gatos_translate() for a stage1 stream must NOT return \
         FAULTCODE=0xFE (INV_STAGE). The pre-check must not apply to translation-enabled \
         streams. par=0x{:016X}",
        par
    );
}
