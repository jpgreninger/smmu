//! ARM SMMU v3 Conformance Gap Tests: NEW-13, NEW-15, NEW-19
//!
//! NEW-13 (§3.9.1.2/3.9.1.3): `SMMUEN=0` must still emit ATS-specific events
//!         (`F_BAD_ATS_TREQ` for TR, `F_TRANSL_FORBIDDEN` for TT).
//! NEW-15 (§6.3.12/§3.9.1.2): `CR2.REC_CFG_ATS` gates `C_BAD_STREAMID` recording
//!         for ATS Translation Requests.
//! NEW-19 (§3.9.1.2): ATS TR on `STE.Config=0b000` (disabled/abort stream) must
//!         be silent UR — no event generated.

use smmu::{
    types::{
        AccessType, EventType, PagePermissions, SecurityState, StreamConfig, TransactionType,
        IOVA, PA, PASID,
    },
    StreamID, SMMU,
};

// ─────────────────────────────────────────────────────────────────────────────
// Shared helpers
// ─────────────────────────────────────────────────────────────────────────────

fn make_enabled_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    smmu
}

/// Build an SMMU with one ATS-capable stage-1 stream and a mapped page.
fn make_smmu_s1_ats() -> SMMU {
    let smmu = make_enabled_smmu();
    let sid = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();
    let cfg = StreamConfig { eats: 2, ..StreamConfig::stage1_only() };
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, pasid).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    smmu.map_page(
        sid,
        pasid,
        iova,
        PA::new(0x8000_1000).unwrap(),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();
    smmu
}

// =============================================================================
// NEW-13: SMMUEN=0 emits F_BAD_ATS_TREQ (TR) or F_TRANSL_FORBIDDEN (TT)
// =============================================================================

/// NEW-13: `SMMUEN=0` + ATS TR + `CR2.REC_CFG_ATS=1` → `F_BAD_ATS_TREQ` emitted.
#[test]
fn test_new13_smmuen_zero_ats_tr_rec_cfg_ats_emits_event() {
    let smmu = make_smmu_s1_ats();
    // Set REC_CFG_ATS bit (bit 3 of CR2)
    smmu.set_cr2(SMMU::CR2_REC_CFG_ATS);
    // Disable SMMU (SMMUEN=0)
    smmu.set_cr0(SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN); // clears SMMUEN
    smmu.clear_event_queue();

    let sid = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    let result = smmu.translate_with_type(
        sid,
        pasid,
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
    );
    assert!(result.is_err(), "SMMUEN=0 ATS TR must fail");

    let events = smmu.get_events();
    let count = events.iter().filter(|e| e.event_type == EventType::FBadAtsTreq).count();
    assert_eq!(
        count, 1,
        "F_BAD_ATS_TREQ must be emitted when SMMUEN=0 and REC_CFG_ATS=1; events: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// NEW-13: `SMMUEN=0` + ATS TR + `CR2.REC_CFG_ATS=0` → no event (silent UR).
#[test]
fn test_new13_smmuen_zero_ats_tr_no_rec_cfg_ats_silent() {
    let smmu = make_smmu_s1_ats();
    // CR2 = 0: REC_CFG_ATS=0
    smmu.set_cr2(0);
    smmu.set_cr0(SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN); // SMMUEN=0
    smmu.clear_event_queue();

    let sid = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    let result = smmu.translate_with_type(
        sid,
        pasid,
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
    );
    assert!(result.is_err(), "SMMUEN=0 ATS TR must fail");

    let events = smmu.get_events();
    assert!(
        events.is_empty(),
        "No event must be emitted when SMMUEN=0 and REC_CFG_ATS=0; events: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// NEW-13: `SMMUEN=0` + ATS TT → `F_TRANSL_FORBIDDEN` always emitted (§7.3.8).
#[test]
fn test_new13_smmuen_zero_ats_tt_emits_transl_forbidden() {
    let smmu = make_smmu_s1_ats();
    smmu.set_cr2(0); // REC_CFG_ATS=0 should not suppress F_TRANSL_FORBIDDEN for TT
    smmu.set_cr0(SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN); // SMMUEN=0
    smmu.clear_event_queue();

    let sid = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    let result = smmu.translate_with_type(
        sid,
        pasid,
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated,
    );
    assert!(result.is_err(), "SMMUEN=0 ATS TT must fail");

    let events = smmu.get_events();
    let count =
        events.iter().filter(|e| e.event_type == EventType::FTranslForbidden).count();
    assert_eq!(
        count, 1,
        "F_TRANSL_FORBIDDEN must be emitted when SMMUEN=0; events: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// NEW-13: SMMUEN=0 + Ordinary type → bypass (no event); existing behavior preserved.
#[test]
fn test_new13_smmuen_zero_ordinary_bypass() {
    let smmu = make_smmu_s1_ats();
    smmu.set_cr0(SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN); // SMMUEN=0
    smmu.clear_event_queue();

    let sid = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    let result = smmu.translate(sid, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "SMMUEN=0 ordinary must bypass successfully");
    assert!(smmu.get_events().is_empty(), "No event on GBPA bypass");
}

// =============================================================================
// NEW-15: CR2.REC_CFG_ATS gates C_BAD_STREAMID for ATS TR
// =============================================================================

/// NEW-15: ATS TR + unknown SID + `RECINVSID=1` + `REC_CFG_ATS=0` → no `C_BAD_STREAMID`.
#[test]
fn test_new15_ats_tr_unknown_sid_no_rec_cfg_ats_suppressed() {
    let smmu = make_enabled_smmu();
    // RECINVSID=1 but REC_CFG_ATS=0
    smmu.set_cr2(SMMU::CR2_RECINVSID);
    smmu.clear_event_queue();

    let unknown_sid = StreamID::new(0xDEAD).unwrap();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    let _ = smmu.translate_with_type(
        unknown_sid,
        pasid,
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
    );

    let events = smmu.get_events();
    let has_bad_sid =
        events.iter().any(|e| e.event_type == EventType::CBadStreamid);
    assert!(
        !has_bad_sid,
        "C_BAD_STREAMID must be suppressed for ATS TR when REC_CFG_ATS=0; events: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// NEW-15: ATS TR + unknown SID + `RECINVSID=1` + `REC_CFG_ATS=1` → `C_BAD_STREAMID` emitted.
#[test]
fn test_new15_ats_tr_unknown_sid_both_bits_set_emits_bad_sid() {
    let smmu = make_enabled_smmu();
    smmu.set_cr2(SMMU::CR2_RECINVSID | SMMU::CR2_REC_CFG_ATS);
    smmu.clear_event_queue();

    let unknown_sid = StreamID::new(0xDEAD).unwrap();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    let _ = smmu.translate_with_type(
        unknown_sid,
        pasid,
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
    );

    let events = smmu.get_events();
    let has_bad_sid =
        events.iter().any(|e| e.event_type == EventType::CBadStreamid);
    assert!(
        has_bad_sid,
        "C_BAD_STREAMID must be emitted for ATS TR when REC_CFG_ATS=1 and RECINVSID=1; events: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// NEW-15: Ordinary TR + unknown SID + `RECINVSID=1` → `C_BAD_STREAMID` emitted
///         without needing `REC_CFG_ATS` (existing behavior preserved).
#[test]
fn test_new15_ordinary_unknown_sid_recinvsid_still_fires() {
    let smmu = make_enabled_smmu();
    smmu.set_cr2(SMMU::CR2_RECINVSID); // no REC_CFG_ATS
    smmu.clear_event_queue();

    let unknown_sid = StreamID::new(0xDEAD).unwrap();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    let _ = smmu.translate(
        unknown_sid,
        pasid,
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    let has_bad_sid =
        events.iter().any(|e| e.event_type == EventType::CBadStreamid);
    assert!(
        has_bad_sid,
        "C_BAD_STREAMID must fire for ordinary traffic with RECINVSID=1; events: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

// =============================================================================
// NEW-19: STE.Config=0b000 (disabled) ATS TR is silent UR
// =============================================================================

/// NEW-19: ATS TR on a disabled stream (`STE.Config=0b000`/abort\_mode) → silent UR.
#[test]
fn test_new19_ats_tr_disabled_stream_silent_ur() {
    let smmu = make_enabled_smmu();
    let sid = StreamID::new(1).unwrap();
    // Configure with disabled=true → abort_mode → STE.Config=0b000 equivalent
    let cfg = StreamConfig { disabled: true, ..StreamConfig::default() };
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.clear_event_queue();

    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    let result = smmu.translate_with_type(
        sid,
        pasid,
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
    );
    assert!(result.is_err(), "ATS TR on disabled stream must fail");

    let events = smmu.get_events();
    let has_ats_event = events.iter().any(|e| {
        e.event_type == EventType::FBadAtsTreq
            || e.event_type == EventType::FTranslForbidden
    });
    assert!(
        !has_ats_event,
        "STE.Config=0b000 ATS TR must be silent UR — no ATS event; events: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// NEW-19: ATS TR on a bypass stream → `F_BAD_ATS_TREQ` (bypass ≠ `STE.Config=0b000`).
#[test]
fn test_new19_ats_tr_bypass_stream_still_emits_event() {
    let smmu = make_enabled_smmu();
    let sid = StreamID::new(2).unwrap();
    let cfg = StreamConfig { eats: 0, ..StreamConfig::bypass() };
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.clear_event_queue();

    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    let result = smmu.translate_with_type(
        sid,
        pasid,
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
    );
    assert!(result.is_err(), "ATS TR on bypass stream must fail");

    let events = smmu.get_events();
    let found = events.iter().any(|e| e.event_type == EventType::FBadAtsTreq);
    assert!(
        found,
        "F_BAD_ATS_TREQ must still be emitted for bypass stream ATS TR; events: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}
