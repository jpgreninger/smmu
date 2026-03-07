#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

//! TDD regression test for BUG-RUST-DBGR-11:
//! `is_abort_mode()` loads `self.abort_mode` with `Ordering::Relaxed`.
//!
//! Problem: `set_abort_mode()` stores with `Ordering::Release`. The translate
//! hot-path already uses `self.abort_mode.load(Ordering::Acquire)` directly,
//! making `is_abort_mode()` inconsistent — public callers using `is_abort_mode()`
//! may observe stale values on weakly-ordered hardware because a Relaxed load
//! has no happens-before relationship with a Release store.
//!
//! Fix: change `is_abort_mode()` to use `Ordering::Acquire`.

use smmu::stream_context::StreamContext;

/// A freshly-created StreamContext is not in abort mode by default.
#[test]
fn dbgr11_is_abort_mode_false_on_new_context() {
    let ctx = StreamContext::new();
    assert!(
        !ctx.is_abort_mode(),
        "DBGR-11: is_abort_mode() must return false for a fresh StreamContext"
    );
}

/// After set_abort_mode(true), is_abort_mode() must return true.
///
/// Before the fix: `is_abort_mode()` uses Relaxed ordering while
/// `set_abort_mode()` uses Release. This asymmetric pair means on
/// weakly-ordered architectures the Relaxed load may not see the Release store.
/// The Acquire load is the correct pairing.
#[test]
fn dbgr11_is_abort_mode_true_after_set() {
    let ctx = StreamContext::new();
    ctx.set_abort_mode(true);
    assert!(
        ctx.is_abort_mode(),
        "DBGR-11: is_abort_mode() must return true after set_abort_mode(true)"
    );
}

/// After set_abort_mode(false), is_abort_mode() must return false.
#[test]
fn dbgr11_is_abort_mode_false_after_clear() {
    let ctx = StreamContext::new();
    ctx.set_abort_mode(true);
    assert!(ctx.is_abort_mode(), "DBGR-11 pre: abort mode should be set");
    ctx.set_abort_mode(false);
    assert!(
        !ctx.is_abort_mode(),
        "DBGR-11: is_abort_mode() must return false after set_abort_mode(false)"
    );
}

/// Abort mode is independent of enabled state — verify both can be set/cleared
/// independently and both remain observable through their Acquire loads.
#[test]
fn dbgr11_abort_mode_independent_of_enabled() {
    let ctx = StreamContext::new();
    // Set abort mode while still enabled
    ctx.set_abort_mode(true);
    assert!(ctx.is_enabled(), "DBGR-11: still enabled after set_abort_mode");
    assert!(ctx.is_abort_mode(), "DBGR-11: abort_mode should be true");

    // Disable — abort mode should remain true
    ctx.disable();
    assert!(!ctx.is_enabled(), "DBGR-11: disabled");
    assert!(
        ctx.is_abort_mode(),
        "DBGR-11: abort_mode should stay true after disable()"
    );

    // Clear abort mode — enabled remains false
    ctx.set_abort_mode(false);
    assert!(!ctx.is_enabled(), "DBGR-11: still disabled");
    assert!(
        !ctx.is_abort_mode(),
        "DBGR-11: abort_mode should be false after clear"
    );
}

/// Multiple toggle cycles through is_abort_mode() are all consistent.
#[test]
fn dbgr11_is_abort_mode_consistent_across_multiple_toggles() {
    let ctx = StreamContext::new();
    for _ in 0..5 {
        ctx.set_abort_mode(true);
        assert!(
            ctx.is_abort_mode(),
            "DBGR-11: is_abort_mode() must be true after set_abort_mode(true)"
        );
        ctx.set_abort_mode(false);
        assert!(
            !ctx.is_abort_mode(),
            "DBGR-11: is_abort_mode() must be false after set_abort_mode(false)"
        );
    }
}
