//! TDD failing tests for BUG-A, BUG-B, BUG-C1, BUG-C2, BUG-C3, BUG-D, BUG-G, NEW-FINDING-1.
//!
//! Each test is written to FAIL with the current code (red) and PASS only after
//! the corresponding fix is applied (green).
//!
//! # BUG-A (§6.3.1): IDR0.VMW (bit 17) must be RES0 when IDR0.S2P==0
//!
//! VMW (VMID Wildcard in CR0) is a stage-2 feature.  When S2P=0 (stage-2 not
//! supported), VMW is meaningless and bit 17 must read as 0 (RES0).
//! Current code: bit 17 is hardcoded to 1 unconditionally in `get_idr0()`.
//!
//! BEFORE FIX: `get_idr0() & (1<<17) != 0` even when `set_s2p_supported(false)`.
//! AFTER FIX:  `get_idr0() & (1<<17) == 0` when S2P=0.
//!
//! # BUG-B (§6.3.1): IDR0.Hyp (bit 9) must be RES0 when IDR0.S2P==0
//!
//! The Hypervisor extension (Hyp) requires stage-2 support.  When S2P=0, Hyp
//! cannot function and IDR0.Hyp (bit 9) must read as 0.
//! Current code: `set_hyp_supported(true)` sets bit 9 independently of S2P.
//!
//! BEFORE FIX: `get_idr0() & (1<<9) != 0` even when `set_s2p_supported(false)`.
//! AFTER FIX:  `get_idr0() & (1<<9) == 0` when S2P=0.
//!
//! # BUG-C1 (§5.5): STALL_MODEL==0b10 + stage2Enabled + s2_stall==false → C_BAD_STE
//!
//! When STALL_MODEL==0b10 (forced-stall), every stage-2-capable stream must
//! have S2S=1.  S2S=0 with a stage-2 stream is illegal → C_BAD_STE.
//! Current code: only validates STALL_MODEL==0b01 + s2_stall=true.
//!
//! BEFORE FIX: `configure_stream()` succeeds without event.
//! AFTER FIX:  C_BAD_STE event recorded.
//!
//! # BUG-C2 (§5.5): STALL_MODEL==0b10 + stage1Enabled + fault_mode!=Stall → C_BAD_CD
//!
//! When STALL_MODEL==0b10 (forced-stall), every stage-1-capable stream must
//! have CD.S=1 (FaultMode::Stall).  FaultMode::Terminate (CD.S=0) → C_BAD_CD.
//! Current code: no check for STALL_MODEL==0b10 with stage-1.
//!
//! BEFORE FIX: `configure_stream()` succeeds without event.
//! AFTER FIX:  C_BAD_CD event recorded.
//!
//! # BUG-C3 (§5.5): STALL_MODEL!=0b00 + s1_stalld==true → C_BAD_STE
//!
//! When STALL_MODEL requires stall but the STE disables stage-1 stall
//! (S1STALLD=1), the configuration is contradictory → C_BAD_STE.
//! Current code: no check for this combination.
//!
//! BEFORE FIX: `configure_stream()` succeeds without event.
//! AFTER FIX:  C_BAD_STE event recorded.
//!
//! # BUG-D (§4.1.6 re-evaluation): SSec is RES0 in 7 TLBI commands — no CERROR_ILL
//!
//! TLBI_NH_ALL, TLBI_NH_ASID, TLBI_NH_VA, TLBI_NH_VAA, TLBI_S12_VMALL,
//! TLBI_S2_IPA, TLBI_NSNH_ALL have no SSec field in their command encoding
//! (the bit is RES0).  Setting a RES0 bit to 1 is CONSTRAINED UNPREDICTABLE;
//! the SMMU must not raise CERROR_ILL.  The BUG-NEW-38 fix incorrectly added
//! SSec guards to these seven commands.
//!
//! BEFORE FIX (= after BUG-NEW-38): GERROR.CMDQ_ERR is set → FAILS.
//! AFTER FIX:  commands execute normally, GERROR.CMDQ_ERR not set → PASSES.
//! (AtcInv excluded: it has a real SSec field.)
//!
//! # BUG-G (§6.3.4): IDR3.MPAM is at bit [7], not bit [6]
//!
//! ARM IHI0070G.b §6.3.4: IDR3[7] = MPAM, IDR3[6] = RES0.
//! The CFGI_VMS_PIDM handler checks `get_idr3() & (1 << 6)` (wrong bit).
//! Since this model reports MPAM=0 always (neither bit set), the observable
//! behaviour is accidentally correct.  Tests document the invariants.
//!
//! # NEW-FINDING-1 (§5.2 STE.S1DSS): ATS TR event suppression for S1DSS==0b10
//!
//! ARM §5.2 STE.S1DSS line 6725: "For ATS Translation Requests, if the cases
//! described in 0b00 and 0b10 lead to termination, the Translation Request is
//! terminated with a CA and no event is recorded."
//!
//! The BUG-E fix records F_STREAM_DISABLED for all transaction types.
//! For ATS Translation Requests the event must be suppressed.
//!
//! BEFORE FIX: F_STREAM_DISABLED event is always recorded.
//! AFTER FIX:  ATS TR aborts (CA) but no event recorded.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventType, FaultMode, PagePermissions, SecurityState,
    StreamConfig, StreamID, TransactionType, IOVA, PA, PASID,
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

/// Returns true when GERROR.CMDQ_ERR is active (GERROR XOR GERRORN has bit 0 set).
fn is_gerror_cmdq_err_active(smmu: &SMMU) -> bool {
    (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR != 0
}

/// Returns true when the event queue contains at least one event of the given type.
fn has_event(smmu: &SMMU, event_type: EventType) -> bool {
    smmu.get_events().iter().any(|e| e.event_type == event_type)
}

/// Build a stage-2-only stream config (no stage-1).
fn stage2_config() -> StreamConfig {
    let mut cfg = StreamConfig::stage2_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg
}

/// Build a stage-1-only stream config (no stage-2).
fn stage1_config() -> StreamConfig {
    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg
}

// ============================================================================
// BUG-A: IDR0.VMW (bit 17) must be RES0 when IDR0.S2P==0 (§6.3.1)
// ============================================================================
// ARM §6.3.1: VMW is a stage-2 feature.  When S2P=0, bit 17 must read as 0.
//
// BEFORE FIX: bit 17 is hardcoded to 1 in get_idr0() → test FAILS (red).
// AFTER FIX:  bit 17 is gated on s2p_supported_ → bit 17 == 0 → PASSES (green).

/// BUG-A: IDR0.VMW (bit 17) must be RES0 when `set_s2p_supported(false)`.
///
/// ARM §6.3.1: VMW is a stage-2 capability and must be RES0 when S2P=0.
///
/// BEFORE FIX: `get_idr0() & (1 << 17) != 0` even after disabling S2P.
/// AFTER FIX:  `get_idr0() & (1 << 17) == 0`.
#[test]
fn bug_new_a_idr0_vmw_res0_when_s2p_disabled() {
    let smmu = make_smmu();
    smmu.set_s2p_supported(false);

    let idr0 = smmu.get_idr0();

    // Prerequisite: S2P (bit 0) must be 0.
    assert_eq!(
        idr0 & (1 << 0),
        0,
        "BUG-A prerequisite: IDR0.S2P (bit 0) must be 0 after set_s2p_supported(false). \
         Got IDR0=0x{:08x}",
        idr0
    );

    // VMW (bit 17) must be 0 when S2P==0.
    assert_eq!(
        idr0 & (1 << 17),
        0,
        "BUG-A: IDR0.VMW (bit 17) must be RES0 when IDR0.S2P==0 (ARM §6.3.1). \
         Got IDR0=0x{:08x}",
        idr0
    );
}

/// BUG-A regression guard: IDR0.VMW (bit 17) must still be set when S2P=1.
#[test]
fn bug_new_a_idr0_vmw_set_when_s2p_enabled() {
    let smmu = make_smmu();
    smmu.set_s2p_supported(true);

    let idr0 = smmu.get_idr0();

    assert_ne!(
        idr0 & (1 << 0),
        0,
        "BUG-A regression: S2P (bit 0) must be 1 after set_s2p_supported(true)"
    );
    assert_ne!(
        idr0 & (1 << 17),
        0,
        "BUG-A regression: IDR0.VMW (bit 17) must be 1 when S2P=1 (ARM §6.3.1)"
    );
}

// ============================================================================
// BUG-B: IDR0.Hyp (bit 9) must be RES0 when IDR0.S2P==0 (§6.3.1)
// ============================================================================
// ARM §6.3.1: Hyp requires stage-2 support.  When S2P=0, bit 9 must read 0.
//
// BEFORE FIX: set_hyp_supported(true) sets bit 9 regardless of S2P → FAILS.
// AFTER FIX:  bit 9 cleared when S2P=0 even if set_hyp_supported(true) → PASSES.

/// BUG-B: IDR0.Hyp (bit 9) must be RES0 when S2P==0, even if Hyp is enabled.
///
/// ARM §6.3.1: IDR0.Hyp is gated on S2P — S2P=0 overrides Hyp to 0.
///
/// BEFORE FIX: `get_idr0() & (1 << 9) != 0` despite S2P=0.
/// AFTER FIX:  `get_idr0() & (1 << 9) == 0`.
#[test]
fn bug_new_b_idr0_hyp_res0_when_s2p_disabled() {
    let smmu = make_smmu();
    smmu.set_hyp_supported(true);
    smmu.set_s2p_supported(false);

    let idr0 = smmu.get_idr0();

    // Prerequisite: S2P (bit 0) must be 0.
    assert_eq!(
        idr0 & (1 << 0),
        0,
        "BUG-B prerequisite: IDR0.S2P (bit 0) must be 0. Got IDR0=0x{:08x}",
        idr0
    );

    // Hyp (bit 9) must be 0 when S2P==0.
    assert_eq!(
        idr0 & (1 << 9),
        0,
        "BUG-B: IDR0.Hyp (bit 9) must be RES0 when IDR0.S2P==0 (ARM §6.3.1). \
         Got IDR0=0x{:08x}",
        idr0
    );
}

/// BUG-B regression guard: IDR0.Hyp (bit 9) must still be set when S2P=1 and
/// Hyp is enabled.
#[test]
fn bug_new_b_idr0_hyp_set_when_s2p_enabled() {
    let smmu = make_smmu();
    smmu.set_hyp_supported(true);
    smmu.set_s2p_supported(true);

    let idr0 = smmu.get_idr0();

    assert_ne!(
        idr0 & (1 << 0),
        0,
        "BUG-B regression: S2P must be 1 after set_s2p_supported(true)"
    );
    assert_ne!(
        idr0 & (1 << 9),
        0,
        "BUG-B regression: IDR0.Hyp (bit 9) must be 1 when S2P=1 and Hyp enabled"
    );
}

// ============================================================================
// BUG-C1: STALL_MODEL==0b10 + stage2Enabled + s2_stall==false → C_BAD_STE (§5.5)
// ============================================================================
// ARM §5.5: Forced-stall model requires S2S=1 for stage-2 streams.
//
// BEFORE FIX: configure_stream succeeds without event → FAILS.
// AFTER FIX:  C_BAD_STE event recorded → PASSES.

/// BUG-C1: STALL_MODEL==0b10 (forced-stall) + stage2_enabled + s2_stall=false
/// must record a C_BAD_STE event.
///
/// ARM §5.5: forced-stall model mandates S2S=1 on stage-2-capable streams.
///
/// BEFORE FIX: no validation for STALL_MODEL==0b10 → no event → test FAILS.
/// AFTER FIX:  C_BAD_STE event recorded → test PASSES.
#[test]
fn bug_new_c1_stall_model2_stage2_s2s_false_bad_ste() {
    let smmu = make_smmu();
    smmu.set_stall_model(0x02); // forced-stall

    let mut cfg = stage2_config();
    cfg.s2_stall = false; // illegal: forced stall requires S2S=1

    // configure_stream may return Ok or Err; in either case the event must be recorded.
    let _ = smmu.configure_stream(StreamID::new(1).unwrap(), cfg);

    assert!(
        has_event(&smmu, EventType::CBadSte),
        "BUG-C1: STALL_MODEL==0b10 + stage2_enabled + s2_stall=false must record \
         C_BAD_STE event (ARM §5.5)"
    );
}

// ============================================================================
// BUG-C2: STALL_MODEL==0b10 + stage1Enabled + fault_mode!=Stall → C_BAD_CD (§5.5)
// ============================================================================
// ARM §5.5: Forced-stall model requires CD.S=1 for stage-1 streams.
//
// BEFORE FIX: configure_stream succeeds without event → FAILS.
// AFTER FIX:  C_BAD_CD event recorded → PASSES.

/// BUG-C2: STALL_MODEL==0b10 (forced-stall) + stage1_enabled + FaultMode::Terminate
/// (CD.S=0) must record a C_BAD_CD event.
///
/// ARM §5.5: forced-stall model mandates CD.S=1 (FaultMode::Stall) on stage-1
/// streams.
///
/// BEFORE FIX: no validation for STALL_MODEL==0b10 → no event → test FAILS.
/// AFTER FIX:  C_BAD_CD event recorded → test PASSES.
#[test]
fn bug_new_c2_stall_model2_stage1_cd_s_false_bad_cd() {
    let smmu = make_smmu();
    smmu.set_stall_model(0x02); // forced-stall

    let mut cfg = stage1_config();
    cfg.fault_mode = FaultMode::Terminate; // CD.S=0 — illegal with forced-stall model

    let _ = smmu.configure_stream(StreamID::new(2).unwrap(), cfg);

    assert!(
        has_event(&smmu, EventType::CBadCd),
        "BUG-C2: STALL_MODEL==0b10 + stage1_enabled + FaultMode::Terminate (CD.S=0) \
         must record C_BAD_CD event (ARM §5.5)"
    );
}

// ============================================================================
// BUG-C3: STALL_MODEL!=0b00 + s1_stalld==true → C_BAD_STE (§5.5)
// ============================================================================
// ARM §5.5: STALL_MODEL==0b10 mandates stall for stage-1, but S1STALLD=1
// disables it — contradictory → C_BAD_STE.
//
// BEFORE FIX: configure_stream succeeds without event → FAILS.
// AFTER FIX:  C_BAD_STE event recorded → PASSES.

/// BUG-C3: STALL_MODEL==0b10 (forced-stall) AND s1_stalld=true for a stage-1
/// stream must record a C_BAD_STE event.
///
/// ARM §5.5: forced-stall mandates stall; S1STALLD=1 disables it — illegal.
///
/// BEFORE FIX: no validation for this combination → no event → test FAILS.
/// AFTER FIX:  C_BAD_STE event recorded → test PASSES.
#[test]
fn bug_new_c3_stall_model_nonzero_s1stalld_true_bad_ste() {
    let smmu = make_smmu();
    smmu.set_stall_model(0x02); // forced-stall (STALL_MODEL != 0b00)

    let mut cfg = stage1_config();
    cfg.fault_mode = FaultMode::Stall; // attempt to satisfy CD.S=1
    cfg.s1_stalld = true;              // but S1STALLD overrides: contradictory

    let _ = smmu.configure_stream(StreamID::new(3).unwrap(), cfg);

    assert!(
        has_event(&smmu, EventType::CBadSte),
        "BUG-C3: STALL_MODEL==0b10 AND s1_stalld=true must record C_BAD_STE event \
         (ARM §5.5)"
    );
}

// ============================================================================
// BUG-D: SSec RES0 in 7 TLBI commands — must NOT raise CERROR_ILL (§4.1.6)
// ============================================================================
// ARM IHI0070G.b: TLBI_NH_ALL, TLBI_NH_ASID, TLBI_NH_VA, TLBI_NH_VAA,
// TLBI_S12_VMALL, TLBI_S2_IPA, TLBI_NSNH_ALL have RES0 SSec bits.
// Setting RES0 to 1 is CONSTRAINED UNPREDICTABLE; must not raise CERROR_ILL.
//
// BEFORE FIX (after BUG-NEW-38 fix): GERROR.CMDQ_ERR set → FAILS.
// AFTER FIX:  commands execute normally → GERROR.CMDQ_ERR not set → PASSES.

/// Helper: submit a command with ssec=true, process, return true if no CERROR_ILL.
fn check_ssec_no_error(smmu: &SMMU, cmd_type: CommandType) -> bool {
    // Clear any pre-existing gerror.
    smmu.clear_gerror(smmu.get_gerror() ^ smmu.get_gerrorn());
    let mut cmd = CommandEntry::new(cmd_type, 0, 0);
    cmd.ssec = true;
    if smmu.submit_command(cmd).is_err() {
        return false;
    }
    let _ = smmu.process_command_queue();
    !is_gerror_cmdq_err_active(smmu)
}

/// BUG-D-1: TlbiNhAll with ssec=true must NOT raise CERROR_ILL.
///
/// ARM §4.4.2.1: TlbiNhAll has no SSec field (RES0 in encoding).
/// Setting a RES0 bit is CONSTRAINED UNPREDICTABLE — must not cause CERROR_ILL.
///
/// BEFORE FIX: ssec guard fires → CERROR_ILL → GERROR set → test FAILS.
/// AFTER FIX:  no error → test PASSES.
#[test]
fn bug_new_d_tlbi_nh_all_ssec1_no_cerror_ill() {
    let smmu = make_smmu();
    assert!(
        check_ssec_no_error(&smmu, CommandType::TlbiNhAll),
        "BUG-D: TlbiNhAll with ssec=true must NOT raise CERROR_ILL — SSec is \
         RES0 in this command's encoding (ARM §4.4.2.1)"
    );
}

/// BUG-D-2: TlbiNhAsid with ssec=true must NOT raise CERROR_ILL.
///
/// ARM §4.4.2.2: TlbiNhAsid has no SSec field (RES0 in encoding).
///
/// BEFORE FIX: ssec guard fires → CERROR_ILL → GERROR set → test FAILS.
/// AFTER FIX:  no error → test PASSES.
#[test]
fn bug_new_d_tlbi_nh_asid_ssec1_no_cerror_ill() {
    let smmu = make_smmu();
    assert!(
        check_ssec_no_error(&smmu, CommandType::TlbiNhAsid),
        "BUG-D: TlbiNhAsid with ssec=true must NOT raise CERROR_ILL — SSec is \
         RES0 in this command's encoding (ARM §4.4.2.2)"
    );
}

/// BUG-D-3: TlbiNhVa with ssec=true must NOT raise CERROR_ILL.
///
/// ARM §4.4.2.3 (per ARM IHI0070G.b): TlbiNhVa has no SSec field (RES0).
///
/// BEFORE FIX: ssec guard fires → CERROR_ILL → GERROR set → test FAILS.
/// AFTER FIX:  no error → test PASSES.
#[test]
fn bug_new_d_tlbi_nh_va_ssec1_no_cerror_ill() {
    let smmu = make_smmu();
    assert!(
        check_ssec_no_error(&smmu, CommandType::TlbiNhVa),
        "BUG-D: TlbiNhVa with ssec=true must NOT raise CERROR_ILL — SSec is \
         RES0 in this command's encoding (ARM §4.4.2.3)"
    );
}

/// BUG-D-4: TlbiNhVaa with ssec=true must NOT raise CERROR_ILL.
///
/// ARM §4.4.2.4 (per ARM IHI0070G.b): TlbiNhVaa has no SSec field (RES0).
///
/// BEFORE FIX: ssec guard fires → CERROR_ILL → GERROR set → test FAILS.
/// AFTER FIX:  no error → test PASSES.
#[test]
fn bug_new_d_tlbi_nh_vaa_ssec1_no_cerror_ill() {
    let smmu = make_smmu();
    assert!(
        check_ssec_no_error(&smmu, CommandType::TlbiNhVaa),
        "BUG-D: TlbiNhVaa with ssec=true must NOT raise CERROR_ILL — SSec is \
         RES0 in this command's encoding (ARM §4.4.2.4)"
    );
}

/// BUG-D-5: TlbiS12Vmall with ssec=true must NOT raise CERROR_ILL.
///
/// ARM §4.4.3.1: TlbiS12Vmall has no SSec field (RES0).
/// S2P must be enabled so the BUG-NEW-39 S2P guard does not fire first.
///
/// BEFORE FIX: ssec guard fires → CERROR_ILL → GERROR set → test FAILS.
/// AFTER FIX:  no error → test PASSES.
#[test]
fn bug_new_d_tlbi_s12_vmall_ssec1_no_cerror_ill() {
    let smmu = make_smmu();
    smmu.set_s2p_supported(true); // prevent S2P==0 guard from firing
    assert!(
        check_ssec_no_error(&smmu, CommandType::TlbiS12Vmall),
        "BUG-D: TlbiS12Vmall with ssec=true must NOT raise CERROR_ILL — SSec is \
         RES0 in this command's encoding (ARM §4.4.3.1)"
    );
}

/// BUG-D-6: TlbiS2Ipa with ssec=true must NOT raise CERROR_ILL.
///
/// ARM §4.4.3.2: TlbiS2Ipa has no SSec field (RES0).
/// S2P must be enabled so the BUG-NEW-39 S2P guard does not fire first.
///
/// BEFORE FIX: ssec guard fires → CERROR_ILL → GERROR set → test FAILS.
/// AFTER FIX:  no error → test PASSES.
#[test]
fn bug_new_d_tlbi_s2_ipa_ssec1_no_cerror_ill() {
    let smmu = make_smmu();
    smmu.set_s2p_supported(true); // prevent S2P==0 guard from firing
    assert!(
        check_ssec_no_error(&smmu, CommandType::TlbiS2Ipa),
        "BUG-D: TlbiS2Ipa with ssec=true must NOT raise CERROR_ILL — SSec is \
         RES0 in this command's encoding (ARM §4.4.3.2)"
    );
}

/// BUG-D-7: TlbiNsnhAll with ssec=true must NOT raise CERROR_ILL.
///
/// ARM §4.4.4.1: TlbiNsnhAll has no SSec field (RES0).
///
/// BEFORE FIX: ssec guard fires → CERROR_ILL → GERROR set → test FAILS.
/// AFTER FIX:  no error → test PASSES.
#[test]
fn bug_new_d_tlbi_nsnh_all_ssec1_no_cerror_ill() {
    let smmu = make_smmu();
    assert!(
        check_ssec_no_error(&smmu, CommandType::TlbiNsnhAll),
        "BUG-D: TlbiNsnhAll with ssec=true must NOT raise CERROR_ILL — SSec is \
         RES0 in this command's encoding (ARM §4.4.4.1)"
    );
}

// ============================================================================
// BUG-G: IDR3.MPAM is at bit [7], not bit [6] (§6.3.4)
// ============================================================================
// ARM IHI0070G.b §6.3.4: IDR3[7]=MPAM, IDR3[6]=RES0.
// Current code checks bit 6 for MPAM; correct bit is 7.
// Since the model reports MPAM=0 (neither bit set), the functional behaviour
// is accidentally correct.  Tests document the invariants.
//
// G-1 / G-2: Verify bit positions are correct in get_idr3() return value.
// G-3: CFGI_VMS_PIDM raises CERROR_ILL when IDR3.MPAM (bit 7) == 0.

/// BUG-G-1: IDR3 bit 6 must be 0 (RES0 per ARM §6.3.4).
///
/// ARM §6.3.4: bit 6 is RES0 — must read as 0.
/// This test documents and verifies the invariant.
#[test]
fn bug_new_g_idr3_bit6_is_res0() {
    let smmu = make_smmu();
    let idr3 = smmu.get_idr3();
    assert_eq!(
        idr3 & (1 << 6),
        0,
        "BUG-G: IDR3 bit 6 must be RES0 (ARM §6.3.4). Got IDR3=0x{:08x}",
        idr3
    );
}

/// BUG-G-2: IDR3 bit 7 (MPAM) must be 0 in this model (MPAM not implemented).
///
/// ARM §6.3.4: IDR3[7] = MPAM.  This model does not implement MPAM → 0.
#[test]
fn bug_new_g_idr3_bit7_mpam_not_set() {
    let smmu = make_smmu();
    let idr3 = smmu.get_idr3();
    assert_eq!(
        idr3 & (1 << 7),
        0,
        "BUG-G: IDR3 bit 7 (MPAM per ARM §6.3.4) must be 0 (MPAM not implemented). \
         Got IDR3=0x{:08x}",
        idr3
    );
}

/// BUG-G-3: CfgiVmsPidm must raise CERROR_ILL when IDR3.MPAM (bit 7) == 0.
///
/// ARM §4.3.5: CMD_CFGI_VMS_PIDM requires IDR3.MPAM=1.  When MPAM is absent,
/// CERROR_ILL must be raised.
/// Current code checks bit 6 (wrong), but since neither bit is set, the
/// observable behaviour is correct.  This test verifies the observable behaviour
/// and documents that bit 7 is the correct MPAM bit.
///
/// This test PASSES today (accidentally correct — wrong bit, same result).
#[test]
fn bug_new_g_cfgi_vms_pidm_no_mpam_cerror_ill() {
    let smmu = make_smmu();

    // Sanity: IDR3.MPAM (bit 7) must be 0 in this model.
    assert_eq!(
        smmu.get_idr3() & (1 << 7),
        0,
        "BUG-G prerequisite: IDR3.MPAM (bit 7) must be 0 in this model"
    );

    // Clear any pre-existing error.
    smmu.clear_gerror(smmu.get_gerror() ^ smmu.get_gerrorn());

    let cmd = CommandEntry::new(CommandType::CfgiVmsPidm, 0, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-G: CfgiVmsPidm with IDR3.MPAM=0 (bit 7) must raise CERROR_ILL (ARM §4.3.5). \
         Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-G: CfgiVmsPidm with IDR3.MPAM=0 must set GERROR.CMDQ_ERR (ARM §4.3.5)"
    );
}

// ============================================================================
// BUG-E: F_STREAM_DISABLED — S1DSS==0b10 + SSV==1 + PASID==0 (§3.9/§7.3.7)
// ============================================================================
// ARM §3.9: When STE.S1DSS==0b10, a transaction arriving with SubstreamID=0
// AND SSV=1 must abort with F_STREAM_DISABLED (event 0x06).
//
// BEFORE FIX: translate_with_ssv() does not exist; this path was unimplemented.
// AFTER FIX:  translate_with_ssv(..., ssv=true) generates F_STREAM_DISABLED.

/// BUG-E: S1DSS==0b10 + SSV==1 + PASID==0 must generate F_STREAM_DISABLED.
///
/// ARM §3.9: "When STE.S1DSS==0b10, transactions that arrive with SubstreamID 0
/// [and SSV=1] are aborted and an event recorded."
///
/// BEFORE FIX: translate_with_ssv does not exist; path unimplemented → FAILS.
/// AFTER FIX:  F_STREAM_DISABLED event recorded → PASSES.
#[test]
fn bug_new_e_s1dss10_ssv1_pasid0_f_stream_disabled() {
    let smmu = make_smmu();

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s1cd_max = 1; // substream-capable (s1cd_max > 0)
    cfg.s1dss = 0x02; // use CD[0] for non-substream — but SSV=1 + PASID=0 aborts

    let sid = StreamID::new(10).unwrap();
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, PASID::new(0).unwrap()).unwrap();
    smmu.map_page(
        sid,
        PASID::new(0).unwrap(),
        IOVA::new(0x1000).unwrap(),
        PA::new(0x1000).unwrap(),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    let result = smmu.translate_with_ssv(
        sid,
        PASID::new(0).unwrap(),
        IOVA::new(0x1000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
        true, // ssv = true: SubstreamID was presented → abort per §3.9
    );

    assert!(
        result.is_err(),
        "BUG-E: S1DSS==0b10 + SSV==1 + PASID==0 must abort (ARM §3.9/§7.3.7)"
    );
    assert!(
        has_event(&smmu, EventType::FStreamDisabled),
        "BUG-E: S1DSS==0b10 + SSV==1 + PASID==0 must generate F_STREAM_DISABLED event (ARM §3.9/§7.3.7)"
    );
}

/// BUG-E regression: S1DSS==0b10 + SSV==0 + PASID==0 must NOT abort.
///
/// ARM §3.9: SSV=0 means no SubstreamID present — use CD[0] normally.
/// Only SSV=1 with PASID=0 aborts; SSV=0 proceeds normally.
#[test]
fn bug_new_e_s1dss10_ssv0_pasid0_no_abort() {
    let smmu = make_smmu();

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s1cd_max = 1;
    cfg.s1dss = 0x02; // use CD[0]

    let sid = StreamID::new(11).unwrap();
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, PASID::new(0).unwrap()).unwrap();
    smmu.map_page(
        sid,
        PASID::new(0).unwrap(),
        IOVA::new(0x1000).unwrap(),
        PA::new(0x2000).unwrap(),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    let result = smmu.translate_with_ssv(
        sid,
        PASID::new(0).unwrap(),
        IOVA::new(0x1000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
        false, // ssv = false — no SubstreamID presented → must NOT abort
    );

    assert!(
        result.is_ok(),
        "BUG-E regression: S1DSS==0b10 + SSV==0 + PASID==0 must NOT abort (ARM §3.9): {result:?}"
    );
    assert!(
        !has_event(&smmu, EventType::FStreamDisabled),
        "BUG-E regression: S1DSS==0b10 + SSV==0 + PASID==0 must NOT generate F_STREAM_DISABLED (ARM §3.9)"
    );
}

// ============================================================================
// BUG-F: Rust CMD_SYNC CS==0b11 inline signal_gerror (§4.7.3/§6.3.17)
// ============================================================================
// The CS==0b11 CERROR_ILL path must set GERROR.CMDQ_ERR, consistent with all
// other CERROR_ILL paths.  This test verifies the observable behaviour: after
// processing a CMD_SYNC with CS=0b11, GERROR.CMDQ_ERR is set and
// CMDQ_CONS.ERR=CERROR_ILL.

/// BUG-F: CMD_SYNC CS==0b11 must set GERROR.CMDQ_ERR and CMDQ_CONS.ERR=CERROR_ILL.
///
/// ARM §4.7.3 / §6.3.17: CMD_SYNC CS==0b11 is a reserved value → CERROR_ILL.
/// CERROR_ILL must set GERROR.CMDQ_ERR.
///
/// This test processes CMD_SYNC with CS=0b11 via process_command_queue() and
/// verifies that both GERROR.CMDQ_ERR and CMDQ_CONS.ERR are correct.
#[test]
fn bug_new_f_cmd_sync_cs3_inline_signal_gerror() {
    let smmu = make_smmu();

    // Clear any pre-existing error state.
    smmu.clear_gerror(smmu.get_gerror() ^ smmu.get_gerrorn());

    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 3; // CS=0b11 = Reserved → CERROR_ILL per ARM §4.7.3

    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    // GERROR.CMDQ_ERR must be set.
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-F: CMD_SYNC CS=0b11 must set GERROR.CMDQ_ERR (ARM §4.7.3 / §6.3.17)"
    );
    // CMDQ_CONS.ERR must be CERROR_ILL.
    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-F: CMD_SYNC CS=0b11 must set CMDQ_CONS.ERR=CERROR_ILL (ARM §6.3.17). \
         Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
}

// ============================================================================
// NEW-FINDING-1: ATS TR + S1DSS==0b10 + SSV==1 + PASID==0 — no event
// ============================================================================
// ARM §5.2 STE.S1DSS: "For ATS Translation Requests, if the cases described in
// 0b00 and 0b10 lead to termination, the Translation Request is terminated with
// a CA and no event is recorded."
//
// The BUG-E fix unconditionally records F_STREAM_DISABLED; for ATS TR the event
// must be suppressed (CA only, no event).
//
// BEFORE FIX: F_STREAM_DISABLED IS recorded → FAILS.
// AFTER FIX:  CA returned, no event → PASSES.

/// NEW-FINDING-1: ATS TR + S1DSS==0b10 + SSV==1 + PASID==0 must abort (CA)
/// but must NOT record F_STREAM_DISABLED.
///
/// ARM §5.2 STE.S1DSS line 6725: ATS TR terminated by S1DSS==0b10 must not
/// record an event.
///
/// The stream must have `eats != 0` so it passes the ATS-support check
/// (otherwise `FBadAtsTreq` fires before the S1DSS path, never reaching the
/// suppression code and making the test vacuously pass).
#[test]
fn new_finding_1_ats_tr_s1dss10_ssv1_pasid0_no_event() {
    let smmu = make_smmu();

    // eats=1: ATS capable — passes the ats_supported check so we reach S1DSS path.
    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s1cd_max = 1;
    cfg.s1dss = 0x02; // S1DSS==0b10: use CD[0]; SSV=1+PASID=0 must abort
    cfg.eats = 1;     // ATS capable: bypass the FBadAtsTreq guard

    let sid = StreamID::new(20).unwrap();
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, PASID::new(0).unwrap()).unwrap();
    smmu.map_page(
        sid,
        PASID::new(0).unwrap(),
        IOVA::new(0x1000).unwrap(),
        PA::new(0x1000).unwrap(),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    // ATS Translation Request + SSV=1 + PASID=0 → CA, no event (ARM §5.2 line 6725).
    let result = smmu.translate_with_type_and_ssv(
        sid,
        PASID::new(0).unwrap(),
        IOVA::new(0x1000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
        true, // ssv = true
    );

    assert!(
        result.is_err(),
        "NF1: ATS TR + S1DSS10 + SSV1 + PASID0 must abort (CA) (ARM §5.2)"
    );
    assert!(
        !has_event(&smmu, EventType::FStreamDisabled),
        "NF1: ATS TR + S1DSS10 + SSV1 + PASID0 must NOT record F_STREAM_DISABLED (ARM §5.2 STE.S1DSS line 6725)"
    );
}

/// NEW-FINDING-1 regression: Ordinary tx + S1DSS==0b10 + SSV==1 + PASID==0
/// must still record F_STREAM_DISABLED (BUG-E fix must remain intact).
#[test]
fn new_finding_1_ordinary_s1dss10_ssv1_pasid0_does_record_event() {
    let smmu = make_smmu();

    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s1cd_max = 1;
    cfg.s1dss = 0x02;

    let sid = StreamID::new(21).unwrap();
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, PASID::new(0).unwrap()).unwrap();
    smmu.map_page(
        sid,
        PASID::new(0).unwrap(),
        IOVA::new(0x1000).unwrap(),
        PA::new(0x2000).unwrap(),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    // Ordinary + SSV=1 + PASID=0 → must record F_STREAM_DISABLED.
    let result = smmu.translate_with_type_and_ssv(
        sid,
        PASID::new(0).unwrap(),
        IOVA::new(0x1000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::Ordinary,
        true, // ssv = true
    );

    assert!(
        result.is_err(),
        "NF1 regression: Ordinary + S1DSS10 + SSV1 + PASID0 must abort (ARM §3.9)"
    );
    assert!(
        has_event(&smmu, EventType::FStreamDisabled),
        "NF1 regression: Ordinary + S1DSS10 + SSV1 + PASID0 must STILL record F_STREAM_DISABLED (ARM §3.9/§7.3.7)"
    );
}
