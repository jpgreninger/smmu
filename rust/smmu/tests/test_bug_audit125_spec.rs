//! TDD test for BUG-AUDIT-125.
//!
//! ARM SMMU v3 spec §3.9.1.3 table:
//!
//! | STE.Config == 0b000 | If ATSCHK==1, aborted. (no event)            |
//! | STE.Config == 0b100 | If ATSCHK == 1, F_TRANSL_FORBIDDEN and aborted. |
//!
//! BUG: The ATS Translated (TT) + ATSCHK=1 path in `translate_with_type_ssv()`
//! checks `stream_is_bypass = !is_stage1_enabled() && !is_stage2_enabled()`.
//! For STE.Config==0b000 (abort mode), both stage flags are false, so
//! `stream_is_bypass` is **true** — and the code generates F_TRANSL_FORBIDDEN.
//! But the spec says abort-mode (Config==0b000) must be silently aborted with
//! NO event; F_TRANSL_FORBIDDEN is only correct for bypass (Config==0b100).
//!
//! BEFORE FIX: ATS TT + ATSCHK=1 on an abort stream emits F_TRANSL_FORBIDDEN.
//! AFTER FIX:  ATS TT + ATSCHK=1 on an abort stream silently aborts, no event.
//!             ATS TT + ATSCHK=1 on a bypass stream still emits F_TRANSL_FORBIDDEN.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use smmu::{
    types::{AccessType, EventType, SecurityState, StreamConfig, TransactionType, IOVA, PASID},
    StreamID, SMMU,
};

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

fn pasid0() -> PASID {
    PASID::new(0).unwrap()
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn make_enabled_smmu_atschk() -> SMMU {
    let smmu = SMMU::new();
    // CR0: SMMUEN | EVENTQEN | CMDQEN | ATSCHK (bit 4)
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_ATSCHK);
    smmu
}

// ============================================================================
// BUG-AUDIT-125: ATS TT + ATSCHK=1 on abort stream (Config==0b000)
// ============================================================================

/// §3.9.1.3 table row 1: STE.Config==0b000 + ATSCHK==1 + ATS TT → silent abort,
/// NO F_TRANSL_FORBIDDEN event.
///
/// This test FAILS before the fix because `stream_is_bypass` is true for abort
/// streams (both stage flags false) and the code emits F_TRANSL_FORBIDDEN.
#[test]
fn test_ats_tt_atschk1_abort_stream_no_event() {
    let smmu = make_enabled_smmu_atschk();

    // Configure abort-mode stream (STE.Config == 0b000 = disabled=true).
    let cfg = StreamConfig { disabled: true, ..StreamConfig::default() };
    smmu.configure_stream(sid(1), cfg).unwrap();
    smmu.clear_event_queue();

    let result = smmu.translate_with_type(
        sid(1),
        pasid0(),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated,
    );

    assert!(result.is_err(), "ATS TT on abort stream must fail");

    let events = smmu.get_events();
    assert!(
        events.is_empty(),
        "ATS TT + ATSCHK=1 on abort stream (Config==0b000) must NOT emit any event; \
         got: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

// ============================================================================
// Regression: ATS TT + ATSCHK=1 on bypass stream (Config==0b100)
// ============================================================================

/// §3.9.1.3 table row 2: STE.Config==0b100 (bypass) + ATSCHK==1 + ATS TT →
/// F_TRANSL_FORBIDDEN and aborted.
///
/// This must continue to work correctly after the fix.
#[test]
fn test_ats_tt_atschk1_bypass_stream_emits_transl_forbidden() {
    let smmu = make_enabled_smmu_atschk();

    // Configure bypass stream (stage1_enabled=false, stage2_enabled=false, disabled=false).
    let cfg = StreamConfig {
        stage1_enabled: false,
        stage2_enabled: false,
        disabled: false,
        ..StreamConfig::default()
    };
    smmu.configure_stream(sid(2), cfg).unwrap();
    smmu.clear_event_queue();

    let result = smmu.translate_with_type(
        sid(2),
        pasid0(),
        iova(0x2000),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated,
    );

    assert!(result.is_err(), "ATS TT on bypass stream + ATSCHK=1 must fail");

    let events = smmu.get_events();
    let count = events.iter().filter(|e| e.event_type == EventType::FTranslForbidden).count();
    assert_eq!(
        count, 1,
        "ATS TT + ATSCHK=1 on bypass stream must emit exactly 1 F_TRANSL_FORBIDDEN; \
         got: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

// ============================================================================
// Regression: ATS TT + ATSCHK=0 passes through for both abort and bypass
// ============================================================================

/// §3.9.1.3: When ATSCHK==0, no checks are performed — ATS TT proceeds.
/// For abort stream: the normal abort behavior applies (not F_TRANSL_FORBIDDEN).
#[test]
fn test_ats_tt_atschk0_abort_stream_no_transl_forbidden() {
    let smmu = SMMU::new();
    // ATSCHK=0 (not set)
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    let cfg = StreamConfig { disabled: true, ..StreamConfig::default() };
    smmu.configure_stream(sid(3), cfg).unwrap();
    smmu.clear_event_queue();

    let result = smmu.translate_with_type(
        sid(3),
        pasid0(),
        iova(0x3000),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated,
    );

    // ATS TT with ATSCHK=0 falls through to ordinary translate; abort stream → silent abort.
    assert!(result.is_err(), "ATS TT on abort stream must fail");

    let events = smmu.get_events();
    let forbidden_count = events
        .iter()
        .filter(|e| e.event_type == EventType::FTranslForbidden)
        .count();
    assert_eq!(
        forbidden_count, 0,
        "ATS TT + ATSCHK=0 on abort stream must NOT emit F_TRANSL_FORBIDDEN; \
         got: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}
