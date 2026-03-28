//! TDD failing tests for BUG-NEW-G, BUG-NEW-I, and BUG-NEW-J (Rust).
//!
//! BUG-NEW-H is C++ only (missing `setPriqConsOvackflg()`) and is not
//! repeated here — Rust already has `set_priq_cons_ovackflg()`.
//!
//! Each test is written to FAIL with the current code (red) and PASS only
//! after the corresponding fix is applied (green), EXCEPT where explicitly
//! marked as a documentation/regression test (which passes both before and
//! after).
//!
//! # BUG-NEW-G (Both §4.5.2): IDR0.PRI==0 → CERROR_ILL for CMD_PRI_RESP
//!
//! ARM IHI0070G.b §4.5.2: CMD_PRI_RESP is only valid when the PRI feature is
//! supported (IDR0.PRI==1).  When IDR0.PRI==0 the command is ILLEGAL and the
//! SMMU must raise CERROR_ILL and toggle GERROR.CMDQ_ERR.
//!
//! Requires new API: `set_pri_supported(bool)`.
//!
//! ## Current behavior (WRONG)
//!
//! No `set_pri_supported()` API exists.  `PriResp` handler does not check
//! IDR0.PRI; the command always executes silently with no CERROR_ILL.
//!
//! ## Required behavior
//!
//! `set_pri_supported(false)` + `PriResp` command → `CERROR_ILL` in
//! `get_cmdq_cons_err()` and `GERROR.CMDQ_ERR` active.
//!
//! BEFORE FIX: won't compile (no `set_pri_supported`) → test FAILS.
//! AFTER FIX:  `CERROR_ILL` raised, `GERROR.CMDQ_ERR` active → PASSES.
//!
//! # BUG-NEW-I (Both §7.3.13): eventClass for stage-2 IPA faults
//!
//! SPECIFICATION DOCUMENT — not a failing test.
//!
//! ARM IHI0070G.b §7.3.13 Table D8-3 CLASS field encoding:
//! - `0b00` (0) = CD-fetch class
//! - `0b01` (1) = TT-walk class
//! - `0b10` (2) = IN class (input address stage-2 fault)  ← SW model generates this
//! - `0b11` (3) = Reserved
//!
//! In the SW model only CLASS=2 (IN) is reachable; CLASS=0 and CLASS=1 require
//! a real HW TT walk that the model does not simulate.  The test verifies the
//! existing correct behavior (passes both before and after any fix).
//!
//! # BUG-NEW-J (Both §5.2): EATS validation missing in configure_stream()
//!
//! ARM IHI0070G.b §5.2 SteIllegal() pseudocode:
//! - (a) `EATS==0b10` AND `Config != two-stage (0b111)` → C_BAD_STE
//! - (b) `EATS==0b10` AND `S2S==1`                     → C_BAD_STE
//! - (c) `EATS==0b01` AND `S2S==1` AND `stage2_enabled` → C_BAD_STE
//!
//! ## Current behavior (WRONG)
//!
//! `configure_stream()` does not validate the `eats` field against Config/S2S.
//! All three cases succeed silently without `CBadSte`.
//!
//! BEFORE FIX: `configure_stream()` returns `Ok`, no `CBadSte` event → FAILS.
//! AFTER FIX:  `configure_stream()` returns `Err`, `CBadSte` emitted → PASSES.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventType, SecurityState, StreamConfig, StreamID, IOVA,
    PASID,
};
use smmu::{PA, SMMU};

// ============================================================================
// Helpers
// ============================================================================

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

/// Build an SMMU with all queues enabled.
fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    smmu
}

/// Returns true when GERROR.CMDQ_ERR is active (GERROR XOR GERRORN has bit 0 set).
fn is_gerror_cmdq_err_active(smmu: &SMMU) -> bool {
    (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR != 0
}

/// Returns true when the event queue contains at least one event of the given type.
fn has_event(smmu: &SMMU, event_type: EventType) -> bool {
    smmu.get_events().iter().any(|e| e.event_type == event_type)
}

/// Submit and process a CMD_PRI_RESP command for the given stream/PASID/PRG.
fn submit_pri_resp(smmu: &SMMU, stream_id: u32, pasid: u32, prg_index: u16) {
    let mut cmd = CommandEntry::new(CommandType::PriResp, stream_id, pasid);
    cmd.prg_index = prg_index;
    smmu.submit_command(cmd).ok();
    smmu.process_command_queue().ok();
}

// ============================================================================
// BUG-NEW-G: IDR0.PRI==0 gates CMD_PRI_RESP (ARM §4.5.2)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: set_pri_supported(false) + PriResp → CERROR_ILL + GERROR.CMDQ_ERR
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.5.2: CMD_PRI_RESP is only legal when IDR0.PRI==1.
// When IDR0.PRI==0, the command is ILLEGAL → CERROR_ILL in get_cmdq_cons_err()
// and GERROR.CMDQ_ERR must be toggled.
//
// This test FAILS before the fix because:
//   1. `set_pri_supported()` does not exist (compilation error), OR
//   2. After the API is added but before the guard is wired in, PriResp executes
//      without checking IDR0.PRI and `get_cmdq_cons_err()` returns 0.
//
// BEFORE FIX: won't compile (no `set_pri_supported`) → test FAILS.
// AFTER FIX:  CERROR_ILL raised, GERROR.CMDQ_ERR active → test PASSES.

/// BUG-NEW-G: `set_pri_supported(false)` + `PriResp` → `CERROR_ILL`.
///
/// ARM §4.5.2: `CMD_PRI_RESP` is illegal when `IDR0.PRI==0`.
///
/// BEFORE FIX: won't compile (no `set_pri_supported`) → FAILS.
/// AFTER FIX:  `CERROR_ILL` raised, `GERROR_CMDQ_ERR` active → PASSES.
#[test]
fn bug_new_g_pri0_pri_resp_raises_cerror_ill() {
    // §4.5.2: IDR0.PRI==0 makes CMD_PRI_RESP illegal → CERROR_ILL.
    let smmu = make_smmu();

    // Disable PRI feature (IDR0.PRI=0).
    smmu.set_pri_supported(false);

    // Submit a PriResp — must be rejected with CERROR_ILL when PRI==0.
    submit_pri_resp(&smmu, 1, 0, 0);

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-G: PriResp with IDR0.PRI==0 must set get_cmdq_cons_err()=CERROR_ILL \
         (ARM §4.5.2: PRI_RESP is only legal when PRI feature is supported). \
         Current code: no set_pri_supported() method and no PRI check in PriResp handler. \
         Got: {}",
        smmu.get_cmdq_cons_err()
    );

    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-G: GERROR.CMDQ_ERR must be active when PriResp is rejected with \
         CERROR_ILL (ARM §4.5.2 + §6.3.17). \
         Current code: GERROR.CMDQ_ERR not toggled because no PRI check exists."
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 (regression): set_pri_supported(true) + PriResp → no CERROR_ILL
// ─────────────────────────────────────────────────────────────────────────────
//
// When IDR0.PRI==1 (default), PriResp must be processed normally.
// This test verifies the guard is properly scoped to PRI==0 only.
//
// BEFORE FIX: (fails to compile because set_pri_supported() is missing).
// AFTER FIX:  PriResp accepted, CMDQ_CONS.ERR != CERROR_ILL → PASSES.

/// BUG-NEW-G regression: `set_pri_supported(true)` + `PriResp` → no error.
///
/// ARM §4.5.2: `CMD_PRI_RESP` is legal when `IDR0.PRI==1`.
///
/// BEFORE FIX: (fails to compile). AFTER FIX: must pass (regression guard).
#[test]
fn bug_new_g_pri1_pri_resp_no_error() {
    // Regression: IDR0.PRI==1 (default) → PriResp must be accepted.
    let smmu = make_smmu();

    // Explicitly set PRI supported (default is true, but be explicit).
    smmu.set_pri_supported(true);

    // Submit a PriResp — must NOT raise CERROR_ILL when PRI==1.
    submit_pri_resp(&smmu, 2, 0, 0);

    assert_ne!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-G regression: PriResp with IDR0.PRI==1 must NOT raise CERROR_ILL \
         (ARM §4.5.2: PRI_RESP is legal when PRI feature is supported). \
         The guard must fire only when PRI==0. \
         Got: {}",
        smmu.get_cmdq_cons_err()
    );

    assert!(
        !is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-G regression: GERROR.CMDQ_ERR must NOT be active when PriResp is \
         accepted normally (IDR0.PRI==1)."
    );
}

// ============================================================================
// BUG-NEW-I (§7.3.13): eventClass=2 (IN) for stage-2 IPA faults
//
// SPECIFICATION DOCUMENT — not a failing test.
// Verifies existing CORRECT behavior; passes both before and after any fix.
// ============================================================================

/// BUG-NEW-I specification document: stage-2 IPA fault sets `event_class=2` (IN).
///
/// ARM §7.3.13 Table D8-3: CLASS=0b10 (2) = IN — input address stage-2 fault.
/// This is the only CLASS reachable in the SW model (no HW TT walk).
///
/// Passes both before and after any code change — regression/documentation guard.
#[test]
fn bug_new_i_stage2_ipa_fault_event_class_is_2_in() {
    // SPECIFICATION DOCUMENT: stage-2-only stream with an IPA outside the
    // stage-2 address space must generate F_TRANSLATION with event_class=2 (IN).
    let smmu = make_smmu();

    let stream = sid(0x30);

    // Configure a stage-2-only stream.
    let cfg = StreamConfig::stage2_only();
    smmu.configure_stream(stream, cfg).unwrap();
    smmu.create_stage2_address_space(stream).unwrap();
    smmu.enable_stream(stream).unwrap();

    // Map one stage-2 page to initialise the address space (required so that
    // translating a *different* unmapped IPA returns PageNotMapped → FTranslation
    // rather than StreamNotConfigured, which would not generate a fault event).
    smmu.map_stage2_page(
        stream,
        IOVA::new(0x1000).unwrap(),
        PA::new(0x2000).unwrap(),
        smmu::types::PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    // Translate an IPA that has no stage-2 mapping → FTranslation fault.
    let unmapped_ipa = IOVA::new(0xDEAD_0000).unwrap();
    let result = smmu.translate(stream, PASID::new(0).unwrap(), unmapped_ipa, AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_err(),
        "BUG-NEW-I doc: stage-2-only stream with no mapping must fault (result was Ok)"
    );

    // Find the FTranslation event and verify event_class==2 (IN).
    let events = smmu.get_events();
    let found = events.iter().find(|ev| {
        ev.event_type == EventType::FTranslation && ev.stream_id == stream.as_u32()
    });

    assert!(
        found.is_some(),
        "BUG-NEW-I doc: FTranslation event for the stage-2 fault not found in queue."
    );

    let ev = found.unwrap();

    assert_eq!(
        ev.event_class,
        2u8,
        "BUG-NEW-I doc: stage-2 IPA fault must set event_class=2 (IN) per ARM §7.3.13 \
         Table D8-3.  CLASS=0 (CD) and CLASS=1 (TT) require HW TT walk — unreachable \
         in SW model.  Got event_class={}",
        ev.event_class
    );

    assert!(
        ev.s2,
        "BUG-NEW-I doc: stage-2 IPA fault must set s2=true (ARM §7.3.13). \
         Got s2={}",
        ev.s2
    );
}

// ============================================================================
// BUG-NEW-J: EATS field validation in configure_stream() (ARM §5.2 SteIllegal)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: eats=2 + stage1-only config (Config != two-stage) → CBadSte
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §5.2 SteIllegal(): EATS==0b10 (split-stage ATS) is only legal for
// two-stage streams (STE.Config=0b111).  Any other Config combined with
// eats=2 is illegal → CBadSte.
//
// BEFORE FIX: configure_stream() returns Ok, no CBadSte → FAILS.
// AFTER FIX:  configure_stream() returns Err, CBadSte emitted → PASSES.

/// BUG-NEW-J: `eats=2` + stage1-only config → `CBadSte`.
///
/// ARM §5.2 SteIllegal(): `EATS==0b10` requires `Config=two-stage (0b111)`.
///
/// BEFORE FIX: `configure_stream()` returns `Ok`, no `CBadSte` → FAILS.
/// AFTER FIX:  `configure_stream()` returns `Err`, `CBadSte` emitted → PASSES.
#[test]
fn bug_new_j_eats2_stage1_only_emits_c_bad_ste() {
    // §5.2 SteIllegal: EATS=0b10 with stage1-only (not two-stage) → CBadSte.
    let smmu = make_smmu();

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.eats = 2; // EATS=0b10: split-stage ATS — requires two-stage Config.

    let result = smmu.configure_stream(sid(0x40), cfg);

    assert!(
        result.is_err(),
        "BUG-NEW-J: configure_stream() with eats=2 AND stage1-only Config must return \
         Err (ARM §5.2 SteIllegal: EATS==0b10 requires Config=two-stage 0b111). \
         Current code: no EATS validation exists. Got: Ok"
    );

    assert!(
        has_event(&smmu, EventType::CBadSte),
        "BUG-NEW-J: CBadSte event must be recorded when eats=2 AND stage1-only \
         (ARM §5.2 SteIllegal: EATS=0b10 with Config != 0b111 is illegal). \
         Current code: no CBadSte emitted."
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: eats=2 + two-stage + s2_stall=true (S2S=1) → CBadSte
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §5.2 SteIllegal(): EATS==0b10 AND STE.S2S==1 → CBadSte,
// even when the Config IS two-stage.
//
// BEFORE FIX: configure_stream() returns Ok, no CBadSte → FAILS.
// AFTER FIX:  CBadSte emitted → PASSES.

/// BUG-NEW-J: `eats=2` + two-stage + `s2_stall=true` (S2S=1) → `CBadSte`.
///
/// ARM §5.2 SteIllegal(): `EATS==0b10` AND `STE.S2S==1` → `CBadSte`
/// (even when Config IS two-stage).
///
/// BEFORE FIX: `configure_stream()` returns `Ok`, no `CBadSte` → FAILS.
/// AFTER FIX:  `CBadSte` emitted → PASSES.
#[test]
fn bug_new_j_eats2_two_stage_s2s1_emits_c_bad_ste() {
    // §5.2 SteIllegal: EATS=0b10 AND S2S==1 → CBadSte (even for two-stage Config).
    let smmu = make_smmu();

    let mut cfg = StreamConfig::two_stage();
    cfg.security_state = SecurityState::NonSecure;
    cfg.eats = 2;      // EATS=0b10: split-stage ATS.
    cfg.s2_stall = true; // S2S=1 — incompatible with EATS=0b10.

    let result = smmu.configure_stream(sid(0x41), cfg);

    assert!(
        result.is_err(),
        "BUG-NEW-J: configure_stream() with eats=2 AND s2_stall=true must return \
         Err (ARM §5.2 SteIllegal: EATS==0b10 AND STE.S2S=1 is illegal regardless \
         of Config). Current code: no EATS+S2S validation. Got: Ok"
    );

    assert!(
        has_event(&smmu, EventType::CBadSte),
        "BUG-NEW-J: CBadSte event must be recorded when eats=2 AND s2_stall=true \
         (ARM §5.2 SteIllegal). Current code: no CBadSte emitted."
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3: eats=1 + s2_stall=true (S2S=1) + stage2_enabled → CBadSte
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §5.2 SteIllegal(): EATS==0b01 AND STE.S2S==1 AND stage2_enabled → CBadSte.
//
// BEFORE FIX: configure_stream() returns Ok, no CBadSte → FAILS.
// AFTER FIX:  CBadSte emitted → PASSES.

/// BUG-NEW-J: `eats=1` + `s2_stall=true` + `stage2_enabled` → `CBadSte`.
///
/// ARM §5.2 SteIllegal(): `EATS==0b01` AND `STE.S2S==1` AND stage2 enabled.
///
/// BEFORE FIX: `configure_stream()` returns `Ok`, no `CBadSte` → FAILS.
/// AFTER FIX:  `CBadSte` emitted → PASSES.
#[test]
fn bug_new_j_eats1_s2s1_stage2_enabled_emits_c_bad_ste() {
    // §5.2 SteIllegal: EATS=0b01 AND S2S==1 AND stage2_enabled → CBadSte.
    let smmu = make_smmu();

    let mut cfg = StreamConfig::two_stage();
    cfg.security_state = SecurityState::NonSecure;
    cfg.eats = 1;      // EATS=0b01: combined-stage ATS.
    cfg.s2_stall = true; // S2S=1 — incompatible with EATS=0b01 when stage2 enabled.

    let result = smmu.configure_stream(sid(0x42), cfg);

    assert!(
        result.is_err(),
        "BUG-NEW-J: configure_stream() with eats=1 AND s2_stall=true AND stage2_enabled \
         must return Err (ARM §5.2 SteIllegal: EATS==0b01 AND STE.S2S=1 AND stage2 \
         enabled is illegal). Current code: no EATS+S2S validation. Got: Ok"
    );

    assert!(
        has_event(&smmu, EventType::CBadSte),
        "BUG-NEW-J: CBadSte event must be recorded when eats=1 AND s2_stall=true AND \
         stage2_enabled (ARM §5.2 SteIllegal). Current code: no CBadSte emitted."
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 (regression): eats=2 + two-stage + s2_stall=false → accepted
// ─────────────────────────────────────────────────────────────────────────────
//
// eats=2 (split-stage ATS) is legal when Config=two-stage AND S2S=0.
// The fix must not over-reject this valid configuration.
//
// BEFORE FIX: (already passes — no EATS validation).
// AFTER FIX:  must still pass — regression guard.

/// BUG-NEW-J regression: `eats=2` + two-stage + `s2_stall=false` → accepted.
///
/// ARM §5.2 SteIllegal(): `EATS=0b10` is only illegal when Config != `0b111`
/// or `STE.S2S=1`.  Two-stage + S2S=0 is a valid combination.
///
/// BEFORE FIX: (passes already). AFTER FIX: must still pass (regression guard).
#[test]
fn bug_new_j_eats2_two_stage_s2s0_accepted() {
    // Regression: eats=2 + two-stage + s2_stall=false must be accepted.
    let smmu = make_smmu();

    let mut cfg = StreamConfig::two_stage();
    cfg.security_state = SecurityState::NonSecure;
    cfg.eats = 2;       // EATS=0b10: split-stage ATS — legal with two-stage.
    cfg.s2_stall = false; // S2S=0 — valid combination.

    let result = smmu.configure_stream(sid(0x43), cfg);

    assert!(
        result.is_ok(),
        "BUG-NEW-J regression: configure_stream() with eats=2 AND two-stage AND \
         s2_stall=false must be ACCEPTED (ARM §5.2 SteIllegal: EATS=0b10 is only \
         illegal when Config != 0b111 or S2S=1). The fix must not over-reject. \
         Got: Err"
    );

    assert!(
        !has_event(&smmu, EventType::CBadSte),
        "BUG-NEW-J regression: no CBadSte expected for eats=2 + two-stage + s2_stall=false."
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5 (regression): eats=1 + s2_stall=false → accepted
// ─────────────────────────────────────────────────────────────────────────────
//
// eats=1 (combined-stage ATS) is legal when S2S=0.
//
// BEFORE FIX: (already passes).
// AFTER FIX:  must still pass — regression guard.

/// BUG-NEW-J regression: `eats=1` + `s2_stall=false` → accepted.
///
/// ARM §5.2 SteIllegal(): `EATS=0b01` only conflicts when `S2S=1` AND stage2 enabled.
///
/// BEFORE FIX: (passes already). AFTER FIX: must still pass (regression guard).
#[test]
fn bug_new_j_eats1_s2s0_accepted() {
    // Regression: eats=1 + s2_stall=false must be accepted.
    let smmu = make_smmu();

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.eats = 1;       // EATS=0b01: combined-stage ATS — legal with S2S=0.
    cfg.s2_stall = false; // S2S=0 — valid combination.

    let result = smmu.configure_stream(sid(0x44), cfg);

    assert!(
        result.is_ok(),
        "BUG-NEW-J regression: configure_stream() with eats=1 AND s2_stall=false must \
         be ACCEPTED (ARM §5.2 SteIllegal: EATS=0b01 only conflicts with S2S=1 AND \
         stage2_enabled). The fix must not over-reject. Got: Err"
    );

    assert!(
        !has_event(&smmu, EventType::CBadSte),
        "BUG-NEW-J regression: no CBadSte expected for eats=1 + s2_stall=false."
    );
}
