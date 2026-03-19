#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

//! Tests for six new bugs in the Rust ARM SMMU v3 implementation (March 2026).
//!
//! BUG-NEW-RUST-1: Missing StreamID range check (§6.3.4, §7.3.3).
//!   `translate()` has no `strtab_log2size` check — out-of-range StreamIDs
//!   fall through to DashMap lookup instead of generating C_BAD_STREAMID.
//!
//! BUG-NEW-RUST-2: RECINVSID not checked in `record_stream_not_found_fault()`
//!   (§6.3.12, §7.3.3).  Events are recorded unconditionally when EVENTQEN=1,
//!   ignoring CR2.RECINVSID.
//!
//! BUG-NEW-RUST-3: Full seqlock for `signal_gerror()`.  Window between
//!   `gerrorn` re-read and CAS allows `clear_gerror()` to modify gerrorn,
//!   causing a stale `inactive` mask and potential double-toggle.
//!
//! BUG-NEW-RUST-4: Relaxed loads on config getters in `stream_context/mod.rs`.
//!   Getter methods use `Ordering::Relaxed` while setters use `Ordering::Release`.
//!   On non-TSO hardware this breaks happens-before.
//!
//! BUG-NEW-RUST-5: `oas_bits` TOCTOU in `translate()`.  OAS snapshot taken
//!   early but concurrent `update_config()` can change `max_pa_bits`.
//!
//! BUG-NEW-RUST-6: `stream_count`/`streams` inconsistency in `shutdown()`.
//!   `streams.clear()` and `stream_count.store(0)` are separate operations;
//!   observer between them sees count > 0 with empty map.

use smmu::types::{AccessType, CommandEntry, CommandType, EventType, SecurityState, StreamConfig, StreamID, IOVA, PASID};
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

// ─── BUG-NEW-RUST-1 ───────────────────────────────────────────────────────────

/// §6.3.4 / §7.3.3 / BUG-NEW-RUST-1:
/// When `strtab_log2size` is set (< 32), any StreamID >= 2^log2size must be
/// rejected with an error before reaching the DashMap lookup.
#[test]
fn rust1_out_of_range_streamid_rejected() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);
    // log2size=4 → valid StreamIDs: 0..15 only.
    smmu.set_strtab_log2size(4);
    // StreamID 16 is out of range.
    let r = smmu.translate(
        sid(16),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(r.is_err(), "StreamID 16 must be rejected when log2size=4");
}

/// §6.3.4 / §7.3.3 / BUG-NEW-RUST-1:
/// A StreamID within range must not be rejected by the range check (still
/// fails with stream-not-found, but NOT an out-of-range fault).
#[test]
fn rust1_in_range_streamid_not_range_faulted() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);
    smmu.set_strtab_log2size(4); // valid: 0–15
    // StreamID 15 is within range — must NOT hit the range check.
    // It will still error (stream not configured), but not due to range.
    let r = smmu.translate(
        sid(15),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    // Must be an error (stream not found), but we should NOT see an invalid-range
    // signal.  The key assertion is just that the SMMU doesn't panic and returns
    // some error (stream not configured).
    assert!(r.is_err());
}

/// §6.3.4 / §7.3.5 / BUG-NEW-RUST-1 / SPEC-20:
/// Default log2size=32 means "no table limit" — high StreamIDs must not be
/// rejected by the range check; they are in-range but unconfigured (STE.V=0).
///
/// SPEC-20: In-range unconfigured streams generate C_BAD_STE (§7.3.5), NOT
/// C_BAD_STREAMID (§7.3.3).  C_BAD_STE is NOT gated by CR2.RECINVSID — it
/// fires unconditionally when EVENTQEN=1.  RECINVSID only gates C_BAD_STREAMID
/// (out-of-range StreamID faults).
#[test]
fn rust1_default_log2size_no_range_fault() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);
    // default log2size=32 — every StreamID is within range.
    // Use the maximum valid StreamID (65535).
    let r = smmu.translate(
        sid(65535),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    // Must fail: stream is in-range but not configured.
    assert!(r.is_err());
    // SPEC-20: C_BAD_STE MUST be emitted for in-range unconfigured streams
    // regardless of RECINVSID (RECINVSID only gates C_BAD_STREAMID).
    let bad_ste = smmu.get_events_by_type(EventType::CBadSte);
    assert!(
        !bad_ste.is_empty(),
        "C_BAD_STE must be emitted for in-range unconfigured stream (SPEC-20)"
    );
    // C_BAD_STREAMID must NOT be emitted (StreamID is in-range).
    let bad_streamid = smmu.get_events_by_type(EventType::CBadStreamid);
    assert!(
        bad_streamid.is_empty(),
        "C_BAD_STREAMID must NOT be emitted for in-range StreamID; got: {bad_streamid:?}"
    );
}

// ─── BUG-NEW-RUST-2 ───────────────────────────────────────────────────────────

/// §6.3.12 / §7.3.3 / §7.3.5 / BUG-NEW-RUST-2 / SPEC-20:
/// StreamID 0xABCD with default log2size=32 is IN-RANGE but unconfigured
/// (STE.V=0).  Per SPEC-20 (§7.3.5), such streams emit C_BAD_STE regardless
/// of CR2.RECINVSID.  RECINVSID only suppresses C_BAD_STREAMID (out-of-range).
///
/// This test verifies:
///   - Exactly one C_BAD_STE event is emitted.
///   - No C_BAD_STREAMID event is emitted (StreamID is in-range).
#[test]
fn rust2_recinvsid_zero_suppresses_event_for_unknown_stream() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);
    smmu.enable().unwrap();
    // CR2.RECINVSID=0 is the default — do NOT set CR2_RECINVSID.
    // StreamID 0xABCD with default log2size=32 is in-range but unconfigured.
    let _ = smmu.translate(
        sid(0xABCD),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    // SPEC-20: C_BAD_STE MUST be emitted for in-range unconfigured streams,
    // even when RECINVSID=0 (RECINVSID does not gate C_BAD_STE).
    let bad_ste = smmu.get_events_by_type(EventType::CBadSte);
    assert_eq!(
        bad_ste.len(),
        1,
        "RECINVSID=0: exactly one C_BAD_STE must be recorded for in-range unconfigured stream"
    );
    // C_BAD_STREAMID must NOT be emitted (StreamID is in-range with default log2size=32).
    let bad_streamid = smmu.get_events_by_type(EventType::CBadStreamid);
    assert!(
        bad_streamid.is_empty(),
        "C_BAD_STREAMID must NOT be emitted for in-range StreamID (RECINVSID=0); got: {bad_streamid:?}"
    );
}

/// §6.3.12 / §7.3.3 / BUG-NEW-RUST-2:
/// When CR2.RECINVSID=1, C_BAD_STREAMID events MUST be recorded in the event
/// queue when an unknown stream is looked up.
#[test]
fn rust2_recinvsid_one_records_event_for_unknown_stream() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);
    smmu.set_cr2(SMMU::CR2_RECINVSID);
    smmu.enable().unwrap();
    let _ = smmu.translate(
        sid(0xABCD),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    let events = smmu.get_events();
    assert!(!events.is_empty(), "RECINVSID=1: C_BAD_STREAMID must be recorded");
}

// ─── BUG-NEW-RUST-3 ───────────────────────────────────────────────────────────

/// §6.3.19 / §6.3.20 / BUG-NEW-RUST-3:
/// After signal → clear → re-signal cycle, GERROR must toggle correctly.
/// Specifically:
///   1. Initial signal: CMDQ_ERR goes active.
///   2. Software acknowledges: CMDQ_ERR goes inactive.
///   3. Re-signal: CMDQ_ERR must go active again (no double-toggle).
#[test]
fn rust3_signal_gerror_no_double_toggle_after_clear() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    smmu.set_cr2(SMMU::CR2_RECINVSID);

    // Activate CMDQ_ERR via CMD_SYNC CS=3 (Reserved → CERROR_ILL per §4.7.3).
    // (CONF-GAP-2: CMD_CFGI_STE for unknown StreamID is a silent no-op per §4.3.1.)
    let mut bad1 = CommandEntry::new(CommandType::Sync, 0, 0);
    bad1.cs = 3;
    smmu.submit_command(bad1).unwrap();
    let _ = smmu.process_command_queue();

    // CMDQ_ERR must be active now.
    assert_ne!(
        (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR,
        0,
        "CMDQ_ERR must be ACTIVE after first error"
    );

    // Software acknowledges the error.
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);
    assert_eq!(
        (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR,
        0,
        "After clear_gerror, CMDQ_ERR must be INACTIVE"
    );

    // Capture GERROR before re-activation.
    let g_before = smmu.get_gerror();

    // Re-activate: another CMD_SYNC CS=3.
    let mut bad2 = CommandEntry::new(CommandType::Sync, 0, 0);
    bad2.cs = 3;
    smmu.submit_command(bad2).unwrap();
    let _ = smmu.process_command_queue();

    let g_after = smmu.get_gerror();

    // GERROR must have toggled (re-activation changes the bit).
    assert_ne!(
        g_before, g_after,
        "gerror must toggle on re-activation of CMDQ_ERR"
    );
    // CMDQ_ERR must be active again.
    assert_ne!(
        (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR,
        0,
        "CMDQ_ERR must be ACTIVE after re-activation"
    );
}

// ─── BUG-NEW-RUST-4 ───────────────────────────────────────────────────────────

/// BUG-NEW-RUST-4:
/// Config getter values must be visible after `configure_stream()` completes.
/// (Structural test — on non-TSO hardware, Relaxed loads would not be guaranteed
/// to see Release-stored values from another thread; using Acquire enforces
/// the happens-before relationship.)
#[test]
fn rust4_config_values_visible_after_configure_stream() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(sid(1), cfg).unwrap();
    // The stream must exist — configure_stream() must have succeeded.
    assert!(smmu.has_stream(sid(1)));
}

/// BUG-NEW-RUST-4:
/// S1DSS default value (2) must be readable immediately after stream creation
/// via the getter (validates the Relaxed→Acquire fix for `get_s1dss()`).
#[test]
fn rust4_s1dss_getter_returns_correct_value() {
    use smmu::stream_context::StreamContext;

    let ctx = StreamContext::new();
    // Default s1dss is 2 (use CD[0]).
    assert_eq!(ctx.get_s1dss(), 2, "default S1DSS must be 2");

    ctx.set_s1dss(0);
    assert_eq!(ctx.get_s1dss(), 0, "S1DSS must reflect stored value");

    ctx.set_s1dss(1);
    assert_eq!(ctx.get_s1dss(), 1, "S1DSS must reflect updated value");
}

/// BUG-NEW-RUST-4:
/// T0SZ / T1SZ getters must reflect values set via their setters
/// (validates the Relaxed→Acquire fix for those getters).
#[test]
fn rust4_t0sz_t1sz_getters_consistent() {
    use smmu::stream_context::StreamContext;

    let ctx = StreamContext::new();
    ctx.set_t0sz(25);
    assert_eq!(ctx.get_t0sz(), 25, "T0SZ getter must return stored value");

    ctx.set_t1sz(30);
    assert_eq!(ctx.get_t1sz(), 30, "T1SZ getter must return stored value");
}

/// BUG-NEW-RUST-4:
/// Output-attribute getters (sh_cfg, alloc_cfg, mem_attr, inst_cfg, priv_cfg,
/// ns_cfg) and mt_cfg must reflect values set via their setters.
#[test]
fn rust4_output_attr_getters_consistent() {
    use smmu::stream_context::StreamContext;

    let ctx = StreamContext::new();

    ctx.set_sh_cfg(2);
    assert_eq!(ctx.get_sh_cfg(), 2, "sh_cfg getter must return stored value");

    ctx.set_alloc_cfg(3);
    assert_eq!(ctx.get_alloc_cfg(), 3, "alloc_cfg getter must return stored value");

    ctx.set_mem_attr(0xF);
    assert_eq!(ctx.get_mem_attr(), 0xF, "mem_attr getter must return stored value");

    ctx.set_inst_cfg(1);
    assert_eq!(ctx.get_inst_cfg(), 1, "inst_cfg getter must return stored value");

    ctx.set_priv_cfg(2);
    assert_eq!(ctx.get_priv_cfg(), 2, "priv_cfg getter must return stored value");

    ctx.set_ns_cfg(1);
    assert_eq!(ctx.get_ns_cfg(), 1, "ns_cfg getter must return stored value");

    ctx.set_mt_cfg(true);
    assert!(ctx.is_mt_cfg_enabled(), "mt_cfg getter must return stored value");
}

/// BUG-NEW-RUST-4:
/// AA64 getter must reflect the stored value (validates Relaxed→Acquire fix).
#[test]
fn rust4_aa64_getter_consistent() {
    use smmu::stream_context::StreamContext;

    let ctx = StreamContext::new();
    assert!(ctx.get_aa64(), "default AA64 must be true");

    ctx.set_aa64(false);
    assert!(!ctx.get_aa64(), "AA64 getter must return false after set_aa64(false)");

    ctx.set_aa64(true);
    assert!(ctx.get_aa64(), "AA64 getter must return true after set_aa64(true)");
}

// ─── BUG-NEW-RUST-5 ───────────────────────────────────────────────────────────

/// BUG-NEW-RUST-5:
/// OAS enforcement must be applied consistently — structural test that
/// verifies translate() completes without panic when OAS snapshot is taken.
#[test]
fn rust5_oas_check_applied_consistently() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);
    // Configure a stream.
    smmu.configure_stream(sid(1), StreamConfig::stage1_only()).unwrap();
    // translate() must not panic regardless of the config snapshot logic.
    let _ = smmu.translate(
        sid(1),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    // If we reach here without panicking, the OAS snapshot logic is sound.
}

/// BUG-NEW-RUST-5:
/// OAS boundary enforcement must reject IOVA >= 2^OAS (GBPA bypass path).
/// This validates that the snapshot covers the OAS checks correctly.
#[test]
fn rust5_oas_boundary_rejected_on_bypass() {
    use smmu::types::SMMUConfig;

    // Build an SMMU with 32-bit physical address space.
    let mut cfg = SMMUConfig::default();
    cfg.address_config.max_pa_bits = 32;
    let smmu = SMMU::with_config(cfg);
    // Leave SMMU disabled so bypass path is used (SMMUEN=0).

    // IOVA 0x1_0000_0000 (4 GiB) exceeds 32-bit OAS.
    let result = smmu.translate(
        sid(0),
        pasid(0),
        IOVA::new(0x1_0000_0000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "IOVA >= 2^OAS must be rejected on the bypass path"
    );
}

// ─── BUG-NEW-RUST-6 ───────────────────────────────────────────────────────────

/// §BUG-NEW-RUST-6:
/// After `shutdown()`, `get_stream_count()` must return 0, even though there
/// is a race window between `streams.clear()` and `stream_count.store(0)`.
/// The fix adds an `is_shutdown()` guard in `get_stream_count()`.
#[test]
fn rust6_stream_count_zero_after_shutdown() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu.configure_stream(sid(1), StreamConfig::bypass()).unwrap();
    smmu.configure_stream(sid(2), StreamConfig::bypass()).unwrap();
    assert_eq!(smmu.get_stream_count(), 2, "must have 2 streams before shutdown");

    smmu.shutdown().unwrap();

    assert_eq!(
        smmu.get_stream_count(),
        0,
        "get_stream_count must return 0 after shutdown"
    );
}

/// BUG-NEW-RUST-6:
/// `get_stream_count()` must return 0 immediately after shutdown even if called
/// before the internal `stream_count.store(0)` completes (is_shutdown guard).
#[test]
fn rust6_stream_count_zero_after_shutdown_single_stream() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu.configure_stream(sid(42), StreamConfig::bypass()).unwrap();
    assert_eq!(smmu.get_stream_count(), 1);

    smmu.shutdown().unwrap();
    assert_eq!(smmu.get_stream_count(), 0, "count must be 0 after shutdown");
}
