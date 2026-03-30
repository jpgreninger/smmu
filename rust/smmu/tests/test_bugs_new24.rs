//! TDD failing tests for BUG-AUDIT-47 (T0SZ/T1SZ min bound) and BUG-AUDIT-48 (s2_t0sz sentinel).
//!
//! BUG-AUDIT-47 (§5.4 CDTxSZInvalid): valid range [16,39] for this model. The minimum
//! bound (t0sz/t1sz < 16) is not currently checked. Values in [1,15] are accepted silently.
//!
//! BUG-AUDIT-48: configure_stream() rejects s2_t0sz=0 (sentinel) — the BUG-AUDIT-45 fix
//! omitted the `!= 0` guard that C++ correctly includes. Stage-2 streams with s2_t0sz=0
//! should be accepted (0 means "no IPA range restriction").
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{
    AccessType, EventType, PagePermissions, SecurityState, StreamConfig, StreamID, IOVA, PA, PASID,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}
fn pasid0() -> PASID {
    PASID::new(0).unwrap()
}
fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}
fn pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

/// Build an SMMU with all queues and global enable active.
fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    smmu
}

/// Returns true when the event queue contains at least one event of the given type.
fn has_event(smmu: &SMMU, event_type: EventType) -> bool {
    smmu.get_events().iter().any(|e| e.event_type == event_type)
}

// ============================================================================
// BUG-AUDIT-47: T0SZ/T1SZ below minimum bound [16] must emit C_BAD_CD
// ============================================================================

/// BUG-AUDIT-47 test 1: t0sz=10 (below minimum 16) must emit C_BAD_CD event on translate.
///
/// ARM §5.4 CDTxSZInvalid(): for VAX=0, STT=0, IAS=48, valid T0SZ range is [16, 39].
/// Values in [1,15] are silently accepted by the current code — no C_BAD_CD event is
/// generated and the translate call does not return Err.
///
/// BEFORE FIX: translate() returns Ok (no event emitted) → FAILS.
/// AFTER FIX:  translate() returns Err AND CBadCd event emitted → PASSES.
#[test]
fn t0sz_below_min_emits_event() {
    let smmu = make_smmu();
    smmu.set_s1p_supported(true);

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.t0sz = 10; // below minimum of 16
    cfg.t1sz = 0; // sentinel — no restriction
    smmu.configure_stream(sid(200), cfg).unwrap();
    smmu.create_pasid(sid(200), pasid0()).unwrap();
    smmu.map_page(
        sid(200),
        pasid0(),
        iova(0x1000),
        pa(0xA000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let _ = smmu.translate(
        sid(200),
        pasid0(),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        has_event(&smmu, EventType::CBadCd),
        "BUG-AUDIT-47: C_BAD_CD event must be emitted when t0sz=10 < 16 \
         (ARM §5.4 CDTxSZInvalid — minimum bound not currently checked)"
    );
}

/// BUG-AUDIT-47 test 2: t1sz=10 (below minimum 16) must emit C_BAD_CD event on translate.
///
/// ARM §5.4 CDTxSZInvalid(): for VAX=0, STT=0, IAS=48, valid T1SZ range is [16, 39].
/// When t0sz is valid (16) but t1sz is below the minimum, C_BAD_CD must still be emitted.
///
/// BEFORE FIX: translate() returns Ok (no event emitted) → FAILS.
/// AFTER FIX:  translate() returns Err AND CBadCd event emitted → PASSES.
#[test]
fn t1sz_below_min_emits_event() {
    let smmu = make_smmu();
    smmu.set_s1p_supported(true);

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.t0sz = 16; // valid
    cfg.t1sz = 10; // below minimum of 16
    smmu.configure_stream(sid(201), cfg).unwrap();
    smmu.create_pasid(sid(201), pasid0()).unwrap();
    smmu.map_page(
        sid(201),
        pasid0(),
        iova(0x1000),
        pa(0xB000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let _ = smmu.translate(
        sid(201),
        pasid0(),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        has_event(&smmu, EventType::CBadCd),
        "BUG-AUDIT-47: C_BAD_CD event must be emitted when t1sz=10 < 16 \
         (ARM §5.4 CDTxSZInvalid — t1sz minimum bound not currently checked)"
    );
}

/// BUG-AUDIT-47 test 3: t0sz=10 (below minimum 16) must cause translate to return Err.
///
/// ARM §5.4 CDTxSZInvalid(): out-of-range T0SZ values must abort translation with C_BAD_CD.
///
/// BEFORE FIX: translate() returns Ok (silently proceeds) → FAILS.
/// AFTER FIX:  translate() returns Err → PASSES.
#[test]
fn t0sz_below_min_translate_fails() {
    let smmu = make_smmu();
    smmu.set_s1p_supported(true);

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.t0sz = 10; // below minimum of 16
    cfg.t1sz = 0; // sentinel — no restriction
    smmu.configure_stream(sid(202), cfg).unwrap();
    smmu.create_pasid(sid(202), pasid0()).unwrap();
    smmu.map_page(
        sid(202),
        pasid0(),
        iova(0x1000),
        pa(0xC000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        sid(202),
        pasid0(),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_err(),
        "BUG-AUDIT-47: translate() must return Err when t0sz=10 < 16 \
         (ARM §5.4 CDTxSZInvalid — translation must be aborted for out-of-range T0SZ)"
    );
}

/// BUG-AUDIT-47 test 4 (regression): t0sz=0 (sentinel, no restriction) must be accepted.
///
/// The model uses t0sz=0 throughout as a "no restriction" sentinel. This must NOT be
/// treated as below-minimum by the new minimum-bound check.
///
/// BEFORE / AFTER FIX: This must always pass (regression guard).
#[test]
fn t0sz_sentinel_zero_accepted() {
    let smmu = make_smmu();
    smmu.set_s1p_supported(true);

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.t0sz = 0; // sentinel — no VA-range restriction
    cfg.t1sz = 0; // sentinel — no restriction
    smmu.configure_stream(sid(203), cfg).unwrap();
    smmu.create_pasid(sid(203), pasid0()).unwrap();
    smmu.map_page(
        sid(203),
        pasid0(),
        iova(0x1000),
        pa(0xD000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        sid(203),
        pasid0(),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_ok(),
        "BUG-AUDIT-47: translate() must succeed when t0sz=0 (sentinel, no VA restriction); \
         0 must NOT be rejected by the minimum-bound check (regression guard)"
    );
    assert!(
        !has_event(&smmu, EventType::CBadCd),
        "BUG-AUDIT-47: no C_BAD_CD must be emitted for t0sz=0 (sentinel — no restriction)"
    );
}

/// BUG-AUDIT-47 test 5 (regression): t0sz=16 (minimum boundary) must be accepted.
///
/// t0sz=16 is the lower bound of the valid range [16, 39] and must not be rejected.
///
/// BEFORE / AFTER FIX: This must always pass (boundary regression guard).
#[test]
fn t0sz_at_min_boundary_accepted() {
    let smmu = make_smmu();
    smmu.set_s1p_supported(true);

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.t0sz = 16; // minimum boundary — legal
    cfg.t1sz = 0;  // sentinel — no restriction
    smmu.configure_stream(sid(204), cfg).unwrap();
    smmu.create_pasid(sid(204), pasid0()).unwrap();
    smmu.map_page(
        sid(204),
        pasid0(),
        iova(0x1000),
        pa(0xE000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        sid(204),
        pasid0(),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_ok(),
        "BUG-AUDIT-47: translate() must succeed for t0sz=16 (minimum boundary, ARM §5.4 CDTxSZInvalid)"
    );
    assert!(
        !has_event(&smmu, EventType::CBadCd),
        "BUG-AUDIT-47: no C_BAD_CD must be emitted for t0sz=16 (lower boundary)"
    );
}

/// BUG-AUDIT-47 test 6 (regression): t0sz=39 (maximum boundary) must be accepted.
///
/// t0sz=39 is the upper bound of the valid range [16, 39]. For t0sz=39, the valid
/// IOVA range is [0, 2^(64-39)) = [0, 2^25). Using IOVA=0x1000 (< 2^25) is valid.
///
/// BEFORE / AFTER FIX: This must always pass (boundary regression guard).
#[test]
fn t0sz_at_max_boundary_accepted() {
    let smmu = make_smmu();
    smmu.set_s1p_supported(true);

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.t0sz = 39; // maximum boundary — legal; valid range [0, 2^25)
    cfg.t1sz = 0;  // sentinel — no restriction
    smmu.configure_stream(sid(205), cfg).unwrap();
    smmu.create_pasid(sid(205), pasid0()).unwrap();
    smmu.map_page(
        sid(205),
        pasid0(),
        iova(0x1000), // 0x1000 < 2^25 — within valid range for t0sz=39
        pa(0xF000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        sid(205),
        pasid0(),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_ok(),
        "BUG-AUDIT-47: translate() must succeed for t0sz=39 (maximum boundary, ARM §5.4 CDTxSZInvalid)"
    );
    assert!(
        !has_event(&smmu, EventType::CBadCd),
        "BUG-AUDIT-47: no C_BAD_CD must be emitted for t0sz=39 (upper boundary)"
    );
}

// ============================================================================
// BUG-AUDIT-48: configure_stream() must accept s2_t0sz=0 (sentinel)
// ============================================================================

/// BUG-AUDIT-48 test 1: configure_stream() with s2_t0sz=0 (sentinel) must return Ok.
///
/// ARM §5.2: s2_t0sz=0 is the model's "no IPA range restriction" sentinel, identical to
/// C++ behavior. The BUG-AUDIT-45 fix introduced a pre-check at configure_stream() that
/// correctly rejects s2_t0sz in [1,15] and [40,255], but omitted the `!= 0` guard,
/// causing s2_t0sz=0 to be incorrectly rejected as "< 16".
///
/// BEFORE FIX: configure_stream() returns Err for s2_t0sz=0 → FAILS.
/// AFTER FIX:  configure_stream() returns Ok → PASSES.
#[test]
fn configure_stream_s2_t0sz_zero_sentinel_accepted() {
    let smmu = make_smmu();
    smmu.set_s2p_supported(true);

    let mut cfg = StreamConfig::stage2_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s2_t0sz = 0; // sentinel — no IPA range restriction

    let result = smmu.configure_stream(sid(210), cfg);
    assert!(
        result.is_ok(),
        "BUG-AUDIT-48: configure_stream() must accept s2_t0sz=0 (sentinel = no IPA range \
         restriction); BUG-AUDIT-45 fix incorrectly rejects 0 as < 16 (ARM §5.2)"
    );
}

/// BUG-AUDIT-48 test 2: configure_stream() with s2_t0sz=0 must not emit C_BAD_STE event.
///
/// ARM §5.2: s2_t0sz=0 is a valid sentinel value. No C_BAD_STE event should be emitted.
///
/// BEFORE FIX: CBadSte event emitted → FAILS.
/// AFTER FIX:  no CBadSte event → PASSES.
#[test]
fn configure_stream_s2_t0sz_zero_no_bad_ste_event() {
    let smmu = make_smmu();
    smmu.set_s2p_supported(true);

    let mut cfg = StreamConfig::stage2_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s2_t0sz = 0; // sentinel — no IPA range restriction

    let _ = smmu.configure_stream(sid(211), cfg);

    assert!(
        !has_event(&smmu, EventType::CBadSte),
        "BUG-AUDIT-48: no C_BAD_STE event must be emitted for s2_t0sz=0 (sentinel); \
         the BUG-AUDIT-45 pre-check incorrectly treats 0 as out-of-range (ARM §5.2)"
    );
}

/// BUG-AUDIT-48 test 3 (regression): configure_stream() with s2_t0sz=16 must still be accepted.
///
/// Valid value at the minimum boundary; must remain accepted both before and after the fix.
///
/// BEFORE / AFTER FIX: This must always pass (regression guard).
#[test]
fn configure_stream_s2_t0sz_valid_regression() {
    let smmu = make_smmu();
    smmu.set_s2p_supported(true);

    let mut cfg = StreamConfig::stage2_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s2_t0sz = 16; // valid minimum boundary

    let result = smmu.configure_stream(sid(212), cfg);
    assert!(
        result.is_ok(),
        "BUG-AUDIT-48: configure_stream() must accept s2_t0sz=16 (valid minimum boundary, \
         ARM §5.2 — regression guard)"
    );
    assert!(
        !has_event(&smmu, EventType::CBadSte),
        "BUG-AUDIT-48: no C_BAD_STE must be emitted for valid s2_t0sz=16 (regression guard)"
    );
}
