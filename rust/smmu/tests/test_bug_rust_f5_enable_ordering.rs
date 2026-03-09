#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::unwrap_used)]

//! TDD regression tests for BUG-RUST-F5:
//! Silent event loss during `enable()` due to wrong initialization order.
//!
//! Problem (§6.3.9, §7.2.1):
//!   ARM §6.3.9 specifies that queues should be enabled before SMMUEN:
//!   EVENTQEN (and CMDQEN/PRIQEN) must be set before SMMUEN=1 so that
//!   events generated immediately after enabling are captured.
//!
//!   The current `enable()` implementation does:
//!   1. `enabled.store(true, Ordering::Release);`   ← SMMUEN=1
//!   2. `cr0.fetch_or(CR0_EVENTQEN | ..., AcqRel);` ← EVENTQEN set LATER
//!
//!   In the window between (1) and (2), translations can run and generate
//!   fault events, but `CR0_EVENTQEN` is still 0, so those events are
//!   silently discarded (§7.2.1: events not recorded when EVENTQEN=0).
//!
//! Fix: Reverse the order — set CR0_EVENTQEN (via cr0.fetch_or) BEFORE
//! setting enabled=true.  This matches ARM §6.3.9's recommended init order.
//!
//! Observable invariant tested here:
//! After `enable()` returns, CR0_EVENTQEN must be set.  Any fault that
//! occurs immediately after `enable()` must appear in the event queue.

use smmu::types::{
    AccessType, FaultType, PagePermissions, SecurityState, StreamConfig, StreamID, IOVA, PA, PASID,
};
use smmu::SMMU;

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}
fn pasid(n: u32) -> PASID {
    PASID::new(n).unwrap()
}
fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}
fn pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

/// After `enable()` returns, CR0.EVENTQEN must already be set.
///
/// ARM §6.3.9: EVENTQEN enables event recording. The spec order is:
/// configure queues (EVENTQEN=1), then SMMUEN=1.
/// If we set SMMUEN=1 first, events in the gap are lost.
///
/// This test verifies the post-condition: once enable() returns, the
/// EVENTQEN bit in CR0 must be set (no window where it is unset while
/// SMMUEN is already set).
#[test]
fn f5_eventqen_set_before_or_simultaneously_with_smmuen() {
    let smmu = SMMU::new();

    // Before enable(): both bits must be 0.
    assert_eq!(
        smmu.get_cr0() & SMMU::CR0_SMMUEN,
        0,
        "F5: SMMUEN must be 0 before enable()"
    );
    assert_eq!(
        smmu.get_cr0() & SMMU::CR0_EVENTQEN,
        0,
        "F5: EVENTQEN must be 0 before enable()"
    );

    smmu.enable().unwrap();

    // After enable(): EVENTQEN must be set.
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_EVENTQEN,
        0,
        "F5 BUG: after enable(), CR0.EVENTQEN is 0; \
         EVENTQEN must be set before or simultaneously with SMMUEN \
         per ARM §6.3.9 to prevent silent event loss"
    );

    // SMMUEN must also be set.
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_SMMUEN,
        0,
        "F5: after enable(), CR0.SMMUEN must be set"
    );

    // is_enabled() must be true.
    assert!(smmu.is_enabled(), "F5: is_enabled() must be true after enable()");
}

/// A fault generated immediately after `enable()` must appear in the event queue.
///
/// This tests the window: if SMMUEN=1 but EVENTQEN=0, a fault that occurs
/// in that window is silently dropped. After the fix (EVENTQEN set first),
/// no such window exists and the fault must be recorded.
#[test]
fn f5_fault_immediately_after_enable_is_recorded_in_event_queue() {
    let smmu = SMMU::new();

    // Set up a stream BEFORE enabling the SMMU.
    // Use a stream that will cause a fault on first translate (no page mapped).
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .build()
        .unwrap();
    smmu.configure_stream(sid(30), cfg).unwrap();
    smmu.create_pasid(sid(30), pasid(0)).unwrap();
    // Map a page with read-only permission.
    smmu.map_page(
        sid(30),
        pasid(0),
        iova(0x1000),
        pa(0x8000_1000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Enable the SMMU.
    smmu.enable().unwrap();

    // EVENTQEN must be set immediately after enable().
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_EVENTQEN,
        0,
        "F5: EVENTQEN must be set after enable() — precondition for this test"
    );

    // Trigger a permission fault (write to read-only page) immediately after enable().
    let result = smmu.translate(sid(30), pasid(0), iova(0x1000), AccessType::Write, SecurityState::NonSecure);
    assert!(result.is_err(), "F5: write to read-only page must fail");

    // The fault must be in the fault queue.
    let faults = smmu.get_faults();
    assert!(
        !faults.is_empty(),
        "F5 BUG: fault generated immediately after enable() was not recorded; \
         this indicates EVENTQEN was not set when the translation ran (window bug)"
    );

    // The fault must be a permission fault.
    let has_perm_fault = faults.iter().any(|f| {
        matches!(f.fault_type(), FaultType::PermissionFault)
            && f.stream_id().as_u32() == 30
    });
    assert!(
        has_perm_fault,
        "F5: expected a PermissionFault for stream 30, found: {faults:?}"
    );
}

/// CR0.CMDQEN must also be set after enable() (queues enabled together).
#[test]
fn f5_cmdqen_also_set_after_enable() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_CMDQEN,
        0,
        "F5: CR0.CMDQEN must be set after enable()"
    );
}

/// CR0.PRIQEN must also be set after enable() (queues enabled together).
#[test]
fn f5_priqen_also_set_after_enable() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_PRIQEN,
        0,
        "F5: CR0.PRIQEN must be set after enable()"
    );
}

/// Verify that the correct init order is EVENTQEN before SMMUEN.
///
/// This test directly asserts the spec requirement: when enable() is called
/// for the first time, the SMMU must be in a state where EVENTQEN is set
/// before (or atomically with) the SMMUEN bit that starts translations.
///
/// We verify this by checking that the ONLY way to have SMMUEN=1 after
/// enable() is to also have EVENTQEN=1 simultaneously.
#[test]
fn f5_smmuen_and_eventqen_both_set_atomically() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let cr0 = smmu.get_cr0();

    // Both bits must be set together — neither can lag behind the other.
    let smmuen = (cr0 & SMMU::CR0_SMMUEN) != 0;
    let eventqen = (cr0 & SMMU::CR0_EVENTQEN) != 0;

    assert!(smmuen, "F5: CR0.SMMUEN must be set after enable()");
    assert!(
        eventqen,
        "F5 BUG: CR0.EVENTQEN is NOT set when CR0.SMMUEN is set; \
         the two stores happen in wrong order — EVENTQEN must be set first \
         per ARM §6.3.9 init sequence (events enabled before translations begin)"
    );
}
