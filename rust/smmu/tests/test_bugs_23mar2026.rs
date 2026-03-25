//! Failing tests (TDD step 1) for two confirmed ARM SMMU v3 Rust bugs.
//!
//! Each test is designed to FAIL with the current buggy code and PASS only
//! after the corresponding fix is applied.
//!
//! # Bug Descriptions
//!
//! ## BUG-RUST-1: CMD_TLBI_NH_ASID ignores VMID
//!
//! Location: `smmu/mod.rs` — `process_single_command()` around line 5348.
//!
//! ARM §4.4.2.2: `CMD_TLBI_NH_ASID` invalidates by **ASID AND VMID** for
//! NS/Realm queues.  The current implementation calls `invalidate_by_asid()`
//! which matches ASID only, causing TLB entries for streams with a different
//! VMID to be incorrectly evicted.
//!
//! Observable symptom: a TLB entry tagged with VMID=20, ASID=5 is evicted by
//! `CMD_TLBI_NH_ASID(vmid=10, asid=5)` when it should NOT be.
//!
//! ## BUG-RUST-2: STRW validation in configure_stream() missing cases
//!
//! Location: `smmu/mod.rs` — `configure_stream()` around lines 1977–2000.
//!
//! Two STRW validation cases are missing from the current implementation:
//!
//! (a) NonSecure + STRW=El2 (bit\[0\]=1) — ARM §5.2 pattern 'x1': any stream
//!     where bit\[0\] of STRW is set (0b01=El2 or 0b11=El3) is illegal for
//!     NonSecure streams.  The current code only rejects El3 (0b11), not El2
//!     (0b01).
//!
//! (b) Secure + STRW=El3 (0b11) — ARM §5.2: STRW=El3 is forbidden even for
//!     Secure streams.  The current code allows Secure+El3.
//!
//! Both checks must be guarded by `strw_unused = !stage1_enabled`: when
//! stage-1 is not active (bypass / stage-2-only) the STRW field is IGNORED.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventType, PagePermissions, SecurityState, StreamConfig,
    StreamID, StreamWorld, IOVA, PA, PASID,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn sid(id: u32) -> StreamID {
    StreamID::new(id).unwrap()
}

fn pasid(id: u32) -> PASID {
    PASID::new(id).unwrap()
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

/// Build an SMMU with SMMUEN + CMDQEN + EVENTQEN enabled.
fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    smmu
}

// ============================================================================
// BUG-RUST-1: CMD_TLBI_NH_ASID must use joint VMID + ASID matching
// ============================================================================

/// BUG-RUST-1 (primary): `CMD_TLBI_NH_ASID` must NOT evict entries whose VMID
/// does not match, even when the ASID matches.
///
/// Setup:
///   - Stream A: vmid=10, asid=5  (NonSecure stage1-only; NS s1-only retains VMID from STE.S2VMID)
///   - Stream B: vmid=20, asid=5  (same ASID, different VMID)
///   - Translate both to warm the TLB.
///   - Issue `CMD_TLBI_NH_ASID(vmid=10, asid=5)`.
///
/// Expected after fix:
///   - Stream A TLB entry (vmid=10, asid=5) is evicted → cache miss on next translate.
///   - Stream B TLB entry (vmid=20, asid=5) is NOT evicted → cache hit on next translate.
///
/// BEFORE FIX: `invalidate_by_asid(5)` evicts BOTH entries → stream B gets a
/// spurious cache miss (this test checks stream B still hits, so it FAILS before the fix).
#[test]
fn bug_rust1_tlbi_nh_asid_uses_vmid_and_asid() {
    let smmu = make_smmu();

    // ── Stream A: vmid=10, asid=5, NS stage1-only (NS s1-only retains STE.S2VMID in TLB) ──
    let sa = sid(0xA1);
    let mut cfg_a = StreamConfig::stage1_only();
    cfg_a.vmid = 10;
    cfg_a.t0sz = 0;
    cfg_a.security_state = SecurityState::NonSecure;
    smmu.configure_stream(sa, cfg_a).unwrap();
    smmu.create_pasid(sa, pasid(0)).unwrap();
    // Set ASID=5 for this PASID via the CD.ASID API.
    smmu.set_pasid_asid(sa, pasid(0), 5).unwrap();
    smmu.map_page(
        sa,
        pasid(0),
        iova(0x1000),
        pa(0x2000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // ── Stream B: vmid=20, asid=5, same ASID different VMID ─────────────────
    let sb = sid(0xA2);
    let mut cfg_b = StreamConfig::stage1_only();
    cfg_b.vmid = 20;
    cfg_b.t0sz = 0;
    cfg_b.security_state = SecurityState::NonSecure;
    smmu.configure_stream(sb, cfg_b).unwrap();
    smmu.create_pasid(sb, pasid(0)).unwrap();
    smmu.set_pasid_asid(sb, pasid(0), 5).unwrap();
    smmu.map_page(
        sb,
        pasid(0),
        iova(0x1000),
        pa(0x3000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // ── Warm the TLB for both streams ─────────────────────────────────────────
    smmu.translate(sa, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    smmu.translate(sb, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();

    // Verify both are now TLB-warm (second translate → cache hit).
    let stats_before_a = smmu.get_cache_statistics();
    smmu.translate(sa, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    let stats_after_a = smmu.get_cache_statistics();
    assert!(
        stats_after_a.tlb_hits() > stats_before_a.tlb_hits(),
        "Stream A TLB must be warm before TLBI"
    );

    let stats_before_b = smmu.get_cache_statistics();
    smmu.translate(sb, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    let stats_after_b = smmu.get_cache_statistics();
    assert!(
        stats_after_b.tlb_hits() > stats_before_b.tlb_hits(),
        "Stream B TLB must be warm before TLBI"
    );

    // ── Issue CMD_TLBI_NH_ASID(vmid=10, asid=5) ──────────────────────────────
    let mut cmd = CommandEntry::new(CommandType::TlbiNhAsid, 0, 0);
    cmd.vmid = 10;
    cmd.asid = 5;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // ── Stream A (vmid=10, asid=5): must be evicted → cache miss ─────────────
    // Snapshot stats; translate; compare.
    let targeted_hits_before = smmu.get_cache_statistics().tlb_hits();
    let targeted_misses_before = smmu.get_cache_statistics().tlb_misses();
    smmu.translate(sa, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    assert_eq!(
        smmu.get_cache_statistics().tlb_hits(),
        targeted_hits_before,
        "BUG-RUST-1: Stream A (vmid=10,asid=5) must be evicted by TLBI(vmid=10,asid=5); \
         expected cache miss but got a hit"
    );
    assert!(
        smmu.get_cache_statistics().tlb_misses() > targeted_misses_before,
        "BUG-RUST-1: Stream A must get a cache miss after targeted TLBI"
    );

    // ── Stream B (vmid=20, asid=5): must NOT be evicted → cache hit ──────────
    let survivor_hits_before = smmu.get_cache_statistics().tlb_hits();
    smmu.translate(sb, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    assert!(
        smmu.get_cache_statistics().tlb_hits() > survivor_hits_before,
        "BUG-RUST-1: Stream B (vmid=20,asid=5) must NOT be evicted by TLBI(vmid=10,asid=5); \
         expected cache hit but got a miss"
    );
}

/// BUG-RUST-1 (control): `CMD_TLBI_EL2_ASID` must still use ASID-only matching
/// (VMID field is RES0 per ARM §4.4.2.10).  Entries with different VMIDs but
/// the same ASID must both be evicted.
///
/// This verifies that splitting the `TlbiNhAsid | TlbiEl2Asid` arm does NOT
/// inadvertently change the `TlbiEl2Asid` behaviour.
#[test]
fn bug_rust1_tlbi_el2_asid_remains_asid_only() {
    let smmu = make_smmu();

    // Two streams: different VMIDs, same ASID.
    let sa = sid(0xB1);
    let mut cfg_a = StreamConfig::stage1_only();
    cfg_a.vmid = 30;
    cfg_a.t0sz = 0;
    cfg_a.security_state = SecurityState::NonSecure;
    smmu.configure_stream(sa, cfg_a).unwrap();
    smmu.create_pasid(sa, pasid(0)).unwrap();
    smmu.set_pasid_asid(sa, pasid(0), 7).unwrap();
    smmu.map_page(
        sa,
        pasid(0),
        iova(0x4000),
        pa(0x5000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let sb = sid(0xB2);
    let mut cfg_b = StreamConfig::stage1_only();
    cfg_b.vmid = 40;
    cfg_b.t0sz = 0;
    cfg_b.security_state = SecurityState::NonSecure;
    smmu.configure_stream(sb, cfg_b).unwrap();
    smmu.create_pasid(sb, pasid(0)).unwrap();
    smmu.set_pasid_asid(sb, pasid(0), 7).unwrap();
    smmu.map_page(
        sb,
        pasid(0),
        iova(0x4000),
        pa(0x6000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Warm both.
    smmu.translate(sa, pasid(0), iova(0x4000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    smmu.translate(sb, pasid(0), iova(0x4000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    // Second hits to confirm warm.
    smmu.translate(sa, pasid(0), iova(0x4000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    smmu.translate(sb, pasid(0), iova(0x4000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();

    // Issue CMD_TLBI_EL2_ASID(asid=7): should evict ALL entries with asid=7
    // regardless of VMID.
    let mut cmd = CommandEntry::new(CommandType::TlbiEl2Asid, 0, 0);
    cmd.asid = 7;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // Both must be misses.
    let hits_before = smmu.get_cache_statistics().tlb_hits();
    let misses_before = smmu.get_cache_statistics().tlb_misses();
    smmu.translate(sa, pasid(0), iova(0x4000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    smmu.translate(sb, pasid(0), iova(0x4000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    let hits_after = smmu.get_cache_statistics().tlb_hits();
    let misses_after = smmu.get_cache_statistics().tlb_misses();

    assert_eq!(
        hits_after,
        hits_before,
        "CMD_TLBI_EL2_ASID must evict ALL asid=7 entries (ASID-only); \
         expected 0 hits but got some"
    );
    assert!(
        misses_after >= misses_before + 2,
        "CMD_TLBI_EL2_ASID must produce at least 2 cache misses (one per stream)"
    );
}

// ============================================================================
// BUG-RUST-2: STRW validation in configure_stream()
// ============================================================================

/// BUG-RUST-2a: NonSecure + STRW=El2 (bit\[0\]=1) must be rejected with C_BAD_STE.
///
/// ARM §5.2 pattern 'x1': bit\[0\] of STRW set means El2 or El3, both forbidden
/// for NonSecure streams.  The current code only rejects STRW=El3 (0b11) and
/// silently accepts STRW=El2 (0b01) for NonSecure streams.
///
/// BEFORE FIX: configure_stream() returns Ok → test FAILS.
/// AFTER FIX:  configure_stream() returns Err(C_BAD_STE) → test PASSES.
#[test]
fn bug_rust2a_ns_stream_strw_el2_rejected() {
    let smmu = make_smmu();
    let stream_id = sid(0xC1);

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.strw = StreamWorld::El2;

    let result = smmu.configure_stream(stream_id, cfg);
    assert!(
        result.is_err(),
        "BUG-RUST-2a: NonSecure + STRW=El2 must be rejected (ARM §5.2 bit[0]=1 rule); \
         got: {result:?}"
    );

    // A C_BAD_STE event must be emitted (EVENTQEN is enabled).
    let events = smmu.get_events();
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::CBadSte && e.stream_id == 0xC1),
        "BUG-RUST-2a: C_BAD_STE must be emitted for NonSecure+El2 rejection; \
         events: {events:?}"
    );
}

/// BUG-RUST-2b: Secure + STRW=El3 (0b11) must be rejected with C_BAD_STE.
///
/// ARM §5.2: STRW=El3 is forbidden even for Secure streams (only El2 and
/// El1El0/El2E2h are valid for Secure streams).
///
/// BEFORE FIX: configure_stream() returns Ok → test FAILS.
/// AFTER FIX:  configure_stream() returns Err(C_BAD_STE) → test PASSES.
#[test]
fn bug_rust2b_secure_stream_strw_el3_rejected() {
    let smmu = make_smmu();
    let stream_id = sid(0xC2);

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::Secure;
    cfg.strw = StreamWorld::El3;

    let result = smmu.configure_stream(stream_id, cfg);
    assert!(
        result.is_err(),
        "BUG-RUST-2b: Secure + STRW=El3 must be rejected (ARM §5.2); \
         got: {result:?}"
    );

    // A C_BAD_STE event must be emitted.
    let events = smmu.get_events();
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::CBadSte && e.stream_id == 0xC2),
        "BUG-RUST-2b: C_BAD_STE must be emitted for Secure+El3 rejection; \
         events: {events:?}"
    );
}

/// BUG-RUST-2a guard: bypass stream (stage1_enabled=false) with NS+El2 is ALLOWED
/// because `strw_unused = !stage1_enabled = true` → STRW field is ignored.
///
/// ARM §5.2: STRW is only meaningful when stage-1 is active.
///
/// BEFORE FIX (without strw_unused guard): configure_stream() returns Err → test FAILS.
/// AFTER FIX:  configure_stream() returns Ok → test PASSES.
#[test]
fn bug_rust2a_strw_unused_bypass_exempt() {
    let smmu = make_smmu();
    let stream_id = sid(0xC3);

    // bypass() has stage1_enabled=false → strw_unused=true → STRW check skipped.
    let mut cfg = StreamConfig::bypass();
    cfg.security_state = SecurityState::NonSecure;
    cfg.strw = StreamWorld::El2; // would be illegal if stage1 were active

    let result = smmu.configure_stream(stream_id, cfg);
    assert!(
        result.is_ok(),
        "BUG-RUST-2a guard: bypass (strw_unused) with NS+El2 must be ALLOWED; \
         got: {result:?}"
    );
}

/// BUG-RUST-2b positive: Secure + STRW=El2 must be ALLOWED (valid per spec).
///
/// El2 (0b01) is a legal STRW value for Secure streams; only El3 (0b11) is
/// forbidden for Secure streams.
///
/// BEFORE AND AFTER FIX: configure_stream() returns Ok → test PASSES both ways.
/// Included to guard against regressions.
#[test]
fn bug_rust2b_secure_stream_strw_el2_allowed() {
    let smmu = make_smmu();
    let stream_id = sid(0xC4);

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::Secure;
    cfg.strw = StreamWorld::El2;
    cfg.t0sz = 0;

    let result = smmu.configure_stream(stream_id, cfg);
    assert!(
        result.is_ok(),
        "BUG-RUST-2b guard: Secure + STRW=El2 must be ALLOWED (ARM §5.2); \
         got: {result:?}"
    );
}

/// BUG-RUST-2b privilege: Secure+El2 must promote access types just like El3 did.
///
/// After the El3→El2 migration in tests, El2 must preserve the STRW privilege-
/// promotion semantics (STRW ∈ {El2, El3} promotes unprivileged → privileged).
/// This verifies the migration is semantically equivalent.
#[test]
fn bug_rust2b_secure_stream_strw_el2_promotes_access() {
    let smmu = make_smmu();
    let stream_id = sid(0xC5);

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::Secure;
    cfg.strw = StreamWorld::El2;
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    // Map a privileged-only page.
    let perms = PagePermissions::read_only().with_privileged_only(true);
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x8000),
        pa(0x9000),
        perms,
        SecurityState::Secure,
    )
    .unwrap();

    // An unprivileged Read with STRW=El2 is promoted to ReadPrivileged,
    // which satisfies the privileged_only constraint → must succeed.
    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x8000),
        AccessType::Read,
        SecurityState::Secure,
    );
    assert!(
        result.is_ok(),
        "BUG-RUST-2b privilege: Secure+El2 must promote Read → ReadPrivileged on \
         privileged-only page; got: {result:?}"
    );
}
