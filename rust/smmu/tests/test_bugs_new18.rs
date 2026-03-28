//! TDD failing tests for AUDIT-NEW-01, AUDIT-NEW-02, AUDIT-NEW-04, and
//! AUDIT-NEW-05 (Rust).
//!
//! Each test is written to FAIL with the current code (red) and PASS only after
//! the corresponding fix is applied (green), EXCEPT where explicitly marked as a
//! regression/baseline test.
//!
//! # AUDIT-NEW-01 — `get_idr0()` missing NS1ATS bit 11 (ARM §6.3.1)
//!
//! ARM IHI0070G.b §6.3.1: IDR0 bit 11 is NS1ATS — "Non-secure stage 1 ATS
//! supported."  When `set_ns1ats_supported(true)` is called and S2P is enabled,
//! this bit must be set in `get_idr0()`.
//!
//! Current `get_idr0()` (around lines 1217–1247 of `rust/smmu/src/smmu/mod.rs`):
//! there is NO expression that reads `ns1ats_supported` and ORs in `1u32 << 11`.
//! The field is read by `configure_stream()` for the SteIllegal guard but is
//! never reflected in IDR0.
//!
//! BEFORE FIX: `get_idr0() & (1 << 11)` is always 0 → failing tests FAIL.
//! AFTER FIX:  bit 11 is set when `ns1ats_supported=true` and S2P enabled → PASS.
//!
//! # AUDIT-NEW-02 — IDR0.NS1ATS must be RES0 when S2P==0 (ARM §6.3.1)
//!
//! ARM IHI0070G.b §6.3.1: IDR0.NS1ATS (bit 11) is RES0 when S2P (bit 0) is 0.
//! Existing precedent: IDR0.VMW (bit 17) and IDR0.Hyp (bit 9) are already gated
//! on S2P in the current code.
//!
//! This test is technically only red after the AUDIT-NEW-01 fix adds bit 11 to
//! `get_idr0()`; write it now as the complete spec-correct behaviour so that
//! AUDIT-NEW-01 and AUDIT-NEW-02 can be fixed together in a single pass.
//!
//! BEFORE AUDIT-NEW-01 fix: bit 11 is always 0 (so this test currently PASSES
//!   trivially), but is not testing the correct post-fix invariant.
//! AFTER AUDIT-NEW-01 fix without AUDIT-NEW-02 gate: bit 11 is 1 even when
//!   S2P=0 → this test FAILS → forcing the S2P gate to be added.
//! AFTER BOTH fixes: bit 11 is 0 when S2P=0 → PASSES.
//!
//! # AUDIT-NEW-03 — IDR0.PRI not formally gated on ATS (ARM §6.3.1, latent)
//!
//! ARM IHI0070G.b §6.3.1 states that PRI (bit 16) requires ATS (bit 10).
//! Since ATS is hardcoded to 1 in the Rust implementation, there is no failing
//! test needed here — the invariant is never violated by construction.
//! A future fix would gate PRI on ATS when ATS becomes configurable.
//!
//! # AUDIT-NEW-04 — `inject_ste_fetch_abort` and `inject_cd_fetch_abort` always
//! set SSV=false (ARM §7.3.20)
//!
//! ARM IHI0070G.b §7.3.20: The SSV (SubstreamID Valid) bit of an event entry
//! must be set to 1 when a SubstreamID was presented.  For abort events the
//! SMMU cannot know whether the original transaction carried a PASID, but the
//! spec requires SSV=1 when the stream is substream-capable (`s1cd_max > 0`),
//! because any transaction on such a stream may have carried a SubstreamID.
//!
//! `inject_walk_eabt()` (lines 1458–1461 of `rust/smmu/src/smmu/mod.rs`) already
//! implements this rule correctly:
//! ```text
//!   ssv: self.streams.get(&stream_id.as_u32())
//!            .map_or(0u8, |ctx| ctx.get_s1cd_max()) > 0
//!        || pasid.as_u32() != 0,
//! ```
//!
//! `inject_ste_fetch_abort()` (line 1396) and `inject_cd_fetch_abort()`
//! (line 1415) both use `..EventEntry::zeroed()` which sets `ssv=false`
//! unconditionally — they never read `s1cd_max`.
//!
//! BEFORE FIX: SSV is always `false` even for substream-capable streams → FAILS.
//! AFTER FIX:  SSV is `true` when `s1cd_max > 0` → PASSES.
//!
//! # AUDIT-NEW-05 — `inject_walk_eabt` regression (ARM §7.3.12)
//!
//! The NEW-AUDIT-05 fix (commit 3e02bbb / test_bugs_new17.rs) already added the
//! `is_stage2` and `event_class` parameters to `inject_walk_eabt()`.  Add one
//! regression test confirming that the `is_stage2=false, event_class=1` path
//! (single-stage walk abort) still produces the correct event.  This test will
//! always PASS.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]
#![allow(dead_code)]

use smmu::types::{EventType, FaultMode, SecurityState, StreamConfig, StreamID, IOVA, PASID};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

fn pasid(n: u32) -> PASID {
    PASID::new(n).unwrap()
}

fn iova(n: u64) -> IOVA {
    IOVA::new(n).unwrap()
}

/// Build an SMMU with all queues and global enable active.
fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    smmu
}

/// Build a stage-1-only `StreamConfig` with the given `s1cd_max`.
///
/// `stage1_enabled=true`, `stage2_enabled=false`.
fn stage1_config_with_s1cd_max(s1cd_max: u8) -> StreamConfig {
    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.fault_mode = FaultMode::Terminate;
    cfg.s1cd_max = s1cd_max;
    cfg
}

// ============================================================================
// AUDIT-NEW-01: get_idr0() missing NS1ATS bit 11 (ARM §6.3.1)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: NS1ATS bit 11 set when ns1ats_supported=true and S2P enabled
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §6.3.1: IDR0 bit 11 is NS1ATS.  When the SMMU is configured with
// `set_ns1ats_supported(true)` and S2P is enabled (default=true), `get_idr0()`
// must return bit 11 = 1.
//
// BEFORE FIX: `get_idr0()` never reads `ns1ats_supported` → bit 11 is 0 → FAILS.
// AFTER FIX:  `get_idr0()` ORs in `1u32 << 11` when ns1ats_supported → PASSES.

/// AUDIT-NEW-01: IDR0.NS1ATS (bit 11) must be 1 when `set_ns1ats_supported(true)`
/// is called and S2P is enabled.
///
/// ARM §6.3.1: NS1ATS = "Non-secure stage 1 ATS supported".
///
/// BEFORE FIX: `get_idr0()` does not include bit 11 → assertion fails.
/// AFTER FIX:  bit 11 is set → PASSES.
#[test]
fn ns1ats_set_when_supported_and_s2p_enabled() {
    // §6.3.1: NS1ATS (IDR0 bit 11) must be 1 when ns1ats_supported=true and S2P=1.
    // S2P defaults to true in SMMU::new(), so no explicit set_s2p_supported() needed.
    let smmu = make_smmu();

    smmu.set_ns1ats_supported(true);
    // S2P is the default (true) — no change needed.

    let idr0 = smmu.get_idr0();

    assert!(
        idr0 & (1u32 << 11) != 0,
        "AUDIT-NEW-01: IDR0.NS1ATS (bit 11) must be 1 when set_ns1ats_supported(true) \
         is called and S2P is enabled (default). \
         ARM §6.3.1: NS1ATS indicates non-secure stage-1 ATS is supported. \
         Current get_idr0() never reads ns1ats_supported → bit 11 is always 0. \
         IDR0=0x{:08X}",
        idr0
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 (baseline): NS1ATS bit 11 must be 0 when ns1ats_supported is not set
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §6.3.1: When `set_ns1ats_supported(true)` has NOT been called, the
// default ns1ats_supported=false means bit 11 must be 0.
//
// BEFORE FIX: passes trivially (bit 11 is always 0 before fix).
// AFTER FIX:  must still pass (regression — only ns1ats_supported=true sets bit 11).

/// AUDIT-NEW-01 baseline: IDR0.NS1ATS (bit 11) must be 0 when
/// `set_ns1ats_supported` is NOT called (default=false).
///
/// BEFORE FIX: passes (bit 11 always 0). AFTER FIX: must still pass (regression).
#[test]
fn ns1ats_zero_when_not_supported() {
    // §6.3.1: NS1ATS (bit 11) = 0 when ns1ats_supported=false (default).
    let smmu = make_smmu();
    // Do NOT call set_ns1ats_supported(true).

    let idr0 = smmu.get_idr0();

    assert!(
        idr0 & (1u32 << 11) == 0,
        "AUDIT-NEW-01 baseline: IDR0.NS1ATS (bit 11) must be 0 when \
         set_ns1ats_supported(true) has NOT been called (default=false). \
         ARM §6.3.1: NS1ATS is 0 when non-secure stage-1 ATS is not supported. \
         IDR0=0x{:08X}",
        idr0
    );
}

// ============================================================================
// AUDIT-NEW-02: IDR0.NS1ATS must be RES0 when S2P==0 (ARM §6.3.1)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: NS1ATS bit 11 must be 0 when S2P is disabled, even if ns1ats_supported=true
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §6.3.1: IDR0.NS1ATS (bit 11) is RES0 when S2P (bit 0) is 0.
// Existing precedent: IDR0.VMW (bit 17) uses the same guard:
//   `if self.s2p_supported.load(...) { 1u32 << 17 } else { 0 }`
// IDR0.Hyp (bit 9) similarly gates on both hyp_supported AND s2p_supported.
//
// Note: this test is trivially green BEFORE the AUDIT-NEW-01 fix (bit 11 is
// always 0), but will go red once AUDIT-NEW-01 adds bit 11 without an S2P gate.
// Write it now so both bugs are captured in a single test file.
//
// BEFORE AUDIT-NEW-01 fix:   trivially passes (bit 11 is always 0).
// AFTER AUDIT-NEW-01 only:   bit 11 set even with S2P=0 → FAILS.
// AFTER BOTH fixes applied:  bit 11 is 0 when S2P=0 → PASSES.

/// AUDIT-NEW-02: IDR0.NS1ATS (bit 11) must be 0 (RES0) when S2P is disabled,
/// even when `set_ns1ats_supported(true)` has been called.
///
/// ARM §6.3.1: NS1ATS is RES0 when S2P==0.  Follows the same gate as VMW/Hyp.
///
/// BEFORE AUDIT-NEW-01 fix: trivially passes.
/// AFTER AUDIT-NEW-01 without S2P gate: FAILS.
/// AFTER BOTH fixes: PASSES.
#[test]
fn ns1ats_res0_when_s2p_disabled() {
    // §6.3.1: NS1ATS (bit 11) = RES0 when S2P=0, regardless of ns1ats_supported.
    let smmu = make_smmu();

    smmu.set_ns1ats_supported(true);
    smmu.set_s2p_supported(false);

    let idr0 = smmu.get_idr0();

    assert!(
        idr0 & (1u32 << 11) == 0,
        "AUDIT-NEW-02: IDR0.NS1ATS (bit 11) must be RES0 (0) when S2P (bit 0) \
         is disabled, even when set_ns1ats_supported(true) was called. \
         ARM §6.3.1: NS1ATS requires S2P; same pattern as VMW (bit 17) and Hyp (bit 9). \
         Current code after AUDIT-NEW-01 fix would set bit 11 without checking S2P. \
         IDR0=0x{:08X}",
        idr0
    );
}

// ============================================================================
// AUDIT-NEW-04: inject_ste_fetch_abort / inject_cd_fetch_abort missing SSV
// (ARM §7.3.20)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 (baseline): inject_ste_fetch_abort on simple stream → ssv=false
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §7.3.20: SSV=false when the stream is not substream-capable (s1cd_max=0)
// and no PASID was presented.
//
// BEFORE FIX: passes (ssv is always false before fix, and should be false here).
// AFTER FIX:  must still pass (regression — simple streams must keep ssv=false).

/// AUDIT-NEW-04 baseline: `inject_ste_fetch_abort` on a simple stream
/// (s1cd_max=0) → SSV=false.
///
/// ARM §7.3.20: SSV=0 when the stream is not substream-capable.
///
/// BEFORE FIX: passes. AFTER FIX: must still pass (regression guard).
#[test]
fn ste_fetch_abort_ssv_false_for_simple_stream() {
    // §7.3.20: SSV=false for a non-substream-capable stream (s1cd_max=0).
    let smmu = make_smmu();
    let stream_id = sid(0x50);

    // Configure a stage-1-only stream with s1cd_max=0 (not substream-capable).
    let cfg = stage1_config_with_s1cd_max(0);
    smmu.configure_stream(stream_id, cfg)
        .expect("configure_stream (s1cd_max=0) should succeed");
    smmu.enable_stream(stream_id)
        .expect("enable_stream should succeed");

    smmu.clear_event_queue();

    smmu.inject_ste_fetch_abort(stream_id);

    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "AUDIT-NEW-04 baseline: inject_ste_fetch_abort must enqueue at least one event"
    );

    let ev = events
        .iter()
        .find(|e| e.event_type == EventType::FSteFetch)
        .expect("AUDIT-NEW-04 baseline: FSteFetch event not found");

    assert!(
        !ev.ssv,
        "AUDIT-NEW-04 baseline: inject_ste_fetch_abort on a simple stream \
         (s1cd_max=0) must produce SSV=false (ARM §7.3.20: no SubstreamID presented). \
         Got ssv={}",
        ev.ssv
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: inject_ste_fetch_abort on substream-capable stream → ssv=true
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §7.3.20: For an STE fetch abort on a substream-capable stream
// (s1cd_max > 0), SSV must be 1 because any transaction on such a stream may
// have carried a SubstreamID.  This matches the existing SSV rule in
// `inject_walk_eabt()`.
//
// BEFORE FIX: `inject_ste_fetch_abort` uses `..EventEntry::zeroed()` → ssv=false
//             even when s1cd_max > 0 → FAILS.
// AFTER FIX:  ssv is read from the stream's s1cd_max field → ssv=true → PASSES.

/// AUDIT-NEW-04: `inject_ste_fetch_abort` on a substream-capable stream
/// (s1cd_max > 0) → SSV=true.
///
/// ARM §7.3.20: SSV=1 when SubstreamID was presented (stream is substream-capable).
///
/// BEFORE FIX: `..EventEntry::zeroed()` → ssv=false → FAILS.
/// AFTER FIX:  s1cd_max read from stream → ssv=true → PASSES.
#[test]
fn ste_fetch_abort_ssv_true_for_substream_capable_stream() {
    // §7.3.20: SSV=true for a substream-capable stream (s1cd_max=4 > 0).
    let smmu = make_smmu();
    let stream_id = sid(0x51);

    // Configure a stage-1-only stream with s1cd_max=4 (substream-capable).
    // s1cd_max=4 means up to 2^4=16 substreams; any transaction on this stream
    // may have carried a SubstreamID, so SSV must be 1 in any fault event.
    let cfg = stage1_config_with_s1cd_max(4);
    smmu.configure_stream(stream_id, cfg)
        .expect("configure_stream (s1cd_max=4) should succeed");
    smmu.enable_stream(stream_id)
        .expect("enable_stream should succeed");

    smmu.clear_event_queue();

    smmu.inject_ste_fetch_abort(stream_id);

    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "AUDIT-NEW-04: inject_ste_fetch_abort must enqueue at least one event \
         for a substream-capable stream"
    );

    let ev = events
        .iter()
        .find(|e| e.event_type == EventType::FSteFetch)
        .expect("AUDIT-NEW-04: FSteFetch event not found (substream-capable stream)");

    assert!(
        ev.ssv,
        "AUDIT-NEW-04: inject_ste_fetch_abort on a substream-capable stream \
         (s1cd_max=4 > 0) must produce SSV=true. \
         ARM §7.3.20: SSV=1 when a SubstreamID was presented; for fault injection \
         on a substream-capable stream we cannot know if a SubstreamID was carried, \
         so SSV must follow the same rule as inject_walk_eabt(): ssv = s1cd_max > 0. \
         Current code uses ..EventEntry::zeroed() → ssv=false unconditionally. \
         Got ssv={}",
        ev.ssv
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3: inject_cd_fetch_abort on substream-capable stream → ssv=true
// ─────────────────────────────────────────────────────────────────────────────
//
// Same rule as Test 2 but for `inject_cd_fetch_abort`.
//
// ARM §7.3.10: F_CD_FETCH events carry the same SSV semantics as F_STE_FETCH
// and F_WALK_EABT (ARM §7.3.20).
//
// BEFORE FIX: `inject_cd_fetch_abort` uses `..EventEntry::zeroed()` → ssv=false → FAILS.
// AFTER FIX:  ssv is read from stream's s1cd_max field → ssv=true → PASSES.

/// AUDIT-NEW-04: `inject_cd_fetch_abort` on a substream-capable stream
/// (s1cd_max > 0) → SSV=true.
///
/// ARM §7.3.10 / §7.3.20: SSV=1 when stream is substream-capable.
///
/// BEFORE FIX: `..EventEntry::zeroed()` → ssv=false → FAILS.
/// AFTER FIX:  s1cd_max read from stream → ssv=true → PASSES.
#[test]
fn cd_fetch_abort_ssv_true_for_substream_capable_stream() {
    // §7.3.10/§7.3.20: SSV=true for a substream-capable stream (s1cd_max=8 > 0).
    let smmu = make_smmu();
    let stream_id = sid(0x52);

    // Configure a stage-1-only stream with s1cd_max=8 (substream-capable).
    let cfg = stage1_config_with_s1cd_max(8);
    smmu.configure_stream(stream_id, cfg)
        .expect("configure_stream (s1cd_max=8) should succeed");
    smmu.enable_stream(stream_id)
        .expect("enable_stream should succeed");

    smmu.clear_event_queue();

    smmu.inject_cd_fetch_abort(stream_id, pasid(0));

    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "AUDIT-NEW-04: inject_cd_fetch_abort must enqueue at least one event \
         for a substream-capable stream"
    );

    let ev = events
        .iter()
        .find(|e| e.event_type == EventType::FCdFetch)
        .expect("AUDIT-NEW-04: FCdFetch event not found (substream-capable stream)");

    assert!(
        ev.ssv,
        "AUDIT-NEW-04: inject_cd_fetch_abort on a substream-capable stream \
         (s1cd_max=8 > 0) must produce SSV=true. \
         ARM §7.3.10/§7.3.20: SSV=1 when a SubstreamID was presented; for fault \
         injection on a substream-capable stream the SSV rule must match \
         inject_walk_eabt(): ssv = s1cd_max > 0 || pasid != 0. \
         Current code uses ..EventEntry::zeroed() → ssv=false unconditionally. \
         Got ssv={}",
        ev.ssv
    );
}

// ============================================================================
// AUDIT-NEW-05 regression: inject_walk_eabt single-stage path still correct
// (ARM §7.3.12)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Regression: single-stage walk abort (is_stage2=false, event_class=1)
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §7.3.12: Single-stage walk abort → CLASS=TT (event_class=1), S2=false.
// The NEW-AUDIT-05 fix (commit 3e02bbb) added the `is_stage2` and `event_class`
// parameters to `inject_walk_eabt()`.  Confirm the existing default-behaviour
// path still produces the correct event fields.
//
// BEFORE FIX: always passes (this is the original behavour).
// AFTER FIX:  must still pass (regression guard).

/// AUDIT-NEW-05 regression: `inject_walk_eabt(is_stage2=false, event_class=1)`
/// produces `event_class=1` and `s2=false`.
///
/// ARM §7.3.12: single-stage walk abort → CLASS=TT, S2=0.
///
/// Always PASSES — regression guard confirming the existing fix is not broken.
#[test]
fn inject_walk_eabt_single_stage_regression() {
    // §7.3.12: single-stage walk abort (is_stage2=false, event_class=1).
    // This is the original default case; verify it still works after AUDIT-NEW-01/02/04 fixes.
    let smmu = make_smmu();

    smmu.inject_walk_eabt(
        sid(0x60),
        pasid(0),
        iova(0x1000),
        /*is_stage2=*/ false,
        /*event_class=*/ 1u8,
    );

    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "AUDIT-NEW-05 regression: inject_walk_eabt must enqueue at least one event"
    );

    let ev = events
        .iter()
        .find(|e| e.event_type == EventType::FWalkEabt)
        .expect("AUDIT-NEW-05 regression: FWalkEabt event not found");

    assert_eq!(
        ev.event_class, 1u8,
        "AUDIT-NEW-05 regression: single-stage walk abort must have event_class=1 \
         (CLASS=TT per ARM §7.3.12). Got event_class={}",
        ev.event_class
    );

    assert!(
        !ev.s2,
        "AUDIT-NEW-05 regression: single-stage walk abort must have s2=false. \
         Got s2={}",
        ev.s2
    );
}
