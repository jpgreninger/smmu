//! TDD tests for BUG-AUDIT-127.
//!
//! ARM SMMU v3 spec §3.9.1.3 table:
//!
//! For ATS Translated transactions with ATSCHK=1, when the ordinary-transaction
//! equivalent would trigger a configuration error:
//!
//! | C_BAD_SUBSTREAMID / F_STREAM_DISABLED / C_BAD_STREAMID / C_BAD_STE / ... |
//! |  If ATSCHK==1, aborted.                                                   |
//! |  If ATSCHK==1 AND CR2.REC_CFG_ATS==1, the event IS recorded.              |
//! |  Otherwise, no event is recorded.                                          |
//!
//! BUG: The current ATS TT + ATSCHK=1 path calls self.translate() internally,
//! then rolls back ALL events it recorded and replaces them with F_TRANSL_FORBIDDEN.
//! This is wrong for config-class errors: C_BAD_SUBSTREAMID, F_STREAM_DISABLED,
//! etc. must keep their event (gated on REC_CFG_ATS=1), not be replaced with
//! F_TRANSL_FORBIDDEN.
//!
//! BEFORE FIX: ATS TT + ATSCHK=1 + C_BAD_SUBSTREAMID emits F_TRANSL_FORBIDDEN
//!             instead of C_BAD_SUBSTREAMID (when REC_CFG_ATS=1).
//! AFTER FIX:  ATS TT + ATSCHK=1 + config error emits the config error event
//!             (gated on REC_CFG_ATS=1) and aborts. No F_TRANSL_FORBIDDEN.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use smmu::{
    types::{AccessType, EventType, SecurityState, StreamConfig, TransactionType, IOVA, PASID},
    StreamID, SMMU,
};

fn sid(n: u32) -> StreamID { StreamID::new(n).unwrap() }

fn make_smmu_atschk_rec_cfg_ats() -> SMMU {
    let smmu = SMMU::new();
    // Set CR2.REC_CFG_ATS before enabling (CR2 is RO while SMMUEN=1).
    smmu.set_cr2(SMMU::CR2_REC_CFG_ATS);
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_ATSCHK);
    smmu
}

// ============================================================================
// BUG-AUDIT-127 Test 1: ATS TT + ATSCHK=1 + F_STREAM_DISABLED + REC_CFG_ATS=1
// ============================================================================

/// §3.9.1.3: ATS TT + ATSCHK=1 on S1DSS=0 stream (no substream presented)
/// with REC_CFG_ATS=1 → F_STREAM_DISABLED event recorded (not F_TRANSL_FORBIDDEN).
///
/// Setup: stage-1 stream with S1DSS=0 (substreams required); send ATS TT
/// without a PASID (SSV=0) → would trigger F_STREAM_DISABLED in ordinary translate.
///
/// This test FAILS before the fix because the rollback replaces F_STREAM_DISABLED
/// with F_TRANSL_FORBIDDEN.
#[test]
fn test_ats_tt_atschk1_stream_disabled_records_config_event() {
    let smmu = make_smmu_atschk_rec_cfg_ats();

    // Stage-1 stream with S1DSS=0 (substreams mandatory, no bypass for non-substream).
    // s1cd_max=1 so the stream is "substream capable".
    let cfg = StreamConfig {
        s1dss: 0,
        s1cd_max: 1,
        ..StreamConfig::stage1_only()
    };
    smmu.configure_stream(sid(1), cfg).unwrap();
    smmu.create_pasid(sid(1), PASID::new(0).unwrap()).unwrap();
    smmu.clear_event_queue();

    // Send ATS TT with SSV=0 (no PASID) → F_STREAM_DISABLED in inner translate.
    let result = smmu.translate_with_type(
        sid(1),
        PASID::new(0).unwrap(),
        IOVA::new(0x1000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated,
    );

    assert!(result.is_err(), "ATS TT + config error must fail");

    let events = smmu.get_events();
    // Must have a config-class event (F_STREAM_DISABLED or C_BAD_SUBSTREAMID),
    // NOT F_TRANSL_FORBIDDEN.
    let forbidden_count = events.iter().filter(|e| e.event_type == EventType::FTranslForbidden).count();
    let stream_disabled_count = events.iter().filter(|e| e.event_type == EventType::FStreamDisabled).count();
    assert_eq!(
        forbidden_count, 0,
        "ATS TT config error must NOT emit F_TRANSL_FORBIDDEN; \
         got: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
    assert_eq!(
        stream_disabled_count, 1,
        "ATS TT config error + REC_CFG_ATS=1 must emit F_STREAM_DISABLED; \
         got: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

// ============================================================================
// BUG-AUDIT-127 Test 2: ATS TT + ATSCHK=1 + C_BAD_SUBSTREAMID + REC_CFG_ATS=1
// ============================================================================

/// §3.9.1.3: ATS TT + ATSCHK=1 + out-of-range PASID + REC_CFG_ATS=1
/// → C_BAD_SUBSTREAMID event recorded (not F_TRANSL_FORBIDDEN).
///
/// Setup: stage-1 stream with s1cd_max=1 (valid PASIDs: 0 only);
/// send ATS TT with PASID=2 (out of range) → C_BAD_SUBSTREAMID.
#[test]
fn test_ats_tt_atschk1_bad_substreamid_records_config_event() {
    let smmu = make_smmu_atschk_rec_cfg_ats();

    let cfg = StreamConfig {
        s1cd_max: 1,  // valid PASID range: 0..1 (2^1 = 2, so 0 and 1 only)
        ..StreamConfig::stage1_only()
    };
    smmu.configure_stream(sid(2), cfg).unwrap();
    smmu.create_pasid(sid(2), PASID::new(0).unwrap()).unwrap();
    smmu.clear_event_queue();

    // PASID=2 is out of range for s1cd_max=1 (limit = 2^1 = 2, so PASID must be < 2).
    // Actually PASID=1 is valid (0 and 1), PASID=2 is out of range.
    let result = smmu.translate_with_type(
        sid(2),
        PASID::new(2).unwrap(),
        IOVA::new(0x1000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated,
    );

    assert!(result.is_err(), "ATS TT + out-of-range PASID must fail");

    let events = smmu.get_events();
    let forbidden_count = events.iter().filter(|e| e.event_type == EventType::FTranslForbidden).count();
    let bad_sub_count = events.iter().filter(|e| e.event_type == EventType::CBadSubstreamid).count();
    assert_eq!(
        forbidden_count, 0,
        "ATS TT C_BAD_SUBSTREAMID must NOT emit F_TRANSL_FORBIDDEN; \
         got: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
    assert_eq!(
        bad_sub_count, 1,
        "ATS TT C_BAD_SUBSTREAMID + REC_CFG_ATS=1 must emit C_BAD_SUBSTREAMID; \
         got: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

// ============================================================================
// Test 3: ATS TT + ATSCHK=1 + config error + REC_CFG_ATS=0 → no event
// ============================================================================

/// §3.9.1.3: ATS TT + ATSCHK=1 + config error + REC_CFG_ATS=0 → no event.
#[test]
fn test_ats_tt_atschk1_config_error_no_rec_cfg_ats_no_event() {
    let smmu = SMMU::new();
    // REC_CFG_ATS=0 (default), ATSCHK=1.
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_ATSCHK);

    let cfg = StreamConfig {
        s1cd_max: 1,
        ..StreamConfig::stage1_only()
    };
    smmu.configure_stream(sid(3), cfg).unwrap();
    smmu.create_pasid(sid(3), PASID::new(0).unwrap()).unwrap();
    smmu.clear_event_queue();

    // PASID=2 out of range → C_BAD_SUBSTREAMID; REC_CFG_ATS=0 → no event.
    let result = smmu.translate_with_type(
        sid(3),
        PASID::new(2).unwrap(),
        IOVA::new(0x1000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated,
    );

    assert!(result.is_err(), "ATS TT + config error must fail");

    let events = smmu.get_events();
    assert!(
        events.is_empty(),
        "ATS TT + ATSCHK=1 + config error + REC_CFG_ATS=0 must emit NO event; \
         got: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

// ============================================================================
// Regression: ATS TT + ATSCHK=1 + translation fault still emits F_TRANSL_FORBIDDEN
// ============================================================================

/// Regression: ATS TT + ATSCHK=1 + F_TRANSLATION (unmapped page) must still
/// emit F_TRANSL_FORBIDDEN (not the translation fault event).
#[test]
fn test_ats_tt_atschk1_translation_fault_still_emits_transl_forbidden() {
    use smmu::types::{PagePermissions, PA};
    let smmu = make_smmu_atschk_rec_cfg_ats();

    let cfg = StreamConfig { eats: 1, ..StreamConfig::stage1_only() };
    smmu.configure_stream(sid(4), cfg).unwrap();
    smmu.create_pasid(sid(4), PASID::new(0).unwrap()).unwrap();
    // Map one page, then try a different (unmapped) address.
    smmu.map_page(
        sid(4), PASID::new(0).unwrap(),
        IOVA::new(0x1000).unwrap(),
        PA::new(0x8000_1000).unwrap(),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();
    smmu.clear_event_queue();

    // Unmapped address → F_TRANSLATION → should become F_TRANSL_FORBIDDEN for ATS TT.
    let result = smmu.translate_with_type(
        sid(4), PASID::new(0).unwrap(),
        IOVA::new(0x9000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated,
    );

    assert!(result.is_err(), "ATS TT on unmapped page must fail");

    let events = smmu.get_events();
    let forbidden_count = events.iter().filter(|e| e.event_type == EventType::FTranslForbidden).count();
    let translation_count = events.iter().filter(|e| e.event_type == EventType::FTranslation).count();
    assert_eq!(forbidden_count, 1,
        "ATS TT + F_TRANSLATION must emit F_TRANSL_FORBIDDEN; got: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
    assert_eq!(translation_count, 0,
        "F_TRANSLATION must NOT appear for ATS TT; got: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}
