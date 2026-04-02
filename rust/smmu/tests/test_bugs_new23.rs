//! TDD failing tests for BUG-AUDIT-44, BUG-AUDIT-45, and BUG-AUDIT-46 (Rust).
//!
//! Each test is written to FAIL with the current code (red) and PASS only after
//! the corresponding fix is applied (green), EXCEPT where explicitly marked as a
//! regression/baseline test.
//!
//! # BUG-AUDIT-44 (§5.2 SteIllegal): S1CDMax > SSIDSIZE drops C_BAD_STE event
//!
//! ARM IHI0070G.b §5.2 SteIllegal():
//! `if UInt(STE.S1CDMax) > UInt(SMMU_IDR1.SSIDSIZE) then return TRUE` (SSIDSIZE=20).
//! When `s1cd_max > 20` AND `stage1_enabled`, `config.validate()` returns an error
//! (via `S1CD_MAX_LIMIT` check) but NO `CBadSte` event is emitted in
//! `configure_stream()` — the `validate()` error path does not call `enqueue_event()`.
//!
//! BEFORE FIX: `configure_stream()` returns `Err` but the event queue is empty → FAILS.
//! AFTER FIX:  `configure_stream()` returns `Err` AND emits `C_BAD_STE` → PASSES.
//!
//! # BUG-AUDIT-45 (§5.2 SteIllegal): S2T0SZ out-of-range accepted / wrong threshold
//!
//! ARM IHI0070G.b §5.2 SteIllegal():
//! Valid range for STT=0, 4KB granule, IAS=48 is [16, 39].
//! Current code:
//!   (a) `config.validate()` rejects `s2_t0sz > 48` but values 40–48 exceed spec max of 39
//!       and pass unchecked.
//!   (b) No `CBadSte` event is emitted for out-of-range values (below 16 or above 39).
//!
//! BEFORE FIX: `configure_stream()` returns `Ok` for s2_t0sz=40 (no event, no error) → FAILS.
//! AFTER FIX:  `configure_stream()` returns `Err` AND emits `C_BAD_STE` for [0,15] and [40,255].
//!
//! # BUG-AUDIT-46 (§6.3.1 IDR0 STALL_MODEL): reserved value 0b11 accepted silently
//!
//! ARM IHI0070G.b §6.3.1 IDR0 bits[25:24] STALL_MODEL:
//! `0b11 = Reserved`. The `set_stall_model(u8)` API currently stores any value
//! including 3 (0b11=reserved), which would expose a reserved encoding in IDR0.
//!
//! BEFORE FIX: `set_stall_model(3)` stores 3; `get_stall_model()` returns 3 → FAILS.
//! AFTER FIX:  `set_stall_model(3)` is a no-op; value stays at 0 → PASSES.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{EventType, SecurityState, StreamConfig, StreamID};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
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
// BUG-AUDIT-44: S1CDMax > SSIDSIZE must emit C_BAD_STE event
// ============================================================================

/// BUG-AUDIT-44 test 1: stage1_enabled=true, s1cd_max=21 → C_BAD_STE event emitted.
///
/// ARM §5.2 SteIllegal(): `UInt(STE.S1CDMax) > UInt(SMMU_IDR1.SSIDSIZE)` (SSIDSIZE=20)
/// → `SteIllegal` → must emit C_BAD_STE event.
/// Current code rejects via `config.validate()` but the validate() error path in
/// `configure_stream()` does NOT emit the event.
///
/// BEFORE FIX: no CBadSte event in queue → FAILS.
/// AFTER FIX:  CBadSte event emitted → PASSES.
#[test]
fn s1cd_max_exceeds_ssidsize_emits_event() {
    let smmu = make_smmu();
    smmu.set_s1p_supported(true);

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s1cd_max = 21; // exceeds SSIDSIZE=20

    let result = smmu.configure_stream(sid(100), cfg);
    assert!(
        result.is_err(),
        "BUG-AUDIT-44: configure_stream() must return Err when stage1_enabled=true AND \
         s1cd_max=21 > SSIDSIZE=20 (ARM §5.2 SteIllegal)"
    );
    assert!(
        has_event(&smmu, EventType::CBadSte),
        "BUG-AUDIT-44: C_BAD_STE event must be emitted when s1cd_max > SSIDSIZE \
         (ARM §5.2 SteIllegal — event was silently dropped via validate() error path)"
    );
}

/// BUG-AUDIT-44 test 2: stage1_enabled=true, s1cd_max=21 → returns Err (regression guard).
///
/// This verifies that configure_stream() continues to reject s1cd_max > 20 after the fix.
/// The validate() path already returns Err; the fix adds event emission before the return.
///
/// BEFORE / AFTER FIX: This must always pass (regression guard).
#[test]
fn s1cd_max_exceeds_ssidsize_rejected() {
    let smmu = make_smmu();
    smmu.set_s1p_supported(true);

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s1cd_max = 21;

    let result = smmu.configure_stream(sid(101), cfg);
    assert!(
        result.is_err(),
        "BUG-AUDIT-44: configure_stream() must return Err when s1cd_max=21 > SSIDSIZE=20 \
         (regression guard — validate() must still reject)"
    );
}

/// BUG-AUDIT-44 test 3: stage1_enabled=true, s1cd_max=20 → accepted (boundary guard).
///
/// Exactly at SSIDSIZE=20 is legal; must NOT emit CBadSte.
///
/// BEFORE / AFTER FIX: This must always pass (boundary test).
#[test]
fn s1cd_max_at_limit_accepted() {
    let smmu = make_smmu();
    smmu.set_s1p_supported(true);

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s1cd_max = 20; // exactly at SSIDSIZE limit — legal

    let result = smmu.configure_stream(sid(102), cfg);
    assert!(
        result.is_ok(),
        "BUG-AUDIT-44: configure_stream() must accept s1cd_max=20 (== SSIDSIZE=20, boundary ok) \
         (ARM §5.2 — range is [0, SSIDSIZE] inclusive)"
    );
    assert!(
        !has_event(&smmu, EventType::CBadSte),
        "BUG-AUDIT-44: no C_BAD_STE must be emitted when s1cd_max is within valid range"
    );
}

// ============================================================================
// BUG-AUDIT-45: S2T0SZ out-of-range [16,39] must emit C_BAD_STE event
// ============================================================================

/// BUG-AUDIT-45 test 1: stage2_enabled=true, s2_t0sz=10 (<16) → C_BAD_STE event emitted.
///
/// ARM §5.2 SteIllegal(): valid S2T0SZ range is [16, 39] for STT=0, 4KB granule, IAS=48.
/// Values below 16 are out of range and must be rejected with a CBadSte event.
///
/// BEFORE FIX: validate() rejects s2_t0sz=10 (> old > 48 threshold misses this, but
/// the real issue is no event is emitted for the in-range violation) → FAILS.
/// AFTER FIX:  CBadSte event emitted AND Err returned → PASSES.
#[test]
fn s2_t0sz_below_range_emits_event() {
    let smmu = make_smmu();
    smmu.set_s2p_supported(true);

    let mut cfg = StreamConfig::stage2_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s2_t0sz = 10; // below minimum of 16

    let result = smmu.configure_stream(sid(110), cfg);
    assert!(
        result.is_err(),
        "BUG-AUDIT-45: configure_stream() must return Err when stage2_enabled=true AND \
         s2_t0sz=10 < 16 (ARM §5.2 SteIllegal — out of range [16,39])"
    );
    assert!(
        has_event(&smmu, EventType::CBadSte),
        "BUG-AUDIT-45: C_BAD_STE event must be emitted when s2_t0sz < 16 \
         (ARM §5.2 SteIllegal — event was silently dropped)"
    );
}

/// BUG-AUDIT-45/88 test 2: stage2_enabled=true, s2_t0sz=45 (>44) → C_BAD_STE event emitted.
///
/// ARM §5.2 SteIllegal(): valid S2T0SZ range for 4KB granule, SL0=1 is [22, 44]
/// (BUG-AUDIT-88 updated the range from the old hardcoded [16,39]).
/// Values above 44 must be rejected.
///
/// BEFORE FIX: configure_stream() returns Ok (old threshold was > 48) → FAILS.
/// AFTER FIX:  CBadSte event emitted AND Err returned → PASSES.
#[test]
fn s2_t0sz_above_range_emits_event() {
    let smmu = make_smmu();
    smmu.set_s2p_supported(true);

    let mut cfg = StreamConfig::stage2_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s2_t0sz = 45; // above maximum of 44 (for s2_tg=0, s2_sl0=1)

    let result = smmu.configure_stream(sid(111), cfg);
    assert!(
        result.is_err(),
        "BUG-AUDIT-45/88: configure_stream() must return Err when stage2_enabled=true AND \
         s2_t0sz=45 > 44 (ARM §5.2 SteIllegal — out of range [22,44] for s2_tg=0, s2_sl0=1)"
    );
    assert!(
        has_event(&smmu, EventType::CBadSte),
        "BUG-AUDIT-45/88: C_BAD_STE event must be emitted when s2_t0sz > 44 \
         (ARM §5.2 SteIllegal — values above 44 must be rejected for s2_tg=0, s2_sl0=1)"
    );
}

/// BUG-AUDIT-45/88 test 3: stage2_enabled=true, s2_t0sz=10 → returns Err (regression guard).
///
/// Confirms the configure_stream() rejection for below-range values.
/// With BUG-AUDIT-88, the minimum for s2_tg=0, s2_sl0=1 is 22, so 10 is still below range.
///
/// BEFORE / AFTER FIX: This must always pass (regression guard).
#[test]
fn s2_t0sz_below_range_rejected() {
    let smmu = make_smmu();
    smmu.set_s2p_supported(true);

    let mut cfg = StreamConfig::stage2_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s2_t0sz = 10;

    let result = smmu.configure_stream(sid(112), cfg);
    assert!(
        result.is_err(),
        "BUG-AUDIT-45/88: configure_stream() must return Err for s2_t0sz=10 < 22 \
         (regression guard — below min for s2_tg=0, s2_sl0=1)"
    );
}

/// BUG-AUDIT-45/88 test 4: stage2_enabled=true, s2_t0sz=45 → returns Err.
///
/// Old threshold was `> 48`; s2_t0sz=40 passed silently.
/// After BUG-AUDIT-88 (range depends on (S2TG, S2SL0)), the valid range for
/// s2_tg=0, s2_sl0=1 is [22, 44], so s2_t0sz=45 must return Err.
///
/// BEFORE FIX: returns Ok (old validate() allowed 40–48) → FAILS.
/// AFTER FIX:  returns Err → PASSES.
#[test]
fn s2_t0sz_above_range_rejected() {
    let smmu = make_smmu();
    smmu.set_s2p_supported(true);

    let mut cfg = StreamConfig::stage2_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s2_t0sz = 45; // above max of 44 for (s2_tg=0, s2_sl0=1)

    let result = smmu.configure_stream(sid(113), cfg);
    assert!(
        result.is_err(),
        "BUG-AUDIT-45/88: configure_stream() must return Err for s2_t0sz=45 > 44 \
         (ARM §5.2 SteIllegal — valid range for s2_tg=0, s2_sl0=1 is [22, 44])"
    );
}

/// BUG-AUDIT-45/88 test 5: stage2_enabled=true, s2_t0sz=22 and s2_t0sz=44 → both accepted.
///
/// BUG-AUDIT-88 updated the valid range for s2_tg=0, s2_sl0=1 to [22, 44].
/// Boundary values at the legal limits must not be rejected.
///
/// BEFORE / AFTER FIX: This must always pass (boundary test).
#[test]
fn s2_t0sz_at_boundaries_accepted() {
    // Test lower boundary: s2_t0sz = 22 (minimum legal value for s2_tg=0, s2_sl0=1)
    let smmu_low = make_smmu();
    smmu_low.set_s2p_supported(true);
    let mut cfg_low = StreamConfig::stage2_only();
    cfg_low.security_state = SecurityState::NonSecure;
    cfg_low.s2_t0sz = 22; // lower bound — legal for (s2_tg=0, s2_sl0=1)

    let result_low = smmu_low.configure_stream(sid(114), cfg_low);
    assert!(
        result_low.is_ok(),
        "BUG-AUDIT-45/88: configure_stream() must accept s2_t0sz=22 (lower boundary for s2_tg=0, s2_sl0=1, ARM §5.2)"
    );
    assert!(
        !has_event(&smmu_low, EventType::CBadSte),
        "BUG-AUDIT-45/88: no C_BAD_STE must be emitted for s2_t0sz=22 (lower boundary for s2_tg=0, s2_sl0=1)"
    );

    // Test upper boundary: s2_t0sz = 44 (maximum legal value for s2_tg=0, s2_sl0=1)
    let smmu_high = make_smmu();
    smmu_high.set_s2p_supported(true);
    let mut cfg_high = StreamConfig::stage2_only();
    cfg_high.security_state = SecurityState::NonSecure;
    cfg_high.s2_t0sz = 44; // upper bound — legal for (s2_tg=0, s2_sl0=1)

    let result_high = smmu_high.configure_stream(sid(115), cfg_high);
    assert!(
        result_high.is_ok(),
        "BUG-AUDIT-45/88: configure_stream() must accept s2_t0sz=44 (upper boundary for s2_tg=0, s2_sl0=1, ARM §5.2)"
    );
    assert!(
        !has_event(&smmu_high, EventType::CBadSte),
        "BUG-AUDIT-45/88: no C_BAD_STE must be emitted for s2_t0sz=44 (upper boundary for s2_tg=0, s2_sl0=1)"
    );
}

// ============================================================================
// BUG-AUDIT-46: set_stall_model(0b11) must be a no-op (reserved value)
// ============================================================================

/// BUG-AUDIT-46 test 1: set_stall_model(3) must be ignored (reserved value 0b11).
///
/// ARM §6.3.1 IDR0 bits[25:24] STALL_MODEL: `0b11 = Reserved`.
/// Current code stores any value including 3 (0b11=reserved).
///
/// BEFORE FIX: get_stall_model() returns 3 after set_stall_model(3) → FAILS.
/// AFTER FIX:  get_stall_model() returns 0 (unchanged) after set_stall_model(3) → PASSES.
#[test]
fn set_stall_model_reserved_value_ignored() {
    let smmu = make_smmu();

    // Start from known state (default = 0)
    assert_eq!(
        smmu.get_stall_model(),
        0,
        "BUG-AUDIT-46: STALL_MODEL default must be 0 (both models supported)"
    );

    smmu.set_stall_model(3); // 0b11 = reserved per ARM §6.3.1

    assert_eq!(
        smmu.get_stall_model(),
        0,
        "BUG-AUDIT-46: set_stall_model(3) must be a no-op (0b11 is Reserved per ARM §6.3.1); \
         value must remain 0, not change to 3"
    );
}

/// BUG-AUDIT-46 test 2: valid values 0, 1, 2 are all accepted by set_stall_model().
///
/// Only 0b11 (3) is reserved; 0b00 (0), 0b01 (1), and 0b10 (2) are valid.
///
/// BEFORE / AFTER FIX: This must always pass (regression guard).
#[test]
fn set_stall_model_valid_values() {
    let smmu = make_smmu();

    smmu.set_stall_model(0);
    assert_eq!(
        smmu.get_stall_model(),
        0,
        "BUG-AUDIT-46: set_stall_model(0) must store 0 (both models, legal)"
    );

    smmu.set_stall_model(1);
    assert_eq!(
        smmu.get_stall_model(),
        1,
        "BUG-AUDIT-46: set_stall_model(1) must store 1 (terminate-only, legal)"
    );

    smmu.set_stall_model(2);
    assert_eq!(
        smmu.get_stall_model(),
        2,
        "BUG-AUDIT-46: set_stall_model(2) must store 2 (forced-stall, legal)"
    );
}

/// BUG-AUDIT-46 test 3: IDR0 bits[25:24] must not expose reserved value 0b11 after set_stall_model(3).
///
/// ARM §6.3.1 — IDR0.STALL_MODEL[25:24]: 0b11 is Reserved.
/// After set_stall_model(3) (which must be a no-op), IDR0 bits[25:24] must == 0.
///
/// BEFORE FIX: IDR0 bits[25:24] == 0b11 after set_stall_model(3) → FAILS.
/// AFTER FIX:  IDR0 bits[25:24] == 0b00 (value unchanged) → PASSES.
#[test]
fn get_idr0_stall_model_not_reserved() {
    let smmu = make_smmu();

    smmu.set_stall_model(3); // attempt to store reserved 0b11

    let idr0 = smmu.get_idr0();
    let stall_model_bits = (idr0 >> 24) & 0x3;
    assert_ne!(
        stall_model_bits,
        3,
        "BUG-AUDIT-46: IDR0 bits[25:24] must not be 0b11 (Reserved) after set_stall_model(3); \
         got IDR0=0x{idr0:08X}, STALL_MODEL={stall_model_bits}"
    );
    assert_eq!(
        stall_model_bits,
        0,
        "BUG-AUDIT-46: IDR0 bits[25:24] must remain 0b00 after rejected set_stall_model(3); \
         got IDR0=0x{idr0:08X}, STALL_MODEL={stall_model_bits}"
    );
}
