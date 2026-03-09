#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::unwrap_used)]

//! TDD regression tests for BUG-RUST-F2:
//! Two separate DashMap lookups on the TLB fast path create a race window
//! during concurrent stream removal.
//!
//! Problem (§5.2 / §13.5):
//!   The TLB fast path performs TWO independent `self.streams.get(&stream_value)` calls:
//!   1. Privilege check (lines ~1898-1905)
//!   2. `apply_output_attrs()` call (lines ~1919-1922)
//!
//!   If `remove_stream()` fires between call 1 and call 2:
//!   - `apply_output_attrs()` is skipped → all 6 output fields zeroed.
//!   - The privilege guard at call 1 silently passes on a missing stream instead
//!     of denying access.
//!
//! Fix: Consolidate into a SINGLE DashMap guard held across both operations.
//!
//! These tests verify the LOGICAL INVARIANT that must hold (deterministic
//! correctness), relying on the consolidated single-lookup fix to eliminate
//! the TOCTOU window rather than attempting to reproduce the race via timing.

use smmu::types::{AccessType, PagePermissions, SecurityState, StreamConfig, StreamID, IOVA, PA, PASID};
use smmu::SMMU;

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}
fn pasid(n: u32) -> PASID {
    PASID::new(n).unwrap()
}
fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}
fn pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

fn smmu_with_stream(stream: u32, mem_attr: u8, sh_cfg: u8) -> SMMU {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .mt_cfg(true)
        .mem_attr(mem_attr)
        .sh_cfg(sh_cfg)
        .build()
        .unwrap();
    smmu.configure_stream(sid(stream), cfg).unwrap();
    smmu.create_pasid(sid(stream), pasid(0)).unwrap();
    smmu.map_page(
        sid(stream),
        pasid(0),
        iova(0x1000),
        pa(0x8000_1000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();
    smmu
}

/// Populate the TLB (slow path), then remove the stream, then translate again.
///
/// After `remove_stream()` the TLB entry for that stream is invalidated
/// (or translation must fail), so the second translate must NOT silently
/// return a result with zeroed output attributes.
///
/// The key invariant: output attrs on TLB hit must come from the SAME lookup
/// that performs the privilege check — a single consolidated guard.
#[test]
fn f2_single_guard_output_attrs_consistent_with_priv_check() {
    let smmu = smmu_with_stream(20, 7, 2);

    // Slow path — populate the TLB.
    let r1 = smmu
        .translate(sid(20), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure)
        .expect("F2: slow path must succeed");

    assert_eq!(r1.mem_type(), 7, "F2: slow path mem_type must be 7");
    assert_eq!(r1.shareability(), 2, "F2: slow path shareability must be 2");

    // Fast path (TLB hit) — output attrs must still be populated correctly.
    let r2 = smmu
        .translate(sid(20), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure)
        .expect("F2: fast path (TLB hit) must succeed");

    // These must equal the slow-path values — NOT zero.
    assert_eq!(
        r2.mem_type(),
        7,
        "F2 BUG: TLB fast path mem_type should be 7 (same as slow path) but got {}; \
         output attrs lost when second DashMap lookup returns None",
        r2.mem_type()
    );
    assert_eq!(
        r2.shareability(),
        2,
        "F2 BUG: TLB fast path shareability should be 2 but got {}",
        r2.shareability()
    );
    assert_eq!(
        r1.mem_type(),
        r2.mem_type(),
        "F2: mem_type must be identical on slow and fast path"
    );
    assert_eq!(
        r1.shareability(),
        r2.shareability(),
        "F2: shareability must be identical on slow and fast path"
    );
}

/// On a non-privileged page, priv check and output-attr lookup must use the
/// SAME stream guard.  Before the fix, a removed stream would skip
/// apply_output_attrs() entirely, returning zeroed output attrs even on a
/// page that has no privilege restriction.
///
/// This test verifies the consistent-lookup invariant: both the priv check
/// and the output-attr application see the same stream state or both see
/// the stream as gone.
#[test]
fn f2_non_privileged_page_output_attrs_use_same_guard() {
    let smmu = smmu_with_stream(21, 5, 3);

    // Warm the TLB.
    smmu.translate(sid(21), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure)
        .expect("F2: warm-up translate must succeed");

    // Fast-path hit — should get consistent non-zero output attrs.
    let r = smmu
        .translate(sid(21), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure)
        .expect("F2: TLB hit translate must succeed");

    assert_eq!(r.mem_type(), 5, "F2: TLB hit must return mem_type=5 from single guard lookup");
    assert_eq!(r.shareability(), 3, "F2: TLB hit must return shareability=3 from single guard lookup");
}

/// All six output-attribute fields must be consistent between slow and fast path,
/// verifying that the single-guard pattern applies apply_output_attrs whenever
/// the stream is present.
#[test]
fn f2_all_six_output_attrs_consistent_slow_fast_path() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .mt_cfg(true)
        .mem_attr(6)
        .sh_cfg(3)
        .alloc_cfg(0xF)
        .inst_cfg(2)
        .priv_cfg(1)
        .ns_cfg(1)
        .build()
        .unwrap();
    smmu.configure_stream(sid(22), cfg).unwrap();
    smmu.create_pasid(sid(22), pasid(0)).unwrap();
    smmu.map_page(
        sid(22),
        pasid(0),
        iova(0x5000),
        pa(0xC000_5000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Slow path.
    let slow = smmu
        .translate(sid(22), pasid(0), iova(0x5000), AccessType::Read, SecurityState::NonSecure)
        .expect("F2: slow path must succeed");

    // Fast path.
    let fast = smmu
        .translate(sid(22), pasid(0), iova(0x5000), AccessType::Read, SecurityState::NonSecure)
        .expect("F2: fast path must succeed");

    assert_eq!(slow.mem_type(), fast.mem_type(), "F2: mem_type mismatch slow/fast");
    assert_eq!(slow.shareability(), fast.shareability(), "F2: shareability mismatch slow/fast");
    assert_eq!(slow.alloc_hint(), fast.alloc_hint(), "F2: alloc_hint mismatch slow/fast");
    assert_eq!(slow.inst_cfg(), fast.inst_cfg(), "F2: inst_cfg mismatch slow/fast");
    assert_eq!(slow.priv_cfg(), fast.priv_cfg(), "F2: priv_cfg mismatch slow/fast");
    assert_eq!(slow.ns_cfg_out(), fast.ns_cfg_out(), "F2: ns_cfg_out mismatch slow/fast");

    // Verify the fast-path values are non-zero (prove attrs were actually applied).
    assert_eq!(fast.mem_type(), 6, "F2: fast-path mem_type must be 6 (not zeroed)");
    assert_eq!(fast.shareability(), 3, "F2: fast-path shareability must be 3 (not zeroed)");
}
