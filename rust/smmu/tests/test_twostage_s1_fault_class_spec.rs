#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::doc_lazy_continuation)]
#![allow(clippy::uninlined_format_args)]

//! TDD tests for BUG-RUST-TWOSTAGE-S1-FAULT-CLASS
//!
//! ARM IHI0070G.b §7.3.14 (F_ADDR_SIZE ≠ F_TRANSLATION) + §3.12.2
//! ("single fault record per transaction").
//!
//! Problem 1 — Wrong fault type:
//!   In `translate_two_stage()` the Stage-1 error match arm
//!   `_ => FaultType::TranslationFault` catches `SecurityViolation` (and any
//!   other non-permission, non-notmapped error) and misclassifies it as
//!   `TranslationFault`.  Per §7.3.14, a security violation must be classified
//!   as `SecurityFault`, not `TranslationFault`.
//!
//! Problem 2 — Double fault record per transaction:
//!   `translate_two_stage()` calls `record_fault_internal()` (writes to the
//!   stream-context's `fault_records`) and then returns `Err(error)`.  The
//!   caller `smmu/mod.rs::translate()` catches that error and calls
//!   `record_translation_fault()` (writes to the SMMU-level `fault_queue`).
//!   This produces **two** fault records for a single transaction, violating
//!   §3.12.2.
//!
//! Fix:
//!   1. Add explicit match arms for `TranslationError::SecurityViolation` →
//!      `FaultType::SecurityFault` (and similarly for other variants in the
//!      `smmu/mod.rs::map_translation_error_to_fault_type` mapping).
//!   2. Remove the `record_fault_internal` call from the Stage-1 error path
//!      in `translate_two_stage()` so that only `smmu/mod.rs` records the
//!      fault via `record_translation_fault()`.  The same applies to the
//!      Stage-2 error path and the matching paths in
//!      `translate_stage1_only()` / `translate_stage2_only()`.

use smmu::address_space::AddressSpace;
use smmu::stream_context::StreamContext;
use smmu::types::{
    AccessType, PagePermissions, SecurityState, TranslationError, IOVA, PA, PASID,
};
use std::sync::Arc;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn pasid(n: u32) -> PASID {
    PASID::new(n).unwrap()
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

/// Build a two-stage StreamContext with:
///   Stage-1: IOVA 0x1000 → IPA 0x2000, perms=RW, security=Secure
///   Stage-2: IPA 0x2000 → PA 0x3000, perms=RW, security=NonSecure
///
/// The mismatch between Stage-1 mapping security (Secure) and the NonSecure
/// security state used in the S2 mapping is intentional: the Stage-2 uses
/// NonSecure so that a NonSecure translate call succeeds Stage-2 lookup.
/// Passing `SecurityState::NonSecure` to `translate()` on this context with
/// a Stage-1 page mapped as Secure will trigger a `SecurityViolation` in
/// Stage-1 translation.
fn setup_two_stage_ctx() -> StreamContext {
    let ctx = StreamContext::new();
    ctx.set_stage1_enabled(true);
    ctx.set_stage2_enabled(true);
    ctx.enable();

    // Stage-1: IOVA 0x1000 → IPA 0x2000, mapped as Secure.
    ctx.create_pasid(pasid(0)).unwrap();
    ctx.map_page(
        pasid(0),
        iova(0x1000),
        pa(0x2000),
        PagePermissions::read_write(),
        SecurityState::Secure,
    )
    .unwrap();

    // Stage-2: IPA 0x2000 → PA 0x3000, mapped as NonSecure.
    ctx.create_stage2_address_space().unwrap();
    ctx.map_stage2_page(
        iova(0x2000),
        pa(0x3000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    ctx
}

// ── Test 1: correct error type propagated; no double-record in stream-context ─
//
// Trigger a Stage-1 SecurityViolation in a two-stage translation.
// After the BUG-RUST-TWOSTAGE-S1-FAULT-CLASS fix, `translate_two_stage()`
// does NOT call `record_fault_internal()` for Stage-1 errors; the single
// canonical fault record is written by the top-level `smmu/mod.rs` caller
// via `record_translation_fault()` using the correct
// `map_translation_error_to_fault_type()` mapping.
//
// This test verifies two things:
//   (a) The correct error variant (SecurityViolation) is returned to the
//       caller — confirming the error path is not silenced or misclassified
//       at the translate_two_stage boundary.
//   (b) The stream-context `fault_records` buffer has 0 entries — confirming
//       that the stream-context-level double-record has been eliminated
//       (§3.12.2: "a single fault record per transaction").
//
// BEFORE FIX: stream-context has 1 record (from `record_fault_internal`),
//             but fault_type == TranslationFault (wrong).  Both the wrong
//             type and the duplicate violate the spec.
// AFTER FIX:  record_fault_internal removed → 0 records in stream-context;
//             SecurityViolation is returned correctly → assertions PASS.
#[test]
fn two_stage_s1_security_violation_records_security_fault_not_translation_fault() {
    let ctx = setup_two_stage_ctx();

    // Translate with NonSecure; Stage-1 page is Secure → SecurityViolation.
    let result = ctx.translate(pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(
        matches!(result, Err(TranslationError::SecurityViolation)),
        "expected SecurityViolation from Stage-1, got {:?}",
        result
    );

    // After the fix: stream-context must NOT record faults for Stage-1 errors
    // in two-stage mode.  The SMMU-level caller records the single canonical
    // fault (§3.12.2).  Recording here would create a duplicate.
    let records = ctx.get_fault_records();
    assert_eq!(
        records.len(),
        0,
        "stream-context must NOT record faults for Stage-1 errors in two-stage \
         mode (SMMU-level handles the single canonical fault record per §3.12.2); \
         got {} record(s)",
        records.len()
    );
}

// ── Test 2: double fault record per transaction ───────────────────────────────
//
// For a single faulting two-stage translation the stream-context's
// `fault_records` must contain exactly ONE record (it currently contains one,
// but after the fix the `record_fault_internal` call is removed from the Stage-1
// error path so the stream-context records ZERO — the SMMU level records the
// single fault via `record_translation_fault`).
//
// We test the stream-context directly: after the fix no fault is recorded in the
// stream-context `fault_records` for a Stage-1 error in two-stage mode, because
// only the top-level SMMU caller should record it.
//
// BEFORE FIX: stream-context has 1 record (the duplicate).
//             We verify count == 0 → assertion FAILS.
// AFTER FIX:  record_fault_internal removed from two-stage S1 error path →
//             stream-context has 0 records → assertion PASSES.
#[test]
fn two_stage_s1_fault_does_not_double_record_in_stream_context() {
    let ctx = setup_two_stage_ctx();

    // Trigger Stage-1 fault in two-stage mode.
    let _ = ctx.translate(pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);

    // After the fix, stream context must NOT record faults internally for errors
    // that the SMMU-level caller (`smmu/mod.rs`) records via record_translation_fault.
    // This prevents the double-recording violation of §3.12.2.
    let records = ctx.get_fault_records();
    assert_eq!(
        records.len(),
        0,
        "stream-context must NOT record faults for Stage-1 errors in two-stage mode \
         (SMMU-level handles the single canonical fault record); got {} record(s)",
        records.len()
    );
}

// ── Test 3: stage-2 error path — same double-recording issue ─────────────────
//
// Build a two-stage context where Stage-1 maps a page successfully but
// Stage-2 is not configured with the IPA → triggers StreamNotConfigured from
// Stage-2 path.  Verify no duplicate record in stream-context.
//
// BEFORE FIX: stream-context has 1 record from `record_fault_internal` in
//             the Stage-2 error branch.  We assert == 0 → FAILS.
// AFTER FIX:  record_fault_internal removed → 0 records in ctx → PASSES.
#[test]
fn two_stage_s2_fault_does_not_double_record_in_stream_context() {
    let ctx = StreamContext::new();
    ctx.set_stage1_enabled(true);
    ctx.set_stage2_enabled(true);
    ctx.enable();

    // Stage-1: IOVA 0x1000 → IPA 0x2000 (Secure).
    ctx.create_pasid(pasid(0)).unwrap();
    ctx.map_page(
        pasid(0),
        iova(0x1000),
        pa(0x2000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Stage-2 address space created but NO page mapped at IPA 0x2000.
    ctx.create_stage2_address_space().unwrap();

    // Translate: Stage-1 OK → IPA 0x2000, Stage-2 lookup fails (PageNotMapped).
    let result = ctx.translate(pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_err(),
        "expected Stage-2 fault, got Ok"
    );

    let records = ctx.get_fault_records();
    assert_eq!(
        records.len(),
        0,
        "stream-context must NOT record faults for Stage-2 errors in two-stage mode; \
         got {} record(s)",
        records.len()
    );
}

// ── Test 4: stage1-only path — same double-recording issue ───────────────────
//
// Build a stage-1-only context, trigger a PageNotMapped fault.  Verify no
// record in stream-context after the fix (SMMU level records it).
//
// BEFORE FIX: ctx.get_fault_records().len() == 1 → assert == 0 FAILS.
// AFTER FIX:  0 → PASSES.
#[test]
fn stage1_only_fault_does_not_double_record_in_stream_context() {
    let ctx = StreamContext::new();
    ctx.set_stage1_enabled(true);
    ctx.set_stage2_enabled(false);
    ctx.enable();

    ctx.create_pasid(pasid(0)).unwrap();
    // No page mapped → PageNotMapped fault.

    let result = ctx.translate(pasid(0), iova(0x5000), AccessType::Read, SecurityState::NonSecure);
    assert!(
        matches!(result, Err(TranslationError::PageNotMapped)),
        "expected PageNotMapped, got {:?}",
        result
    );

    let records = ctx.get_fault_records();
    assert_eq!(
        records.len(),
        0,
        "stream-context must NOT record faults for Stage-1-only errors; \
         got {} record(s)",
        records.len()
    );
}

// ── Test 5: stage2-only path — same double-recording issue ───────────────────
//
// Build a stage-2-only context, trigger a PageNotMapped fault.  Verify no
// record in stream-context after the fix.
//
// BEFORE FIX: ctx.get_fault_records().len() == 1 → assert == 0 FAILS.
// AFTER FIX:  0 → PASSES.
#[test]
fn stage2_only_fault_does_not_double_record_in_stream_context() {
    let ctx = StreamContext::new();
    ctx.set_stage1_enabled(false);
    ctx.set_stage2_enabled(true);
    ctx.enable();

    // Create stage-2 address space but map nothing.
    let s2 = Arc::new(AddressSpace::new());
    ctx.set_stage2_address_space(Some(s2));

    // pasid(0) needed even for stage-2-only because translate() checks non-zero PASID.
    // Don't create a PASID (stage-2-only ignores PASID map; PASID must be 0).
    let result = ctx.translate(pasid(0), iova(0x6000), AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_err(),
        "expected error (stage-2 page not mapped), got Ok"
    );

    let records = ctx.get_fault_records();
    assert_eq!(
        records.len(),
        0,
        "stream-context must NOT record faults for Stage-2-only errors; \
         got {} record(s)",
        records.len()
    );
}
