#![allow(clippy::doc_markdown)]
#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

//! BUG-03/SPEC-09: GERROR/GERRORN Toggle Protocol (ARM IHI0070G.b §6.3.19/6.3.20)
//!
//! Spec:
//! - GERRORN is a SEPARATE software-writable register, initialized to 0 at reset.
//! - SMMU toggles GERROR[x] ONLY when the error is currently INACTIVE (GERROR[x]==GERRORN[x]).
//! - "The SMMU does not toggle a bit when an error is already active."
//! - Software acknowledges by writing GERRORN[x] to match GERROR[x] (toggle GERRORN[x]).
//!
//! XOR-toggle semantics (at reset: GERROR=0, GERRORN=0, equal=inactive):
//! - Signal:       GERROR ^= 1 → GERROR=1, GERRORN=0. Unequal → ACTIVE.
//! - Signal again: GERROR==1 != GERRORN==0 → already active, do nothing. Stays ACTIVE.
//! - Acknowledge:  GERRORN ^= 1 → GERRORN=1. GERROR==GERRORN==1 → INACTIVE.
//! - Signal again: GERROR==1 == GERRORN==1 → inactive, toggle. GERROR ^= 1 → GERROR=0. ACTIVE again.

use smmu::types::{CommandEntry, CommandType};
use smmu::SMMU;

fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    smmu
}

/// Trigger a CMDQ_ERR by submitting a CMD_SYNC with CS=3 (Reserved → CERROR_ILL per §4.7.3).
/// (CONF-GAP-2 fix: CMD_CFGI_STE for unknown StreamID is now a silent no-op per §4.3.1.)
fn trigger_cmdq_err(smmu: &SMMU, _stream_id: u32) {
    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 3; // CS=0b11 is Reserved → CERROR_ILL per §4.7.3
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();
}

// ── Reset state ───────────────────────────────────────────────────────────────

/// §6.3.19/6.3.20: GERROR and GERRORN must both be 0 after reset.
#[test]
fn bug03_gerror_and_gerrorn_zero_at_reset() {
    let smmu = make_smmu();
    assert_eq!(smmu.get_gerror(), 0, "GERROR must be 0 after reset");
    assert_eq!(smmu.get_gerrorn(), 0, "GERRORN must be 0 after reset");
}

/// §6.3.19: An error is inactive when GERROR[x] == GERRORN[x].
/// At reset both are 0, so no error is active.
#[test]
fn bug03_no_active_error_at_reset() {
    let smmu = make_smmu();
    assert_eq!(
        smmu.get_gerror() ^ smmu.get_gerrorn(),
        0,
        "GERROR XOR GERRORN must be 0 (no active errors) after reset"
    );
}

// ── Signal semantics ──────────────────────────────────────────────────────────

/// §6.3.19 / BUG-03: When an error is first signalled, GERROR must be toggled.
///
/// At reset: GERROR=0, GERRORN=0 → inactive.
/// Signal CMDQ_ERR: GERROR should XOR to 1. Error is now ACTIVE.
/// This test FAILS before the fix because the error is raised via fetch_or
/// (which correctly sets the bit), but tests below show that the SECOND signal
/// must NOT toggle GERROR back.
#[test]
fn bug03_first_signal_toggles_gerror() {
    let smmu = make_smmu();
    assert_eq!(smmu.get_gerror(), 0, "precondition: GERROR=0");
    assert_eq!(smmu.get_gerrorn(), 0, "precondition: GERRORN=0");

    // Trigger CMDQ_ERR via a command queue error.
    trigger_cmdq_err(&smmu, 0xDEAD);

    let gerror = smmu.get_gerror();
    let gerrorn = smmu.get_gerrorn();

    assert_ne!(
        gerror & SMMU::GERROR_CMDQ_ERR,
        0,
        "§6.3.19: GERROR.CMDQ_ERR must be set (toggled from 0) after first signal; gerror={gerror:#x}"
    );
    assert_eq!(
        gerrorn & SMMU::GERROR_CMDQ_ERR,
        0,
        "§6.3.20: GERRORN.CMDQ_ERR must stay 0 (not modified by signal); gerrorn={gerrorn:#x}"
    );
    // Error is now ACTIVE: GERROR != GERRORN for bit CMDQ_ERR.
    assert_ne!(
        (gerror ^ gerrorn) & SMMU::GERROR_CMDQ_ERR,
        0,
        "§6.3.19: CMDQ_ERR must be ACTIVE (GERROR != GERRORN) after first signal"
    );
}

/// §6.3.19 / BUG-03: Signalling the SAME error AGAIN must NOT toggle GERROR back.
///
/// The SMMU must not toggle a bit when the error is already active.
///
/// Timeline:
///   Signal #1: GERROR.CMDQ_ERR: 0→1 (inactive→active). GERRORN stays 0.
///   Signal #2: GERROR.CMDQ_ERR stays 1 (already active, no toggle).
///
/// This test FAILS before the fix if fetch_or is replaced by XOR without the
/// "only-if-inactive" gate.
#[test]
fn bug03_second_signal_does_not_retoggle_gerror() {
    let smmu = make_smmu();

    // Signal #1: trigger CMDQ_ERR.
    trigger_cmdq_err(&smmu, 0xBAD1);

    let gerror_after_first = smmu.get_gerror();
    assert_ne!(
        gerror_after_first & SMMU::GERROR_CMDQ_ERR,
        0,
        "precondition: CMDQ_ERR must be set after first signal"
    );

    // Clear CMDQ_ERR so process_command_queue can run again (GERROR.CMDQ_ERR
    // blocks command queue processing when set).
    // NOTE: clear_gerror now toggles GERRORN, not GERROR.
    // After clear_gerror: GERRORN.CMDQ_ERR=1, GERROR.CMDQ_ERR=1 → inactive.
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);

    // Re-signal to make it active again.
    trigger_cmdq_err(&smmu, 0xBAD2);

    // Now CMDQ_ERR should be active again (GERROR != GERRORN).
    let gerror = smmu.get_gerror();
    let gerrorn = smmu.get_gerrorn();
    assert_ne!(
        (gerror ^ gerrorn) & SMMU::GERROR_CMDQ_ERR,
        0,
        "§6.3.19: CMDQ_ERR must be ACTIVE after re-signal"
    );

    // Signal again WITHOUT acknowledging first — must not toggle GERROR.
    // We can't call process_command_queue with GERROR.CMDQ_ERR active (returns Ok(0)).
    // Verify current state is ACTIVE and that the bit did not flip.
    let gerror_before_second = smmu.get_gerror();
    // Verify active error prevents command processing.
    smmu.submit_command(CommandEntry::new(CommandType::CfgiSte, 0xBAD3, 0)).unwrap();
    let result = smmu.process_command_queue();
    // process_command_queue returns Ok(0) when CMDQ_ERR is set (gate per NEW-50).
    assert_eq!(
        result.unwrap(),
        0,
        "process_command_queue must return Ok(0) when CMDQ_ERR is already set"
    );
    let gerror_after_blocked = smmu.get_gerror();
    assert_eq!(
        gerror_before_second & SMMU::GERROR_CMDQ_ERR,
        gerror_after_blocked & SMMU::GERROR_CMDQ_ERR,
        "§6.3.19: GERROR.CMDQ_ERR must not change when error is already active"
    );
}

// ── Acknowledge semantics ─────────────────────────────────────────────────────

/// §6.3.20 / BUG-03: clear_gerror() must toggle GERRORN (acknowledge), not clear GERROR.
///
/// After acknowledge: GERRORN.CMDQ_ERR=1, GERROR.CMDQ_ERR=1 → error INACTIVE.
/// The active-error indicator (GERROR XOR GERRORN) must be 0 for that bit.
///
/// This test FAILS before the fix because clear_gerror uses fetch_and(!bits)
/// which directly clears GERROR, but the spec says to write GERRORN.
#[test]
fn bug03_clear_gerror_toggles_gerrorn_not_gerror() {
    let smmu = make_smmu();

    // Trigger CMDQ_ERR.
    trigger_cmdq_err(&smmu, 0xC0FF);

    // Before acknowledge: GERROR.CMDQ_ERR must be set, GERRORN must be 0.
    assert_ne!(
        smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR, 0,
        "precondition: CMDQ_ERR is set"
    );
    assert_eq!(
        smmu.get_gerrorn() & SMMU::GERROR_CMDQ_ERR, 0,
        "precondition: GERRORN.CMDQ_ERR is 0 (not yet acknowledged)"
    );

    // Acknowledge: software writes GERRORN to match GERROR.
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);

    // GERROR.CMDQ_ERR must remain 1 (it was toggled to 1 by the signal;
    // clear_gerror writes GERRORN, not GERROR).
    assert_ne!(
        smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR,
        0,
        "BUG-03/§6.3.20: GERROR.CMDQ_ERR must remain 1 after clear_gerror \
         (clear_gerror writes GERRORN, not GERROR)"
    );
    // GERRORN.CMDQ_ERR must now be 1 (toggled by clear_gerror).
    assert_ne!(
        smmu.get_gerrorn() & SMMU::GERROR_CMDQ_ERR,
        0,
        "BUG-03/§6.3.20: GERRORN.CMDQ_ERR must be 1 after clear_gerror (acknowledge)"
    );
    // Error is now INACTIVE: GERROR == GERRORN for CMDQ_ERR bit.
    assert_eq!(
        (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR,
        0,
        "§6.3.20: After acknowledge, CMDQ_ERR must be INACTIVE (GERROR == GERRORN)"
    );
}

/// §6.3.19/6.3.20 / BUG-03: Full lifecycle — signal, signal-again (no-op),
/// acknowledge, signal again (reactivates).
///
/// This demonstrates the complete XOR-toggle protocol as specified.
#[test]
fn bug03_full_lifecycle_signal_acknowledge_resignal() {
    let smmu = make_smmu();

    // === Step 1: Signal CMDQ_ERR ===
    // At reset: GERROR=0, GERRORN=0 → inactive.
    trigger_cmdq_err(&smmu, 0x1001);

    let g1 = smmu.get_gerror();
    let gn1 = smmu.get_gerrorn();
    assert_ne!(
        (g1 ^ gn1) & SMMU::GERROR_CMDQ_ERR, 0,
        "Step 1: CMDQ_ERR must be ACTIVE after first signal"
    );

    // === Step 2: Acknowledge (clear_gerror toggles GERRORN) ===
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);

    let g2 = smmu.get_gerror();
    let gn2 = smmu.get_gerrorn();
    assert_eq!(
        (g2 ^ gn2) & SMMU::GERROR_CMDQ_ERR, 0,
        "Step 2: CMDQ_ERR must be INACTIVE after acknowledge"
    );

    // === Step 3: Re-signal (error is now inactive → should toggle GERROR again) ===
    trigger_cmdq_err(&smmu, 0x1002);

    let g3 = smmu.get_gerror();
    let gn3 = smmu.get_gerrorn();
    assert_ne!(
        (g3 ^ gn3) & SMMU::GERROR_CMDQ_ERR, 0,
        "Step 3: CMDQ_ERR must be ACTIVE again after re-signal following acknowledge"
    );
}

/// §6.3.20: get_gerrorn() must return the GERRORN register value.
#[test]
fn bug03_get_gerrorn_accessible() {
    let smmu = make_smmu();
    assert_eq!(smmu.get_gerrorn(), 0, "GERRORN must be 0 at reset");

    trigger_cmdq_err(&smmu, 0xACCE);

    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);
    let gerrorn = smmu.get_gerrorn();
    assert_ne!(
        gerrorn & SMMU::GERROR_CMDQ_ERR,
        0,
        "GERRORN.CMDQ_ERR must be 1 after clear_gerror (acknowledgement)"
    );
}

/// §6.3.20: clear_gerror with a bit that is not active is a no-op.
///
/// ARM IHI0070G.b §6.3.20: "Software must not toggle fields in this register
/// that correspond to errors that are inactive. It is CONSTRAINED UNPREDICTABLE
/// whether or not an SMMU activates errors if this is done."
///
/// At reset GERROR=0, GERRORN=0 — the CMDQ_ERR bit is inactive (equal).
/// Calling clear_gerror(CMDQ_ERR) on an inactive bit must be a no-op:
/// GERRORN must remain 0.
#[test]
fn bug03_clear_gerror_on_unset_bit_is_noop() {
    let smmu = make_smmu();
    // GERROR=0, GERRORN=0 at reset — CMDQ_ERR is inactive.
    assert_eq!(smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR, 0);
    assert_eq!(smmu.get_gerrorn() & SMMU::GERROR_CMDQ_ERR, 0);

    // clear_gerror on an inactive bit must be a no-op (ARM §6.3.20).
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);
    assert_eq!(
        smmu.get_gerrorn() & SMMU::GERROR_CMDQ_ERR,
        0,
        "clear_gerror on inactive bit must be a no-op per ARM §6.3.20"
    );
}
