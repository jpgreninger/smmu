//! TDD failing tests for NEW-A and NEW-B bug fixes.
//!
//! Each test is written to FAIL with the current code (red) and PASS only after
//! the corresponding fix is applied (green).
//!
//! # NEW-A (§5.2): S1DSS==0b00 ATS TR event suppression
//!
//! ARM §5.2: "For ATS Translation Requests, if the cases described in 0b00 and
//! 0b10 lead to termination, the Translation Request is terminated with a CA
//! and no event is recorded."
//!
//! When an ATS Translation Request arrives on a substream-capable stream
//! (S1CDMax > 0) with PASID==0 and S1DSS==0b00, the implementation currently
//! records an F_STREAM_DISABLED event. This violates the spec.
//!
//! The S1DSS==0b10 path already has ATS suppression (BUG-E + NEW-FINDING-1).
//! The S1DSS==0b00 path does not.
//!
//! BEFORE FIX: F_STREAM_DISABLED event is recorded for ATS TR → test FAILS.
//! AFTER FIX:  abort with no event → test PASSES.
//!
//! # NEW-B (§5.5): STALL_MODEL==0b01 + CD.S==1 → C_BAD_CD
//!
//! ARM §5.5 CdIllegal() pseudocode: "if stall_model == '01' && CD.S == '1'
//! then return TRUE"
//!
//! When STALL_MODEL==0b01 (terminate-only), configuring a stage-1 stream with
//! FaultMode::Stall (CD.S==1) is illegal. Currently silently accepted.
//!
//! BEFORE FIX: configure_stream() succeeds without event → test FAILS.
//! AFTER FIX:  C_BAD_CD event recorded → test PASSES.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{
    AccessType, EventType, FaultMode, SecurityState, StreamConfig, StreamID, TransactionType,
    IOVA, PASID,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

/// Build an SMMU with all queues enabled.
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
// NEW-A: S1DSS==0b00 ATS TR event suppression (§5.2)
// ============================================================================

/// NEW-A-1: ATS Translation Request with S1DSS==0b00 + PASID==0 must abort
/// WITHOUT recording F_STREAM_DISABLED.
///
/// ARM §5.2: "For ATS Translation Requests, if the cases described in 0b00
/// and 0b10 lead to termination, the Translation Request is terminated with
/// a CA and no event is recorded."
///
/// BEFORE FIX: F_STREAM_DISABLED event is recorded → test FAILS.
/// AFTER FIX:  abort with no event → test PASSES.
#[test]
fn new_a_s1dss00_ats_tr_no_event() {
    let smmu = make_smmu();

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s1cd_max = 1; // substream-capable
    cfg.s1dss = 0x00; // abort non-substream transactions
    cfg.eats = 1; // ATS-capable (so ATS check passes before S1DSS)

    let sid = StreamID::new(20).unwrap();
    let _ = smmu.configure_stream(sid, cfg);

    // ATS TR with PASID==0: must abort, must NOT record event.
    let result = smmu.translate_with_type_and_ssv(
        sid,
        PASID::new(0).unwrap(),
        IOVA::new(0x1000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
        false,
    );

    assert!(
        result.is_err(),
        "NEW-A-1: S1DSS==0b00 + ATS TR + PASID==0 must abort (ARM §5.2)"
    );
    assert!(
        !has_event(&smmu, EventType::FStreamDisabled),
        "NEW-A-1: ATS TR must NOT record F_STREAM_DISABLED when S1DSS==0b00 (ARM §5.2)"
    );
}

/// NEW-A-2: Ordinary transaction with S1DSS==0b00 + PASID==0 MUST record
/// F_STREAM_DISABLED (regression guard).
///
/// ARM §5.2: Only ATS TR suppresses the event. Ordinary transactions must
/// still generate the event.
///
/// BEFORE and AFTER FIX: event is recorded → test PASSES both times.
#[test]
fn new_a_s1dss00_ordinary_records_event() {
    let smmu = make_smmu();

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s1cd_max = 1;
    cfg.s1dss = 0x00;

    let sid = StreamID::new(21).unwrap();
    let _ = smmu.configure_stream(sid, cfg);

    let result = smmu.translate_with_type_and_ssv(
        sid,
        PASID::new(0).unwrap(),
        IOVA::new(0x1000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::Ordinary,
        false,
    );

    assert!(
        result.is_err(),
        "NEW-A-2 regression: S1DSS==0b00 + Ordinary + PASID==0 must abort"
    );
    assert!(
        has_event(&smmu, EventType::FStreamDisabled),
        "NEW-A-2 regression: Ordinary transaction MUST record F_STREAM_DISABLED (ARM §5.2)"
    );
}

// ============================================================================
// NEW-B: STALL_MODEL==0b01 + CD.S==1 → C_BAD_CD (§5.5)
// ============================================================================

/// NEW-B-1: STALL_MODEL==0b01 (terminate-only) + stage1_enabled +
/// FaultMode::Stall (CD.S==1) must record C_BAD_CD.
///
/// ARM §5.5 CdIllegal() pseudocode: "if stall_model == '01' && CD.S == '1'
/// then return TRUE" — illegal configuration.
///
/// BEFORE FIX: configure_stream succeeds without event → test FAILS.
/// AFTER FIX:  C_BAD_CD event recorded → test PASSES.
#[test]
fn new_b_stall_model01_cd_s1_bad_cd() {
    let smmu = make_smmu();
    smmu.set_stall_model(0x01); // terminate-only model

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.fault_mode = FaultMode::Stall; // CD.S==1 — illegal with terminate-only model

    let _ = smmu.configure_stream(StreamID::new(22).unwrap(), cfg);

    assert!(
        has_event(&smmu, EventType::CBadCd),
        "NEW-B-1: STALL_MODEL==0b01 + stage1_enabled + FaultMode::Stall (CD.S==1) \
         must record C_BAD_CD event (ARM §5.5 CdIllegal)"
    );
}

/// NEW-B-2: STALL_MODEL==0b01 + stage1_enabled + FaultMode::Terminate
/// (CD.S==0) must NOT generate C_BAD_CD (regression guard).
///
/// This is the valid configuration — no error expected.
#[test]
fn new_b_stall_model01_cd_s0_no_error() {
    let smmu = make_smmu();
    smmu.set_stall_model(0x01); // terminate-only model

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.fault_mode = FaultMode::Terminate; // CD.S==0 — valid with terminate-only

    let _ = smmu.configure_stream(StreamID::new(23).unwrap(), cfg);

    assert!(
        !has_event(&smmu, EventType::CBadCd),
        "NEW-B-2 regression: STALL_MODEL==0b01 + FaultMode::Terminate must NOT \
         generate C_BAD_CD (ARM §5.5)"
    );
}
