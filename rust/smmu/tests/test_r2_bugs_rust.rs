#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

//! TDD tests for three ARM SMMU v3 Rust bug fixes (BUG-R2-RUST-2, -6, -7).
//!
//! BUG-R2-RUST-2: `disable()` ordering inversion vs `enable()`.
//!   `enable()` writes CR0 first then sets `enabled=true`.
//!   `disable()` (before fix) sets `enabled=false` first then clears CR0.SMMUEN.
//!   This creates a window where `is_enabled()` returns false but CR0.SMMUEN=1.
//!   Fix: swap the two stores in `disable()` to match `enable()` ordering.
//!
//! BUG-R2-RUST-6: `get_stream_count()` shutdown guard semantics.
//!   The early `is_shutdown()` guard in `get_stream_count()` returns 0 before
//!   the DashMap is cleared, masking concurrent TOCTOU windows.  Since
//!   `shutdown()` already atomically stores 0 to `stream_count` after
//!   `streams.clear()`, removing the guard and relying on `stream_count` is
//!   correct — callers see 0 after a completed `shutdown()`.
//!   Fix: remove the `is_shutdown()` early-return guard; rely solely on
//!   `stream_count` (which `shutdown()` stores to 0) and add a doc comment.
//!
//! BUG-R2-RUST-7: `signal_gerror()` CAS+seqlock retry loop missing
//!   `std::hint::spin_loop()`.  The loop retries indefinitely without yielding
//!   CPU hints, which is suboptimal under adversarial concurrent load and
//!   increases memory-bus contention.
//!   Fix: add `std::hint::spin_loop()` at each retry point in the loop.

use smmu::types::{CommandEntry, CommandType, StreamConfig, StreamID};
use smmu::SMMU;

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

// ─── BUG-R2-RUST-2 ────────────────────────────────────────────────────────────

/// After `disable()`, both `is_enabled()` and `get_cr0() & SMMUEN` must be 0.
///
/// Before the fix, `disable()` stored `enabled=false` BEFORE clearing
/// CR0.SMMUEN, so there was a brief window where `is_enabled()` returned
/// `false` while `get_cr0()` still showed SMMUEN=1.
///
/// The fix swaps the order: CR0.SMMUEN is cleared first, then `enabled` is
/// stored to false — exactly mirroring the `enable()` ordering (CR0 written
/// before `enabled` flag).
///
/// This test also verifies that `disable()` preserves other CR0 bits
/// (EVENTQEN, CMDQEN) as required by the ARM SMMU v3 spec §6.3.9 —
/// `disable()` only clears SMMUEN, it must not disturb queue-enable bits.
#[test]
fn rust2_disable_clears_cr0_before_enabled_flag() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    smmu.enable().unwrap();
    assert!(smmu.is_enabled());

    smmu.disable().unwrap();

    assert!(!smmu.is_enabled(), "is_enabled() must be false after disable()");
    let cr0 = smmu.get_cr0();
    assert_eq!(
        cr0 & SMMU::CR0_SMMUEN,
        0,
        "CR0.SMMUEN must be cleared after disable()"
    );
    assert_ne!(
        cr0 & SMMU::CR0_EVENTQEN,
        0,
        "EVENTQEN must be preserved by disable()"
    );
    assert_ne!(
        cr0 & SMMU::CR0_CMDQEN,
        0,
        "CMDQEN must be preserved by disable()"
    );
}

/// Symmetry check: `enable()` then `disable()` leaves both flags consistently
/// false — `is_enabled()` and CR0.SMMUEN must both be 0.
#[test]
fn rust2_disable_enable_ordering_consistent() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    assert!(smmu.is_enabled());
    assert_ne!(smmu.get_cr0() & SMMU::CR0_SMMUEN, 0);

    smmu.disable().unwrap();

    assert!(!smmu.is_enabled());
    assert_eq!(smmu.get_cr0() & SMMU::CR0_SMMUEN, 0);
}

// ─── BUG-R2-RUST-6 ────────────────────────────────────────────────────────────

/// After `shutdown()` completes, `get_stream_count()` must return 0 (streams
/// have been cleared and `stream_count` stored to 0 by `shutdown()`), and
/// `is_shutdown()` must be true.
///
/// With the removed shutdown guard, `get_stream_count()` now always returns
/// the authoritative `stream_count` atomic — which `shutdown()` stores to 0
/// after `streams.clear()`.  This test exercises the completed-shutdown path.
#[test]
fn rust6_stream_count_consistent_after_shutdown() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu.configure_stream(sid(1), StreamConfig::bypass()).unwrap();
    smmu.configure_stream(sid(2), StreamConfig::bypass()).unwrap();
    assert_eq!(smmu.get_stream_count(), 2);

    smmu.shutdown().unwrap();

    // After shutdown completes, streams are cleared — count must be 0
    assert_eq!(
        smmu.get_stream_count(),
        0,
        "After shutdown(), get_stream_count must return 0 (streams cleared)"
    );
    // Also verify is_shutdown() is true
    assert!(smmu.is_shutdown(), "SMMU must be in shutdown state");
}

// ─── BUG-R2-RUST-7 ────────────────────────────────────────────────────────────

/// Verify `signal_gerror()` terminates correctly under normal (non-concurrent)
/// conditions and produces consistent GERROR state after repeated
/// activate/acknowledge cycles.
///
/// The livelock risk is theoretical and not testable in unit tests, but this
/// test exercises the loop repeatedly to confirm no correctness regression from
/// adding `std::hint::spin_loop()` at the retry points.
#[test]
fn rust7_signal_gerror_terminates_correctly() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    smmu.set_cr2(SMMU::CR2_RECINVSID);

    // Trigger and acknowledge multiple times to exercise the CAS loop.
    for i in 0u32..5 {
        smmu.submit_command(CommandEntry::new(CommandType::CfgiSte, 0xF000 + i, 0))
            .unwrap();
        let _ = smmu.process_command_queue();
        smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);
    }

    // All iterations should have completed without deadlock.
    assert_eq!(
        (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR,
        0,
        "CMDQ_ERR must be inactive after final clear"
    );
}
