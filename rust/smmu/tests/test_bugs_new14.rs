//! TDD failing test for BUG-NEW-F (Rust).
//!
//! Each test is written to FAIL with the current code (red) and PASS only
//! after the corresponding fix is applied (green).
//!
//! # BUG-NEW-F (§5.5 CdIllegal() pseudocode line 9748)
//!
//! ARM IHI0070G.b §5.5 CdIllegal() pseudocode:
//! ```text
//!     if STE.S1STALLD == '1' && CD.S == '1' then
//!         return TRUE;  // CD is ILLEGAL → C_BAD_CD event
//! ```
//!
//! When `s1_stalld=true` AND `fault_mode=FaultMode::Stall` (CD.S==1),
//! `configure_stream()` MUST emit `CBadCd` and return `Err`, for ALL values
//! of `STALL_MODEL`.
//!
//! The existing BUG-C3 check (`STALL_MODEL!=0 + s1_stalld → C_BAD_STE`) already
//! rejects `STALL_MODEL≠0` before reaching the CD check, so the only uncovered
//! case is `STALL_MODEL==0b00`.
//!
//! ## Current behavior (WRONG)
//!
//! `configure_stream()` silently accepts `s1_stalld=true` + `fault_mode=Stall`
//! when `STALL_MODEL==0b00`.  No `CBadCd` is emitted.
//!
//! ## Required behavior
//!
//! `configure_stream()` must return `Err` and emit a `CBadCd` event when
//! `stage1_enabled=true` AND `s1_stalld=true` AND `fault_mode=Stall`,
//! regardless of `STALL_MODEL`.
//!
//! BEFORE FIX: `configure_stream()` returns `Ok`, no `CBadCd` event → test 1 FAILS.
//! AFTER FIX:  `configure_stream()` returns `Err`, `CBadCd` in event queue → PASSES.

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

/// Build an SMMU with all queues enabled.
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
// BUG-NEW-F test 1: s1_stalld=true + fault_mode=Stall + STALL_MODEL==0 → CBadCd
// ============================================================================
//
// ARM §5.5 CdIllegal() pseudocode line 9748:
//   if STE.S1STALLD == '1' && CD.S == '1' then return TRUE;
//
// This check applies unconditionally — for ALL values of STALL_MODEL, including
// STALL_MODEL==0b00 (the unordered model).
//
// The BUG-C3 fix guards against STALL_MODEL!=0b00 + s1_stalld → C_BAD_STE,
// but that guard fires only when STALL_MODEL != 0.  When STALL_MODEL==0b00,
// the BUG-C3 guard is skipped and the CdIllegal() check at line 9748 applies.
// The current code has no check for STALL_MODEL==0b00 + s1_stalld + CD.S==1.
//
// Setup:
//   set_stall_model(0)                — STALL_MODEL==0b00 (BUG-C3 guard will NOT fire)
//   stage1_enabled=true
//   s1_stalld=true                    — STE.S1STALLD==1
//   fault_mode=FaultMode::Stall       — CD.S==1
//
// BEFORE FIX: configure_stream() returns Ok, no CBadCd event → FAILS.
// AFTER FIX:  configure_stream() returns Err, CBadCd in queue → PASSES.

/// BUG-NEW-F: `s1_stalld=true` + `fault_mode=Stall` + `STALL_MODEL==0` → `CBadCd`.
///
/// ARM §5.5 CdIllegal() line 9748: `STE.S1STALLD==1 && CD.S==1` is an illegal CD.
/// This condition must be checked for ALL `STALL_MODEL` values including 0b00.
///
/// BEFORE FIX: `configure_stream()` returns `Ok`, no `CBadCd` event → FAILS.
/// AFTER FIX:  `configure_stream()` returns `Err`, `CBadCd` event emitted → PASSES.
#[test]
fn bug_new_f_s1stalld_cd_stall_model0_emits_c_bad_cd() {
    // §5.5 CdIllegal line 9748: S1STALLD=1 AND CD.S=1 → C_BAD_CD.
    // This must fire for STALL_MODEL==0b00 (the uncovered case).
    let smmu = make_smmu();

    // Ensure STALL_MODEL==0b00 so the BUG-C3 guard (STALL_MODEL!=0 + s1_stalld)
    // does NOT fire and mask the missing CdIllegal() check.
    smmu.set_stall_model(0x00);

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s1_stalld = true;                  // STE.S1STALLD==1
    cfg.fault_mode = FaultMode::Stall;     // CD.S==1

    let result = smmu.configure_stream(sid(0x20), cfg);

    // configure_stream() must return an error (InvalidConfiguration / Err).
    assert!(
        result.is_err(),
        "BUG-NEW-F: configure_stream() with s1_stalld=true AND fault_mode=Stall \
         AND STALL_MODEL==0b00 must return Err (ARM §5.5 CdIllegal line 9748: \
         S1STALLD==1 && CD.S==1 → C_BAD_CD). \
         Current code: no check for this combination when STALL_MODEL==0b00. \
         Got: Ok"
    );

    // CBadCd must appear in the event queue.
    assert!(
        has_event(&smmu, EventType::CBadCd),
        "BUG-NEW-F: CBadCd event must be recorded when s1_stalld=true AND \
         fault_mode=Stall AND STALL_MODEL==0b00 (ARM §5.5 CdIllegal line 9748). \
         Current code silently accepts this combination with no event."
    );
}

// ============================================================================
// BUG-NEW-F test 2 (regression): s1_stalld=true + fault_mode=Terminate + STALL_MODEL==0
// → ACCEPTED (valid — CD.S==0, so CdIllegal check does not fire)
// ============================================================================
//
// ARM §5.5 CdIllegal() line 9748:
//   if STE.S1STALLD == '1' && CD.S == '1' then return TRUE;
//
// When fault_mode=Terminate (CD.S==0), the CdIllegal condition is NOT triggered,
// even with s1_stalld=true.  This is a valid configuration.
//
// This test verifies the fix is properly scoped: it must reject only the
// s1_stalld=true + CD.S==1 combination.
//
// BEFORE FIX: (already passes — configure_stream accepts this today).
// AFTER FIX:  must still pass — regression guard.

/// BUG-NEW-F regression: `s1_stalld=true` + `fault_mode=Terminate` + `STALL_MODEL==0` accepted.
///
/// ARM §5.5 CdIllegal() line 9748: condition requires `CD.S==1`; with `Terminate`
/// (CD.S==0) the check does not apply → valid config.
///
/// BEFORE FIX: (passes already). AFTER FIX: must still pass (regression guard).
#[test]
fn bug_new_f_s1stalld_cd_terminate_accepted() {
    // Regression: s1_stalld=true + fault_mode=Terminate + STALL_MODEL==0 must succeed.
    // CD.S==0 (Terminate) → CdIllegal condition is NOT met → valid config.
    let smmu = make_smmu();

    smmu.set_stall_model(0x00);

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s1_stalld = true;                      // STE.S1STALLD==1
    cfg.fault_mode = FaultMode::Terminate;     // CD.S==0 → CdIllegal does NOT apply

    let result = smmu.configure_stream(sid(0x21), cfg);

    assert!(
        result.is_ok(),
        "BUG-NEW-F regression: s1_stalld=true + fault_mode=Terminate + STALL_MODEL==0 \
         must be ACCEPTED (ARM §5.5 CdIllegal: condition requires both S1STALLD=1 AND CD.S=1; \
         CD.S=0 means the check does not apply). \
         The fix must not over-reject configs where fault_mode=Terminate. \
         Got: Err"
    );

    assert!(
        !has_event(&smmu, EventType::CBadCd),
        "BUG-NEW-F regression: no CBadCd event expected when fault_mode=Terminate \
         (CD.S==0) — the CdIllegal condition only fires when both S1STALLD=1 AND CD.S=1."
    );
}

// ============================================================================
// BUG-NEW-F test 3 (regression): s1_stalld=false + fault_mode=Stall + STALL_MODEL==0
// → ACCEPTED (normal stall configuration)
// ============================================================================
//
// ARM §5.5 CdIllegal() line 9748:
//   if STE.S1STALLD == '1' && CD.S == '1' then return TRUE;
//
// When s1_stalld=false, the CdIllegal condition is NOT triggered regardless of
// fault_mode.  This is the standard stall configuration.
//
// This test verifies the fix targets only the s1_stalld=true + CD.S==1 combo.
//
// BEFORE FIX: (already passes — configure_stream accepts this today).
// AFTER FIX:  must still pass — regression guard.

/// BUG-NEW-F regression: `s1_stalld=false` + `fault_mode=Stall` + `STALL_MODEL==0` accepted.
///
/// ARM §5.5 CdIllegal() line 9748: condition requires `S1STALLD==1`; with
/// `s1_stalld=false` the check does not apply → normal stall config is valid.
///
/// BEFORE FIX: (passes already). AFTER FIX: must still pass (regression guard).
#[test]
fn bug_new_f_s1stalld_false_cd_stall_accepted() {
    // Regression: s1_stalld=false + fault_mode=Stall + STALL_MODEL==0 must succeed.
    // STE.S1STALLD==0 → CdIllegal condition is NOT met → valid stall config.
    let smmu = make_smmu();

    smmu.set_stall_model(0x00);

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s1_stalld = false;                 // STE.S1STALLD==0 → CdIllegal does NOT apply
    cfg.fault_mode = FaultMode::Stall;     // CD.S==1

    let result = smmu.configure_stream(sid(0x22), cfg);

    assert!(
        result.is_ok(),
        "BUG-NEW-F regression: s1_stalld=false + fault_mode=Stall + STALL_MODEL==0 \
         must be ACCEPTED (ARM §5.5 CdIllegal: condition requires S1STALLD==1; \
         S1STALLD=0 means the check does not apply). \
         The fix must not reject normal stall configurations. \
         Got: Err"
    );

    assert!(
        !has_event(&smmu, EventType::CBadCd),
        "BUG-NEW-F regression: no CBadCd event expected when s1_stalld=false \
         (the CdIllegal condition only fires when both S1STALLD=1 AND CD.S=1)."
    );
}
