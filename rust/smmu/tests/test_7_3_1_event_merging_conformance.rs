#![allow(clippy::doc_markdown)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::cast_possible_truncation)]
#![allow(missing_docs)]

//! §7.3.1 Event Record Merging Conformance Tests
//!
//! Spec: ARM IHI0070G.b §7.3.1
//!
//! "Events with a Stall parameter are never merged if Stall == 1."
//!
//! BUG-7.3.1-01: When `stream_mev == true`, the MEV dedup guard in
//! `record_translation_fault()` at line 5912 checks:
//!
//! ```text
//! if stream_mev && queue.iter().any(|e| e.type == event.type && e.stream_id == ... && e.pasid == ...)
//!     { return; }   // drops the event
//! ```
//!
//! This guard does NOT check `event.stall`. Consequently, when MEV=true and a stall
//! event (Stall==1) shares (type, stream_id, pasid) with a prior queued event (which
//! may be an earlier stall event), the second stall event is silently dropped — a
//! direct violation of §7.3.1: "Events with Stall=1 are never merged."
//!
//! Fix: Add `&& !event.stall` to the MEV dedup predicate.

use smmu::types::{
    AccessType, FaultMode, PagePermissions, QueueConfig, SMMUConfig, SecurityState, StreamConfig,
    StreamID, IOVA, PA, PASID,
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

/// Build an SMMU with event queue capacity `size`.
fn make_smmu(size: usize) -> SMMU {
    let config = SMMUConfig::from(QueueConfig {
        event_queue_size: size,
        ..Default::default()
    });
    let smmu = SMMU::with_config(config);
    smmu.enable().unwrap();
    smmu
}

/// Configure a stream with MEV=true and stall fault mode; map one valid page.
/// Accesses to unmapped addresses will produce stall faults.
fn setup_mev_stall_stream(smmu: &SMMU, stream_n: u32) {
    let s = sid(stream_n);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .security_enforced(false)
        .fault_mode(FaultMode::Stall)
        .mev(true)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    smmu.map_page(
        s,
        pasid(0),
        iova(0x1000),
        pa(0x8000_1000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();
}

/// Configure a stream with MEV=true and terminate fault mode; map one valid page.
fn setup_mev_terminate_stream(smmu: &SMMU, stream_n: u32) {
    let s = sid(stream_n);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .security_enforced(false)
        .fault_mode(FaultMode::Terminate)
        .mev(true)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    smmu.map_page(
        s,
        pasid(0),
        iova(0x1000),
        pa(0x8000_1000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();
}

/// BUG-7.3.1-01: §7.3.1 — stall events (Stall==1) must NEVER be merged/suppressed,
/// even when STE.MEV==1 and a stall event of the same type/stream/pasid is already
/// in the queue.
///
/// Scenario:
/// - Stream 1: MEV=true, stall fault mode.
/// - Trigger a stall fault at 0x2000 (first): stall event enters queue (space available).
/// - Trigger a second stall fault at the same 0x2000 (same type/stream/pasid).
/// - Before fix: the second stall event is silently dropped by the MEV dedup guard.
/// - After fix:  the second stall event is recorded (stall events must never be merged).
///
/// We verify by checking that `get_event_count()` increases after the second fault.
#[test]
fn bug_7_3_1_stall_event_not_suppressed_by_mev_dedup() {
    // Use a large queue so both stall events fit without hitting overflow.
    let smmu = make_smmu(16);
    setup_mev_stall_stream(&smmu, 1);

    // First stall fault at unmapped address 0x2000.
    let r1 = smmu.translate(
        sid(1),
        pasid(0),
        iova(0x2000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(r1.is_err(), "expected fault at unmapped address");

    let count_after_first = smmu.get_events().len();

    // Second stall fault — same address/type/stream/pasid (identical event).
    // With MEV=true, the dedup guard matches the first stall event in the queue.
    // BUG: without the `!event.stall` guard, this is silently dropped.
    let r2 = smmu.translate(
        sid(1),
        pasid(0),
        iova(0x2000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(r2.is_err(), "expected fault at unmapped address");

    let count_after_second = smmu.get_events().len();

    // §7.3.1: stall events are NEVER merged — both must be recorded.
    assert!(
        count_after_second > count_after_first,
        "§7.3.1 violation: stall event (Stall==1) was illegally suppressed by MEV dedup guard. \
         Event count must increase after second stall fault; \
         was {count_after_first} after first, {count_after_second} after second.",
    );
}

/// Positive case: MEV=true correctly suppresses duplicate non-stall (terminate) events.
///
/// This verifies the fix does not regress the normal MEV merging behavior.
#[test]
fn mev_dedup_still_suppresses_terminate_duplicates() {
    let smmu = make_smmu(16);
    setup_mev_terminate_stream(&smmu, 1);

    // First terminate fault.
    let _ = smmu.translate(
        sid(1),
        pasid(0),
        iova(0x2000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    let count_after_first = smmu.get_events().len();

    // Second identical terminate fault — should be suppressed by MEV dedup.
    let _ = smmu.translate(
        sid(1),
        pasid(0),
        iova(0x2000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    let count_after_second = smmu.get_events().len();

    // MEV=true, Stall==0 → duplicate must be suppressed (no new event).
    assert_eq!(
        count_after_second, count_after_first,
        "MEV=true must suppress duplicate non-stall (terminate) events; \
         count must not increase from {count_after_first} (got {count_after_second})",
    );
}

/// §7.3.1: MEV=false → all events including duplicates must be recorded.
#[test]
fn mev_false_all_events_recorded() {
    let smmu = make_smmu(16);

    let s = sid(2);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .security_enforced(false)
        .fault_mode(FaultMode::Terminate)
        .mev(false)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    smmu.map_page(
        s,
        pasid(0),
        iova(0x1000),
        pa(0x8000_1000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let _ = smmu.translate(s, pasid(0), iova(0x2000), AccessType::Read, SecurityState::NonSecure);
    let count_after_first = smmu.get_events().len();

    let _ = smmu.translate(s, pasid(0), iova(0x2000), AccessType::Read, SecurityState::NonSecure);
    let count_after_second = smmu.get_events().len();

    // MEV=false: dedup disabled — both events must be recorded.
    assert!(
        count_after_second > count_after_first,
        "MEV=false must record all events including duplicates; \
         count must increase from {count_after_first} (got {count_after_second})",
    );
}
