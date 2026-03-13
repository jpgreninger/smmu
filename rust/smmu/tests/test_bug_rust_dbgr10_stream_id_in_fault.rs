//! Tests for BUG-RUST-DBGR-10 — StreamID stored on StreamContext (§7.3)
#![allow(clippy::doc_markdown)]
//!
//! BUG-RUST-DBGR-10 originally verified that fault records at the StreamContext
//! level carry the correct (non-zero) StreamID.  After the BUG-RUST-TWOSTAGE-S1-
//! FAULT-CLASS fix, StreamContext no longer records faults internally — all fault
//! recording is delegated to the top-level SMMU caller via `record_translation_fault()`
//! (§3.12.2: "a single fault record per transaction").
//!
//! These tests now verify:
//!   (a) The StreamContext correctly stores and exposes the stream_id set on it.
//!   (b) StreamContext.fault_records is empty after a translation fault
//!       (fault recording is the SMMU caller's responsibility).

use smmu::stream_context::StreamContext;
use smmu::types::{AccessType, SecurityState, StreamConfig, IOVA, PASID};

/// StreamContext correctly stores the stream_id set via set_stream_id().
/// After BUG-RUST-TWOSTAGE-S1-FAULT-CLASS fix, fault records are no longer
/// written to StreamContext.fault_records — the SMMU caller records the fault.
#[test]
fn dbgr10_fault_record_has_nonzero_stream_id() {
    let ctx = StreamContext::new();
    let cfg = StreamConfig::stage1_only();
    ctx.update_configuration(cfg);

    // Set the stream_id on the StreamContext
    let expected_stream_id: u32 = 42;
    ctx.set_stream_id(expected_stream_id);

    let pasid = PASID::new(0).unwrap();
    ctx.create_pasid(pasid).unwrap();

    // Trigger a translation fault (no page mapped)
    let iova = IOVA::new(0x1000).unwrap();
    let result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "translate must return an error (PageNotMapped)");

    // After the BUG-RUST-TWOSTAGE-S1-FAULT-CLASS fix, StreamContext does NOT
    // record faults internally.  Fault recording is the SMMU caller's responsibility
    // (§3.12.2).  The stream_id is still stored on the context for use by the caller.
    let records = ctx.get_fault_records();
    assert!(
        records.is_empty(),
        "StreamContext must not record faults internally (SMMU caller records them); \
         got {} record(s)",
        records.len()
    );

    // The stream_id set on the context must still be accessible for the caller.
    assert_eq!(
        ctx.get_stream_id(),
        expected_stream_id,
        "set_stream_id must persist; expected {expected_stream_id}, got {}",
        ctx.get_stream_id()
    );
}

/// After BUG-RUST-TWOSTAGE-S1-FAULT-CLASS fix, StreamContext.fault_records
/// is always empty — all fault records go through the SMMU-level caller.
#[test]
fn dbgr10_fault_record_stream_id_not_placeholder_zero() {
    let ctx = StreamContext::new();
    ctx.update_configuration(StreamConfig::stage1_only());
    ctx.set_stream_id(7);
    let pasid = PASID::new(0).unwrap();
    ctx.create_pasid(pasid).unwrap();
    let result = ctx.translate(pasid, IOVA::new(0x2000).unwrap(), AccessType::Write, SecurityState::NonSecure);
    assert!(result.is_err(), "translate must return an error (PageNotMapped)");

    // No fault records in StreamContext — SMMU caller is responsible for recording.
    let records = ctx.get_fault_records();
    assert!(
        records.is_empty(),
        "StreamContext must not record faults internally after the fix; \
         got {} record(s)",
        records.len()
    );
}
