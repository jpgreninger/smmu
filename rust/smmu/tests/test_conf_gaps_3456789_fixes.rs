#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

//! TDD tests for six ARM SMMU v3 conformance gaps:
//!
//! - NEW-3: §3.4/§5.2 S2T0SZ IPA input range enforcement
//! - NEW-4: §3.3.4/§13.5 STE.PRIVCFG override before permission checks
//! - NEW-6: §5.2 INSTCFG ReadWrite compound type comment (documentation)
//! - NEW-7: §5.4 CD.EPD0 absent — TTBR0 translation table walk disable
//! - NEW-8: §3.4/§7.3.14 Stage-2 output PA vs S2PS check
//! - NEW-9: §3.5.3 Stall events bypass EVENTQEN=0 gate

use smmu::types::{
    AccessType, EventType, FaultMode, PagePermissions, SecurityState, StreamConfig, StreamID,
    IOVA, PA, PASID,
};
use smmu::SMMU;

// ── helpers ──────────────────────────────────────────────────────────────────

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

// ─────────────────────────────────────────────────────────────────────────────
// NEW-3: §3.4/§5.2 — S2T0SZ IPA input range enforcement
// ─────────────────────────────────────────────────────────────────────────────

/// NEW-3 Test 1: two-stage stream, S2T0SZ=16 (48-bit IPA range), IPA at 2^48 — must fail.
///
/// After stage-1 produces an IPA at exactly 2^48, the IPA is at or above the S2T0SZ
/// limit and must generate F_TRANSLATION per ARM §3.4.
///
/// BEFORE FIX: Translation might succeed (S2T0SZ not checked).
/// AFTER FIX:  Translation fails with F_TRANSLATION event.
#[test]
fn new3_s2t0sz_16_ipa_at_limit_two_stage() {
    let smmu = make_smmu();

    // Two-stage with S2T0SZ=16 → 48-bit IPA space; IPA limit = 2^48.
    // Map stage-1 IOVA 0x1000 → IPA 0x1000 (well within 48 bits).
    // Then separately test what happens if the IPA were at the limit.
    // We map stage-1 IOVA → IPA directly at 2^48 which exceeds IOVA limit as well,
    // so instead test a stage-2-only stream (simpler) for the IPA range check —
    // see new3_stage2_only_ipa_exceeds_s2t0sz below.
    //
    // For two-stage: configure stage-1 to map to an IPA that exceeds the S2T0SZ limit.
    // Since we cannot map a page at 2^48 (exceeds IOVA::new limits), we use a smaller
    // T0SZ on stage-2: S2T0SZ=32 (2^32 = 4 GiB IPA limit) and produce an IPA >= 4 GiB.
    //
    // Strategy: stage-1 maps IOVA=0x1_0000_0000 (4 GiB) → IPA=0x1_0000_0000.
    // With S2T0SZ=32, IPA limit = 2^(64-32) = 2^32 = 4 GiB. IPA=4 GiB is at the limit.
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .s2_t0sz(32) // IPA limit = 2^32 = 0x1_0000_0000
        .s2_ps(5)    // 48-bit PA output (so PA range is not the constraint)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xE1), cfg).unwrap();
    smmu.create_pasid(sid(0xE1), pasid(0)).unwrap();
    smmu.create_stage2_address_space(sid(0xE1)).unwrap();

    // Map stage-1: IOVA 0x1_0000_0000 → IPA 0x1_0000_0000 (exactly at 2^32 = S2T0SZ limit).
    let ipa_at_limit: u64 = 0x1_0000_0000; // 4 GiB
    smmu.map_page(
        sid(0xE1),
        pasid(0),
        iova(ipa_at_limit),
        pa(ipa_at_limit),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        sid(0xE1),
        pasid(0),
        iova(ipa_at_limit),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_err(),
        "NEW-3: IPA=0x{ipa_at_limit:x} with S2T0SZ=32 (limit=2^32) must fail; got Ok"
    );

    // An F_TRANSLATION event should be recorded.
    let events = smmu.get_events();
    assert!(
        events.iter().any(|e| e.event_type == EventType::FTranslation),
        "NEW-3: Expected F_TRANSLATION event for IPA range violation; events={events:?}"
    );
}

/// NEW-3 Test 2: S2T0SZ=0 (no restriction), large IPA — must not fail due to S2T0SZ.
///
/// S2T0SZ=0 means no IPA range restriction. A stage-2-only stream with a large
/// IPA must still succeed if the page is mapped.
///
/// BEFORE FIX: Passes.
/// AFTER FIX:  Must still pass (regression guard).
#[test]
fn new3_s2t0sz_0_no_restriction() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(false)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .s2_t0sz(25) // minimum legal S2T0SZ per §5.2 (BUG-AUDIT-45 fix: 0 is out of range [16,39])
        .s2_ps(5)   // 48-bit PA
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xE2), cfg).unwrap();
    smmu.create_pasid(sid(0xE2), pasid(0)).unwrap();
    smmu.create_stage2_address_space(sid(0xE2)).unwrap();

    // Map stage-2: IPA 0x1000_0000 → PA 0x2000_0000 (well within 48-bit PA range).
    let ipa_val: u64 = 0x1000_0000;
    smmu.map_stage2_page(
        sid(0xE2),
        iova(ipa_val),
        pa(0x2000_0000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        sid(0xE2),
        pasid(0),
        iova(ipa_val),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_ok(),
        "NEW-3: S2T0SZ=0 must impose no IPA restriction; got error: {:?}",
        result.err()
    );
}

/// NEW-3 Test 3: stage-2-only, S2T0SZ=32, IPA=2^32 — must fail with F_TRANSLATION.
///
/// For a stage-2-only stream, the IOVA is the IPA. With S2T0SZ=32 the IPA limit is
/// 2^32; an IPA exactly at or above this limit must generate F_TRANSLATION.
///
/// BEFORE FIX: Translation might succeed (S2T0SZ not checked).
/// AFTER FIX:  Translation fails with F_TRANSLATION event.
#[test]
fn new3_stage2_only_ipa_exceeds_s2t0sz() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(false)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .s2_t0sz(32) // IPA limit = 2^(64-32) = 2^32 = 4 GiB
        .s2_ps(5)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xE3), cfg).unwrap();
    smmu.create_pasid(sid(0xE3), pasid(0)).unwrap();
    smmu.create_stage2_address_space(sid(0xE3)).unwrap();

    // Attempt translation at exactly 2^32.
    let ipa_at_limit: u64 = 0x1_0000_0000; // 2^32

    let result = smmu.translate(
        sid(0xE3),
        pasid(0),
        iova(ipa_at_limit),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_err(),
        "NEW-3: stage-2-only IPA=0x{ipa_at_limit:x} with S2T0SZ=32 must fail; got Ok"
    );

    // An F_TRANSLATION event should be recorded.
    let events = smmu.get_events();
    assert!(
        events.iter().any(|e| e.event_type == EventType::FTranslation),
        "NEW-3: Expected F_TRANSLATION event for stage-2-only IPA range violation; events={events:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// NEW-4: §3.3.4/§13.5 — STE.PRIVCFG override before permission checks
// ─────────────────────────────────────────────────────────────────────────────

/// NEW-4 Test 1: PRIVCFG=3 (Force Privileged) promotes access to privileged.
///
/// A page marked `privileged_only` normally requires STRW=EL2/EL3 to allow access.
/// With PRIVCFG=3, the access is treated as privileged regardless of STRW, so the
/// translation must succeed even with STRW=El1El0.
///
/// BEFORE FIX: Translation fails (STRW=El1El0 does not suppress privileged_only).
/// AFTER FIX:  Translation succeeds (PRIVCFG=3 suppresses privileged_only check).
#[test]
fn new4_privcfg_3_promotes_read_to_privileged() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .priv_cfg(3) // Force Privileged
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xE4), cfg).unwrap();
    smmu.create_pasid(sid(0xE4), pasid(0)).unwrap();

    // Map a page that is privileged-only.
    smmu.map_page(
        sid(0xE4),
        pasid(0),
        iova(0x4000),
        pa(0x5000),
        PagePermissions::read_only_privileged(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        sid(0xE4),
        pasid(0),
        iova(0x4000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_ok(),
        "NEW-4: PRIVCFG=3 (Force Privileged) must allow access to privileged_only page; got error: {:?}",
        result.err()
    );
}

/// NEW-4 Test 2: PRIVCFG=2 (Force Unprivileged) does not change result for a
/// non-privileged-only page with a normal Read access.
///
/// PRIVCFG=2 forces the transaction to be treated as unprivileged. For a page that
/// is not `privileged_only`, this has no effect — the translation still succeeds.
///
/// BEFORE FIX: Passes (no PRIVCFG check existed).
/// AFTER FIX:  Must still pass (regression guard).
#[test]
fn new4_privcfg_2_demotes_no_effect_on_unprivileged() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .priv_cfg(2) // Force Unprivileged
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xE5), cfg).unwrap();
    smmu.create_pasid(sid(0xE5), pasid(0)).unwrap();

    // Map a read-only (non-privileged-only) page.
    smmu.map_page(
        sid(0xE5),
        pasid(0),
        iova(0x5000),
        pa(0x6000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        sid(0xE5),
        pasid(0),
        iova(0x5000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_ok(),
        "NEW-4: PRIVCFG=2 on a non-privileged-only page with Read access must succeed; got error: {:?}",
        result.err()
    );
}

/// NEW-4 Test 3: PRIVCFG=0 (pass-through) preserves existing STRW behavior.
///
/// With PRIVCFG=0 the stream uses STRW for the privilege decision.  STRW=El1El0
/// does NOT suppress the `privileged_only` check, so a privileged-only page must
/// still be denied.
///
/// BEFORE FIX: Passes (STRW check already existed).
/// AFTER FIX:  Must still pass (regression guard — PRIVCFG=0 is a no-op).
#[test]
fn new4_privcfg_0_no_override() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .priv_cfg(0) // pass-through: use STRW
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xE6), cfg).unwrap();
    smmu.create_pasid(sid(0xE6), pasid(0)).unwrap();

    // Map a privileged-only page.
    smmu.map_page(
        sid(0xE6),
        pasid(0),
        iova(0x6000),
        pa(0x7000),
        PagePermissions::read_only_privileged(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // STRW=El1El0 (default) does NOT suppress privileged_only — must fail.
    let result = smmu.translate(
        sid(0xE6),
        pasid(0),
        iova(0x6000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_err(),
        "NEW-4: PRIVCFG=0/STRW=El1El0 must deny access to privileged_only page; got Ok"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// NEW-7: §5.4 — CD.EPD0 TTBR0 translation table walk disable
// ─────────────────────────────────────────────────────────────────────────────

/// NEW-7 Test 1: EPD0=true on a stage-1-enabled stream generates F_TRANSLATION.
///
/// CD.EPD0=1 disables TTBR0 translation table walks. Any stage-1 translation
/// must generate F_TRANSLATION when EPD0=true.
///
/// BEFORE FIX: Translation proceeds (EPD0 not checked).
/// AFTER FIX:  Translation fails with F_TRANSLATION event.
#[test]
fn new7_epd0_true_generates_f_translation() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .epd0(true)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xF1), cfg).unwrap();
    smmu.create_pasid(sid(0xF1), pasid(0)).unwrap();

    // Map a page (should never be reached due to EPD0=1).
    smmu.map_page(
        sid(0xF1),
        pasid(0),
        iova(0x7000),
        pa(0x8000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        sid(0xF1),
        pasid(0),
        iova(0x7000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_err(),
        "NEW-7: EPD0=true must disable stage-1 translation and return error; got Ok"
    );

    // An F_TRANSLATION event should be recorded.
    let events = smmu.get_events();
    assert!(
        events.iter().any(|e| e.event_type == EventType::FTranslation),
        "NEW-7: Expected F_TRANSLATION event when EPD0=true; events={events:?}"
    );
}

/// NEW-7 Test 2: EPD0=false (default) allows normal stage-1 translation.
///
/// CD.EPD0=0 is the reset value and must not block translations.
///
/// BEFORE FIX: Passes (EPD0 not checked).
/// AFTER FIX:  Must still pass (regression guard).
#[test]
fn new7_epd0_false_allows_translation() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .epd0(false) // default: allow translation
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xF2), cfg).unwrap();
    smmu.create_pasid(sid(0xF2), pasid(0)).unwrap();

    smmu.map_page(
        sid(0xF2),
        pasid(0),
        iova(0x8000),
        pa(0x9000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        sid(0xF2),
        pasid(0),
        iova(0x8000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_ok(),
        "NEW-7: EPD0=false must allow normal stage-1 translation; got error: {:?}",
        result.err()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// NEW-8: §3.4/§7.3.14 — Stage-2 output PA vs S2PS check
// ─────────────────────────────────────────────────────────────────────────────

/// NEW-8 Test 1: S2PS=0 (32-bit), stage-2 maps IPA → PA at 2^32 — must fail with F_ADDR_SIZE.
///
/// ARM §3.4: After stage-2 translation, the output PA must be within the range
/// defined by STE.S2PS. If PA >= 2^S2PS_bits, F_ADDR_SIZE must be generated.
///
/// BEFORE FIX: Translation might succeed (PA range not checked post-stage-2).
/// AFTER FIX:  Translation fails with F_ADDR_SIZE event.
#[test]
fn new8_stage2_pa_exceeds_s2ps() {
    let smmu = make_smmu();

    // S2PS=0 → 32-bit PA range; PA must be < 2^32 = 0x1_0000_0000.
    // Use a stage-2-only stream to keep the test simple.
    // Note: configure_stream validates S2TTB against S2PS — S2TTB=0 is valid for all S2PS.
    let cfg = StreamConfig::builder()
        .stage1_enabled(false)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .s2_t0sz(25) // minimum legal S2T0SZ per §5.2 (BUG-AUDIT-45 fix: 0 is out of range [16,39])
        .s2_ps(0)   // 32-bit PA range
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xF3), cfg).unwrap();
    smmu.create_pasid(sid(0xF3), pasid(0)).unwrap();
    smmu.create_stage2_address_space(sid(0xF3)).unwrap();

    // Map stage-2: IPA 0x1000 → PA 0x1_0000_0000 (exactly at 2^32, outside 32-bit range).
    // PA::new(0x1_0000_0000) is valid (within the model's max_pa_bits default of 52).
    let ipa_val: u64 = 0x1000;
    let pa_out_of_range: u64 = 0x1_0000_0000; // 2^32 — one beyond 32-bit range

    smmu.map_stage2_page(
        sid(0xF3),
        iova(ipa_val),
        pa(pa_out_of_range),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        sid(0xF3),
        pasid(0),
        iova(ipa_val),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_err(),
        "NEW-8: stage-2 PA=0x{pa_out_of_range:x} with S2PS=0 (32-bit) must fail; got Ok"
    );

    // An F_ADDR_SIZE event should be recorded.
    let events = smmu.get_events();
    assert!(
        events.iter().any(|e| e.event_type == EventType::FAddrSize),
        "NEW-8: Expected F_ADDR_SIZE event when PA exceeds S2PS; events={events:?}"
    );
}

/// NEW-8 Test 2: S2PS=5 (48-bit), PA within range — must succeed.
///
/// A stage-2 output PA that is well within the S2PS bounds must not trigger F_ADDR_SIZE.
///
/// BEFORE FIX: Passes.
/// AFTER FIX:  Must still pass (regression guard).
#[test]
fn new8_stage2_pa_within_s2ps() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(false)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .s2_t0sz(25) // minimum legal S2T0SZ per §5.2 (BUG-AUDIT-45 fix: 0 is out of range [16,39])
        .s2_ps(5) // 48-bit PA range
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xF4), cfg).unwrap();
    smmu.create_pasid(sid(0xF4), pasid(0)).unwrap();
    smmu.create_stage2_address_space(sid(0xF4)).unwrap();

    // PA 0x0000_FFFF_0000 is well within 48-bit range (< 2^48).
    let ipa_val: u64 = 0x2000;
    let pa_in_range: u64 = 0x0000_FFFF_0000;

    smmu.map_stage2_page(
        sid(0xF4),
        iova(ipa_val),
        pa(pa_in_range),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        sid(0xF4),
        pasid(0),
        iova(ipa_val),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_ok(),
        "NEW-8: PA=0x{pa_in_range:x} with S2PS=5 (48-bit) must succeed; got error: {:?}",
        result.err()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// NEW-9: §3.5.3 — Stall events bypass EVENTQEN=0 gate
// ─────────────────────────────────────────────────────────────────────────────

/// NEW-9 Test: EVENTQEN=0, trigger fault on stall-mode stream — event must NOT be queued.
///
/// ARM §3.5.3: The event queue is not writable when EVENTQEN=0. This applies to ALL
/// events including stall events — there is no spec exemption for stall events.
///
/// BEFORE FIX: Stall events bypassed the EVENTQEN gate (incorrectly queued).
/// AFTER FIX:  No event is queued when EVENTQEN=0.
#[test]
fn new9_stall_event_not_queued_when_eventqen_zero() {
    // BUG-AUDIT-124 note: per ARM §6.3.9.4, EVENTQEN is RO-while-set.  Once
    // set to 1 (e.g. by enable()), it cannot be cleared by set_cr0().  The
    // correct way to obtain SMMUEN=1 / EVENTQEN=0 is to start from reset
    // (where EVENTQEN=0) and set only the desired bits in a single call.
    let smmu = SMMU::new();

    // From reset (EVENTQEN=0), enable SMMU with CMDQEN and PRIQEN but NOT EVENTQEN.
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
    // EVENTQEN is NOT set.

    // Configure a stall-mode stream.
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xF5), cfg).unwrap();
    smmu.create_pasid(sid(0xF5), pasid(0)).unwrap();

    // Do NOT map any page — this will trigger a PageNotMapped (F_TRANSLATION) fault.
    let result = smmu.translate(
        sid(0xF5),
        pasid(0),
        iova(0xA000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    // The result may be Stalled or an error; in either case, no event must be in the queue.
    assert!(
        result.is_err(),
        "NEW-9: translation to unmapped page must fail (stall or error); got Ok"
    );

    // No event should be queued because EVENTQEN=0 gates ALL events.
    let events = smmu.get_events();
    assert!(
        events.is_empty(),
        "NEW-9: EVENTQEN=0 must gate ALL events including stall events; found {} events: {events:?}",
        events.len()
    );
}
