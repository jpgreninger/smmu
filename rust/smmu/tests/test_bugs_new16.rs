//! TDD failing tests for BUG-AUDIT-01, BUG-AUDIT-02, and BUG-AUDIT-03 (Rust).
//!
//! Each test is written to FAIL with the current code (red) and PASS only after
//! the corresponding fix is applied (green), EXCEPT where explicitly marked as a
//! regression/baseline test.
//!
//! # BUG-AUDIT-01 (Both §5.2 SteIllegal()): EATS==0b10 + NS1ATS==1 → CBadSte
//!
//! ARM IHI0070G.b §5.2 SteIllegal() pseudocode:
//! ```text
//!   if STE.EATS == '10' && NS1ATS == '1' then
//!       return TRUE;  // → C_BAD_STE
//! ```
//!
//! When `NS1ATS==1` AND `eats==0b10`, `configure_stream()` must emit `CBadSte`
//! and return `Err`, even for a two-stage stream with `S2S==0`.
//!
//! Requires new API: `set_ns1ats_supported(bool)`.
//!
//! BEFORE FIX: method missing → compile error → FAILS.
//! AFTER FIX:  `CBadSte` emitted, `Err` returned → PASSES.
//!
//! # BUG-AUDIT-02 (Both §6.3.4 IDR3): XNX (bit 4) not gated on S2P
//!
//! ARM IHI0070G.b §6.3.4 IDR3 bit 4 (XNX):
//! > RES0 when IDR0.S2P==0.
//!
//! `get_idr3()` must return bit 4 == 0 when `set_s2p_supported(false)` has been called.
//!
//! BEFORE FIX: `get_idr3()` always returns bit 4 == 1 → FAILS.
//! AFTER FIX:  bit 4 == 0 when S2P=0 → PASSES.
//!
//! # BUG-AUDIT-03 (Both §4.5.2): CMD_PRI_RESP not silently ignored when SMMUEN==0
//!
//! ARM IHI0070G.b §4.5.2 lines 6075-6079:
//! ```text
//!   if SMMU_CR0.SMMUEN == '0' then RETURN;  // silent ignore — no side-effects
//! ```
//!
//! When SMMUEN=0, `CMD_PRI_RESP` must be silently discarded with no CERROR_ILL
//! and no advance of `PRIQ_CONS`.  The pending PRG entry must remain in the PRI queue.
//!
//! BEFORE FIX: `process_command_queue()` gates only on CMDQEN, not SMMUEN.
//!             `CMD_PRI_RESP` executes, consumes the PRG entry, `get_pri_queue()`
//!             becomes empty → FAILS.
//! AFTER FIX:  `CMD_PRI_RESP` silently ignored → entry remains → PASSES.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventType, FaultMode, PRIEntry, SecurityState,
    StreamConfig, StreamID,
};
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

/// Returns true when GERROR.CMDQ_ERR is active (GERROR[0] set in the active mask).
fn is_gerror_cmdq_err_active(smmu: &SMMU) -> bool {
    smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR != 0
}

/// Build a two-stage StreamConfig with the given EATS and S2S values.
///
/// In Rust, `STE.S2S` is represented by the `s2_stall` field of `StreamConfig`.
fn two_stage_config(eats: u8, s2s: bool) -> StreamConfig {
    let mut cfg = StreamConfig::two_stage();
    cfg.security_state = SecurityState::NonSecure;
    cfg.fault_mode = FaultMode::Terminate;
    cfg.eats = eats;
    cfg.s2_stall = s2s; // STE.S2S — stage-2 stall; maps to s2_stall in Rust
    cfg
}

/// Build a stage-1-only StreamConfig with EATS=1 (ATS enabled).
///
/// EATS=1 is required for `submit_page_request()` to succeed (the PRI queue
/// only accepts page requests when ATS is enabled on the stream).
fn stage1_ats_config() -> StreamConfig {
    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.fault_mode = FaultMode::Terminate;
    cfg.eats = 1u8; // ATS enabled — required for submit_page_request
    cfg.s2_stall = false;
    cfg
}

/// Submit a page request and call `process_pri_queue()` to emit `EPageRequest`.
///
/// Returns the number of entries in the PRI queue after emission.
/// The entry remains in the VecDeque until `CMD_PRI_RESP` pops it.
fn submit_and_emit_page_request(smmu: &SMMU, stream_id: u32, prg_idx: u16) -> usize {
    let req = PRIEntry {
        stream_id,
        pasid: 0,
        requested_address: 0x1000,
        access_type: AccessType::Read,
        is_last_request: true,
        timestamp: 0,
        prg_index: prg_idx,
        security_state: SecurityState::NonSecure,
        span: 0,
    };
    // Ignore the result from submit_page_request — it may return Ok or Err
    // depending on whether PRIQEN is effectively on; the queue state is what matters.
    let _ = smmu.submit_page_request(req);
    let _ = smmu.process_pri_queue();
    smmu.get_pri_queue().len()
}

/// Submit a `CMD_PRI_RESP` and call `process_command_queue()`.
fn submit_pri_resp(smmu: &SMMU, stream_id: u32, pasid: u32, prg_index: u16) {
    let mut cmd = CommandEntry::new(CommandType::PriResp, stream_id, pasid);
    cmd.prg_index = prg_index;
    let _ = smmu.submit_command(cmd);
    let _ = smmu.process_command_queue();
}

// ============================================================================
// BUG-AUDIT-01: EATS==0b10 + NS1ATS==1 → CBadSte (ARM §5.2 SteIllegal)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: NS1ATS=1, EATS=2, two-stage, S2S=0 → CBadSte
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §5.2 SteIllegal(): EATS==0b10 AND NS1ATS==1 → CBadSte,
// even for a two-stage stream with S2S=0 (which is otherwise legal).
//
// Requires new API: set_ns1ats_supported(bool).
//
// BEFORE FIX: set_ns1ats_supported() won't compile (method missing) → FAILS.
// AFTER FIX:  CBadSte emitted → PASSES.

/// BUG-AUDIT-01: `NS1ATS=1` + `eats=2` + two-stage + `s2s=false` → `CBadSte`.
///
/// ARM §5.2 SteIllegal(): `EATS==0b10 && NS1ATS==1` is an illegal STE regardless
/// of Config or S2S value.
///
/// BEFORE FIX: compile error (no `set_ns1ats_supported`) → FAILS.
/// AFTER FIX:  `configure_stream()` returns `Err`, `CBadSte` in event queue → PASSES.
#[test]
fn bug_audit_01_ns1ats1_eats2_two_stage_emits_c_bad_ste() {
    // §5.2 SteIllegal(): NS1ATS=1 AND EATS=2 (split-stage ATS) → CBadSte.
    let smmu = make_smmu();

    // Enable NS1ATS feature (IDR0.NS1ATS=1).
    // Requires new API: set_ns1ats_supported(bool).
    smmu.set_ns1ats_supported(true);

    // EATS=2 (split-stage ATS), two-stage Config, S2S=0.
    let cfg = two_stage_config(2u8, false);

    let result = smmu.configure_stream(sid(0x50), cfg);

    assert!(
        result.is_err(),
        "BUG-AUDIT-01: configure_stream() with NS1ATS=1 AND eats=2 must return Err \
         (ARM §5.2 SteIllegal: EATS==0b10 AND NS1ATS==1 is illegal). \
         Current code: no set_ns1ats_supported() method exists. Got: Ok"
    );

    assert!(
        has_event(&smmu, EventType::CBadSte),
        "BUG-AUDIT-01: CBadSte event must be emitted when NS1ATS=1 AND eats=2 \
         (ARM §5.2 SteIllegal()). Current code: no NS1ATS+EATS validation."
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 (baseline): NS1ATS=0, EATS=2, two-stage, S2S=0 → accepted (no error)
// ─────────────────────────────────────────────────────────────────────────────
//
// When NS1ATS==0, EATS=2 + two-stage + S2S=0 is legal.
// This test verifies the NS1ATS guard fires ONLY when NS1ATS==1.
//
// BEFORE FIX: (fails to compile because set_ns1ats_supported() is missing).
// AFTER FIX:  config accepted, no CBadSte → PASSES.

/// BUG-AUDIT-01 baseline: `NS1ATS=0` + `eats=2` + two-stage + `s2s=false` → accepted.
///
/// ARM §5.2 SteIllegal(): the NS1ATS+EATS check fires only when `NS1ATS==1`.
///
/// BEFORE FIX: (passes once API exists). AFTER FIX: must still pass (regression guard).
#[test]
fn bug_audit_01_ns1ats0_eats2_two_stage_accepted() {
    // Baseline: NS1ATS=0 AND EATS=2 AND two-stage AND S2S=0 must be ACCEPTED.
    let smmu = make_smmu();

    // Disable NS1ATS — default must be 0; set explicitly for clarity.
    smmu.set_ns1ats_supported(false);

    let cfg = two_stage_config(2u8, false);

    let result = smmu.configure_stream(sid(0x51), cfg);

    assert!(
        result.is_ok(),
        "BUG-AUDIT-01 baseline: configure_stream() with NS1ATS=0 AND eats=2 AND \
         two-stage AND S2S=0 must be ACCEPTED (ARM §5.2 SteIllegal: the NS1ATS guard \
         fires only when NS1ATS==1). The fix must not over-reject this config. Got: Err"
    );

    assert!(
        !has_event(&smmu, EventType::CBadSte),
        "BUG-AUDIT-01 baseline: no CBadSte expected for NS1ATS=0 + eats=2 + \
         two-stage + S2S=0."
    );
}

// ============================================================================
// BUG-AUDIT-02: IDR3.XNX (bit 4) not gated on S2P (ARM §6.3.4)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: S2P disabled → IDR3 bit 4 (XNX) must be 0
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §6.3.4 IDR3 bit 4 (XNX): "RES0 when IDR0.S2P==0."
// When set_s2p_supported(false) is called, get_idr3() must clear bit 4.
//
// BEFORE FIX: get_idr3() returns bit 4 == 1 even when S2P=0 → FAILS.
// AFTER FIX:  get_idr3() returns bit 4 == 0 when S2P=0 → PASSES.

/// BUG-AUDIT-02: S2P disabled → `get_idr3()` bit 4 (XNX) must be 0.
///
/// ARM §6.3.4: `IDR3.XNX` (bit 4) is RES0 when `IDR0.S2P==0`.
///
/// BEFORE FIX: `get_idr3()` always sets bit 4 → FAILS.
/// AFTER FIX:  bit 4 == 0 when S2P=0 → PASSES.
#[test]
fn bug_audit_02_s2p_disabled_idr3_xnx_is_zero() {
    // §6.3.4: IDR3.XNX (bit 4) must be 0 when IDR0.S2P==0.
    let smmu = make_smmu();

    // Disable stage-2 translation (S2P=0).
    smmu.set_s2p_supported(false);

    let idr3 = smmu.get_idr3();

    assert_eq!(
        idr3 & (1u32 << 4),
        0u32,
        "BUG-AUDIT-02: get_idr3() bit 4 (XNX) must be 0 when S2P==0 \
         (ARM §6.3.4: XNX is RES0 when IDR0.S2P=0). \
         Current code: get_idr3() always sets bit 4 regardless of S2P setting. \
         Actual IDR3 = 0x{:08X}",
        idr3
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 (regression guard updated by BUG-AUDIT-S2-XNX):
// S2P enabled → IDR3 bit 4 (XNX) must be 0 (FEAT_XNX not implemented)
// ─────────────────────────────────────────────────────────────────────────────
//
// BUG-AUDIT-S2-XNX supersedes the original BUG-AUDIT-02 regression guard.
// FEAT_XNX (separate S2UXN enforcement in PagePermissions) is not implemented,
// so IDR3.XNX must be 0 regardless of S2P (ARM §6.3.4, §2.3).
//
// BEFORE FIX: get_idr3() sets bit 4 when S2P=1 — incorrect, FEAT_XNX not implemented.
// AFTER FIX:  bit 4 == 0 regardless of S2P setting → PASSES.

/// BUG-AUDIT-02 / BUG-AUDIT-S2-XNX: S2P enabled → `get_idr3()` bit 4 (XNX) must be 0.
///
/// BUG-AUDIT-S2-XNX (ARM §6.3.4, §2.3): IDR3.XNX must be 0 because FEAT_XNX
/// (separate stage-2 unprivileged execute-never enforcement) is not implemented
/// in the PagePermissions model.  Advertising XNX=1 without enforcing S2UXN
/// would be spec-non-compliant.  XNX=0 is correct for all S2P states.
///
/// Regression guard updated to reflect the corrected spec interpretation.
#[test]
fn bug_audit_02_s2p_enabled_idr3_xnx_is_one() {
    // BUG-AUDIT-S2-XNX: even with S2P=1, XNX must be 0 (FEAT_XNX not implemented).
    let smmu = make_smmu();

    // Enable stage-2 translation (S2P=1).
    smmu.set_s2p_supported(true);

    let idr3 = smmu.get_idr3();

    assert_eq!(
        idr3 & (1u32 << 4),
        0u32,
        "BUG-AUDIT-S2-XNX: get_idr3() bit 4 (XNX) must be 0 even when S2P==1 \
         because FEAT_XNX (S2UXN enforcement) is not implemented (ARM §6.3.4, §2.3). \
         Actual IDR3 = 0x{:08X}",
        idr3
    );
}

// ============================================================================
// BUG-AUDIT-03: CMD_PRI_RESP not silently ignored when SMMUEN==0 (ARM §4.5.2)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: SMMUEN=0, CMD_PRI_RESP submitted → PRG entry NOT consumed
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.5.2 lines 6075-6079:
//   if SMMU_CR0.SMMUEN == '0' then RETURN;  // silent ignore
//
// When SMMUEN=0, CMD_PRI_RESP must be silently discarded:
//   - No CERROR_ILL raised.
//   - PRIQ_CONS must NOT advance.
//   - The pending PRG entry must remain in the PRI queue.
//
// Test strategy:
//   1. Fully enable the SMMU (SMMUEN=1, CMDQEN=1, PRIQEN=1).
//   2. Configure a stream with EATS=1 (ATS enabled).
//   3. Submit a page request; process_pri_queue() emits EPageRequest
//      but keeps the entry in the VecDeque.
//   4. Clear SMMUEN while keeping CMDQEN=1 via set_cr0().
//   5. Submit CMD_PRI_RESP + process_command_queue().
//   6. Assert: PRI queue unchanged, no CERROR_ILL.
//
// BEFORE FIX: process_command_queue() gates on CMDQEN only; CMD_PRI_RESP runs,
//             pops the VecDeque entry → get_pri_queue() empty → FAILS.
// AFTER FIX:  CMD_PRI_RESP silently ignored → entry remains → PASSES.

/// BUG-AUDIT-03: `SMMUEN=0`, `CMD_PRI_RESP` submitted → PRG entry NOT consumed.
///
/// ARM §4.5.2 lines 6075-6079: CMD_PRI_RESP is silently ignored when SMMUEN=0.
///
/// BEFORE FIX: command executes, consumes PRG entry → FAILS.
/// AFTER FIX:  silently ignored → entry remains → PASSES.
#[test]
fn bug_audit_03_smmuen0_pri_resp_prg_entry_not_consumed() {
    // §4.5.2: CMD_PRI_RESP must be silently ignored when SMMUEN=0.
    let smmu = make_smmu(); // SMMUEN=1, CMDQEN=1, PRIQEN=1

    let stream_id: u32 = 0x60;
    let prg_idx: u16 = 0;

    // Configure a stream with ATS enabled (eats=1) so page requests are accepted.
    smmu.configure_stream(sid(stream_id), stage1_ats_config())
        .expect("configure_stream should succeed for stage1 + eats=1");
    smmu.enable_stream(sid(stream_id))
        .expect("enable_stream should succeed");

    // Submit a page request and emit EPageRequest.
    // After process_pri_queue(), the entry remains in the VecDeque.
    let queue_size_before = submit_and_emit_page_request(&smmu, stream_id, prg_idx);
    assert!(
        queue_size_before > 0,
        "BUG-AUDIT-03 setup: PRI entry must be present in queue after submission. \
         Got queue_size={}",
        queue_size_before
    );

    // Disable SMMUEN while keeping CMDQEN=1.
    // set_cr0 with CMDQEN | EVENTQEN | PRIQEN, but NOT SMMUEN.
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    assert!(
        !smmu.is_enabled(),
        "BUG-AUDIT-03 setup: SMMU must be disabled (SMMUEN=0) for this test; \
         is_enabled() returned true"
    );
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_CMDQEN,
        0,
        "BUG-AUDIT-03 setup: CMDQEN must remain set after disabling SMMUEN"
    );

    // Submit CMD_PRI_RESP — must be silently ignored because SMMUEN=0.
    submit_pri_resp(&smmu, stream_id, 0, prg_idx);

    // PRG entry must still be in the queue (PRIQ_CONS not advanced).
    let queue_size_after = smmu.get_pri_queue().len();
    assert_eq!(
        queue_size_after, queue_size_before,
        "BUG-AUDIT-03: CMD_PRI_RESP with SMMUEN=0 must NOT consume the PRG entry \
         (ARM §4.5.2 lines 6075-6079: when SMMUEN=0, CMD_PRI_RESP is silently ignored \
         with no side-effects). \
         Current code: process_command_queue() gates on CMDQEN only, so CMD_PRI_RESP \
         executes and advances PRIQ_CONS even when SMMUEN=0. \
         Queue size before={}, after={}",
        queue_size_before, queue_size_after
    );

    // No CERROR_ILL must be raised (silent ignore, not an error).
    assert!(
        !is_gerror_cmdq_err_active(&smmu),
        "BUG-AUDIT-03: CMD_PRI_RESP with SMMUEN=0 must NOT raise CERROR_ILL \
         (ARM §4.5.2: the command is silently ignored, not flagged as illegal)."
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 (baseline): SMMUEN=1, CMD_PRI_RESP submitted → PRG entry consumed normally
// ─────────────────────────────────────────────────────────────────────────────
//
// When SMMUEN=1 (normal operation), CMD_PRI_RESP must be processed normally
// and consume the matching PRG entry.
//
// BEFORE FIX: (already passes — CMD_PRI_RESP is always executed today).
// AFTER FIX:  must still pass — regression guard.

/// BUG-AUDIT-03 baseline: `SMMUEN=1`, `CMD_PRI_RESP` → PRG entry consumed normally.
///
/// ARM §4.5.2: the SMMUEN=0 guard must not fire when SMMUEN=1.
///
/// BEFORE FIX: (passes already). AFTER FIX: must still pass (regression guard).
#[test]
fn bug_audit_03_smmuen1_pri_resp_prg_entry_consumed() {
    // Regression: SMMUEN=1 → CMD_PRI_RESP must be processed normally.
    let smmu = make_smmu(); // SMMUEN=1, CMDQEN=1, PRIQEN=1

    let stream_id: u32 = 0x61;
    let prg_idx: u16 = 0;

    smmu.configure_stream(sid(stream_id), stage1_ats_config())
        .expect("configure_stream should succeed");
    smmu.enable_stream(sid(stream_id))
        .expect("enable_stream should succeed");

    let queue_size_before = submit_and_emit_page_request(&smmu, stream_id, prg_idx);
    assert!(
        queue_size_before > 0,
        "BUG-AUDIT-03 regression setup: PRI entry must be present; got queue_size={}",
        queue_size_before
    );

    // SMMU is enabled (SMMUEN=1) — CMD_PRI_RESP must be processed normally.
    submit_pri_resp(&smmu, stream_id, 0, prg_idx);

    let queue_size_after = smmu.get_pri_queue().len();
    assert!(
        queue_size_after < queue_size_before,
        "BUG-AUDIT-03 regression: CMD_PRI_RESP with SMMUEN=1 must consume the PRG \
         entry (ARM §4.5.2 normal operation). \
         The fix must not prevent CMD_PRI_RESP from executing when SMMUEN=1. \
         Queue size before={}, after={}",
        queue_size_before, queue_size_after
    );
}
