//! TDD tests for two ARM SMMU v3 Rust implementation bugs:
//!
//! - NEW-01 (§7.3): T0SZ inline event uses `matches!` instead of `can_write()`/`can_execute()`,
//!   and the `pnu` field is missing (defaults to false) — wrong for privileged access types.
//!
//! - NEW-03 (§7.3.13): `translate_two_stage_with_ipa()` returns `None` for the IPA when the
//!   S2T0SZ range check fails, causing the SMMU to emit an event with `s2=false` and `ipa=0`.
//!   The fix must return `Some(ipa_raw)` so the event reflects the actual IPA and `s2=true`.

#![allow(missing_docs)]
#![allow(clippy::doc_markdown)]

use smmu::types::{
    AccessType, EventType, FaultMode, PagePermissions, SecurityState, StreamConfig, StreamID,
    IOVA, PA, PASID,
};
use smmu::SMMU;

// ── helpers ───────────────────────────────────────────────────────────────────

fn make_smmu() -> SMMU {
    let s = SMMU::new();
    s.enable().unwrap();
    s
}

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

// =============================================================================
// NEW-01: T0SZ inline event — missing pnu, wrong rnw/ind for compound types
//
// BEFORE FIX: rnw = matches!(access, AccessType::Write) — misses WritePrivileged,
//             ReadWrite, ReadWritePrivileged etc.
//             ind = matches!(access, AccessType::Execute) — misses ExecutePrivileged etc.
//             pnu field absent (defaults to false) — wrong for privileged access types.
// AFTER FIX:  rnw = access.can_write()
//             ind = access.can_execute()
//             pnu = is_access_privileged(access) (via stream_ref before drop)
// =============================================================================

/// NEW-01 Test 1: T0SZ fault with PRIVCFG=3 and ReadPrivileged access → pnu=true.
///
/// Configure a stage-1 stream with PRIVCFG=3 (force privileged) and T0SZ=16 (48-bit VA).
/// Translate with an IOVA outside the 48-bit range.  The inline T0SZ event must have
/// `pnu=true` because PRIVCFG=3 forces the access to be privileged.
///
/// BEFORE FIX: pnu=false (field absent, ..EventEntry::zeroed() spread).
/// AFTER FIX:  pnu=true.
#[test]
fn new01_t0sz_fault_privcfg3_pnu_is_true() {
    let smmu = make_smmu();
    let stream_id = sid(0xB1);

    // Stage-1-only stream, PRIVCFG=3 (force privileged), T0SZ=16 (48-bit VA space).
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .stage2_enabled(false)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .t0sz(16)       // VA limit = 2^(64-16) = 2^48
        .priv_cfg(3)    // PRIVCFG=3: Force Privileged
        .build()
        .unwrap();
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    // IOVA above 2^48 → triggers T0SZ range check fault.
    let out_of_range_iova: u64 = 0x1_0000_0000_0000; // 2^48 exactly = at limit

    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(out_of_range_iova),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "NEW-01: IOVA=0x{out_of_range_iova:x} with T0SZ=16 must fail; got Ok"
    );

    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "NEW-01: Expected an F_TRANSLATION event to be recorded"
    );
    let ev = events.last().unwrap();
    assert_eq!(
        ev.event_type,
        EventType::FTranslation,
        "NEW-01: Expected FTranslation event; got {:?}",
        ev.event_type
    );
    // BEFORE FIX: pnu=false (field not set in inline event construction).
    // AFTER FIX:  pnu=true (PRIVCFG=3 forces privileged → pnu=true).
    assert!(
        ev.pnu,
        "NEW-01: T0SZ fault with PRIVCFG=3 must have pnu=true; got pnu=false"
    );
}

/// NEW-01 Test 2: T0SZ fault with WritePrivileged access → rnw=true.
///
/// `matches!(access, AccessType::Write)` returns false for `WritePrivileged`,
/// so the bug causes `rnw=false` for write-privileged accesses.  After the fix,
/// `access.can_write()` correctly returns true for `WritePrivileged`,
/// and BUG-NEW-RUST-1 fix: rnw = !can_write(), so Write → rnw=false (RnW=0=Write per ARM §7.3).
///
/// BEFORE BUG-NEW-RUST-1: rnw=can_write()=true for WritePrivileged (inverted — wrong).
/// AFTER BUG-NEW-RUST-1:  rnw=!can_write()=false (correct — ARM §7.3: RnW=0 means Write).
#[test]
fn new01_t0sz_fault_write_privileged_rnw_is_true() {
    let smmu = make_smmu();
    let stream_id = sid(0xB2);

    // Stage-1-only stream, T0SZ=16 (48-bit VA space).  No PRIVCFG override needed
    // for this test — we just verify rnw is set correctly for WritePrivileged.
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .stage2_enabled(false)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .t0sz(16) // VA limit = 2^48
        .build()
        .unwrap();
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    let out_of_range_iova: u64 = 0x1_0000_0000_0000; // at 2^48 limit

    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(out_of_range_iova),
        AccessType::WritePrivileged, // compound write-privileged type
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "NEW-01: IOVA=0x{out_of_range_iova:x} with T0SZ=16 must fail; got Ok"
    );

    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "NEW-01: Expected an F_TRANSLATION event to be recorded"
    );
    let ev = events.last().unwrap();
    assert_eq!(
        ev.event_type,
        EventType::FTranslation,
        "NEW-01: Expected FTranslation event; got {:?}",
        ev.event_type
    );
    // BUG-NEW-RUST-1 fix: ARM §7.3 RnW=0 means Write, RnW=1 means Read.
    // WritePrivileged.can_write()=true → rnw = !true = false (correct).
    assert!(
        !ev.rnw,
        "NEW-01: T0SZ fault with WritePrivileged must have rnw=false (ARM §7.3: RnW=0=Write); got rnw=true"
    );
}

// =============================================================================
// NEW-03: S2T0SZ violation returns None IPA → event has s2=false, ipa=0
//
// BEFORE FIX: translate_two_stage_with_ipa() returns (Err(..), None) for S2T0SZ
//             violation → SMMU emits event with s2=false and ipa=0.
// AFTER FIX:  Returns (Err(..), Some(ipa_raw)) so the event has s2=true and
//             ipa=<actual IPA>.
// =============================================================================

/// NEW-03 Test: Two-stage stream, S2T0SZ=16 (48-bit IPA), IPA produced by
/// stage-1 at 0x1_0000_0000_0000 (2^48 — at limit) → F_TRANSLATION event with
/// s2=true and ipa=0x1_0000_0000_0000.
///
/// We use S2T0SZ=32 and map stage-1 IOVA 0x1_0000_0000 → IPA 0x1_0000_0000
/// (4 GiB = 2^32 = at the S2T0SZ=32 limit) to keep address sizes manageable.
///
/// BEFORE FIX: event.s2=false, event.ipa=0 (None propagated to SMMU caller).
/// AFTER FIX:  event.s2=true,  event.ipa=0x1_0000_0000 (Some(ipa_raw)).
#[test]
fn new03_s2t0sz_violation_event_has_s2_true_and_correct_ipa() {
    let smmu = make_smmu();
    let stream_id = sid(0xC1);

    // Two-stage stream with S2T0SZ=32 → IPA limit = 2^(64-32) = 2^32 = 4 GiB.
    // S2PS=5 (48-bit PA output) so the PA range is not the binding constraint.
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .s2_t0sz(32) // IPA limit = 2^32 = 0x1_0000_0000
        .s2_ps(5)    // 48-bit PA output
        .t0sz(0)     // no VA range restriction
        .build()
        .unwrap();
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();

    // Stage-1 maps IOVA=0x1_0000_0000 → IPA=0x1_0000_0000 (exactly at 2^32 = S2T0SZ limit).
    // The S2T0SZ check fires: ipa_raw >= ipa_limit.
    let ipa_at_limit: u64 = 0x1_0000_0000; // 4 GiB
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(ipa_at_limit),
        pa(ipa_at_limit), // stage-1 output IPA = 4 GiB
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(ipa_at_limit),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "NEW-03: IPA=0x{ipa_at_limit:x} with S2T0SZ=32 must fail; got Ok"
    );

    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "NEW-03: Expected an F_TRANSLATION event to be recorded"
    );
    let ev = events.last().unwrap();
    assert_eq!(
        ev.event_type,
        EventType::FTranslation,
        "NEW-03: Expected FTranslation event; got {:?}",
        ev.event_type
    );
    // BEFORE FIX: s2=false (None returned from translate_two_stage_with_ipa).
    // AFTER FIX:  s2=true  (Some(ipa_raw) returned).
    assert!(
        ev.s2,
        "NEW-03: S2T0SZ fault event must have s2=true; got s2=false"
    );
    // BEFORE FIX: ipa=0 (None → ipa defaults to 0 in SMMU caller).
    // AFTER FIX:  ipa=0x1_0000_0000 (actual IPA).
    assert_eq!(
        ev.ipa, ipa_at_limit,
        "NEW-03: S2T0SZ fault event must have ipa=0x{ipa_at_limit:x}; got ipa=0x{:x}",
        ev.ipa
    );
}
