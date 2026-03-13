#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

//! TDD tests for BUG-PRIQEN-ASYMMETRY
//!
//! ARM IHI0070G.b §6.3.9.5 + §8.2: effective PRIQEN = (CR0.PRIQEN != 0) AND
//! (CR0.SMMUEN != 0).  When SMMUEN == 0 the PRI queue is inactive regardless
//! of the raw PRIQEN bit value.  All incoming PPRs must be rejected.
//!
//! Bug: `submit_page_request()` checks only the raw CR0.PRIQEN bit.  After
//! `enable()` + `disable()`, PRIQEN remains 1 in CR0 but SMMUEN is cleared
//! to 0.  The effective PRIQEN should be 0, so `submit_page_request()` must
//! return an error — but the buggy code accepts the request.

use smmu::types::{AccessType, PRIEntry};
use smmu::SMMU;

// ── Helpers ──────────────────────────────────────────────────────────────────

const fn make_pri_entry() -> PRIEntry {
    PRIEntry::with_address(1, 0, 0x1000, AccessType::Read)
}

// ── BUG-PRIQEN-ASYMMETRY Test 1 ──────────────────────────────────────────────
//
// Scenario: enable() sets SMMUEN=1 and PRIQEN=1.  disable() clears SMMUEN but
// leaves PRIQEN=1.  The effective PRIQEN is 0 because SMMUEN==0, so
// submit_page_request() MUST return an error.
//
// BEFORE FIX: submit_page_request() checks raw CR0.PRIQEN (still 1) → accepts
//             the request → test FAILS.
// AFTER FIX:  submit_page_request() checks effective PRIQEN (PRIQEN & SMMUEN)
//             = 0 → rejects the request → test PASSES.
#[test]
fn priqen_asymmetry_submit_after_enable_then_disable_returns_error() {
    let smmu = SMMU::new();

    // Step 1: enable — sets SMMUEN=1, PRIQEN=1.
    smmu.enable().unwrap();

    // Step 2: disable — clears SMMUEN, leaves PRIQEN=1 in the raw register.
    smmu.disable().unwrap();

    // Step 3: submit a page request.  Effective PRIQEN = PRIQEN & SMMUEN = 0.
    // Per §6.3.9.5 + §8.2 the request MUST be rejected.
    let result = smmu.submit_page_request(make_pri_entry());
    assert!(
        result.is_err(),
        "submit_page_request() must fail when SMMUEN==0 (effective PRIQEN==0), got Ok"
    );
}

// ── BUG-PRIQEN-ASYMMETRY Test 2 ──────────────────────────────────────────────
//
// Baseline: SMMU never enabled — both SMMUEN and PRIQEN are 0 in the
// freshly-constructed state.  submit_page_request() must also fail.
//
// This test validates the pre-existing behaviour is preserved by the fix.
#[test]
fn priqen_submit_on_never_enabled_smmu_returns_error() {
    let smmu = SMMU::new();
    // Neither enable() nor disable() called — SMMUEN=0, PRIQEN=0.
    let result = smmu.submit_page_request(make_pri_entry());
    assert!(
        result.is_err(),
        "submit_page_request() must fail when SMMU was never enabled"
    );
}

// ── BUG-PRIQEN-ASYMMETRY Test 3 ──────────────────────────────────────────────
//
// Positive control: after enable() (SMMUEN=1, PRIQEN=1) submit_page_request()
// must succeed.  Verifies the fix does not break the enabled path.
#[test]
fn priqen_submit_while_enabled_succeeds() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let result = smmu.submit_page_request(make_pri_entry());
    assert!(
        result.is_ok(),
        "submit_page_request() must succeed when SMMU is enabled (SMMUEN=1, PRIQEN=1), got Err: {:?}",
        result.err()
    );
}

// ── BUG-PRIQEN-ASYMMETRY Test 4 ──────────────────────────────────────────────
//
// Re-enable after disable: enable() → disable() → enable() must restore
// PRIQEN to effective 1 and allow submissions again.
#[test]
fn priqen_submit_after_enable_disable_reenable_succeeds() {
    let smmu = SMMU::new();

    smmu.enable().unwrap();
    smmu.disable().unwrap();
    smmu.enable().unwrap();

    let result = smmu.submit_page_request(make_pri_entry());
    assert!(
        result.is_ok(),
        "submit_page_request() must succeed after re-enable, got Err: {:?}",
        result.err()
    );
}
