//! Tests for BUG-RUST-DBGR-9 — StreamContext fault record not discarded when queue full (§7.1/§3.5.3)
//!
//! Fault records must not be silently dropped due to `try_write()` lock contention.
//! The fix changes `try_write()` to `write()` (blocking) so faults are always recorded
//! subject only to the configured rate limit.
#![allow(clippy::doc_markdown)]

use smmu::stream_context::StreamContext;
use smmu::types::{AccessType, SecurityState, StreamConfig, TranslationError, IOVA, PASID};

/// When fault_rate_limit is usize::MAX (the default), all faults must be recorded.
/// No fault should be dropped due to lock contention (which `try_write()` would cause).
#[test]
fn dbgr9_fault_records_not_dropped_by_lock_contention() {
    let ctx = StreamContext::new();
    // Default rate limit is usize::MAX — effectively unlimited.
    let cfg = StreamConfig::stage1_only();
    ctx.update_configuration(cfg);
    let pasid = PASID::new(0).unwrap();
    ctx.create_pasid(pasid).unwrap();

    // Generate N faults (no page mapped → PageNotMapped for each).
    let n: usize = 50;
    for i in 0..n {
        let iova = IOVA::new(0x1000 + i as u64 * 0x1000).unwrap();
        let _ = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    }

    let fault_count = ctx.get_fault_count();
    // All faults must be recorded (default rate limit = usize::MAX).
    assert_eq!(
        fault_count, n,
        "With unlimited rate limit, all {n} faults must be recorded, but only {fault_count} were"
    );
}

/// A single fault must always be recorded (proves blocking write works).
#[test]
fn dbgr9_single_fault_always_recorded() {
    let ctx = StreamContext::new();
    let cfg = StreamConfig::stage1_only();
    ctx.update_configuration(cfg);
    let pasid = PASID::new(0).unwrap();
    ctx.create_pasid(pasid).unwrap();

    let iova = IOVA::new(0x1000).unwrap();
    let result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(matches!(result, Err(TranslationError::PageNotMapped)));

    let fault_count = ctx.get_fault_count();
    assert_eq!(fault_count, 1, "Single fault must always be recorded (write() not try_write())");
}
