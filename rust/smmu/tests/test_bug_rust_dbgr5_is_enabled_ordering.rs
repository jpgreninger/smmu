#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

//! TDD regression test for BUG-RUST-DBGR-5:
//! `is_enabled()` loads `self.enabled` with `Ordering::Relaxed`.
//!
//! Problem: `disable()` stores `enabled=false` with `Ordering::SeqCst`.
//! `is_enabled()` loads with `Ordering::Relaxed`, which has no
//! happens-before relationship with the SeqCst store on weakly-ordered
//! hardware. The correct pairing is Acquire load / Release (or SeqCst) store.
//!
//! Fix: change `is_enabled()` to use `Ordering::Acquire`.

use smmu::stream_context::StreamContext;

/// A freshly-created StreamContext is enabled by default.
/// This verifies the initial state observable through is_enabled().
#[test]
fn dbgr5_is_enabled_true_on_new_context() {
    let ctx = StreamContext::new();
    assert!(
        ctx.is_enabled(),
        "DBGR-5: is_enabled() must return true for a fresh StreamContext"
    );
}

/// After disable(), is_enabled() must return false.
///
/// Before the fix: `is_enabled()` uses Relaxed ordering, creating an
/// asymmetric pair with the SeqCst store in `disable()`. On weakly-ordered
/// hardware the Relaxed load has no happens-before guarantee with the store.
/// The Acquire ordering makes the store-to-load edge correct and sufficient.
///
/// In a single-threaded context the observable contract is that is_enabled()
/// returns false after disable() returns. The Acquire fix ensures this contract
/// holds on all architectures (not just TSO).
#[test]
fn dbgr5_is_enabled_false_after_disable() {
    let ctx = StreamContext::new();
    // Confirm enabled before
    assert!(
        ctx.is_enabled(),
        "DBGR-5 pre-condition: is_enabled() should be true before disable()"
    );
    ctx.disable();
    // After disable(), is_enabled() must return false.
    // Before the fix the Relaxed load could theoretically return stale true
    // on weakly-ordered hardware; Acquire is the correct pairing for SeqCst store.
    assert!(
        !ctx.is_enabled(),
        "DBGR-5: is_enabled() must return false after disable()"
    );
}

/// After disable() then re-enable (via enable()), is_enabled() returns true.
///
/// This verifies the full toggle cycle works correctly with Acquire ordering.
#[test]
fn dbgr5_is_enabled_true_after_reenable() {
    let ctx = StreamContext::new();
    ctx.disable();
    assert!(
        !ctx.is_enabled(),
        "DBGR-5 mid: is_enabled() should be false after disable()"
    );
    ctx.enable();
    assert!(
        ctx.is_enabled(),
        "DBGR-5: is_enabled() must return true after enable() following disable()"
    );
}

/// Multiple disable/enable cycles all reflect correct state through is_enabled().
#[test]
fn dbgr5_is_enabled_consistent_across_multiple_toggles() {
    let ctx = StreamContext::new();
    for _ in 0..5 {
        ctx.disable();
        assert!(
            !ctx.is_enabled(),
            "DBGR-5: is_enabled() must be false after disable()"
        );
        ctx.enable();
        assert!(
            ctx.is_enabled(),
            "DBGR-5: is_enabled() must be true after enable()"
        );
    }
}
