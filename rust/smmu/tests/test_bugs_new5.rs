//! TDD failing tests for BUG-NEW-27 and BUG-NEW-25 (Rust side).
//!
//! Each test is written to FAIL with the current code and PASS only after the
//! corresponding fix is applied.  Positive/regression tests that should
//! already PASS are explicitly noted.
//!
//! # Bug Summary
//!
//! ## BUG-NEW-27 (Rust, §4.4.2.8/9/10)
//!
//! `TlbiEl2Va` is grouped with `TlbiNhVa` and `TlbiSEl2Va` in a single match
//! arm (line ~5641 of smmu/mod.rs) with no IDR0.Hyp guard.
//! `TlbiEl2Vaa` is grouped with `TlbiNhVaa` and `TlbiSEl2Vaa` (line ~5683)
//! with no IDR0.Hyp guard.
//! `TlbiEl2Asid` (line ~5593) also has no IDR0.Hyp guard.
//!
//! ARM §4.4.2.8/9/10: "When SMMU_IDR0.Hyp == 0, this command causes CERROR_ILL."
//!
//! BEFORE FIX: Hyp=false does not prevent execution → CMDQ_ERR not set → test FAILS.
//! AFTER FIX:  IDR0.Hyp==0 guard added to each arm → CERROR_ILL raised → passes.
//!
//! ## BUG-NEW-25 (Rust, §4.1.6/§4.2.1/§4.2.2)
//!
//! `CommandType::PrefetchConfig | CommandType::PrefetchAddr` at line ~6035 of
//! smmu/mod.rs is an unconditional no-op with no `ssec` check.
//!
//! ARM §4.1.6: ALL NS-queue commands with SSec=1 must raise CERROR_ILL.
//!
//! BEFORE FIX: no ssec check → no CERROR → test FAILS.
//! AFTER FIX:  ssec guard added → CERROR_ILL raised → passes.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{CommandEntry, CommandType, SecurityState};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

/// Build an SMMU with SMMUEN + CMDQEN + EVENTQEN + PRIQEN enabled.
fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    smmu
}

/// Returns true when GERROR.CMDQ_ERR is active (GERROR XOR GERRORN has bit 0 set).
fn is_gerror_cmdq_err_active(smmu: &SMMU) -> bool {
    (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR != 0
}

// ============================================================================
// BUG-NEW-27: TlbiEl2Va/TlbiEl2Vaa/TlbiEl2Asid must check IDR0.Hyp
// ============================================================================
// ARM §4.4.2.8/9/10: "When SMMU_IDR0.Hyp == 0, this command causes CERROR_ILL."
// The three EL2-targeted TLBI commands need their own match arms with an
// IDR0.Hyp guard, separated from the TlbiNhVa/TlbiNhVaa/TlbiNhAsid peers
// that have no such restriction.

/// BUG-NEW-27 (primary): with `set_hyp_supported(false)`, submitting
/// `TlbiEl2Va` must set CMDQ_ERR and write CERROR_ILL.
///
/// ARM §4.4.2.8: "If SMMU_IDR0.Hyp==0, CERROR_ILL."
///
/// BEFORE FIX: `TlbiEl2Va` is grouped with `TlbiNhVa` — no guard fires →
///   `is_gerror_cmdq_err_active` returns false → test FAILS (red).
/// AFTER FIX:  dedicated arm with Hyp guard raises CERROR_ILL → passes (green).
#[test]
fn bug_new27_tlbi_el2_va_missing_hyp_guard() {
    let smmu = make_smmu();

    // Override IDR0.Hyp to simulate a non-hypervisor implementation.
    smmu.set_hyp_supported(false);
    assert_eq!(
        smmu.get_idr0() & (1 << 9),
        0,
        "BUG-NEW-27 precondition: IDR0.Hyp (bit 9) must be 0 after set_hyp_supported(false)"
    );

    let cmd = CommandEntry::new(CommandType::TlbiEl2Va, 0, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-27: TlbiEl2Va with IDR0.Hyp=0 must write CERROR_ILL \
         (ARM §4.4.2.8). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-27: TlbiEl2Va with IDR0.Hyp=0 must assert GERROR.CMDQ_ERR \
         (ARM §4.4.2.8)"
    );
}

/// BUG-NEW-27: with `set_hyp_supported(false)`, `TlbiEl2Vaa` must
/// set CMDQ_ERR and write CERROR_ILL.
///
/// ARM §4.4.2.9: "If SMMU_IDR0.Hyp==0, CERROR_ILL."
///
/// BEFORE FIX: `TlbiEl2Vaa` grouped with `TlbiNhVaa` — no guard → test FAILS.
/// AFTER FIX:  dedicated arm with Hyp guard raises CERROR_ILL → passes.
#[test]
fn bug_new27_tlbi_el2_vaa_missing_hyp_guard() {
    let smmu = make_smmu();

    smmu.set_hyp_supported(false);
    assert_eq!(
        smmu.get_idr0() & (1 << 9),
        0,
        "BUG-NEW-27 precondition: IDR0.Hyp must be 0"
    );

    let cmd = CommandEntry::new(CommandType::TlbiEl2Vaa, 0, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-27: TlbiEl2Vaa with IDR0.Hyp=0 must write CERROR_ILL \
         (ARM §4.4.2.9). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-27: TlbiEl2Vaa with IDR0.Hyp=0 must assert GERROR.CMDQ_ERR \
         (ARM §4.4.2.9)"
    );
}

/// BUG-NEW-27: with `set_hyp_supported(false)`, `TlbiEl2Asid` must
/// set CMDQ_ERR and write CERROR_ILL.
///
/// ARM §4.4.2.10: "If SMMU_IDR0.Hyp==0, CERROR_ILL."
///
/// BEFORE FIX: `TlbiEl2Asid` has its own arm but no Hyp guard → test FAILS.
/// AFTER FIX:  Hyp guard added → CERROR_ILL raised → passes.
#[test]
fn bug_new27_tlbi_el2_asid_missing_hyp_guard() {
    let smmu = make_smmu();

    smmu.set_hyp_supported(false);
    assert_eq!(
        smmu.get_idr0() & (1 << 9),
        0,
        "BUG-NEW-27 precondition: IDR0.Hyp must be 0"
    );

    let cmd = CommandEntry::new(CommandType::TlbiEl2Asid, 0, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-27: TlbiEl2Asid with IDR0.Hyp=0 must write CERROR_ILL \
         (ARM §4.4.2.10). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-27: TlbiEl2Asid with IDR0.Hyp=0 must assert GERROR.CMDQ_ERR \
         (ARM §4.4.2.10)"
    );
}

/// BUG-NEW-27 (positive): with the default IDR0.Hyp=1, `TlbiEl2Va` must
/// execute normally — no CERROR_ILL, no GERROR.CMDQ_ERR.
///
/// This test verifies that the IDR0.Hyp guard does NOT fire when Hyp is
/// supported (the common/default case).
///
/// BEFORE FIX: passes trivially (no guard at all).
/// AFTER FIX:  guard present but Hyp=1 → no CERROR_ILL → still passes.
#[test]
fn bug_new27_hyp1_el2_va_succeeds() {
    let smmu = make_smmu();

    // IDR0.Hyp defaults to true (bit 9 = 1).
    let idr0 = smmu.get_idr0();
    assert!(
        (idr0 & (1 << 9)) != 0,
        "BUG-NEW-27 precondition: IDR0.Hyp (bit 9) must be 1 by default. \
         got IDR0=0x{:08x}",
        idr0
    );

    let cmd = CommandEntry::new(CommandType::TlbiEl2Va, 0, 0);
    smmu.submit_command(cmd).unwrap();
    let result = smmu.process_command_queue();

    assert!(
        result.is_ok(),
        "BUG-NEW-27 (positive): TlbiEl2Va with IDR0.Hyp=1 must succeed. Got {:?}",
        result
    );
    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_NONE,
        "BUG-NEW-27 (positive): TlbiEl2Va with IDR0.Hyp=1 must not set CERROR_ILL \
         (ARM §4.4.2.8)"
    );
    assert!(
        !is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-27 (positive): TlbiEl2Va with IDR0.Hyp=1 must not assert GERROR.CMDQ_ERR \
         (ARM §4.4.2.8)"
    );
}

// ============================================================================
// BUG-NEW-25 (Rust): PrefetchConfig and PrefetchAddr with ssec=true
// must raise CERROR_ILL (§4.1.6: all NS-queue commands with SSec=1 illegal)
// ============================================================================

/// BUG-NEW-25 (primary): `PrefetchConfig` with `ssec=true` must set
/// CMDQ_ERR and write CERROR_ILL.
///
/// ARM §4.1.6: "Any NS-queue command with SSec=1 is illegal — CERROR_ILL."
///
/// BEFORE FIX: `PrefetchConfig | PrefetchAddr => {}` (unconditional no-op,
///   no ssec check) → CMDQ_ERR not set → test FAILS (red).
/// AFTER FIX:  ssec guard added before the no-op branch → CERROR_ILL raised
///   → passes (green).
#[test]
fn bug_new25_prefetch_config_ssec1_cerror_ill() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::PrefetchConfig, 0, 0);
    cmd.ssec = true;
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-25: PrefetchConfig with ssec=1 must write CERROR_ILL \
         (ARM §4.1.6). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-25: PrefetchConfig with ssec=1 must assert GERROR.CMDQ_ERR \
         (ARM §4.1.6)"
    );
}

/// BUG-NEW-25: `PrefetchAddr` with `ssec=true` must set CMDQ_ERR and
/// write CERROR_ILL.
///
/// BEFORE FIX: unconditional no-op → no CERROR → test FAILS (red).
/// AFTER FIX:  ssec guard fires → CERROR_ILL raised → passes (green).
#[test]
fn bug_new25_prefetch_addr_ssec1_cerror_ill() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::PrefetchAddr, 0, 0);
    cmd.ssec = true;
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-25: PrefetchAddr with ssec=1 must write CERROR_ILL \
         (ARM §4.1.6). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-25: PrefetchAddr with ssec=1 must assert GERROR.CMDQ_ERR \
         (ARM §4.1.6)"
    );
}

/// BUG-NEW-25 (positive): `PrefetchConfig` with `ssec=false` must NOT
/// raise any error.
///
/// This positive path must PASS both before and after the fix (regression guard).
#[test]
fn bug_new25_prefetch_config_ssec0_no_error() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::PrefetchConfig, 0, 0);
    cmd.ssec = false;
    smmu.submit_command(cmd).unwrap();
    let result = smmu.process_command_queue();

    assert!(
        result.is_ok(),
        "BUG-NEW-25 (positive): PrefetchConfig with ssec=0 must succeed. Got {:?}",
        result
    );
    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_NONE,
        "BUG-NEW-25 (positive): PrefetchConfig with ssec=0 must not set CERROR_ILL \
         (ARM §4.1.6)"
    );
    assert!(
        !is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-25 (positive): PrefetchConfig with ssec=0 must not assert \
         GERROR.CMDQ_ERR (ARM §4.1.6)"
    );
}

// ============================================================================
// Additional regression: BUG-NEW-25 PrefetchAddr ssec=false (no error)
// ============================================================================

/// BUG-NEW-25 (positive): `PrefetchAddr` with `ssec=false` must NOT raise
/// any error.
///
/// Regression guard to ensure the ssec=false path still works after the fix.
#[test]
fn bug_new25_prefetch_addr_ssec0_no_error() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::PrefetchAddr, 0, 0);
    cmd.ssec = false;
    smmu.submit_command(cmd).unwrap();
    let result = smmu.process_command_queue();

    assert!(
        result.is_ok(),
        "BUG-NEW-25 (positive): PrefetchAddr with ssec=0 must succeed. Got {:?}",
        result
    );
    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_NONE,
        "BUG-NEW-25 (positive): PrefetchAddr with ssec=0 must not set CERROR_ILL \
         (ARM §4.1.6)"
    );
    assert!(
        !is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-25 (positive): PrefetchAddr with ssec=0 must not assert \
         GERROR.CMDQ_ERR (ARM §4.1.6)"
    );
}

// ============================================================================
// Additional BUG-NEW-27 regression: TlbiEl2Vaa and TlbiEl2Asid with Hyp=true
// ============================================================================

/// BUG-NEW-27 (positive): `TlbiEl2Vaa` with IDR0.Hyp=1 must NOT raise CERROR_ILL.
#[test]
fn bug_new27_hyp1_el2_vaa_succeeds() {
    let smmu = make_smmu();

    assert!(
        (smmu.get_idr0() & (1 << 9)) != 0,
        "BUG-NEW-27 precondition: IDR0.Hyp must be 1 by default"
    );

    let cmd = CommandEntry::new(CommandType::TlbiEl2Vaa, 0, 0);
    smmu.submit_command(cmd).unwrap();
    let result = smmu.process_command_queue();

    assert!(
        result.is_ok(),
        "BUG-NEW-27 (positive): TlbiEl2Vaa with IDR0.Hyp=1 must succeed. Got {:?}",
        result
    );
    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_NONE,
        "BUG-NEW-27 (positive): TlbiEl2Vaa with IDR0.Hyp=1 must not set CERROR_ILL"
    );
}

/// BUG-NEW-27 (positive): `TlbiEl2Asid` with IDR0.Hyp=1 must NOT raise CERROR_ILL.
#[test]
fn bug_new27_hyp1_el2_asid_succeeds() {
    let smmu = make_smmu();

    assert!(
        (smmu.get_idr0() & (1 << 9)) != 0,
        "BUG-NEW-27 precondition: IDR0.Hyp must be 1 by default"
    );

    let cmd = CommandEntry::new(CommandType::TlbiEl2Asid, 0, 0);
    smmu.submit_command(cmd).unwrap();
    let result = smmu.process_command_queue();

    assert!(
        result.is_ok(),
        "BUG-NEW-27 (positive): TlbiEl2Asid with IDR0.Hyp=1 must succeed. Got {:?}",
        result
    );
    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_NONE,
        "BUG-NEW-27 (positive): TlbiEl2Asid with IDR0.Hyp=1 must not set CERROR_ILL"
    );
}

// ============================================================================
// Additional BUG-NEW-26 regression (Rust side)
// ============================================================================
// BUG-NEW-26 was already fixed in Rust (test_bugs_new4.rs covers it), but we
// add a cross-reference guard here to ensure the Rust side stays correct.

/// BUG-NEW-26 (regression): Rust CMD_SYNC completion event must be NonSecure
/// regardless of stream security state.
///
/// This test verifies the already-fixed Rust path remains correct as a
/// regression guard alongside the new BUG-NEW-27 / BUG-NEW-25 tests.
#[test]
fn bug_new26_cmd_sync_completion_nonsecure_regression() {
    use smmu::types::{StreamConfig, StreamID};

    let smmu = make_smmu();

    let stream_id = StreamID::new(0xC0).unwrap();
    let secure_config = StreamConfig {
        security_state: SecurityState::Secure,
        ..StreamConfig::stage1_only()
    };
    smmu.configure_stream(stream_id, secure_config).unwrap();

    let mut cmd = CommandEntry::new(CommandType::Sync, 0xC0, 0);
    cmd.cs = 0x01; // SIG_IRQ → emits CommandSyncCompletion

    smmu.submit_command(cmd).expect("submit_command must succeed");
    smmu.process_command_queue().expect("process_command_queue must succeed");

    let events = smmu.get_events();
    use smmu::types::EventType;
    let completion = events
        .iter()
        .find(|e| e.event_type == EventType::CommandSyncCompletion)
        .expect("CommandSyncCompletion event must be present");

    assert_eq!(
        completion.security_state,
        SecurityState::NonSecure,
        "BUG-NEW-26 (regression): CommandSyncCompletion.security_state must always \
         be NonSecure. Got: {:?}",
        completion.security_state
    );
}
