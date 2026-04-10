//! Conformance tests for ARM SMMU v3 §7.3.22 "Event queue record priorities".
//!
//! §7.3.22 defines the priority ordering for events raised during transaction
//! processing, and lists the four structural-fetch fault types that can be
//! injected externally:
//!   F_STE_FETCH  (§7.3.5)  — inject_ste_fetch_abort()
//!   F_CD_FETCH   (§7.3.10) — inject_cd_fetch_abort()
//!   F_VMS_FETCH  (§7.3.20) — inject_vms_fetch_abort()  ← BUG-7322-01: missing
//!   F_WALK_EABT  (§7.3.12) — inject_walk_eabt()
//!
//! BUG-7322-01: F_VMS_FETCH (event 0x25) event type is defined in EventType
//! but no inject_vms_fetch_abort() method exists on the SMMU, making it
//! impossible to simulate a VMS fetch external abort per §7.3.20.
#![allow(missing_docs)]
#![allow(clippy::doc_markdown)]
use smmu::types::{EventType, StreamID, PASID};
use smmu::SMMU;

fn make_sid(id: u32) -> StreamID {
    StreamID::new(id).unwrap()
}

fn make_pasid(id: u32) -> PASID {
    PASID::new(id).unwrap()
}

// ============================================================================
// BUG-7322-01: inject_vms_fetch_abort() — §7.3.20 / §7.3.22
// ============================================================================

/// §7.3.20 / §7.3.22: inject_vms_fetch_abort() with EVENTQEN=1 must enqueue
/// an FVmsFetch event with the correct stream_id and pasid fields.
#[test]
fn test_7322_vms_fetch_abort_event_enqueued() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_EVENTQEN);

    let sid = make_sid(7);
    let pasid = make_pasid(3);
    smmu.inject_vms_fetch_abort(sid, pasid);

    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "§7.3.20: Event queue must be non-empty after VMS fetch abort injection"
    );
    assert_eq!(
        events[0].event_type,
        EventType::FVmsFetch,
        "§7.3.20: Injected event must be FVmsFetch (0x25)"
    );
    assert_eq!(
        events[0].stream_id, 7,
        "§7.3.20: Event stream_id must match injected stream_id"
    );
    assert_eq!(
        events[0].pasid, 3,
        "§7.3.20: Event pasid must match injected pasid"
    );
}

/// §7.3.20: inject_vms_fetch_abort() with EVENTQEN=0 must not enqueue any event.
#[test]
fn test_7322_vms_fetch_abort_no_event_when_eventqen_zero() {
    let smmu = SMMU::new();
    smmu.set_cr0(0); // EVENTQEN=0

    let sid = make_sid(8);
    let pasid = make_pasid(0);
    smmu.inject_vms_fetch_abort(sid, pasid);

    let events = smmu.get_events();
    assert!(
        events.is_empty(),
        "§7.3.20: No events must be recorded when EVENTQEN=0"
    );
}

/// §7.3.20: inject_vms_fetch_abort() on a substream-capable stream must set
/// SSV=1 in the event record (SubstreamID was presented).
#[test]
fn test_7322_vms_fetch_abort_ssv_substream_capable() {
    use smmu::types::{FaultMode, StreamConfig};
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_EVENTQEN);

    // Configure a substream-capable stream (s1cd_max=1 → supports PASID[0])
    let sid = make_sid(9);
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .stage2_enabled(false)
        .s1cd_max(1)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(sid, cfg).unwrap();

    let pasid = make_pasid(0);
    smmu.inject_vms_fetch_abort(sid, pasid);

    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "§7.3.20: VMS fetch abort on substream-capable stream must produce event"
    );
    assert_eq!(events[0].event_type, EventType::FVmsFetch);
    assert!(
        events[0].ssv,
        "§7.3.20: SSV must be 1 on a substream-capable stream (s1cd_max > 0)"
    );
}

/// §7.3.20: inject_vms_fetch_abort() on a simple (non-substream-capable) stream
/// must set SSV=0 in the event record.
#[test]
fn test_7322_vms_fetch_abort_ssv_simple_stream() {
    use smmu::types::{FaultMode, StreamConfig};
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_EVENTQEN);

    let sid = make_sid(10);
    let cfg = StreamConfig::builder()
        .stage1_enabled(false)
        .stage2_enabled(false)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(sid, cfg).unwrap();

    let pasid = make_pasid(0);
    smmu.inject_vms_fetch_abort(sid, pasid);

    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "§7.3.20: VMS fetch abort on simple stream must produce event"
    );
    assert_eq!(events[0].event_type, EventType::FVmsFetch);
    assert!(
        !events[0].ssv,
        "§7.3.20: SSV must be 0 on a non-substream-capable stream"
    );
}
