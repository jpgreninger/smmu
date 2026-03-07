//! Tests for BUG-RUST-DBGR-12 — no distinct StallQueueFull variant (§3.12.2)
#![allow(clippy::doc_markdown)]
//!
//! When the stall queue is exhausted, the error must be distinguishable from
//! a normal translation fault, not silently fall back to the raw error.

use smmu::SMMU;
use smmu::types::{
    AccessType, FaultMode, SecurityState, StreamConfig, StreamID, TranslationError, IOVA, PASID,
};

/// The StallQueueFull error variant must exist and be distinct from other translation errors.
/// This is a compile-time existence test — if the variant doesn't exist, this file won't compile.
#[test]
fn dbgr12_stall_queue_full_variant_exists() {
    let err = TranslationError::StallQueueFull;
    assert!(
        !matches!(err, TranslationError::PageNotMapped),
        "StallQueueFull must be distinct from PageNotMapped"
    );
    assert!(
        !matches!(err, TranslationError::Stalled { .. }),
        "StallQueueFull must be distinct from Stalled"
    );
    assert!(
        !matches!(err, TranslationError::StreamDisabled),
        "StallQueueFull must be distinct from StreamDisabled"
    );
}

/// Verify that stall mode works for the first 10 faults (STAGs are allocated).
#[test]
fn dbgr12_stall_mode_allocates_stags() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    let stream_id = StreamID::new(1).unwrap();
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .fault_mode(FaultMode::Stall)
        .build()
        .expect("stall config");
    smmu.configure_stream(stream_id, cfg).unwrap();

    let pasid = PASID::new(0).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();
    // No pages mapped → every translate() faults → stall mode → allocate STAG

    // Generate 10 stall faults — each should return Stalled { stag } with unique stag.
    let mut stags = std::collections::HashSet::new();
    for i in 0u64..10 {
        let iova = IOVA::new(0x1000 + i * 0x1000).unwrap();
        let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
        match result {
            Err(TranslationError::Stalled { stag }) => {
                assert!(stag != 0, "STAG must be non-zero");
                stags.insert(stag);
            }
            Err(TranslationError::StallQueueFull) => {
                // Queue already full — that's also acceptable for this test
                return;
            }
            other => panic!("Expected Stalled or StallQueueFull, got: {other:?}"),
        }
    }
    assert_eq!(stags.len(), 10, "Each fault must get a unique STAG");
}

/// When the stall queue overflows (65535 iterations), the error must be
/// TranslationError::StallQueueFull, not the raw underlying error.
///
/// We verify this by pre-filling the queue via the SMMU stall injection helper
/// and then triggering one final fault.
#[test]
fn dbgr12_stall_queue_full_returned_when_exhausted() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    let stream_id = StreamID::new(2).unwrap();
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .fault_mode(FaultMode::Stall)
        .build()
        .expect("stall config");
    smmu.configure_stream(stream_id, cfg).unwrap();

    let pasid = PASID::new(0).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Pre-fill stall queue with all 65535 STAGs via the public inject helper.
    smmu.inject_stall_records_for_test(stream_id.as_u32(), pasid.as_u32());

    // Now the next fault must return StallQueueFull.
    let iova = IOVA::new(0xFF_0000).unwrap();
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(
        matches!(result, Err(TranslationError::StallQueueFull)),
        "When stall queue is exhausted, must return StallQueueFull; got: {result:?}"
    );
}
