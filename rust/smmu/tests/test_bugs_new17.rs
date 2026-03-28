//! TDD tests for NEW-AUDIT-04 and NEW-AUDIT-05 (Rust).
//!
//! Tests are written against the correct ARM specification behaviour.  The three
//! tests that were previously written with incorrect expectations (testing
//! per-stream stage-1 state rather than the global IDR0.S1P flag) have been
//! corrected per §4.3.3 / §4.3.4 line 6605 clarification:
//!
//!   CERROR_ILL fires when **SMMU_IDR0.S1P == 0** (stage-1 not implemented
//!   globally), NOT when a specific stream happens to not use stage-1.
//!
//! # NEW-AUDIT-04 (Both §4.3.3 / §4.3.4): CMD_CFGI_CD / CMD_CFGI_CD_ALL
//! global IDR0.S1P guard (BUG-AUDIT-NEW-02 — see test_bugs_new19.rs)
//!
//! ARM IHI0070G.b §4.3.3 line 5362:
//! ```text
//!   "This command raises CERROR_ILL when stage 1 is not implemented."
//! ```
//! ARM IHI0070G.b §4.3.4 line 5388:
//! ```text
//!   "This command raises CERROR_ILL when stage 1 is not implemented."
//! ```
//!
//! The condition "stage 1 is not implemented" refers to the SMMU-global
//! `IDR0.S1P` bit, not to whether a particular stream uses stage-1.
//! When `IDR0.S1P == 1` (default), `CfgiCd` / `CfgiCdAll` targeted at a
//! bypass or stage-2-only stream must execute as a silent no-op — there is
//! nothing to invalidate, but no error is raised.
//!
//! ## Corrections applied to three tests (BUG-AUDIT-NEW-02 pre-fix)
//!
//! The original tests in this file tested **wrong** behaviour (per-stream
//! stage-1 check):
//!
//! - `new_audit_04_bypass_stream_cfgi_cd_raises_cerror_ill` →
//!   renamed `new_audit_04_bypass_stream_cfgi_cd_no_error_when_s1p_enabled`,
//!   assertion flipped to `!is_gerror_cmdq_err_active`.
//! - `new_audit_04_stage2_only_stream_cfgi_cd_raises_cerror_ill` →
//!   renamed `new_audit_04_stage2_only_stream_cfgi_cd_no_error_when_s1p_enabled`,
//!   assertion flipped.
//! - `new_audit_04_bypass_stream_cfgi_cd_all_raises_cerror_ill` →
//!   renamed `new_audit_04_bypass_stream_cfgi_cd_all_no_error_when_s1p_enabled`,
//!   assertion flipped.
//!
//! These corrected tests now:
//! - **FAIL** with the *current* code (current code incorrectly raises GERROR
//!   because it checks per-stream stage-1 state rather than IDR0.S1P).
//! - **PASS** after the BUG-AUDIT-NEW-02 fix changes the guard to check
//!   `IDR0.S1P == 0` instead of per-stream `is_stage1_enabled()`.
//!
//! The BUG-AUDIT-NEW-02 tests that verify the positive case (S1P disabled →
//! CERROR_ILL) live in `test_bugs_new19.rs`.
//!
//! # NEW-AUDIT-05 (Both §7.3.12 line 27078): `inject_walk_eabt()` missing
//! `is_stage2` / `event_class` parameters
//!
//! ARM IHI0070G.b §7.3.12: F_WALK_EABT can arise in three contexts:
//! 1. S1 walk (`is_stage2=false`, `event_class=1`).
//! 2. S2 walk of a TT descriptor (`is_stage2=true`, `event_class=1`).
//! 3. S2 walk of an IPA input (`is_stage2=true`, `event_class=2`).
//!
//! Current API: `inject_walk_eabt(stream_id, pasid, iova)` always emits
//! `event_class=1` and `s2=false` — the two-stage cases cannot be expressed.
//!
//! Desired new API:
//! ```text
//!   inject_walk_eabt(stream_id, pasid, iova, is_stage2: bool, event_class: u8)
//! ```
//! Existing default behaviour preserved: `is_stage2=false`, `event_class=1`.
//!
//! BEFORE FIX: new five-parameter signature does not compile (method missing
//!             with `is_stage2` and `event_class` params) → tests 2 and 3 FAIL.
//! AFTER FIX:  `s2` and `event_class` fields populated → tests 2 and 3 PASS.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]
#![allow(dead_code)]

use smmu::types::{
    CommandEntry, CommandType, EventType, FaultMode, SecurityState, StreamConfig, StreamID, IOVA,
    PASID,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

fn pasid(n: u32) -> PASID {
    PASID::new(n).unwrap()
}

fn iova(n: u64) -> IOVA {
    IOVA::new(n).unwrap()
}

/// Build an SMMU with all queues and global enable active.
fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    smmu
}

/// Returns true when `GERROR.CMDQ_ERR` (bit 0) is active.
fn is_gerror_cmdq_err_active(smmu: &SMMU) -> bool {
    smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR != 0
}

/// Returns true when the event queue contains at least one event of the given type.
fn has_event(smmu: &SMMU, event_type: EventType) -> bool {
    smmu.get_events().iter().any(|e| e.event_type == event_type)
}

/// Build a bypass-only `StreamConfig` (STE.Config = 0b100: bypass, no translation).
///
/// `stage1_enabled=false`, `stage2_enabled=false`, bypass mode.
fn bypass_config() -> StreamConfig {
    StreamConfig::bypass()
}

/// Build a stage-2-only `StreamConfig` (STE.Config = 0b110: S2 only).
///
/// `stage1_enabled=false`, `stage2_enabled=true`.
fn stage2_only_config() -> StreamConfig {
    let mut cfg = StreamConfig::stage2_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.fault_mode = FaultMode::Terminate;
    cfg
}

/// Build a stage-1-only `StreamConfig`.
///
/// `stage1_enabled=true`, `stage2_enabled=false`.
fn stage1_only_config() -> StreamConfig {
    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.fault_mode = FaultMode::Terminate;
    cfg
}

/// Submit `CfgiCd` for `(stream_id, pasid)` and process the command queue.
fn submit_cfgi_cd(smmu: &SMMU, stream_id: u32, pasid_val: u32) {
    let cmd = CommandEntry::new(CommandType::CfgiCd, stream_id, pasid_val);
    let _ = smmu.submit_command(cmd);
    let _ = smmu.process_command_queue();
}

/// Submit `CfgiCdAll` for `stream_id` and process the command queue.
fn submit_cfgi_cd_all(smmu: &SMMU, stream_id: u32) {
    let cmd = CommandEntry::new(CommandType::CfgiCdAll, stream_id, 0);
    let _ = smmu.submit_command(cmd);
    let _ = smmu.process_command_queue();
}

// ============================================================================
// BUG-AUDIT-NEW-02: CMD_CFGI_CD IDR0.S1P global guard (ARM §4.3.3 line 6605)
// ============================================================================
// The three tests below were originally written with wrong expectations
// (per-stream stage-1 check).  They have been corrected: the spec condition
// is IDR0.S1P==0, not per-stream stage-1 absence.

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 (CORRECTED): bypass-only stream + CfgiCd when IDR0.S1P==1 → NO error
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.3.3 / §4.3.4 line 6605 clarification: the CERROR_ILL condition is
// SMMU-global `IDR0.S1P == 0`, NOT whether a specific stream uses stage-1.
//
// When IDR0.S1P==1 (default), CfgiCd targeted at a bypass stream is a silent
// no-op — there is nothing to invalidate, but no error must be raised.
//
// BEFORE FIX (BUG-AUDIT-NEW-02): current code checks per-stream
//   `is_stage1_enabled()` → raises GERROR for bypass stream → FAILS.
// AFTER FIX:  guard checks `IDR0.S1P == 0`; bypass stream + S1P=1 → no GERROR
//   → PASSES.

/// BUG-AUDIT-NEW-02: `CfgiCd` on a bypass-only stream when `IDR0.S1P==1`
/// (default) must NOT raise `GERROR_CMDQ_ERR`.
///
/// ARM §4.3.3 line 6605: CERROR_ILL fires only when the SMMU-global IDR0.S1P
/// bit is 0, not when a specific stream lacks stage-1 configuration.
///
/// BEFORE FIX: current code raises GERROR (per-stream check is wrong) → FAILS.
/// AFTER FIX:  bypass stream + S1P=1 → silent no-op, no GERROR → PASSES.
#[test]
fn new_audit_04_bypass_stream_cfgi_cd_no_error_when_s1p_enabled() {
    // §4.3.3: when IDR0.S1P==1 (default), CfgiCd on a bypass-only stream is
    // a silent no-op — no CERROR_ILL, no GERROR.CMDQ_ERR.
    let smmu = make_smmu();
    // IDR0.S1P defaults to 1 in SMMU::new() — no set_s1p_supported() call needed.

    let stream_id: u32 = 0x40;

    // Configure a bypass-only stream.
    smmu.configure_stream(sid(stream_id), bypass_config())
        .expect("configure_stream (bypass) should succeed");
    smmu.enable_stream(sid(stream_id))
        .expect("enable_stream should succeed");

    // Clear any events from setup.
    smmu.clear_event_queue();

    // Submit CfgiCd — must be a silent no-op when S1P==1, regardless of the
    // stream's own stage-1 configuration.
    submit_cfgi_cd(&smmu, stream_id, 0);

    assert!(
        !is_gerror_cmdq_err_active(&smmu),
        "BUG-AUDIT-NEW-02: CfgiCd on a bypass-only stream when IDR0.S1P==1 \
         must NOT raise GERROR.CMDQ_ERR. \
         ARM §4.3.3 line 6605: CERROR_ILL fires only when IDR0.S1P==0 \
         (stage-1 not implemented globally). \
         Current code checks per-stream stage-1 state → incorrectly raises GERROR. \
         GERROR=0x{:08X}",
        smmu.get_gerror()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 (CORRECTED): stage-2-only stream + CfgiCd when IDR0.S1P==1 → NO error
// ─────────────────────────────────────────────────────────────────────────────
//
// Same rationale as Test 1.  A stage-2-only stream has no CDs, but IDR0.S1P==1
// means the SMMU globally supports stage-1.  CfgiCd must be a silent no-op.
//
// BEFORE FIX (BUG-AUDIT-NEW-02): current code checks per-stream
//   `is_stage1_enabled()` → raises GERROR for stage-2-only stream → FAILS.
// AFTER FIX:  guard checks `IDR0.S1P == 0`; S1P=1 → no GERROR → PASSES.

/// BUG-AUDIT-NEW-02: `CfgiCd` on a stage-2-only stream when `IDR0.S1P==1`
/// must NOT raise `GERROR_CMDQ_ERR`.
///
/// ARM §4.3.3 line 6605: CERROR_ILL condition is `IDR0.S1P == 0`, not per-stream.
///
/// BEFORE FIX: current code raises GERROR (per-stream check) → FAILS.
/// AFTER FIX:  S1P=1 → silent no-op → PASSES.
#[test]
fn new_audit_04_stage2_only_stream_cfgi_cd_no_error_when_s1p_enabled() {
    // §4.3.3: when IDR0.S1P==1 (default), CfgiCd on a stage-2-only stream is
    // a silent no-op.  Stage-2-only has no CDs, but that is not a CERROR_ILL condition
    // unless IDR0.S1P==0.
    let smmu = make_smmu();
    // IDR0.S1P defaults to 1 — no set_s1p_supported() call needed.

    let stream_id: u32 = 0x41;

    smmu.configure_stream(sid(stream_id), stage2_only_config())
        .expect("configure_stream (stage2-only) should succeed");
    smmu.enable_stream(sid(stream_id))
        .expect("enable_stream should succeed");

    smmu.clear_event_queue();

    submit_cfgi_cd(&smmu, stream_id, 0);

    assert!(
        !is_gerror_cmdq_err_active(&smmu),
        "BUG-AUDIT-NEW-02: CfgiCd on a stage-2-only stream when IDR0.S1P==1 \
         must NOT raise GERROR.CMDQ_ERR. \
         ARM §4.3.3 line 6605: CERROR_ILL fires only when IDR0.S1P==0 globally. \
         A stage-2-only stream has no CDs to invalidate, but that is not an error \
         when stage-1 is globally implemented (IDR0.S1P==1). \
         Current code checks per-stream stage-1 → incorrectly raises GERROR. \
         GERROR=0x{:08X}",
        smmu.get_gerror()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 (baseline): stage-1-enabled stream + CfgiCd → no GERROR_CMDQ_ERR
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.3.3: When stage 1 IS implemented, `CfgiCd` must execute normally.
//
// BEFORE FIX: (already passes — no guard means stage-1 streams work fine).
// AFTER FIX:  must still pass (regression guard).

/// NEW-AUDIT-04 baseline: `CfgiCd` on a stage-1-enabled stream → no error.
///
/// BEFORE FIX: passes already. AFTER FIX: must still pass (regression guard).
#[test]
fn new_audit_04_stage1_stream_cfgi_cd_no_error() {
    // Regression/baseline: CfgiCd on a stage-1-enabled stream → no CERROR_ILL.
    let smmu = make_smmu();

    let stream_id: u32 = 0x42;

    smmu.configure_stream(sid(stream_id), stage1_only_config())
        .expect("configure_stream (stage1-only) should succeed");
    smmu.enable_stream(sid(stream_id))
        .expect("enable_stream should succeed");

    smmu.clear_event_queue();

    submit_cfgi_cd(&smmu, stream_id, 0);

    assert!(
        !is_gerror_cmdq_err_active(&smmu),
        "NEW-AUDIT-04 baseline: CfgiCd on a stage-1-enabled stream must NOT \
         raise CERROR_ILL (ARM §4.3.3: the guard fires only when stage-1 is absent). \
         The fix must not over-reject stage-1 streams. \
         GERROR=0x{:08X}",
        smmu.get_gerror()
    );
}

// ============================================================================
// BUG-AUDIT-NEW-02: CMD_CFGI_CD_ALL IDR0.S1P global guard (ARM §4.3.4 / §4.3.3)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 (CORRECTED): bypass-only stream + CfgiCdAll when IDR0.S1P==1 → NO error
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.3.4 / §4.3.3 line 6605 clarification: same rule applies to CfgiCdAll.
// CERROR_ILL condition is `IDR0.S1P == 0`, not per-stream stage-1 absence.
//
// BEFORE FIX (BUG-AUDIT-NEW-02): current code checks per-stream
//   `is_stage1_enabled()` → raises GERROR for bypass stream → FAILS.
// AFTER FIX:  guard checks `IDR0.S1P == 0`; S1P=1 → no GERROR → PASSES.

/// BUG-AUDIT-NEW-02: `CfgiCdAll` on a bypass-only stream when `IDR0.S1P==1`
/// must NOT raise `GERROR_CMDQ_ERR`.
///
/// ARM §4.3.4 line 5388 / §4.3.3 line 6605: CERROR_ILL fires only when
/// `IDR0.S1P == 0`.  Bypass stream + S1P=1 is a silent no-op.
///
/// BEFORE FIX: current code raises GERROR (per-stream check) → FAILS.
/// AFTER FIX:  S1P=1 → silent no-op → PASSES.
#[test]
fn new_audit_04_bypass_stream_cfgi_cd_all_no_error_when_s1p_enabled() {
    // §4.3.4: when IDR0.S1P==1 (default), CfgiCdAll on a bypass-only stream is
    // a silent no-op — there are no CDs to invalidate, but no error is raised.
    let smmu = make_smmu();
    // IDR0.S1P defaults to 1 — no set_s1p_supported() call needed.

    let stream_id: u32 = 0x43;

    smmu.configure_stream(sid(stream_id), bypass_config())
        .expect("configure_stream (bypass) should succeed");
    smmu.enable_stream(sid(stream_id))
        .expect("enable_stream should succeed");

    smmu.clear_event_queue();

    // Submit CfgiCdAll — must be a silent no-op when IDR0.S1P==1.
    submit_cfgi_cd_all(&smmu, stream_id);

    assert!(
        !is_gerror_cmdq_err_active(&smmu),
        "BUG-AUDIT-NEW-02: CfgiCdAll on a bypass-only stream when IDR0.S1P==1 \
         must NOT raise GERROR.CMDQ_ERR. \
         ARM §4.3.4 / §4.3.3 line 6605: CERROR_ILL fires only when IDR0.S1P==0 \
         (stage-1 not globally implemented). \
         Current code checks per-stream stage-1 → incorrectly raises GERROR. \
         GERROR=0x{:08X}",
        smmu.get_gerror()
    );
}

// ============================================================================
// NEW-AUDIT-05: inject_walk_eabt() missing is_stage2 / event_class parameters
// (ARM §7.3.12 line 27078)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 (baseline): single-stage walk abort → event_class=1, s2=false
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §7.3.12: Single-stage walk abort has CLASS=TT (event_class=1) and S2=false.
// New API: inject_walk_eabt(stream_id, pasid, iova, is_stage2=false, event_class=1).
//
// BEFORE FIX: new five-parameter overload does not compile → FAILS.
// AFTER FIX:  compiles; event has event_class=1 and s2=false → PASSES.

/// NEW-AUDIT-05 baseline: `inject_walk_eabt(is_stage2=false, event_class=1)` →
/// `event_class==1`, `s2==false`.
///
/// ARM §7.3.12: single-stage walk abort → CLASS=TT, S2=0.
///
/// BEFORE FIX: five-parameter signature does not compile → FAILS.
/// AFTER FIX:  event has correct fields → PASSES.
#[test]
fn new_audit_05_single_stage_walk_event_class_1_s2_false() {
    // §7.3.12: single-stage walk abort (is_stage2=false, event_class=1).
    // New API must preserve the existing behaviour as the default case.
    let smmu = make_smmu();

    // New API: inject_walk_eabt(stream_id, pasid, iova, is_stage2, event_class).
    // BEFORE FIX: five-parameter overload does not exist → compile error.
    smmu.inject_walk_eabt(
        sid(0x70),
        pasid(0),
        iova(0x1000),
        /*is_stage2=*/ false,
        /*event_class=*/ 1u8,
    );

    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "NEW-AUDIT-05 baseline: at least one event must be enqueued after \
         inject_walk_eabt(is_stage2=false, event_class=1)"
    );

    let ev = events
        .iter()
        .find(|e| e.event_type == EventType::FWalkEabt)
        .expect("NEW-AUDIT-05 baseline: FWalkEabt event not found in queue");

    assert_eq!(
        ev.event_class, 1u8,
        "NEW-AUDIT-05 baseline: single-stage walk abort must have event_class=1 \
         (CLASS=TT per ARM §7.3.12). Got event_class={}",
        ev.event_class
    );

    assert!(
        !ev.s2,
        "NEW-AUDIT-05 baseline: single-stage walk abort must have s2=false. \
         Got s2={}",
        ev.s2
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: two-stage walk abort at TT level → s2=true, event_class=1
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §7.3.12 line 27078: walk abort during stage-2 TT descriptor fetch has
// S2=true and CLASS=TT (event_class=1).
//
// BEFORE FIX: existing API has no is_stage2/event_class params → compile error → FAILS.
// AFTER FIX:  event has s2=true and event_class=1 → PASSES.

/// NEW-AUDIT-05: `inject_walk_eabt(is_stage2=true, event_class=1)` →
/// `s2==true`, `event_class==1`.
///
/// ARM §7.3.12: two-stage walk abort (TT level) → S2=true, CLASS=TT.
///
/// BEFORE FIX: five-parameter signature does not compile → FAILS.
/// AFTER FIX:  `s2=true` and `event_class=1` → PASSES.
#[test]
fn new_audit_05_two_stage_walk_tt_s2_true_event_class_1() {
    // §7.3.12: two-stage walk abort at TT descriptor level → S2=true, CLASS=TT.
    // This is the stage-2 TT walk abort case.
    let smmu = make_smmu();

    // New API: is_stage2=true, event_class=1 → stage-2 TT walk abort.
    // BEFORE FIX: five-parameter overload does not exist → compile error.
    smmu.inject_walk_eabt(
        sid(0x71),
        pasid(0),
        iova(0x2000),
        /*is_stage2=*/ true,
        /*event_class=*/ 1u8,
    );

    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "NEW-AUDIT-05: at least one event must be enqueued after \
         inject_walk_eabt(is_stage2=true, event_class=1)"
    );

    let ev = events
        .iter()
        .find(|e| e.event_type == EventType::FWalkEabt)
        .expect("NEW-AUDIT-05: FWalkEabt event not found (two-stage TT abort test)");

    assert!(
        ev.s2,
        "NEW-AUDIT-05: two-stage walk abort (TT level) must have s2=true \
         (ARM §7.3.12: S2 field indicates the fault occurred during stage-2 walk). \
         Current code: inject_walk_eabt() always sets s2=false. \
         Got s2={}",
        ev.s2
    );

    assert_eq!(
        ev.event_class, 1u8,
        "NEW-AUDIT-05: two-stage walk abort (TT level) must have event_class=1 \
         (CLASS=TT per ARM §7.3.12). Got event_class={}",
        ev.event_class
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3: two-stage walk abort at IPA input level → s2=true, event_class=2
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §7.3.12 line 27078: walk abort when the IPA is the input address to the
// stage-2 walk has S2=true and CLASS=IN (event_class=2).
//
// BEFORE FIX: five-parameter overload does not exist → compile error → FAILS.
// AFTER FIX:  event has s2=true and event_class=2 → PASSES.

/// NEW-AUDIT-05: `inject_walk_eabt(is_stage2=true, event_class=2)` →
/// `s2==true`, `event_class==2`.
///
/// ARM §7.3.12: two-stage walk abort (IPA input) → S2=true, CLASS=IN.
///
/// BEFORE FIX: five-parameter signature does not compile → FAILS.
/// AFTER FIX:  `s2=true` and `event_class=2` → PASSES.
#[test]
fn new_audit_05_two_stage_walk_ipa_s2_true_event_class_2() {
    // §7.3.12: two-stage walk abort at IPA input → S2=true, CLASS=IN (event_class=2).
    // This is the stage-2 IPA input fault case.
    let smmu = make_smmu();

    // New API: is_stage2=true, event_class=2 → stage-2 IPA input walk abort.
    // BEFORE FIX: five-parameter overload does not exist → compile error.
    smmu.inject_walk_eabt(
        sid(0x72),
        pasid(0),
        iova(0x3000),
        /*is_stage2=*/ true,
        /*event_class=*/ 2u8,
    );

    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "NEW-AUDIT-05: at least one event must be enqueued after \
         inject_walk_eabt(is_stage2=true, event_class=2)"
    );

    let ev = events
        .iter()
        .find(|e| e.event_type == EventType::FWalkEabt)
        .expect("NEW-AUDIT-05: FWalkEabt event not found (two-stage IPA abort test)");

    assert!(
        ev.s2,
        "NEW-AUDIT-05: two-stage walk abort (IPA input) must have s2=true \
         (ARM §7.3.12: S2 flag indicates stage-2 fault). \
         Current code: inject_walk_eabt() always sets s2=false. \
         Got s2={}",
        ev.s2
    );

    assert_eq!(
        ev.event_class, 2u8,
        "NEW-AUDIT-05: two-stage walk abort (IPA input) must have event_class=2 \
         (CLASS=IN per ARM §7.3.12: the IPA is the input address, not a TT descriptor). \
         Current code: inject_walk_eabt() always sets event_class=1 (TT). \
         Got event_class={}",
        ev.event_class
    );
}
