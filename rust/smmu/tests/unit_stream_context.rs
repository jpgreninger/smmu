#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(clippy::uninlined_format_args)]

//! Unit tests for `StreamContext` module
//!
//! Tests per-stream state management including PASID creation/removal,
//! translation, and state machine transitions per ARM SMMU v3 specification.

use smmu::stream_context::StreamContext;
use smmu::types::{
    AccessType, PagePermissions, SecurityState, StreamConfig, StreamContextError, TranslationError, IOVA, PA,
    PAGE_SIZE, PASID,
};

#[allow(unused_imports)]
use std::sync::{Arc, RwLock};

// ============================================================================
// Construction Tests
// ============================================================================

#[test]
fn test_new_stream_context() {
    let stream_context = StreamContext::new();
    assert!(stream_context.is_stage1_enabled());
    assert!(!stream_context.is_stage2_enabled());
}

#[test]
fn test_default_stream_context() {
    let stream_context = StreamContext::default();
    assert!(stream_context.is_stage1_enabled());
}

// ============================================================================
// PASID Creation Tests
// ============================================================================

#[test]
fn test_create_pasid() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();

    let result = stream_context.create_pasid(pasid);
    assert!(result.is_ok());
    assert!(stream_context.has_pasid(pasid));
}

#[test]
fn test_create_pasid_zero() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(0).unwrap();

    let result = stream_context.create_pasid(pasid);
    assert!(result.is_ok());
    assert!(stream_context.has_pasid(pasid));
}

#[test]
fn test_create_multiple_pasids() {
    let stream_context = StreamContext::new();

    for i in 0..10 {
        let pasid = PASID::new(i).unwrap();
        stream_context.create_pasid(pasid).unwrap();
    }

    assert_eq!(stream_context.pasid_count(), 10);
}

#[test]
fn test_create_duplicate_pasid() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();

    stream_context.create_pasid(pasid).unwrap();
    let result = stream_context.create_pasid(pasid);

    assert!(matches!(result, Err(StreamContextError::PASIDAlreadyExists(1))));
}

// ============================================================================
// PASID Removal Tests
// ============================================================================

#[test]
fn test_remove_pasid() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();

    stream_context.create_pasid(pasid).unwrap();
    assert!(stream_context.has_pasid(pasid));

    stream_context.remove_pasid(pasid).unwrap();
    assert!(!stream_context.has_pasid(pasid));
}

#[test]
fn test_remove_nonexistent_pasid() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();

    let result = stream_context.remove_pasid(pasid);
    assert!(matches!(result, Err(StreamContextError::PASIDNotFound(1))));
}

#[test]
fn test_remove_pasid_zero() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(0).unwrap();

    stream_context.create_pasid(pasid).unwrap();
    stream_context.remove_pasid(pasid).unwrap();
    assert!(!stream_context.has_pasid(pasid));
}

// ============================================================================
// Translation Tests
// ============================================================================

#[test]
fn test_translate_with_pasid() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    stream_context.create_pasid(pasid).unwrap();
    stream_context
        .map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().physical_address().as_u64(), 0x2000);
}

#[test]
fn test_translate_with_pasid_zero() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    stream_context.create_pasid(pasid).unwrap();
    stream_context
        .map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok());
}

#[test]
fn test_translate_nonexistent_pasid() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    let result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(matches!(result, Err(TranslationError::PASIDNotFound)));
}

#[test]
fn test_translate_unmapped_page() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    stream_context.create_pasid(pasid).unwrap();

    let result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(matches!(result, Err(TranslationError::PageNotMapped)));
}

// ============================================================================
// Multiple PASID Tests
// ============================================================================

#[test]
fn test_multiple_pasids_independent() {
    let stream_context = StreamContext::new();
    let pasid1 = PASID::new(1).unwrap();
    let pasid2 = PASID::new(2).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa1 = PA::new(0x2000).unwrap();
    let pa2 = PA::new(0x3000).unwrap();

    stream_context.create_pasid(pasid1).unwrap();
    stream_context.create_pasid(pasid2).unwrap();

    stream_context
        .map_page(pasid1, iova, pa1, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();
    stream_context
        .map_page(pasid2, iova, pa2, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let result1 = stream_context
        .translate(pasid1, iova, AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    let result2 = stream_context
        .translate(pasid2, iova, AccessType::Read, SecurityState::NonSecure)
        .unwrap();

    assert_eq!(result1.physical_address().as_u64(), 0x2000);
    assert_eq!(result2.physical_address().as_u64(), 0x3000);
}

// ============================================================================
// State Machine Tests
// ============================================================================

#[test]
fn test_enable_disable_stream() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();

    stream_context.create_pasid(pasid).unwrap();
    assert!(stream_context.is_enabled());

    stream_context.disable();
    assert!(!stream_context.is_enabled());

    // Bug 6 fix: ARM §3.21 — PASID/CD setup is independent of stream enable state;
    // create_pasid must succeed on a disabled stream so drivers can pre-configure
    // page tables before re-enabling the stream.
    let result = stream_context.create_pasid(PASID::new(2).unwrap());
    assert!(result.is_ok(), "create_pasid must succeed on a disabled stream (ARM §3.21)");

    stream_context.enable();
    assert!(stream_context.is_enabled());
}

#[test]
fn test_stage1_enable_disable() {
    let stream_context = StreamContext::new();

    assert!(stream_context.is_stage1_enabled());

    stream_context.set_stage1_enabled(false);
    assert!(!stream_context.is_stage1_enabled());

    stream_context.set_stage1_enabled(true);
    assert!(stream_context.is_stage1_enabled());
}

#[test]
fn test_stage2_enable_disable() {
    let stream_context = StreamContext::new();

    assert!(!stream_context.is_stage2_enabled());

    stream_context.set_stage2_enabled(true);
    assert!(stream_context.is_stage2_enabled());

    stream_context.set_stage2_enabled(false);
    assert!(!stream_context.is_stage2_enabled());
}

// ============================================================================
// Shared AddressSpace Tests
// ============================================================================

#[test]
fn test_shared_address_space() {
    let stream_context = StreamContext::new();
    let pasid1 = PASID::new(1).unwrap();
    let pasid2 = PASID::new(2).unwrap();

    stream_context.create_pasid(pasid1).unwrap();
    let addr_space = stream_context.get_pasid_address_space(pasid1).unwrap();

    stream_context.add_pasid(pasid2, addr_space).unwrap();

    assert!(stream_context.has_pasid(pasid1));
    assert!(stream_context.has_pasid(pasid2));
}

// ============================================================================
// Bulk Operations Tests
// ============================================================================

#[test]
fn test_bulk_pasid_creation() {
    let stream_context = StreamContext::new();

    // Create 100 PASIDs
    for i in 0..100 {
        let pasid = PASID::new(i).unwrap();
        stream_context.create_pasid(pasid).unwrap();
    }

    assert_eq!(stream_context.pasid_count(), 100);
}

#[test]
fn test_bulk_pasid_removal() {
    let stream_context = StreamContext::new();

    // Create and remove 50 PASIDs
    for i in 0..50 {
        let pasid = PASID::new(i).unwrap();
        stream_context.create_pasid(pasid).unwrap();
    }

    for i in 0..50 {
        let pasid = PASID::new(i).unwrap();
        stream_context.remove_pasid(pasid).unwrap();
    }

    assert_eq!(stream_context.pasid_count(), 0);
}

#[test]
fn test_bulk_translation() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();

    stream_context.create_pasid(pasid).unwrap();

    // Map 100 pages
    for i in 0..100 {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let pa = PA::new(0x2000 + i * PAGE_SIZE).unwrap();
        stream_context
            .map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();
    }

    // Translate all pages
    for i in 0..100 {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
        assert!(result.is_ok());
    }
}

// ============================================================================
// Page Mapping Tests
// ============================================================================

#[test]
fn test_map_page() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    stream_context.create_pasid(pasid).unwrap();
    let result = stream_context.map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure);

    assert!(result.is_ok());
}

#[test]
fn test_unmap_page() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    stream_context.create_pasid(pasid).unwrap();
    stream_context
        .map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let result = stream_context.unmap_page(pasid, iova);
    assert!(result.is_ok());
}

// ============================================================================
// Thread Safety Tests
// ============================================================================

#[test]
fn test_stream_context_send() {
    fn assert_send<T: Send>() {}
    assert_send::<StreamContext>();
}

#[test]
fn test_stream_context_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<StreamContext>();
}

// ============================================================================
// Configuration Tests
// ============================================================================

#[test]
fn test_pasid_count() {
    let stream_context = StreamContext::new();

    assert_eq!(stream_context.pasid_count(), 0);

    for i in 0..5 {
        let pasid = PASID::new(i).unwrap();
        stream_context.create_pasid(pasid).unwrap();
    }

    assert_eq!(stream_context.pasid_count(), 5);
}

#[test]
fn test_has_pasid() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();

    assert!(!stream_context.has_pasid(pasid));

    stream_context.create_pasid(pasid).unwrap();
    assert!(stream_context.has_pasid(pasid));
}

#[test]
fn test_clear_all_pasids() {
    let stream_context = StreamContext::new();

    for i in 0..10 {
        let pasid = PASID::new(i).unwrap();
        stream_context.create_pasid(pasid).unwrap();
    }

    assert_eq!(stream_context.pasid_count(), 10);

    stream_context.clear_all_pasids().unwrap();
    assert_eq!(stream_context.pasid_count(), 0);
}

// ============================================================================
// GAP-B: PASID-Recycling ASID Stale-Value Protection Tests
//
// Regression tests for Bug 4 (remove_pasid clears pasid_asid_map) and
// Bug 5 (clear_all_pasids and disable clear pasid_asid_map).
//
// Without the fixes a recycled PASID value inherits the stale ASID from its
// predecessor, causing incorrect TLB tagging (ARM SMMU v3 spec §3.17).
// ============================================================================

#[test]
fn test_remove_pasid_clears_asid_map() {
    // Bug 4 regression: remove_pasid must clear the ASID entry so a recycled
    // PASID value does not inherit the stale ASID (ARM §3.17).
    let stream_context = StreamContext::new();
    let pasid = PASID::new(5).unwrap();

    stream_context.create_pasid(pasid).unwrap();
    stream_context.set_pasid_asid(pasid, 42).unwrap();

    // Verify the ASID is set before removal.
    assert_eq!(stream_context.get_pasid_asid(pasid).unwrap(), 42);

    // Remove the PASID.
    stream_context.remove_pasid(pasid).unwrap();

    // Recycle the same PASID value.
    stream_context.create_pasid(pasid).unwrap();

    // The recycled PASID must NOT inherit the stale ASID — it must read back 0.
    assert_eq!(
        stream_context.get_pasid_asid(pasid).unwrap(),
        0,
        "Recycled PASID must not inherit stale ASID from previous owner"
    );
}

#[test]
fn test_clear_all_pasids_clears_asid_map() {
    // Bug 5 regression: clear_all_pasids must clear the ASID map so a recycled
    // PASID value does not inherit a stale ASID (ARM §3.17).
    let stream_context = StreamContext::new();
    let pasid = PASID::new(3).unwrap();

    stream_context.create_pasid(pasid).unwrap();
    stream_context.set_pasid_asid(pasid, 99).unwrap();
    assert_eq!(stream_context.get_pasid_asid(pasid).unwrap(), 99);

    // Clear all PASIDs (bulk teardown).
    stream_context.clear_all_pasids().unwrap();

    // Recycle the same PASID value.
    stream_context.create_pasid(pasid).unwrap();

    // The recycled PASID must NOT inherit the stale ASID — it must read back 0.
    assert_eq!(
        stream_context.get_pasid_asid(pasid).unwrap(),
        0,
        "Recycled PASID must not inherit stale ASID after clear_all_pasids"
    );
}

#[test]
fn test_disable_clears_asid_map() {
    // Bug 5 regression: disable must clear the ASID map so a recycled PASID
    // value on stream re-use does not inherit a stale ASID (ARM §3.17).
    let stream_context = StreamContext::new();
    let pasid = PASID::new(7).unwrap();

    stream_context.create_pasid(pasid).unwrap();
    stream_context.set_pasid_asid(pasid, 77).unwrap();
    assert_eq!(stream_context.get_pasid_asid(pasid).unwrap(), 77);

    // Simulate stream teardown.
    stream_context.disable();

    // Re-enable and recycle the same PASID value.
    stream_context.enable();
    stream_context.create_pasid(pasid).unwrap();

    // The recycled PASID must NOT inherit the stale ASID — it must read back 0.
    assert_eq!(
        stream_context.get_pasid_asid(pasid).unwrap(),
        0,
        "Recycled PASID must not inherit stale ASID after disable/enable cycle"
    );
}

// ============================================================================
// BUG-RUST-1: TOCTOU race in disable() — wrong error type
//
// ARM IHI0070G.b §7.3.6 (F_STREAM_DISABLED): when a stream is being disabled
// concurrently, any translation that raced past the is_enabled() check must
// still receive TranslationError::StreamDisabled, never PASIDNotFound.
//
// The sequential reproduction: a translate() call observes is_enabled()==true,
// then disable() fires (clears the pasid_map), then the translate() reaches
// pasid_map.get() and finds no entry.  Without the fix the result is
// PASIDNotFound; with the fix it must be StreamDisabled.
// ============================================================================

/// BUG-RUST-1 sequential reproduction test.
///
/// Simulates the TOCTOU window: a `translate()` call observes
/// `is_enabled()==true` and then the stream is disabled (`pasid_map` cleared)
/// before the translate reaches `pasid_map.get()`.  The result must be
/// `StreamDisabled` (ARM §7.3.6 `F_STREAM_DISABLED`), not `PASIDNotFound`.
///
/// The fix adds a `disabling` atomic flag.  When `translate_stage1_only()`
/// gets `None` from `pasid_map.get()` it re-checks the disabling flag and
/// returns `StreamDisabled` if set, so the race window yields the correct error.
#[test]
fn test_bug_rust1_disable_toctou_returns_stream_disabled_not_pasid_not_found() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;

    let ctx = Arc::new(StreamContext::new());

    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    // Set up a valid mapping so the PASID exists before disable.
    ctx.create_pasid(pasid).unwrap();
    ctx.map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    // Confirm the stream is enabled and the translation works before any race.
    assert!(ctx.is_enabled());
    assert!(ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure).is_ok());

    // Use a barrier to synchronise: both threads are ready, then race begins.
    let barrier = Arc::new(Barrier::new(2));

    let ctx_disabler = Arc::clone(&ctx);
    let barrier_disabler = Arc::clone(&barrier);
    let saw_bad_error = Arc::new(AtomicBool::new(false));
    let saw_bad_error_reader = Arc::clone(&saw_bad_error);

    // Reader thread: spin at barrier, then translate as fast as possible.
    let ctx_reader = Arc::clone(&ctx);
    let reader = thread::spawn(move || {
        barrier_disabler.wait(); // sync with disabler
        for _ in 0..50_000_u32 {
            match ctx_reader.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure) {
                Ok(_) => {}
                Err(TranslationError::StreamDisabled) => break,
                Err(other) => {
                    eprintln!("BUG-RUST-1: unexpected error during concurrent disable: {:?}", other);
                    saw_bad_error_reader.store(true, Ordering::SeqCst);
                    break;
                }
            }
        }
    });

    // Disabler thread: sync then immediately call disable().
    let disabler = thread::spawn(move || {
        barrier.wait();
        ctx_disabler.disable();
    });

    disabler.join().expect("disabler thread panicked");
    reader.join().expect("reader thread panicked");

    assert!(
        !saw_bad_error.load(Ordering::SeqCst),
        "BUG-RUST-1: translate() returned PASIDNotFound instead of StreamDisabled during concurrent disable"
    );
}

/// BUG-RUST-1 concurrent stress test.
///
/// Exercises the race window directly: a reader thread translates in a tight
/// loop while the main thread calls `disable()`.  Every error returned must be
/// `StreamDisabled`, never `PASIDNotFound`.
#[test]
fn test_bug_rust1_concurrent_disable_error_is_stream_disabled() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;

    let ctx = Arc::new(StreamContext::new());

    let pasid = PASID::new(2).unwrap();
    let iova = IOVA::new(0x4000).unwrap();
    let pa = PA::new(0x8000).unwrap();

    ctx.create_pasid(pasid).unwrap();
    ctx.map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let ctx_reader = Arc::clone(&ctx);
    let saw_bad_error = Arc::new(AtomicBool::new(false));
    let saw_bad_error_reader = Arc::clone(&saw_bad_error);

    // Reader thread: translate in a tight loop until the stream is disabled.
    let reader = thread::spawn(move || {
        for _ in 0..10_000_u32 {
            match ctx_reader.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure) {
                Ok(_) => {}
                Err(TranslationError::StreamDisabled) => {
                    // Correct: stream was disabled.
                    break;
                }
                Err(other) => {
                    // Any other error (e.g., PASIDNotFound) is the bug.
                    eprintln!("BUG-RUST-1: unexpected error during concurrent disable: {:?}", other);
                    saw_bad_error_reader.store(true, Ordering::SeqCst);
                    break;
                }
            }
        }
    });

    // Main thread: disable after a brief yield to maximise race opportunity.
    std::thread::yield_now();
    ctx.disable();

    reader.join().expect("reader thread panicked");

    assert!(
        !saw_bad_error.load(Ordering::SeqCst),
        "BUG-RUST-1: translator returned wrong error type during concurrent disable"
    );
}

// ============================================================================
// BUG-RUST-3: Relaxed atomic loads for config fields — insufficient memory
//             ordering on the translation hot path
//
// Config atomics (`stage1_enabled`, `stage2_enabled`, `strw`, `t0sz`, …) use
// `Ordering::Relaxed` for loads in translate() but `Ordering::SeqCst` for
// stores in `update_configuration()`.  On weakly-ordered architectures a
// concurrent translator can observe a partially-updated config snapshot.
//
// Fix: loads → `Acquire`, stores in `update_configuration()` → `Release`.
// This creates a happens-before edge: every translator that performs an
// Acquire load is guaranteed to see all stores that preceded the Release store
// as a complete, consistent unit.
//
// The test below is sequential and verifies the observable contract: after
// `update_configuration()` returns, a subsequent `translate()` must reflect
// the new config consistently — not a partial or stale view.
// ============================================================================

/// BUG-RUST-3 test: config atomics must be consistently visible after
/// `update_configuration()` completes.
///
/// Scenario 1 — bypass → stage1:
///   - Stream starts in bypass mode (no stages enabled, the default).
///   - `update_configuration()` switches to `stage1_enabled=true`.
///   - `translate()` must see stage1=true and return `PASIDNotFound` (PASID
///     not yet created), NOT an identity-mapped bypass result.
///
/// Scenario 2 — stage1 → bypass:
///   - Stream has stage1 active with a mapped page.
///   - `update_configuration()` switches to bypass.
///   - `translate()` must see bypass and return the identity-mapped PA, NOT
///     `PASIDNotFound` (which would indicate it still saw stage1=true).
///
/// On x86 these scenarios pass even with Relaxed loads due to TSO ordering.
/// On weakly-ordered architectures (ARM, POWER) Acquire/Release are required.
/// The test is valuable as a contract specification and for Miri verification.
#[test]
fn test_bug_rust3_update_configuration_config_visible_to_translate() {
    let ctx = StreamContext::new();

    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x5000).unwrap();
    let pa = PA::new(0x8000).unwrap();

    // --- Scenario 1: bypass → stage1 ---

    // Default StreamContext starts with stage1_enabled=true.  Switch to bypass
    // by applying a bypass config (no stages enabled, translation_enabled=false).
    let bypass_cfg = StreamConfig::builder()
        // No .translation_enabled(true) → disabled=false, translation_enabled=false
        // → bypass mode (STE.Config==0b100).
        .build()
        .unwrap();
    ctx.update_configuration(bypass_cfg);

    // In bypass mode, translate() returns the IOVA as the PA (identity mapping).
    let bypass_result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(
        bypass_result.is_ok(),
        "bypass translate should succeed, got {:?}",
        bypass_result
    );
    assert_eq!(
        bypass_result.unwrap().physical_address().as_u64(),
        0x5000,
        "bypass mode must return identity mapping"
    );

    // Now switch to stage1_enabled=true.  After update_configuration() the
    // translate() MUST see the new stage1_enabled value — NOT a stale bypass.
    let stage1_cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .build()
        .unwrap();
    ctx.update_configuration(stage1_cfg);

    // PASID 0 is not created yet — translate() must return PASIDNotFound,
    // confirming it observed stage1=true.  A bypass (identity) result would
    // mean it read a stale stage1=false.
    let stage1_result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(
        matches!(stage1_result, Err(TranslationError::PASIDNotFound)),
        "BUG-RUST-3: Expected PASIDNotFound (stage1 mode) after switching from bypass, got {:?}",
        stage1_result
    );

    // --- Scenario 2: stage1 → bypass ---

    // Create the PASID and map a page so stage1 succeeds.
    ctx.create_pasid(pasid).unwrap();
    ctx.map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    // Confirm stage1 translation works and returns the mapped PA.
    let mapped_result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(mapped_result.is_ok(), "stage1 translate should succeed: {:?}", mapped_result);
    assert_eq!(
        mapped_result.unwrap().physical_address().as_u64(),
        0x8000,
        "stage1 must return the mapped PA"
    );

    // Switch back to bypass mode.
    let bypass_cfg2 = StreamConfig::builder().build().unwrap();
    ctx.update_configuration(bypass_cfg2);

    // Translate must now return identity mapping (bypass), NOT 0x8000.
    // If it returns PASIDNotFound it saw a stale stage1=true.
    let back_to_bypass = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(
        back_to_bypass.is_ok(),
        "BUG-RUST-3: Expected bypass Ok after switching back to bypass mode, got {:?}",
        back_to_bypass
    );
    assert_eq!(
        back_to_bypass.unwrap().physical_address().as_u64(),
        0x5000,
        "BUG-RUST-3: bypass mode must return identity mapping (IOVA==PA), \
         not the previously mapped PA — stale stage1_enabled load suspected"
    );
}

/// BUG-RUST-3 sequential multi-toggle test: verifies that repeated
/// `update_configuration()` → `translate()` pairs always observe the latest
/// config values.  This exercises the Acquire/Release ordering guarantee:
/// the store in `update_configuration()` (Release) must be visible to the
/// immediately following `translate()` load (Acquire).
#[test]
fn test_bug_rust3_repeated_config_toggle_observes_latest_values() {
    let ctx = StreamContext::new();

    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0xA000).unwrap();
    let pa = PA::new(0xB000).unwrap();

    ctx.create_pasid(pasid).unwrap();
    ctx.map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let stage1_cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .build()
        .unwrap();
    let bypass_cfg = StreamConfig::builder().build().unwrap();

    for i in 0..200_u32 {
        if i % 2 == 0 {
            // Stage1 mode: must see mapped PA.
            ctx.update_configuration(stage1_cfg.clone());
            let result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
            assert!(
                matches!(&result, Ok(data) if data.physical_address().as_u64() == 0xB000),
                "BUG-RUST-3 iteration {}: expected PA=0xB000 in stage1 mode, got {:?}",
                i,
                result
            );
        } else {
            // Bypass mode: must see identity PA.
            ctx.update_configuration(bypass_cfg.clone());
            let result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
            assert!(
                matches!(&result, Ok(data) if data.physical_address().as_u64() == 0xA000),
                "BUG-RUST-3 iteration {}: expected PA=0xA000 in bypass mode, got {:?}",
                i,
                result
            );
        }
    }
}
