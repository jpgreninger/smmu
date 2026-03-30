//! TDD tests for NEW-04: EPD0 check missing in `translate_and_get_stage2_ipa()`.
//!
//! ARM IHI0070G.b §5.4 — CD.EPD0=1 disables ALL TTBR0 (stage-1) translation
//! table walks and generates `F_TRANSLATION`.  This applies regardless of whether
//! stage-2 is also enabled.
//!
//! BUG: `translate_and_get_stage2_ipa()` (the two-stage path that also returns
//! the IPA for event records) was missing the EPD0 guard that exists in the
//! single-stage `translate()` path.  When both stage-1 and stage-2 were enabled
//! and EPD0=true, the two-stage path called `translate_two_stage_with_ipa()`
//! without first checking EPD0, allowing stage-1 walks that should be blocked.
//!
//! FIX: Add `if stage1_enabled && stage2_enabled && epd0 { return (Err(PageNotMapped), None); }`
//! before the call to `translate_two_stage_with_ipa()`.
//!
//! TEST MUST FAIL before the fix and PASS after.

#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]

use smmu::types::{AccessType, PagePermissions, SecurityState, StreamConfig, StreamID, IOVA, PA, PASID};
use smmu::SMMU;

fn make_smmu() -> SMMU {
    let s = SMMU::new();
    s.enable().unwrap();
    s
}

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

/// Build a two-stage stream config with EPD0=true.
///
/// Both stage-1 and stage-2 are enabled; EPD0=true blocks all TTBR0 S1 walks.
fn two_stage_epd0_config() -> StreamConfig {
    StreamConfig::builder()
        .stage1_enabled(true)
        .stage2_enabled(true)
        .translation_enabled(true)
        .epd0(true)
        .t0sz(0)
        .s2_t0sz(16)
        .build()
        .unwrap()
}

/// Build a two-stage stream config with EPD0=false (the baseline / happy path).
fn two_stage_no_epd0_config() -> StreamConfig {
    StreamConfig::builder()
        .stage1_enabled(true)
        .stage2_enabled(true)
        .translation_enabled(true)
        .epd0(false)
        .t0sz(0)
        .s2_t0sz(16)
        .build()
        .unwrap()
}

// ─── NEW-04: EPD0 in two-stage mode ──────────────────────────────────────────

/// §5.4 / NEW-04:
/// When a stream is configured with stage-1 AND stage-2 enabled and EPD0=true,
/// `translate()` must return an error (`F_TRANSLATION` / `PageNotMapped`).
///
/// BEFORE FIX: The two-stage dispatch path in `translate_and_get_stage2_ipa()`
/// does not check EPD0, so it falls through to `translate_two_stage_with_ipa()`
/// and the translation succeeds (or fails for a different reason).
///
/// AFTER FIX: EPD0 is checked before calling `translate_two_stage_with_ipa()`;
/// the function returns `(Err(PageNotMapped), None)` immediately.
#[test]
fn new04_epd0_true_blocks_two_stage_translation() {
    let smmu = make_smmu();
    let stream_id = sid(0xE0);

    // Configure stream: two-stage with EPD0=true.
    smmu.configure_stream(stream_id, two_stage_epd0_config()).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();

    // Map a page in stage-1 address space: IOVA 0x1000 → IPA 0x5000.
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x1000),
        pa(0x5000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Map a page in stage-2 address space: IPA 0x5000 → PA 0xA000.
    smmu.map_stage2_page(
        stream_id,
        iova(0x5000),
        pa(0xA000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // With EPD0=true the stage-1 walk must be blocked, so translate() must fail.
    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "EPD0=true must block two-stage translation but got: {result:?}"
    );
}

/// §5.4 / NEW-04 (regression guard):
/// When EPD0=false, a fully-mapped two-stage translation must succeed.
/// This test verifies the fix does not break the normal two-stage path.
#[test]
fn new04_epd0_false_allows_two_stage_translation() {
    let smmu = make_smmu();
    let stream_id = sid(0xE1);

    // Configure stream: two-stage with EPD0=false (default / normal).
    smmu.configure_stream(stream_id, two_stage_no_epd0_config()).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();

    // Stage-1: IOVA 0x1000 → IPA 0x5000.
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x1000),
        pa(0x5000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Stage-2: IPA 0x5000 → PA 0xA000.
    smmu.map_stage2_page(
        stream_id,
        iova(0x5000),
        pa(0xA000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // With EPD0=false the translation must complete successfully.
    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_ok(),
        "EPD0=false must allow two-stage translation but got: {result:?}"
    );
}

/// §5.4 / NEW-04:
/// In two-stage mode with EPD0=true, no IPA is produced (stage-1 never ran),
/// so the event record must reflect a stage-1 fault (s2=false, ipa=0).
///
/// Specifically, the SMMU must NOT report a stage-2 fault even though both
/// stages are configured — because the stage-1 walk was blocked by EPD0 before
/// any IPA could be produced.
#[test]
fn new04_epd0_true_two_stage_no_ipa_in_event() {
    let smmu = make_smmu();
    let stream_id = sid(0xE2);

    smmu.configure_stream(stream_id, two_stage_epd0_config()).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();

    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x2000),
        pa(0x6000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    smmu.map_stage2_page(
        stream_id,
        iova(0x6000),
        pa(0xB000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x2000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(result.is_err(), "EPD0=true must block translation: {result:?}");

    // The event (if any) must NOT be a stage-2 fault: no IPA was produced.
    let events = smmu.get_events();
    for ev in &events {
        assert!(
            !ev.s2,
            "EPD0-blocked fault must be a stage-1 fault (s2=false); got s2=true in {ev:?}"
        );
        assert_eq!(
            ev.ipa, 0,
            "No IPA should be recorded when EPD0 blocks stage-1; got ipa={:#x} in {ev:?}",
            ev.ipa
        );
    }
}
