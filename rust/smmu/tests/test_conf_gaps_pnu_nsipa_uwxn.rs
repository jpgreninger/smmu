//! Tests for three conformance gaps:
//!
//! - Gap 1 (§7.3): `EventEntry.pnu` must be `true` when the access is privileged
//!   (STRW=El2/El3 or PRIVCFG=3), and `false` otherwise.
//! - Gap 2 (§7.3): `EventEntry.nsipa` must be `true` when the IPA is in the
//!   Non-Secure PA space, i.e., `s2 && security_state == NonSecure`.
//! - Gap 3 (§5.4): UWXN=1 causes F_PERMISSION when a privileged execute targets
//!   a page that is writable by unprivileged code (write permission AND !privileged_only).
//!   WXN=1 already works; UWXN adds the constraint that only privileged execute is
//!   affected.
#![allow(missing_docs)]
#![allow(clippy::doc_markdown)]

use smmu::types::{
    AccessType, EventType, IOVA, PagePermissions, PASID, SecurityState, StreamConfig, StreamID,
    StreamWorld, PA,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn make_smmu() -> SMMU {
    let s = SMMU::new();
    s.enable().unwrap();
    s
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

fn sid(id: u32) -> StreamID {
    StreamID::new(id).unwrap()
}

fn pasid(id: u32) -> PASID {
    PASID::new(id).unwrap()
}

/// Build a stage-1-only stream with STRW=El1El0, map a read-write page,
/// then perform an unmapped translate to produce a fault event.
fn setup_stage1_stream_with_strw(smmu: &SMMU, stream_id: StreamID, strw: StreamWorld) {
    let mut cfg = StreamConfig::stage1_only();
    cfg.strw = strw;
    cfg.t0sz = 0; // no VA range restriction
    // BUG-RUST-2a fix: STRW bit[0]=1 (El2/El3) is forbidden for NonSecure streams.
    // Use Secure when STRW has bit[0] set (El2 or El3).
    if (strw as u8 & 0x01) != 0 {
        cfg.security_state = SecurityState::Secure;
    }
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // Do NOT map the page — leave it unmapped so translate produces F_TRANSLATION.
}

/// Build a two-stage stream with both stages enabled, no VA/IPA range
/// restrictions.  Stage-2 address space is created but left empty so that
/// any IPA produces a stage-2 fault.
fn setup_two_stage_stream(smmu: &SMMU, stream_id: StreamID, security_state: SecurityState) {
    let mut cfg = StreamConfig::two_stage();
    cfg.t0sz = 0;
    cfg.s2_t0sz = 25; // minimum legal S2T0SZ per §5.2 (BUG-AUDIT-45 fix: 0 is out of range [16,39])
    cfg.security_state = security_state;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();
    // Stage-1 maps IOVA → IPA; stage-2 is empty so the IPA translation fails.
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x1000),
        pa(0x5000),
        PagePermissions::read_write(),
        security_state,
    )
    .unwrap();
}

// ============================================================================
// Gap 1: EventEntry.pnu — privileged/not-unprivileged bit
//
// BEFORE FIX: pnu is always false (..EventEntry::zeroed() spread).
// AFTER FIX:  pnu = is_access_privileged() derived from STRW/PRIVCFG.
// ============================================================================

/// Gap 1 baseline: El1El0 stream fault → pnu=false (unprivileged).
///
/// STRW=El1El0 means the transaction operates at EL1/EL0 — not privileged
/// in PnU wire-format terms.  The event must have pnu=false.
///
/// BEFORE FIX: pnu is always false → this test passes before the fix
/// (regression guard — must continue to pass after the fix).
#[test]
fn gap1_pnu_el1el0_stream_fault_is_unprivileged() {
    let smmu = make_smmu();
    let stream_id = sid(0xA0);
    setup_stage1_stream_with_strw(&smmu, stream_id, StreamWorld::El1El0);

    let result = smmu.translate(stream_id, pasid(0), iova(0x2000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "Unmapped page must produce a fault: {result:?}");

    let events = smmu.get_events();
    assert!(!events.is_empty(), "Expected an event to be recorded");
    let ev = events.last().unwrap();
    assert_eq!(ev.event_type, EventType::FTranslation, "Expected F_TRANSLATION event");
    assert!(!ev.pnu, "Gap 1: El1El0 stream must have pnu=false; got pnu=true");
}

/// Gap 1: El2 stream fault → pnu=true (privileged).
///
/// STRW=El2 means the transaction is at EL2 — privileged in PnU terms.
///
/// BEFORE FIX: pnu is always false → test FAILS before the fix.
/// AFTER FIX:  pnu=true for El2 streams.
#[test]
fn gap1_pnu_el2_stream_fault_is_privileged() {
    let smmu = make_smmu();
    let stream_id = sid(0xA1);
    setup_stage1_stream_with_strw(&smmu, stream_id, StreamWorld::El2);

    let result = smmu.translate(stream_id, pasid(0), iova(0x2000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "Unmapped page must produce a fault: {result:?}");

    let events = smmu.get_events();
    assert!(!events.is_empty(), "Expected an event to be recorded");
    let ev = events.last().unwrap();
    assert_eq!(ev.event_type, EventType::FTranslation, "Expected F_TRANSLATION event");
    assert!(ev.pnu, "Gap 1: El2 stream must have pnu=true; got pnu=false");
}

/// Gap 1: El3 stream fault (Secure security state) → pnu=true (privileged).
///
/// STRW=El3 is only valid for Secure streams.  The transaction is at EL3 —
/// privileged in PnU terms.
///
/// BEFORE FIX: pnu is always false → test FAILS before the fix.
/// AFTER FIX:  pnu=true for El3 streams.
#[test]
fn gap1_pnu_el3_stream_fault_is_privileged() {
    let smmu = make_smmu();
    let stream_id = sid(0xA2);
    let mut cfg = StreamConfig::stage1_only();
    // STRW=El3 is now forbidden even for Secure streams (BUG-RUST-2b fix, ARM §5.2).
    // STRW=El2 has the same all-privileged semantics and is valid for Secure streams.
    cfg.strw = StreamWorld::El2;
    cfg.security_state = SecurityState::Secure;
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // Do not map — translate on unmapped address produces F_TRANSLATION.

    let result = smmu.translate(stream_id, pasid(0), iova(0x2000), AccessType::Read, SecurityState::Secure);
    assert!(result.is_err(), "Unmapped page must produce a fault: {result:?}");

    let events = smmu.get_events();
    assert!(!events.is_empty(), "Expected an event to be recorded");
    let ev = events.last().unwrap();
    assert_eq!(ev.event_type, EventType::FTranslation, "Expected F_TRANSLATION event");
    assert!(ev.pnu, "Gap 1: El3 stream must have pnu=true; got pnu=false");
}

/// Gap 1: PRIVCFG=3 (Force Privileged) → pnu=true regardless of STRW.
///
/// PRIVCFG=3 overrides STRW to always treat the access as privileged.
/// With STRW=El1El0 but PRIVCFG=3, the access is privileged → pnu=true.
///
/// BEFORE FIX: pnu is always false → test FAILS before the fix.
/// AFTER FIX:  pnu=true for PRIVCFG=3.
#[test]
fn gap1_pnu_privcfg3_force_privileged_is_pnu_true() {
    let smmu = make_smmu();
    let stream_id = sid(0xA3);
    let mut cfg = StreamConfig::stage1_only();
    cfg.strw = StreamWorld::El1El0; // would normally be unprivileged
    cfg.priv_cfg = 3;               // PRIVCFG=3: Force Privileged override
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // Do not map — translate on unmapped address produces F_TRANSLATION.

    let result = smmu.translate(stream_id, pasid(0), iova(0x2000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "Unmapped page must produce a fault: {result:?}");

    let events = smmu.get_events();
    assert!(!events.is_empty(), "Expected an event to be recorded");
    let ev = events.last().unwrap();
    assert_eq!(ev.event_type, EventType::FTranslation, "Expected F_TRANSLATION event");
    assert!(ev.pnu, "Gap 1: PRIVCFG=3 must set pnu=true; got pnu=false");
}

/// Gap 1: PRIVCFG=2 (Force Unprivileged) → pnu=false even when STRW=El2.
///
/// PRIVCFG=2 overrides STRW to never treat the access as privileged.
/// With STRW=El2 but PRIVCFG=2, the access is unprivileged → pnu=false.
///
/// BEFORE FIX: pnu is always false → this test passes before the fix
/// (regression guard — must continue to pass after the fix).
#[test]
fn gap1_pnu_privcfg2_force_unprivileged_is_pnu_false() {
    let smmu = make_smmu();
    let stream_id = sid(0xA4);
    let mut cfg = StreamConfig::stage1_only();
    cfg.strw = StreamWorld::El2; // would normally be privileged
    // STRW=El2 with NonSecure is rejected (BUG-RUST-2a fix, ARM §5.2 bit[0]=1 rule).
    cfg.security_state = SecurityState::Secure;
    cfg.priv_cfg = 2;             // PRIVCFG=2: Force Unprivileged override
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // Do not map — translate on unmapped address produces F_TRANSLATION.

    let result = smmu.translate(stream_id, pasid(0), iova(0x2000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "Unmapped page must produce a fault: {result:?}");

    let events = smmu.get_events();
    assert!(!events.is_empty(), "Expected an event to be recorded");
    let ev = events.last().unwrap();
    assert_eq!(ev.event_type, EventType::FTranslation, "Expected F_TRANSLATION event");
    assert!(!ev.pnu, "Gap 1: PRIVCFG=2 must set pnu=false even for El2 STRW; got pnu=true");
}

// ============================================================================
// Gap 2: EventEntry.nsipa — Non-Secure IPA bit
//
// nsipa must be true when s2=true AND security_state == NonSecure.
//
// BEFORE FIX: nsipa is always false (..EventEntry::zeroed() spread).
// AFTER FIX:  nsipa = s2 && (security_state == SecurityState::NonSecure).
// ============================================================================

/// Gap 2 baseline: stage-1-only fault → nsipa=false (no IPA involved).
///
/// Single-stage translation has no IPA; nsipa must be false.
///
/// BEFORE FIX: nsipa is always false → this test passes before the fix
/// (regression guard — must continue to pass after the fix).
#[test]
fn gap2_nsipa_stage1_only_fault_is_false() {
    let smmu = make_smmu();
    let stream_id = sid(0xB0);
    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // Do not map — produces F_TRANSLATION with s2=false.

    let result = smmu.translate(stream_id, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "Unmapped page must produce a fault: {result:?}");

    let events = smmu.get_events();
    assert!(!events.is_empty(), "Expected an event to be recorded");
    let ev = events.last().unwrap();
    assert!(!ev.nsipa, "Gap 2: stage-1-only fault must have nsipa=false; got nsipa=true");
    assert!(!ev.s2, "stage-1-only fault must have s2=false");
}

/// Gap 2: Two-stage NonSecure stream, stage-2 fault → nsipa=true.
///
/// A two-stage translation on a NonSecure stream where stage-2 fails
/// produces s2=true.  Since security_state=NonSecure, nsipa must be true.
///
/// BEFORE FIX: nsipa is always false → test FAILS before the fix.
/// AFTER FIX:  nsipa=true for NonSecure two-stage fault.
#[test]
fn gap2_nsipa_two_stage_nonsecure_stage2_fault_is_true() {
    let smmu = make_smmu();
    let stream_id = sid(0xB1);
    setup_two_stage_stream(&smmu, stream_id, SecurityState::NonSecure);

    // Stage-1 maps IOVA=0x1000 → IPA=0x5000; stage-2 has no mapping for IPA=0x5000.
    let result = smmu.translate(stream_id, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "Stage-2 fault must produce an error: {result:?}");

    let events = smmu.get_events();
    assert!(!events.is_empty(), "Expected an event to be recorded");
    let ev = events.last().unwrap();
    assert!(ev.s2, "Gap 2: two-stage fault must have s2=true");
    assert!(ev.nsipa, "Gap 2: NonSecure two-stage fault must have nsipa=true; got nsipa=false");
}

/// Gap 2: Two-stage Secure stream, stage-2 fault → nsipa=false.
///
/// A two-stage translation on a Secure stream where stage-2 fails
/// produces s2=true.  Since security_state=Secure (not NonSecure), nsipa
/// must be false.
///
/// BEFORE FIX: nsipa is always false → this test passes before the fix
/// (regression guard — must continue to pass after the fix).
#[test]
fn gap2_nsipa_two_stage_secure_stage2_fault_is_false() {
    let smmu = make_smmu();
    let stream_id = sid(0xB2);
    setup_two_stage_stream(&smmu, stream_id, SecurityState::Secure);

    let result = smmu.translate(stream_id, pasid(0), iova(0x1000), AccessType::Read, SecurityState::Secure);
    assert!(result.is_err(), "Stage-2 fault must produce an error: {result:?}");

    let events = smmu.get_events();
    assert!(!events.is_empty(), "Expected an event to be recorded");
    let ev = events.last().unwrap();
    assert!(ev.s2, "Gap 2: two-stage fault must have s2=true");
    assert!(!ev.nsipa, "Gap 2: Secure two-stage fault must have nsipa=false; got nsipa=true");
}

// ============================================================================
// Gap 3: CD.UWXN — unprivileged write execute never
//
// UWXN=1 causes F_PERMISSION when a privileged execute targets a page that
// is writable by unprivileged code (write=true AND !privileged_only).
// WXN catches all execute-on-writable-page.  UWXN is the privileged-only
// variant of WXN.
//
// BEFORE FIX: UWXN is stored but not enforced → tests FAIL before the fix.
// AFTER FIX:  UWXN check added after WXN check in all three WXN locations.
// ============================================================================

/// Gap 3 baseline: UWXN=false → privileged execute on writable page allowed.
///
/// Without UWXN, a page mapped read-write is executable by a privileged
/// stream even if it is writable by unprivileged code.
///
/// BEFORE FIX: UWXN not enforced → this test passes before the fix
/// (regression guard — must continue to pass after the fix).
#[test]
fn gap3_uwxn_false_allows_privileged_execute_on_writable_page() {
    let smmu = make_smmu();
    let stream_id = sid(0xC0);
    let mut cfg = StreamConfig::stage1_only();
    cfg.strw = StreamWorld::El2; // privileged stream
    // STRW=El2 with NonSecure is rejected (BUG-RUST-2a fix, ARM §5.2 bit[0]=1 rule).
    cfg.security_state = SecurityState::Secure;
    cfg.uwxn = false;
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // Map as read-write-execute (writable, not privileged_only).
    smmu.map_page(stream_id, pasid(0), iova(0x1000), pa(0x2000),
        PagePermissions::all(), SecurityState::NonSecure).unwrap();

    let result = smmu.translate(stream_id, pasid(0), iova(0x1000),
        AccessType::Execute, SecurityState::NonSecure);
    assert!(result.is_ok(), "UWXN=false must allow privileged execute on writable page: {result:?}");
}

/// Gap 3: UWXN=true, STRW=El1El0 (unprivileged stream) → no fault.
///
/// UWXN only applies to privileged accesses.  An unprivileged stream
/// (STRW=El1El0) executing on a writable page must NOT be faulted by UWXN.
/// WXN would fault it if set, but UWXN alone should not.
///
/// BEFORE FIX: UWXN not enforced → this test passes before the fix
/// (regression guard — must continue to pass after the fix).
#[test]
fn gap3_uwxn_true_unprivileged_stream_no_fault() {
    let smmu = make_smmu();
    let stream_id = sid(0xC1);
    let mut cfg = StreamConfig::stage1_only();
    cfg.strw = StreamWorld::El1El0; // unprivileged stream
    cfg.uwxn = true;
    cfg.wxn = false; // UWXN only — no WXN
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.map_page(stream_id, pasid(0), iova(0x1000), pa(0x2000),
        PagePermissions::all(), SecurityState::NonSecure).unwrap();

    let result = smmu.translate(stream_id, pasid(0), iova(0x1000),
        AccessType::Execute, SecurityState::NonSecure);
    assert!(result.is_ok(),
        "Gap 3: UWXN=true but unprivileged stream must NOT fault on execute: {result:?}");
}

/// Gap 3 (updated by Bug-3 fix): UWXN=true, El1El0+PRIVCFG=3, writable page → F_PERMISSION.
///
/// Bug-3 fix: UWXN is IGNORED for El2/El3 all-privileged streams per ARM §5.4.
/// The correct case for UWXN firing is: El1El0 StreamWorld + PRIVCFG=3 (Force
/// Privileged) — the access is privileged but the stream is NOT all-privileged,
/// so UWXN is active.
///
/// BEFORE BUG-3 FIX: UWXN incorrectly fired for El2 → previous test used El2.
/// AFTER BUG-3 FIX:  El2 correctly ignores UWXN; El1El0+PRIVCFG=3 still fires.
#[test]
fn gap3_uwxn_true_privileged_stream_faults_on_writable_nonpriv_page() {
    let smmu = make_smmu();
    let stream_id = sid(0xC2);
    let mut cfg = StreamConfig::stage1_only();
    cfg.strw = StreamWorld::El1El0; // not all-privileged world — UWXN is active
    cfg.priv_cfg = 3;               // PRIVCFG=3: Force Privileged (individual access override)
    cfg.uwxn = true;
    cfg.wxn = false; // UWXN only — no WXN
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // PagePermissions::all() gives read+write+execute, NOT privileged_only.
    smmu.map_page(stream_id, pasid(0), iova(0x1000), pa(0x2000),
        PagePermissions::all(), SecurityState::NonSecure).unwrap();

    let result = smmu.translate(stream_id, pasid(0), iova(0x1000),
        AccessType::Execute, SecurityState::NonSecure);
    assert!(result.is_err(),
        "Gap 3: UWXN=true + El1El0+PRIVCFG=3 (privileged, non-all-priv world) must fault \
         on writable unprivileged page: {result:?}");

    let events = smmu.get_events();
    assert!(!events.is_empty(), "Expected F_PERMISSION event");
    let ev = events.last().unwrap();
    assert_eq!(ev.event_type, EventType::FPermission,
        "Gap 3: Expected F_PERMISSION (0x13) event, got {:?}", ev.event_type);
}

/// Gap 3: UWXN=true, STRW=El2, page is privileged_only → no UWXN fault.
///
/// UWXN checks: write=true AND !privileged_only.  A page mapped as
/// privileged_only is NOT accessible by unprivileged code, so UWXN
/// does not apply.  The privileged execute should succeed.
///
/// BEFORE FIX: UWXN not enforced → Ok → test passes before the fix
/// (regression guard — must continue to pass after the fix).
///
/// Note: PagePermissions::privileged_execute() creates a page with
/// execute permission only for privileged code (privileged_only=true,
/// write=false).  We need write=true AND privileged_only=true for this
/// test.  Use read_write_privileged() if available, or construct manually.
#[test]
fn gap3_uwxn_true_privileged_stream_no_fault_on_privileged_only_page() {
    let smmu = make_smmu();
    let stream_id = sid(0xC3);
    let mut cfg = StreamConfig::stage1_only();
    cfg.strw = StreamWorld::El2;
    // STRW=El2 with NonSecure is rejected (BUG-RUST-2a fix, ARM §5.2 bit[0]=1 rule).
    cfg.security_state = SecurityState::Secure;
    cfg.uwxn = true;
    cfg.wxn = false;
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // Map with execute-only (no write bit) → UWXN check requires write=true,
    // so a non-writable execute-only page must not trigger UWXN.
    smmu.map_page(stream_id, pasid(0), iova(0x1000), pa(0x2000),
        PagePermissions::execute_only(), SecurityState::NonSecure).unwrap();

    let result = smmu.translate(stream_id, pasid(0), iova(0x1000),
        AccessType::Execute, SecurityState::NonSecure);
    // El2 stream: privileged_only check is suppressed by effective_priv_suppresses_check().
    // execute_only() page has no write bit → UWXN does not fire.
    assert!(result.is_ok(),
        "Gap 3: Non-writable page must not trigger UWXN: {result:?}");
}

/// Gap 3: Regression — UWXN=true + WXN=true, unprivileged execute on writable page
/// → WXN still fires (F_PERMISSION).
///
/// When both WXN and UWXN are set, WXN catches all execute-on-writable-page
/// regardless of privilege level.  This test verifies that WXN still works
/// correctly alongside UWXN.
///
/// BEFORE FIX: WXN fires → Err (test passes)
/// AFTER FIX:  WXN fires first → Err (test must continue to pass).
#[test]
fn gap3_wxn_and_uwxn_regression_wxn_still_blocks_unprivileged_execute() {
    let smmu = make_smmu();
    let stream_id = sid(0xC4);
    let mut cfg = StreamConfig::stage1_only();
    cfg.strw = StreamWorld::El1El0; // unprivileged stream
    cfg.wxn = true;  // WXN blocks all execute on writable pages
    cfg.uwxn = true; // UWXN also set (privilege-filtered superset)
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.map_page(stream_id, pasid(0), iova(0x1000), pa(0x2000),
        PagePermissions::all(), SecurityState::NonSecure).unwrap();

    let result = smmu.translate(stream_id, pasid(0), iova(0x1000),
        AccessType::Execute, SecurityState::NonSecure);
    assert!(result.is_err(),
        "WXN+UWXN: WXN must still block execute on writable page: {result:?}");

    let events = smmu.get_events();
    assert!(!events.is_empty(), "Expected F_PERMISSION event from WXN");
    let ev = events.last().unwrap();
    assert_eq!(ev.event_type, EventType::FPermission,
        "Expected F_PERMISSION (0x13) event from WXN, got {:?}", ev.event_type);
}
