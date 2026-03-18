//! Tests for Bug 3: UWXN condition is inverted.
//!
//! ARM IHI0070G.b §5.4 states:
//!   "In configurations for which all accesses are considered privileged,
//!    for example StreamWorld == any-EL2 or StreamWorld == EL3, this field
//!    has no effect and is IGNORED."
//!
//! The current UWXN check fires when `effective_priv_suppresses_check()` returns
//! true (STRW=El2/El2E2h/El3 or PRIVCFG=3).  This is BACKWARDS.
//!
//! Per §5.4, UWXN must be IGNORED (not fire) when StreamWorld=EL2/EL3.
//! UWXN is meant to protect privileged code from executing writable pages that
//! are also accessible to unprivileged code.  In EL2/EL3 "all-privileged"
//! worlds there is no unprivileged code to worry about, so UWXN is irrelevant.
//!
//! Before fix: El2 + UWXN=1 + writable page + execute → FAILS (fires F_PERMISSION incorrectly).
//!
//! After fix:
//!
//! - El2 + UWXN=1 + writable page + execute → PASSES (UWXN ignored for EL2)
//! - El1El0 + PRIVCFG=3 + UWXN=1 + writable page + execute → FAILS (UWXN fires)
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

// ============================================================================
// Bug 3: STRW=El2, UWXN=1, writable page, execute → UWXN IGNORED per §5.4
//
// BEFORE FIX: fires F_PERMISSION incorrectly (UWXN applied when it should be ignored).
// AFTER FIX:  translation succeeds (UWXN ignored for all-privileged EL2 streams).
// ============================================================================

/// Bug 3a: El2 stream + UWXN=1 + writable page + execute → should SUCCEED.
///
/// Per ARM §5.4, UWXN is IGNORED when StreamWorld=EL2 because all accesses are
/// already considered privileged; there is no unprivileged code to protect against.
///
/// BEFORE FIX: UWXN fires → Err(WxnFault) → F_PERMISSION — test FAILS.
/// AFTER FIX:  UWXN ignored → translation succeeds — test PASSES.
#[test]
fn bug3_el2_stream_uwxn_ignored_writable_page_execute_succeeds() {
    let smmu = make_smmu();
    let stream_id = sid(0xD0);
    let mut cfg = StreamConfig::stage1_only();
    cfg.strw = StreamWorld::El2; // all-privileged EL2 stream
    cfg.uwxn = true;
    cfg.wxn = false; // UWXN only — WXN disabled
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // Map read-write-execute: writable AND executable, not privileged_only.
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x1000),
        pa(0x2000),
        PagePermissions::all(), // write=true, privileged_only=false
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::Execute,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_ok(),
        "Bug 3: EL2 stream + UWXN=1 must IGNORE UWXN per §5.4 — execute on writable \
         page should succeed, got: {result:?}"
    );
}

/// Bug 3b: El3 stream + UWXN=1 + writable page + execute → should SUCCEED.
///
/// Per ARM §5.4, UWXN is also IGNORED when StreamWorld=EL3.
///
/// BEFORE FIX: UWXN fires → Err(WxnFault) → test FAILS.
/// AFTER FIX:  UWXN ignored → succeeds — test PASSES.
#[test]
fn bug3_el3_stream_uwxn_ignored_writable_page_execute_succeeds() {
    let smmu = make_smmu();
    let stream_id = sid(0xD1);
    let mut cfg = StreamConfig::stage1_only();
    cfg.strw = StreamWorld::El3;
    cfg.security_state = SecurityState::Secure; // EL3 requires Secure stream
    cfg.uwxn = true;
    cfg.wxn = false;
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x1000),
        pa(0x2000),
        PagePermissions::all(),
        SecurityState::Secure,
    )
    .unwrap();

    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::Execute,
        SecurityState::Secure,
    );
    assert!(
        result.is_ok(),
        "Bug 3: EL3 stream + UWXN=1 must IGNORE UWXN per §5.4 — got: {result:?}"
    );
}

/// Bug 3c: El2E2h stream + UWXN=1 + writable page + execute → should SUCCEED.
///
/// El2E2h is a VHE (Virtualization Host Extension) variant of EL2. Per §5.4,
/// UWXN is also IGNORED for this all-privileged variant.
///
/// BEFORE FIX: test FAILS (UWXN incorrectly fires for El2E2h).
/// AFTER FIX:  UWXN ignored → succeeds — test PASSES.
#[test]
fn bug3_el2e2h_stream_uwxn_ignored_writable_page_execute_succeeds() {
    let smmu = make_smmu();
    let stream_id = sid(0xD2);
    let mut cfg = StreamConfig::stage1_only();
    cfg.strw = StreamWorld::El2E2h;
    cfg.uwxn = true;
    cfg.wxn = false;
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x1000),
        pa(0x2000),
        PagePermissions::all(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::Execute,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_ok(),
        "Bug 3: El2E2h stream + UWXN=1 must IGNORE UWXN per §5.4 — got: {result:?}"
    );
}

// ============================================================================
// Bug 3: Positive test — PRIVCFG=3 (Force Privileged), STRW=El1El0, UWXN=1 fires.
//
// PRIVCFG=3 promotes individual accesses to privileged but does NOT make the
// entire StreamWorld "all-privileged" (EL2/EL3).  UWXN must still fire for
// El1El0 streams even when PRIVCFG=3 promotes the access to privileged.
//
// Per §5.4: "In configurations for which all accesses are considered privileged,
// for example StreamWorld == any-EL2 or StreamWorld == EL3 ..."
// PRIVCFG=3 is NOT listed as one of these configurations.
// ============================================================================

/// Bug 3d: El1El0 + PRIVCFG=3 + UWXN=1 + writable page + execute → F_PERMISSION.
///
/// PRIVCFG=3 makes the access privileged, but the StreamWorld is still El1El0.
/// Per §5.4, UWXN is only ignored when StreamWorld itself is EL2/EL3.
/// For El1El0 + PRIVCFG=3, UWXN must fire: the access is privileged AND the
/// page is writable by unprivileged code.
///
/// BEFORE FIX: UWXN fires correctly (PRIVCFG=3 triggers it via effective_priv_suppresses_check).
/// AFTER FIX:  UWXN still fires correctly (El1El0 is not all-privileged → UWXN active).
/// This test must PASS both before and after the fix (it is a regression guard).
#[test]
fn bug3_el1el0_privcfg3_uwxn_fires_f_permission() {
    let smmu = make_smmu();
    let stream_id = sid(0xD3);
    let mut cfg = StreamConfig::stage1_only();
    cfg.strw = StreamWorld::El1El0; // not all-privileged world
    cfg.priv_cfg = 3;               // PRIVCFG=3: Force Privileged (individual access override)
    cfg.uwxn = true;
    cfg.wxn = false;
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x1000),
        pa(0x2000),
        PagePermissions::all(), // write=true, privileged_only=false
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::Execute,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "Bug 3: El1El0 + PRIVCFG=3 + UWXN=1 must fire F_PERMISSION — got: {result:?}"
    );

    let events = smmu.get_events();
    assert!(!events.is_empty(), "Expected F_PERMISSION event");
    let ev = events.last().unwrap();
    assert_eq!(
        ev.event_type,
        EventType::FPermission,
        "Expected F_PERMISSION, got {:?}",
        ev.event_type
    );
}

/// Bug 3e: El1El0, no PRIVCFG override, UWXN=1 + writable page + execute → no fault.
///
/// El1El0 with default PRIVCFG=0 (Use Incoming): the access is not privileged.
/// UWXN only fires when the access IS privileged AND the page is writable by
/// unprivileged code.  A non-privileged execute must not be affected by UWXN.
///
/// This is a regression guard: both before and after the fix, this test passes.
#[test]
fn bug3_el1el0_no_privcfg_uwxn_unprivileged_execute_no_fault() {
    let smmu = make_smmu();
    let stream_id = sid(0xD4);
    let mut cfg = StreamConfig::stage1_only();
    cfg.strw = StreamWorld::El1El0;
    cfg.priv_cfg = 0; // Use Incoming (default)
    cfg.uwxn = true;
    cfg.wxn = false;
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x1000),
        pa(0x2000),
        PagePermissions::all(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // AccessType::Execute with PRIVCFG=0 (Use Incoming) — not privileged incoming.
    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::Execute,
        SecurityState::NonSecure,
    );
    // UWXN requires privileged access; non-privileged execute should not be blocked.
    assert!(
        result.is_ok(),
        "Bug 3: El1El0 + PRIVCFG=0 + UWXN=1 + non-privileged execute must NOT fault: {result:?}"
    );
}
