//! Tests for BUG-RUST-DBGR-10 — StreamID missing from StreamContext fault records (§7.3)
#![allow(clippy::doc_markdown)]
//!
//! Fault records generated at the StreamContext level must include the StreamID
//! of the requesting stream, not 0 or a placeholder.

use smmu::stream_context::StreamContext;
use smmu::types::{AccessType, SecurityState, StreamConfig, IOVA, PASID};

/// Fault records generated inside translate() must have non-zero stream_id
/// when the StreamContext has been configured with a real stream_id.
///
/// With the current bug, the stream_id field in fault records is always 0.
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
    let _ = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);

    let records = ctx.get_fault_records();
    assert!(!records.is_empty(), "At least one fault record must exist");

    let record = &records[0];
    assert_eq!(
        record.stream_id().as_u32(),
        expected_stream_id,
        "Fault record stream_id must be {expected_stream_id}, got {}",
        record.stream_id().as_u32()
    );
}

/// Fault records must not use StreamID=0 as a placeholder.
#[test]
fn dbgr10_fault_record_stream_id_not_placeholder_zero() {
    let ctx = StreamContext::new();
    ctx.update_configuration(StreamConfig::stage1_only());
    ctx.set_stream_id(7);
    let pasid = PASID::new(0).unwrap();
    ctx.create_pasid(pasid).unwrap();
    let _ = ctx.translate(pasid, IOVA::new(0x2000).unwrap(), AccessType::Write, SecurityState::NonSecure);

    let records = ctx.get_fault_records();
    assert!(!records.is_empty());
    assert_ne!(
        records[0].stream_id().as_u32(),
        0,
        "Fault record must not use placeholder StreamID=0"
    );
}
