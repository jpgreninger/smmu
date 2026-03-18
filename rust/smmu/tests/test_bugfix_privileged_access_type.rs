//! Tests for Bug 4: Rust AccessType lacks privileged variants.
//!
//! ARM IHI0070G.b §7.3 (EventEntry.PnU): "Privileged/Non-Unprivileged (post-STE override)"
//!
//! For PRIVCFG=Use Incoming (0b00/0b01), pnu must reflect the AxPROT bit of the
//! incoming transaction.  Since the Rust AccessType enum has no privileged variants,
//! is_access_effectively_privileged() always returns false for PRIVCFG=0/1, causing
//! pnu to be wrong for privileged incoming transactions.
//!
//! Fix: Add *Privileged variants to AccessType and update is_access_effectively_privileged()
//! to return access_type.is_privileged() for the PRIVCFG=0/1 case.
//!
//! Backward compatibility: All existing AccessType values remain unchanged.
//! Only new *Privileged variants cause pnu=true for PRIVCFG=Use Incoming streams.
//!
//! BEFORE FIX: No privileged variants exist → tests fail to compile or pnu=false.
//! AFTER FIX:  Privileged variants exist → pnu=true for PRIVCFG=0+ReadPrivileged.
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
// Bug 4: AccessType::is_privileged() must exist and return true only for
// *Privileged variants.
// ============================================================================

/// Bug 4a: AccessType::ReadPrivileged exists and is_privileged() returns true.
///
/// BEFORE FIX: AccessType::ReadPrivileged does not exist → test fails to compile.
/// AFTER FIX:  ReadPrivileged exists and is_privileged() returns true.
#[test]
fn bug4_access_type_read_privileged_is_privileged() {
    assert!(AccessType::ReadPrivileged.is_privileged(), "ReadPrivileged.is_privileged() must be true");
    assert!(AccessType::ReadPrivileged.can_read(), "ReadPrivileged.can_read() must be true");
    assert!(!AccessType::ReadPrivileged.can_write(), "ReadPrivileged.can_write() must be false");
    assert!(!AccessType::ReadPrivileged.can_execute(), "ReadPrivileged.can_execute() must be false");
}

/// Bug 4b: AccessType::WritePrivileged exists and is_privileged() returns true.
///
/// BEFORE FIX: WritePrivileged does not exist → compile error.
/// AFTER FIX:  WritePrivileged.is_privileged() == true, can_write() == true.
#[test]
fn bug4_access_type_write_privileged_is_privileged() {
    assert!(AccessType::WritePrivileged.is_privileged(), "WritePrivileged.is_privileged() must be true");
    assert!(!AccessType::WritePrivileged.can_read(), "WritePrivileged.can_read() must be false");
    assert!(AccessType::WritePrivileged.can_write(), "WritePrivileged.can_write() must be true");
    assert!(!AccessType::WritePrivileged.can_execute(), "WritePrivileged.can_execute() must be false");
}

/// Bug 4c: AccessType::ExecutePrivileged exists and is_privileged() returns true.
///
/// BEFORE FIX: ExecutePrivileged does not exist → compile error.
/// AFTER FIX:  ExecutePrivileged.is_privileged() == true, can_execute() == true.
#[test]
fn bug4_access_type_execute_privileged_is_privileged() {
    assert!(AccessType::ExecutePrivileged.is_privileged(), "ExecutePrivileged.is_privileged() must be true");
    assert!(!AccessType::ExecutePrivileged.can_read(), "ExecutePrivileged.can_read() must be false");
    assert!(!AccessType::ExecutePrivileged.can_write(), "ExecutePrivileged.can_write() must be false");
    assert!(AccessType::ExecutePrivileged.can_execute(), "ExecutePrivileged.can_execute() must be true");
}

/// Bug 4d: AccessType::ReadWritePrivileged exists and is_privileged() returns true.
///
/// BEFORE FIX: ReadWritePrivileged does not exist → compile error.
/// AFTER FIX:  ReadWritePrivileged.is_privileged() == true, can_read() and can_write() == true.
#[test]
fn bug4_access_type_read_write_privileged_is_privileged() {
    assert!(AccessType::ReadWritePrivileged.is_privileged(), "ReadWritePrivileged.is_privileged() must be true");
    assert!(AccessType::ReadWritePrivileged.can_read(), "ReadWritePrivileged.can_read() must be true");
    assert!(AccessType::ReadWritePrivileged.can_write(), "ReadWritePrivileged.can_write() must be true");
    assert!(!AccessType::ReadWritePrivileged.can_execute(), "ReadWritePrivileged.can_execute() must be false");
}

/// Bug 4e: Non-privileged variants must return false for is_privileged().
///
/// Backward compatibility guard: existing variants must not be affected.
///
/// BEFORE FIX: is_privileged() does not exist → compile error.
/// AFTER FIX:  all non-privileged variants return false for is_privileged().
#[test]
fn bug4_non_privileged_variants_return_false_for_is_privileged() {
    assert!(!AccessType::None.is_privileged(),          "None.is_privileged() must be false");
    assert!(!AccessType::Read.is_privileged(),          "Read.is_privileged() must be false");
    assert!(!AccessType::Write.is_privileged(),         "Write.is_privileged() must be false");
    assert!(!AccessType::ReadWrite.is_privileged(),     "ReadWrite.is_privileged() must be false");
    assert!(!AccessType::Execute.is_privileged(),       "Execute.is_privileged() must be false");
    assert!(!AccessType::ReadExecute.is_privileged(),   "ReadExecute.is_privileged() must be false");
    assert!(!AccessType::WriteExecute.is_privileged(),  "WriteExecute.is_privileged() must be false");
    assert!(!AccessType::ReadWriteExecute.is_privileged(), "ReadWriteExecute.is_privileged() must be false");
}

// ============================================================================
// Bug 4: EventEntry.pnu must be true when PRIVCFG=0 (Use Incoming) and the
// incoming access is AccessType::ReadPrivileged (or any other *Privileged variant).
// ============================================================================

/// Bug 4f: PRIVCFG=0 (Use Incoming) + AccessType::ReadPrivileged → pnu=true.
///
/// When PRIVCFG=Use Incoming and the incoming transaction is privileged
/// (e.g., AxPROT[1]=1 represented as ReadPrivileged), pnu must be true.
///
/// BEFORE FIX: AccessType::ReadPrivileged does not exist → test fails to compile.
///             Even after adding the type, pnu would be false (bug).
/// AFTER FIX:  pnu=true because is_access_effectively_privileged(ReadPrivileged) returns true.
#[test]
fn bug4_privcfg0_read_privileged_sets_pnu_true() {
    let smmu = make_smmu();
    let stream_id = sid(0xE0);
    let mut cfg = StreamConfig::stage1_only();
    cfg.strw = StreamWorld::El1El0; // not all-privileged
    cfg.priv_cfg = 0;               // PRIVCFG=0: Use Incoming
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // Leave page unmapped so translation produces F_TRANSLATION fault.

    // Use AccessType::ReadPrivileged to represent a privileged incoming transaction.
    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x2000),
        AccessType::ReadPrivileged,
        SecurityState::NonSecure,
    );
    assert!(result.is_err(), "Unmapped page must produce a fault: {result:?}");

    let events = smmu.get_events();
    assert!(!events.is_empty(), "Expected an event to be recorded");
    let ev = events.last().unwrap();
    assert_eq!(
        ev.event_type,
        EventType::FTranslation,
        "Expected F_TRANSLATION event, got {:?}",
        ev.event_type
    );
    assert!(
        ev.pnu,
        "Bug 4: PRIVCFG=0 + AccessType::ReadPrivileged must set pnu=true; got pnu=false"
    );
}

/// Bug 4g: PRIVCFG=0 + AccessType::Read (non-privileged) → pnu=false.
///
/// Backward compatibility: existing non-privileged AccessType::Read with PRIVCFG=0
/// must still produce pnu=false.
///
/// BEFORE FIX: pnu=false → this test passes (regression guard).
/// AFTER FIX:  pnu=false → must still pass.
#[test]
fn bug4_privcfg0_read_non_privileged_pnu_false() {
    let smmu = make_smmu();
    let stream_id = sid(0xE1);
    let mut cfg = StreamConfig::stage1_only();
    cfg.strw = StreamWorld::El1El0;
    cfg.priv_cfg = 0;
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x2000),
        AccessType::Read, // non-privileged — pnu must be false
        SecurityState::NonSecure,
    );
    assert!(result.is_err(), "Unmapped page must produce a fault: {result:?}");

    let events = smmu.get_events();
    assert!(!events.is_empty(), "Expected an event to be recorded");
    let ev = events.last().unwrap();
    assert!(
        !ev.pnu,
        "Bug 4: PRIVCFG=0 + AccessType::Read must keep pnu=false (non-privileged incoming); got pnu=true"
    );
}

/// Bug 4h: PRIVCFG=0 + AccessType::WritePrivileged → pnu=true.
///
/// WritePrivileged represents a privileged write transaction.
/// With PRIVCFG=Use Incoming, pnu must reflect the incoming privilege level.
///
/// BEFORE FIX: WritePrivileged does not exist → compile error.
/// AFTER FIX:  pnu=true.
#[test]
fn bug4_privcfg0_write_privileged_sets_pnu_true() {
    let smmu = make_smmu();
    let stream_id = sid(0xE2);
    let mut cfg = StreamConfig::stage1_only();
    cfg.strw = StreamWorld::El1El0;
    cfg.priv_cfg = 0;
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // Leave page unmapped to produce a fault.

    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x2000),
        AccessType::WritePrivileged,
        SecurityState::NonSecure,
    );
    assert!(result.is_err(), "Unmapped page must produce a fault: {result:?}");

    let events = smmu.get_events();
    assert!(!events.is_empty(), "Expected an event to be recorded");
    let ev = events.last().unwrap();
    assert!(
        ev.pnu,
        "Bug 4: PRIVCFG=0 + AccessType::WritePrivileged must set pnu=true; got pnu=false"
    );
}

/// Bug 4i: PRIVCFG=3 (Force Privileged) + AccessType::Read → pnu=true.
///
/// PRIVCFG=3 overrides the incoming privilege, so pnu is always true regardless
/// of whether the incoming AccessType is privileged.
/// This is a regression guard: pnu=true for PRIVCFG=3 must still work after Bug 4 fix.
///
/// BEFORE FIX: pnu=true (PRIVCFG=3 works correctly).
/// AFTER FIX:  pnu=true (must still work).
#[test]
fn bug4_privcfg3_read_non_privileged_pnu_still_true() {
    let smmu = make_smmu();
    let stream_id = sid(0xE3);
    let mut cfg = StreamConfig::stage1_only();
    cfg.strw = StreamWorld::El1El0;
    cfg.priv_cfg = 3; // Force Privileged — overrides incoming
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x2000),
        AccessType::Read, // non-privileged incoming, but PRIVCFG=3 overrides
        SecurityState::NonSecure,
    );
    assert!(result.is_err(), "Unmapped page must produce a fault: {result:?}");

    let events = smmu.get_events();
    assert!(!events.is_empty(), "Expected an event to be recorded");
    let ev = events.last().unwrap();
    assert!(
        ev.pnu,
        "Regression: PRIVCFG=3 + AccessType::Read must still set pnu=true after Bug 4 fix"
    );
}

// ============================================================================
// Bug 4: UWXN interaction with *Privileged variants
//
// is_access_effectively_privileged() now uses access_type.is_privileged() for
// PRIVCFG=0/1.  Verify UWXN fires correctly for *Privileged variants.
// ============================================================================

/// Bug 4j: El1El0 + PRIVCFG=0 + UWXN=1 + AccessType::ExecutePrivileged on writable page → F_PERMISSION.
///
/// ExecutePrivileged represents a privileged execute (AxPROT[1]=1, AxPROT[2]=0).
/// For PRIVCFG=Use Incoming, is_access_effectively_privileged() should return true for
/// ExecutePrivileged.  UWXN should then fire for the writable page.
///
/// BEFORE FIX: ExecutePrivileged does not exist → compile error.
/// AFTER FIX:  UWXN fires → F_PERMISSION.
#[test]
fn bug4_execute_privileged_uwxn_fires_for_el1el0_privcfg0() {
    let smmu = make_smmu();
    let stream_id = sid(0xE4);
    let mut cfg = StreamConfig::stage1_only();
    cfg.strw = StreamWorld::El1El0;
    cfg.priv_cfg = 0;  // Use Incoming
    cfg.uwxn = true;
    cfg.wxn = false;
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // writable, non-privileged_only page:
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
        AccessType::ExecutePrivileged, // privileged execute coming in
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "Bug 4: El1El0 + PRIVCFG=0 + UWXN + ExecutePrivileged must fire UWXN: {result:?}"
    );

    let events = smmu.get_events();
    assert!(!events.is_empty(), "Expected F_PERMISSION event from UWXN");
    let ev = events.last().unwrap();
    assert_eq!(
        ev.event_type,
        EventType::FPermission,
        "Expected F_PERMISSION, got {:?}",
        ev.event_type
    );
}

/// Bug 4k: El1El0 + PRIVCFG=0 + UWXN=1 + AccessType::Execute (non-privileged) → no fault.
///
/// A non-privileged execute (AccessType::Execute with PRIVCFG=0) must NOT trigger UWXN
/// since the access is not privileged.
///
/// BEFORE FIX: already correct (PRIVCFG=0 + Execute = not privileged → UWXN doesn't fire).
/// AFTER FIX:  still correct.
#[test]
fn bug4_execute_non_privileged_uwxn_does_not_fire_el1el0_privcfg0() {
    let smmu = make_smmu();
    let stream_id = sid(0xE5);
    let mut cfg = StreamConfig::stage1_only();
    cfg.strw = StreamWorld::El1El0;
    cfg.priv_cfg = 0;
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
        AccessType::Execute, // non-privileged execute
        SecurityState::NonSecure,
    );
    assert!(
        result.is_ok(),
        "Bug 4 regression: El1El0 + PRIVCFG=0 + UWXN + non-privileged Execute must NOT fault: {result:?}"
    );
}
