//! Tests for BUG-RUST-DBGR-9 — StreamContext fault record not discarded (§7.1/§3.5.3)
//!
//! BUG-RUST-DBGR-9 originally verified that fault records written to
//! StreamContext.fault_records were not silently dropped due to `try_write()`
//! lock contention (the fix changed `try_write()` to `write()`).
//!
//! After the BUG-RUST-TWOSTAGE-S1-FAULT-CLASS architectural fix, StreamContext
//! no longer records faults internally — all fault recording is delegated to the
//! top-level SMMU caller via `record_translation_fault()` (§3.12.2).  The
//! try_write/write concern is therefore moot at the StreamContext level.
//!
//! These tests now verify that StreamContext correctly propagates translation
//! errors to the caller (so the SMMU-level can record them), and that the
//! StreamContext.fault_records buffer remains empty (no internal recording).
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::stream_context::StreamContext;
use smmu::types::{AccessType, SecurityState, StreamConfig, TranslationError, IOVA, PASID};

/// All translation errors must be returned to the caller (no silent drops).
/// StreamContext.fault_records must remain empty — recording is the caller's job.
#[test]
fn dbgr9_fault_records_not_dropped_by_lock_contention() {
    let ctx = StreamContext::new();
    let cfg = StreamConfig::stage1_only();
    ctx.update_configuration(cfg);
    let pasid = PASID::new(0).unwrap();
    ctx.create_pasid(pasid).unwrap();

    // Generate N faults (no page mapped → PageNotMapped for each).
    let n: usize = 50;
    let mut errors_returned: usize = 0;
    for i in 0..n {
        let iova = IOVA::new(0x1000 + i as u64 * 0x1000).unwrap();
        if ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure).is_err() {
            errors_returned += 1;
        }
    }

    // All errors must be returned to the caller — none silently dropped.
    assert_eq!(
        errors_returned, n,
        "All {n} translation errors must be returned to the caller (none silently dropped), \
         but only {errors_returned} were"
    );

    // StreamContext must NOT record faults internally (SMMU caller is responsible).
    let fault_count = ctx.get_fault_count();
    assert_eq!(
        fault_count, 0,
        "StreamContext must have 0 internal fault records after the fix; got {fault_count}"
    );
}

/// A single translation error must be returned to the caller.
/// StreamContext.fault_records must remain empty.
#[test]
fn dbgr9_single_fault_always_recorded() {
    let ctx = StreamContext::new();
    let cfg = StreamConfig::stage1_only();
    ctx.update_configuration(cfg);
    let pasid = PASID::new(0).unwrap();
    ctx.create_pasid(pasid).unwrap();

    let iova = IOVA::new(0x1000).unwrap();
    let result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(
        matches!(result, Err(TranslationError::PageNotMapped)),
        "translate must return PageNotMapped; got {:?}",
        result
    );

    // StreamContext must NOT record faults internally after the fix.
    let fault_count = ctx.get_fault_count();
    assert_eq!(
        fault_count, 0,
        "StreamContext must have 0 internal fault records (SMMU caller records the fault); \
         got {fault_count}"
    );
}
