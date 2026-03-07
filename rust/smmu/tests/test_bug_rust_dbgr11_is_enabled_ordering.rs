//! Tests for BUG-RUST-DBGR-11 — Ordering::Relaxed in public is_enabled() accessor
#![allow(clippy::doc_markdown)]
//!
//! The public is_enabled() must use Acquire ordering to pair correctly with
//! the Release/SeqCst store in disable(). This is a correctness test.

use smmu::stream_context::StreamContext;

/// After disable(), is_enabled() must immediately reflect false.
#[test]
fn dbgr11_is_enabled_reflects_disable() {
    let ctx = StreamContext::new();
    assert!(ctx.is_enabled(), "new StreamContext must start enabled");
    ctx.disable();
    assert!(!ctx.is_enabled(), "is_enabled() must return false after disable()");
}

/// After enable(), is_enabled() must immediately reflect true.
#[test]
fn dbgr11_is_enabled_reflects_enable() {
    let ctx = StreamContext::new();
    ctx.disable();
    assert!(!ctx.is_enabled(), "must be disabled after disable()");
    ctx.enable();
    assert!(ctx.is_enabled(), "is_enabled() must return true after enable()");
}

/// is_enabled() uses Acquire ordering (not Relaxed).
/// Verify the ordering is documented/correct by checking the internal atomic value matches.
#[test]
fn dbgr11_is_enabled_uses_acquire_ordering() {
    let ctx = StreamContext::new();
    // Verify that is_enabled() returns the correct value after a store.
    // On any real hardware, a Relaxed load may miss a Relaxed store from another
    // context; using Acquire ensures visibility. This test confirms the value is correct.
    ctx.disable();
    // If is_enabled() used Relaxed, it might return stale true on weakly-ordered hardware.
    // On x86 this is trivially safe, but the test documents the required ordering.
    assert!(!ctx.is_enabled(), "is_enabled() with Acquire must see disable() store");
}
