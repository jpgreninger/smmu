//! TDD failing tests for NEW-AUDIT-04 and NEW-AUDIT-05 (Rust).
//!
//! Each test is written to FAIL with the current code (red) and PASS only after
//! the corresponding fix is applied (green), EXCEPT where explicitly marked as a
//! regression/baseline test.
//!
//! # NEW-AUDIT-04 (Both §4.3.3 / §4.3.4): CMD_CFGI_CD / CMD_CFGI_CD_ALL
//! missing stage-1 guard
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
//! When a stream is configured without stage-1 (bypass-only or stage-2-only),
//! issuing `CMD_CFGI_CD` or `CMD_CFGI_CD_ALL` for that stream must:
//! - Signal `GERROR.CMDQ_ERR` (bit 0).
//!
//! Current behavior (WRONG): both commands execute silently — no CERROR_ILL,
//! no `GERROR.CMDQ_ERR`.
//!
//! BEFORE FIX: `GERROR.CMDQ_ERR` remains 0 → failing tests FAIL.
//! AFTER FIX:  `GERROR.CMDQ_ERR` is set → PASS.
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
// NEW-AUDIT-04: CMD_CFGI_CD missing stage-1 guard (ARM §4.3.3 line 5362)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: bypass-only stream + CfgiCd → GERROR_CMDQ_ERR active
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.3.3 line 5362: "This command raises CERROR_ILL when stage 1 is not
// implemented."  A bypass-only stream has stage-1 disabled → CERROR_ILL.
//
// BEFORE FIX: `CfgiCd` executes silently; `GERROR.CMDQ_ERR` remains 0 → FAILS.
// AFTER FIX:  `GERROR.CMDQ_ERR` toggled → PASSES.

/// NEW-AUDIT-04: `CfgiCd` on a bypass-only stream (no stage-1) → `GERROR_CMDQ_ERR`.
///
/// ARM §4.3.3 line 5362: "CERROR_ILL when stage 1 is not implemented."
///
/// BEFORE FIX: command executes silently, `GERROR.CMDQ_ERR` unchanged → FAILS.
/// AFTER FIX:  `GERROR.CMDQ_ERR` toggled → PASSES.
#[test]
fn new_audit_04_bypass_stream_cfgi_cd_raises_cerror_ill() {
    // §4.3.3: CfgiCd on a bypass-only stream (stage-1 not implemented) → CERROR_ILL.
    let smmu = make_smmu();

    let stream_id: u32 = 0x40;

    // Configure a bypass-only stream (no stage-1 CD exists for this stream).
    smmu.configure_stream(sid(stream_id), bypass_config())
        .expect("configure_stream (bypass) should succeed");
    smmu.enable_stream(sid(stream_id))
        .expect("enable_stream should succeed");

    // Clear setup events before the assertion.
    smmu.clear_event_queue();

    // Submit CfgiCd — must raise CERROR_ILL because stage-1 is absent.
    submit_cfgi_cd(&smmu, stream_id, 0);

    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "NEW-AUDIT-04: CfgiCd on a bypass-only stream (stage-1 not implemented) \
         must toggle GERROR.CMDQ_ERR (bit 0). \
         ARM §4.3.3 line 5362: 'This command raises CERROR_ILL when stage 1 is \
         not implemented.' \
         Current code: no stage-1 guard in CfgiCd handler → GERROR unchanged. \
         GERROR=0x{:08X}",
        smmu.get_gerror()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: stage-2-only stream + CfgiCd → GERROR_CMDQ_ERR active
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.3.3: Stage-2-only (stage2_enabled=true, stage1_enabled=false) also has
// "stage 1 not implemented" for this stream → CERROR_ILL.
//
// BEFORE FIX: `CfgiCd` executes silently → `GERROR.CMDQ_ERR` remains 0 → FAILS.
// AFTER FIX:  `GERROR.CMDQ_ERR` toggled → PASSES.

/// NEW-AUDIT-04: `CfgiCd` on a stage-2-only stream → `GERROR_CMDQ_ERR`.
///
/// ARM §4.3.3 line 5362: no stage-1 → CERROR_ILL.
///
/// BEFORE FIX: executes silently → FAILS. AFTER FIX: CERROR_ILL raised → PASSES.
#[test]
fn new_audit_04_stage2_only_stream_cfgi_cd_raises_cerror_ill() {
    // §4.3.3: CfgiCd on a stage-2-only stream (stage-1 absent) → CERROR_ILL.
    let smmu = make_smmu();

    let stream_id: u32 = 0x41;

    smmu.configure_stream(sid(stream_id), stage2_only_config())
        .expect("configure_stream (stage2-only) should succeed");
    smmu.enable_stream(sid(stream_id))
        .expect("enable_stream should succeed");

    smmu.clear_event_queue();

    submit_cfgi_cd(&smmu, stream_id, 0);

    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "NEW-AUDIT-04: CfgiCd on a stage-2-only stream (stage-1 not implemented) \
         must toggle GERROR.CMDQ_ERR (bit 0). \
         ARM §4.3.3 line 5362: 'This command raises CERROR_ILL when stage 1 is \
         not implemented.' A stage-2-only stream has no CD — stage-1 is absent. \
         Current code: no stage-1 guard → GERROR unchanged. \
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
// NEW-AUDIT-04: CMD_CFGI_CD_ALL missing stage-1 guard (ARM §4.3.4 line 5388)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 4: bypass-only stream + CfgiCdAll → GERROR_CMDQ_ERR active
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.3.4 line 5388: "This command raises CERROR_ILL when stage 1 is not
// implemented."
//
// BEFORE FIX: `CfgiCdAll` executes silently → `GERROR.CMDQ_ERR` remains 0 → FAILS.
// AFTER FIX:  `GERROR.CMDQ_ERR` toggled → PASSES.

/// NEW-AUDIT-04: `CfgiCdAll` on a bypass-only stream → `GERROR_CMDQ_ERR`.
///
/// ARM §4.3.4 line 5388: no stage-1 → CERROR_ILL.
///
/// BEFORE FIX: executes silently → FAILS. AFTER FIX: CERROR_ILL raised → PASSES.
#[test]
fn new_audit_04_bypass_stream_cfgi_cd_all_raises_cerror_ill() {
    // §4.3.4: CfgiCdAll on a bypass-only stream (stage-1 absent) → CERROR_ILL.
    let smmu = make_smmu();

    let stream_id: u32 = 0x43;

    smmu.configure_stream(sid(stream_id), bypass_config())
        .expect("configure_stream (bypass) should succeed");
    smmu.enable_stream(sid(stream_id))
        .expect("enable_stream should succeed");

    smmu.clear_event_queue();

    // Submit CfgiCdAll — must raise CERROR_ILL because stage-1 is absent.
    submit_cfgi_cd_all(&smmu, stream_id);

    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "NEW-AUDIT-04: CfgiCdAll on a bypass-only stream (stage-1 not implemented) \
         must toggle GERROR.CMDQ_ERR (bit 0). \
         ARM §4.3.4 line 5388: 'This command raises CERROR_ILL when stage 1 is \
         not implemented.' \
         Current code: no stage-1 guard in CfgiCdAll handler → GERROR unchanged. \
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
