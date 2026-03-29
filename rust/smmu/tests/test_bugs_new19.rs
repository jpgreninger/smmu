//! TDD failing tests for BUG-AUDIT-NEW-01, BUG-AUDIT-NEW-02, and
//! BUG-AUDIT-NEW-03 (Rust).
//!
//! Each test that exercises a bug is written to FAIL with the current code
//! (red) and PASS only after the corresponding fix is applied (green).
//! Tests explicitly marked as regression or baseline always PASS.
//!
//! # BUG-AUDIT-NEW-01 (§4.4.1.1): `TlbiS2Ipa` missing RIL reserved-param
//! CERROR_ILL
//!
//! ARM IHI0070G.b §4.4.1.1: the combination `TG != 0` AND `NUM == 0` AND
//! `SCALE == 0` AND `TTL == 0` is Reserved for all RIL TLBI commands.
//!
//! This guard is already implemented for `TlbiNhVa`, `TlbiNhVaa`, `TlbiEl2Va`,
//! and `TlbiEl2Vaa` (see `test_bugs_new12.rs` NEW-BUG-2 and `test_bugs_new13.rs`
//! BUG-NEW-C).  It is missing from `TlbiS2Ipa`.
//!
//! Current `TlbiS2Ipa` handler (`smmu/mod.rs` ~line 6379):
//! ```text
//!   CommandType::TlbiS2Ipa => {
//!       // S2P guard ...
//!       let ipa_end = if command.ril {
//!           command.end_address
//!       } else {
//!           ipa_start | 0xFFF
//!       };
//!   }
//! ```
//! There is no check for the reserved `TG != 0, NUM=0, SCALE=0, TTL=0`
//! combination before entering the RIL path.
//!
//! BEFORE FIX: `TlbiS2Ipa` with `ril=true, tg=1, num=0, scale=0, ttl=0`
//!   executes silently; `GERROR.CMDQ_ERR` remains 0 → FAILS.
//! AFTER FIX:  `CERROR_ILL` raised, `GERROR.CMDQ_ERR` set → PASSES.
//!
//! # BUG-AUDIT-NEW-03 (§4.4.2): IDR0.S1P not configurable; NH_* missing
//! S1P==0 guard
//!
//! ARM IHI0070G.b §4.4.2 / §4.4.2.1..4: `CMD_TLBI_NH_ALL`, `CMD_TLBI_NH_ASID`,
//! and `CMD_TLBI_NH_VA` are non-hypervisor stage-1 TLB invalidation commands.
//! When `IDR0.S1P == 0` (stage-1 translation not implemented), these commands
//! must raise `CERROR_ILL` — there are no stage-1 TLB entries to invalidate.
//!
//! The existing NH_* handlers perform the TLB invalidation unconditionally;
//! there is no `IDR0.S1P == 0 → CERROR_ILL` guard.
//!
//! Furthermore, `IDR0.S1P` (bit 1) is currently hardcoded to `1u32 << 1` in
//! `get_idr0()` — there is no `s1p_supported` `AtomicBool` field and no
//! `set_s1p_supported(bool)` API.
//!
//! BEFORE FIX:
//!   - `set_s1p_supported(false)` does not compile (method does not exist) → tests FAIL.
//!   - NH_* handlers always execute without S1P guard → tests FAIL.
//!
//! AFTER FIX:
//!   - `set_s1p_supported(false)` lowers bit 1 of `get_idr0()`.
//!   - NH_* handlers check `IDR0.S1P == 0` → CERROR_ILL → PASSES.
//!
//! # BUG-AUDIT-NEW-02 (§4.3.3): `CfgiCd` / `CfgiCdAll` global IDR0.S1P guard
//!
//! ARM IHI0070G.b §4.3.3 / §4.3.4: `CMD_CFGI_CD` and `CMD_CFGI_CD_ALL` raise
//! `CERROR_ILL` when "stage 1 is not implemented", meaning `IDR0.S1P == 0`.
//!
//! The corrected `test_bugs_new17.rs` tests verify the NO-error path (bypass /
//! stage-2-only stream with `S1P=1` → silent no-op).  This file adds the two
//! tests that verify the positive CERROR_ILL path (`S1P=0` → error).
//!
//! BEFORE FIX: `set_s1p_supported(false)` does not compile (method missing)
//!   and `CfgiCd` / `CfgiCdAll` do not check IDR0.S1P → FAILS.
//! AFTER FIX:  `IDR0.S1P == 0` → CERROR_ILL raised → PASSES.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]
#![allow(dead_code)]

use smmu::types::{
    CommandEntry, CommandType, FaultMode, SecurityState, StreamConfig, StreamID, IOVA, PASID,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

fn pasid(n: u32) -> PASID {
    PASID::new(n).unwrap()
}

fn iova(n: u64) -> IOVA {
    IOVA::new(n).unwrap()
}

/// Build an SMMU with all queues and global enable active.
fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    smmu
}

/// Returns true when `GERROR.CMDQ_ERR` (bit 0) is active.
///
/// Matches the helper style of `test_bugs_new17.rs` (non-XOR form).
fn is_gerror_cmdq_err_active(smmu: &SMMU) -> bool {
    smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR != 0
}

/// Build a stage-1-only `StreamConfig`.
fn stage1_only_config() -> StreamConfig {
    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.fault_mode = FaultMode::Terminate;
    cfg
}

/// Build a bypass-only `StreamConfig`.
fn bypass_config() -> StreamConfig {
    StreamConfig::bypass()
}

// ============================================================================
// BUG-AUDIT-NEW-01 (§4.4.1.1): TlbiS2Ipa missing RIL reserved-param guard
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: TlbiS2Ipa with ril=true, tg=1, num=0, scale=0, ttl=0 → CERROR_ILL
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.4.1.1: The combination TG != 0b00, NUM == 0, SCALE == 0, TTL == 0b00
// is Reserved for all RIL TLBI commands.  SMMU must raise CERROR_ILL.
//
// This guard exists for TlbiNhVa, TlbiEl2Va, etc., but is absent in
// TlbiS2Ipa.
//
// BEFORE FIX: TlbiS2Ipa executes silently; GERROR.CMDQ_ERR remains 0 → FAILS.
// AFTER FIX:  CERROR_ILL raised, GERROR.CMDQ_ERR set → PASSES.

/// BUG-AUDIT-NEW-01: `TlbiS2Ipa` with `ril=true, tg=1, num=0, scale=0, ttl=0`
/// must raise `CERROR_ILL` and set `GERROR.CMDQ_ERR`.
///
/// ARM §4.4.1.1: `TG != 0b00` AND `NUM == 0` AND `SCALE == 0` AND `TTL == 0`
/// is Reserved for all RIL TLBI commands.
///
/// BEFORE FIX: no reserved-param guard in `TlbiS2Ipa` → command executes
///   silently, `GERROR.CMDQ_ERR` remains 0 → FAILS.
/// AFTER FIX:  guard added to `TlbiS2Ipa` → `CERROR_ILL` raised → PASSES.
#[test]
fn tlbi_s2_ipa_ril_reserved_raises_cerror_ill() {
    // §4.4.1.1: TlbiS2Ipa with TG!=0 AND NUM=0 AND SCALE=0 AND TTL=0 is Reserved.
    // set_s2p_supported(true) ensures the S2P guard does not fire before the RIL check.
    let smmu = make_smmu();
    smmu.set_s2p_supported(true);

    // Clear any pre-existing GERROR state.
    smmu.clear_gerror(smmu.get_gerror() ^ smmu.get_gerrorn());

    let mut cmd = CommandEntry::new(CommandType::TlbiS2Ipa, 0, 0);
    cmd.start_address = 0x0010_0000;
    cmd.vmid          = 1;
    cmd.ril           = true;
    cmd.tg            = 1;   // TG != 0b00 (64KB in wrong old encoding; 4KB per §4.4.1.1)
    cmd.num           = 0;   // NUM == 0 — must trigger Reserved check
    cmd.scale         = 0;   // SCALE == 0
    cmd.ttl           = 0;   // TTL == 0b00 → Reserved combination per §4.4.1.1

    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-AUDIT-NEW-01: TlbiS2Ipa with ril=true, tg=1, num=0, scale=0, ttl=0 \
         must raise CERROR_ILL and set GERROR.CMDQ_ERR. \
         ARM §4.4.1.1: TG!=0 AND NUM=0 AND SCALE=0 AND TTL=0 is Reserved for all \
         RIL TLBI commands — the guard already exists for TlbiNhVa/TlbiEl2Va but is \
         absent in TlbiS2Ipa. \
         Current code: no reserved-param check in TlbiS2Ipa → executes silently. \
         GERROR=0x{:08X}",
        smmu.get_gerror()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 (regression): TlbiS2Ipa with ril=true, tg=1, num=4, scale=1, ttl=0
//   → valid RIL params, no GERROR
// ─────────────────────────────────────────────────────────────────────────────
//
// Valid RIL combination: TG=1, NUM=4 (!=0) — the Reserved condition is not met.
// The command must execute without CERROR_ILL.
//
// ALWAYS PASSES — regression guard confirming valid RIL still works after fix.

/// BUG-AUDIT-NEW-01 regression: `TlbiS2Ipa` with valid RIL params
/// (`ril=true, tg=1, num=4, scale=1, ttl=0`) must NOT raise `GERROR_CMDQ_ERR`.
///
/// ARM §4.4.1.1: the Reserved condition requires NUM == 0; `num=4` is valid.
///
/// Always PASSES — regression guard.
#[test]
fn tlbi_s2_ipa_ril_valid_no_error() {
    // §4.4.1.1: TlbiS2Ipa with TG=1, NUM=4 (!=0), SCALE=1 — valid RIL params.
    // Must execute successfully without raising CERROR_ILL.
    let smmu = make_smmu();
    smmu.set_s2p_supported(true);

    smmu.clear_gerror(smmu.get_gerror() ^ smmu.get_gerrorn());

    let mut cmd = CommandEntry::new(CommandType::TlbiS2Ipa, 0, 0);
    cmd.start_address = 0x0020_0000;
    cmd.vmid          = 2;
    cmd.ril           = true;
    cmd.tg            = 1;  // TG=1 (4KB granule per §4.4.1.1)
    cmd.num           = 4;  // NUM != 0 → not the Reserved combination
    cmd.scale         = 1;  // SCALE=1
    cmd.ttl           = 0;

    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert!(
        !is_gerror_cmdq_err_active(&smmu),
        "BUG-AUDIT-NEW-01 regression: TlbiS2Ipa with valid RIL params \
         (tg=1, num=4, scale=1, ttl=0) must NOT raise GERROR.CMDQ_ERR. \
         ARM §4.4.1.1: the Reserved condition requires NUM==0 AND SCALE==0; \
         num=4 is a valid non-zero value. \
         GERROR=0x{:08X}",
        smmu.get_gerror()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 (regression): TlbiS2Ipa with ril=false → no GERROR regardless of tg
// ─────────────────────────────────────────────────────────────────────────────
//
// The Reserved check only applies when ril=true.  When ril=false the TG, NUM,
// SCALE, and TTL fields are ignored.
//
// ALWAYS PASSES — regression guard.

/// BUG-AUDIT-NEW-01 regression: `TlbiS2Ipa` with `ril=false` must NOT raise
/// `GERROR_CMDQ_ERR`, even when `tg=1, num=0, scale=0, ttl=0`.
///
/// ARM §4.4.1.1: the Reserved condition only applies when `ril=true`.
///
/// Always PASSES — regression guard.
#[test]
fn tlbi_s2_ipa_ril_false_no_error() {
    // §4.4.1.1: when ril=false, TG/NUM/SCALE/TTL are ignored — no Reserved check.
    let smmu = make_smmu();
    smmu.set_s2p_supported(true);

    smmu.clear_gerror(smmu.get_gerror() ^ smmu.get_gerrorn());

    let mut cmd = CommandEntry::new(CommandType::TlbiS2Ipa, 0, 0);
    cmd.start_address = 0x0030_0000;
    cmd.vmid          = 3;
    cmd.ril           = false; // RIL disabled — TG/NUM/SCALE/TTL fields are ignored
    cmd.tg            = 1;     // Would be Reserved if ril=true, but ril=false here
    cmd.num           = 0;
    cmd.scale         = 0;
    cmd.ttl           = 0;

    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert!(
        !is_gerror_cmdq_err_active(&smmu),
        "BUG-AUDIT-NEW-01 regression: TlbiS2Ipa with ril=false must NOT raise \
         GERROR.CMDQ_ERR even when tg=1, num=0, scale=0, ttl=0. \
         ARM §4.4.1.1: the Reserved RIL parameter check only applies when ril=true; \
         when ril=false the TG/NUM/SCALE/TTL fields are ignored. \
         GERROR=0x{:08X}",
        smmu.get_gerror()
    );
}

// ============================================================================
// BUG-AUDIT-NEW-03 (§4.4.2): IDR0.S1P not configurable; NH_* missing S1P guard
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 (regression): IDR0 bit 1 (S1P) is 1 by default
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §6.3.1: IDR0.S1P (bit 1) must be 1 when stage-1 translation is supported.
// This is the existing hard-coded default.
//
// ALWAYS PASSES — regression baseline confirming the default value is correct.

/// BUG-AUDIT-NEW-03 baseline: `IDR0.S1P` (bit 1) is 1 by default.
///
/// ARM §6.3.1: S1P = 1 means stage-1 translation is implemented.
///
/// Always PASSES — regression guard confirming the default.
#[test]
fn s1p_default_true_idr0_bit1_set() {
    // §6.3.1: IDR0.S1P (bit 1) must be 1 in the default SMMU configuration.
    let smmu = make_smmu();

    let idr0 = smmu.get_idr0();

    assert!(
        idr0 & (1u32 << 1) != 0,
        "BUG-AUDIT-NEW-03 baseline: IDR0.S1P (bit 1) must be 1 by default. \
         ARM §6.3.1: S1P=1 indicates stage-1 translation is implemented. \
         IDR0=0x{:08X}",
        idr0
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: set_s1p_supported(false) clears IDR0 bit 1
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §6.3.1: IDR0.S1P (bit 1) must reflect whether stage-1 is supported.
// When `set_s1p_supported(false)` is called, bit 1 must be 0.
//
// BEFORE FIX: `set_s1p_supported` method does not exist → compile error → FAILS.
// AFTER FIX:  `s1p_supported` AtomicBool added; `get_idr0()` gates bit 1 on it
//   → IDR0.S1P == 0 when set_s1p_supported(false) → PASSES.

/// BUG-AUDIT-NEW-03: `set_s1p_supported(false)` must clear `IDR0.S1P` (bit 1).
///
/// ARM §6.3.1: IDR0.S1P (bit 1) is 0 when stage-1 is not supported.
///
/// BEFORE FIX: method does not exist → compile error → FAILS.
/// AFTER FIX:  bit 1 cleared when s1p_supported=false → PASSES.
#[test]
fn set_s1p_supported_false_clears_idr0_bit1() {
    // §6.3.1: IDR0.S1P (bit 1) must be 0 after set_s1p_supported(false).
    let smmu = make_smmu();

    // BEFORE FIX: this line does not compile — set_s1p_supported() does not exist.
    // AFTER FIX:  the method is added; bit 1 in get_idr0() is cleared.
    smmu.set_s1p_supported(false);

    let idr0 = smmu.get_idr0();

    assert!(
        idr0 & (1u32 << 1) == 0,
        "BUG-AUDIT-NEW-03: IDR0.S1P (bit 1) must be 0 after set_s1p_supported(false). \
         ARM §6.3.1: S1P=0 indicates stage-1 translation is not implemented. \
         Current get_idr0() hardcodes `1u32 << 1` unconditionally — no s1p_supported field. \
         IDR0=0x{:08X}",
        idr0
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3: TlbiNhAll when S1P disabled → CERROR_ILL
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.4.2.1: CMD_TLBI_NH_ALL invalidates non-hypervisor stage-1 TLB entries.
// When IDR0.S1P == 0, stage-1 is not implemented; this command must raise
// CERROR_ILL per the general NH_* command rule.
//
// BEFORE FIX: `set_s1p_supported` does not compile → FAILS (compile error).
// AFTER FIX:  S1P=0 guard fires in TlbiNhAll handler → CERROR_ILL raised → PASSES.

/// BUG-AUDIT-NEW-03: `TlbiNhAll` when `IDR0.S1P == 0` must raise
/// `CERROR_ILL` and set `GERROR.CMDQ_ERR`.
///
/// ARM §4.4.2.1 / §4.4.2: NH_* commands require stage-1; CERROR_ILL when S1P=0.
///
/// BEFORE FIX: method does not exist (compile error) and no S1P guard → FAILS.
/// AFTER FIX:  `set_s1p_supported(false)` lowers S1P; guard in handler fires → PASSES.
#[test]
fn tlbi_nh_all_s1p_disabled_raises_cerror_ill() {
    // §4.4.2.1: TlbiNhAll with IDR0.S1P==0 must raise CERROR_ILL.
    let smmu = make_smmu();

    // BEFORE FIX: does not compile.
    smmu.set_s1p_supported(false);

    smmu.clear_gerror(smmu.get_gerror() ^ smmu.get_gerrorn());

    let mut cmd = CommandEntry::new(CommandType::TlbiNhAll, 0, 0);
    cmd.vmid = 1;

    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-AUDIT-NEW-03: TlbiNhAll when IDR0.S1P==0 must raise CERROR_ILL and \
         set GERROR.CMDQ_ERR. \
         ARM §4.4.2.1 / §4.4.2: NH_* commands invalidate non-hypervisor stage-1 TLB \
         entries; when S1P=0 stage-1 is not implemented and these commands are illegal. \
         Current code: no S1P guard in TlbiNhAll handler → executes silently. \
         GERROR=0x{:08X}",
        smmu.get_gerror()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4: TlbiNhAsid when S1P disabled → CERROR_ILL
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.4.2.2: CMD_TLBI_NH_ASID invalidates non-hypervisor stage-1 TLB entries
// by ASID.  Same S1P guard applies.
//
// BEFORE FIX: compile error / no guard → FAILS.
// AFTER FIX:  CERROR_ILL raised → PASSES.

/// BUG-AUDIT-NEW-03: `TlbiNhAsid` when `IDR0.S1P == 0` must raise
/// `CERROR_ILL` and set `GERROR.CMDQ_ERR`.
///
/// ARM §4.4.2.2: CMD_TLBI_NH_ASID requires stage-1; CERROR_ILL when S1P=0.
///
/// BEFORE FIX: compile error and no guard → FAILS.
/// AFTER FIX:  guard fires → PASSES.
#[test]
fn tlbi_nh_asid_s1p_disabled_raises_cerror_ill() {
    // §4.4.2.2: TlbiNhAsid with IDR0.S1P==0 must raise CERROR_ILL.
    let smmu = make_smmu();

    // BEFORE FIX: does not compile.
    smmu.set_s1p_supported(false);

    smmu.clear_gerror(smmu.get_gerror() ^ smmu.get_gerrorn());

    let mut cmd = CommandEntry::new(CommandType::TlbiNhAsid, 0, 0);
    cmd.vmid = 2;
    cmd.asid = 10;

    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-AUDIT-NEW-03: TlbiNhAsid when IDR0.S1P==0 must raise CERROR_ILL and \
         set GERROR.CMDQ_ERR. \
         ARM §4.4.2.2: CMD_TLBI_NH_ASID invalidates stage-1 entries; when S1P=0 \
         stage-1 is not implemented → command is illegal. \
         Current code: no S1P guard in TlbiNhAsid handler → executes silently. \
         GERROR=0x{:08X}",
        smmu.get_gerror()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5: TlbiNhVa when S1P disabled → CERROR_ILL
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.4.2.3: CMD_TLBI_NH_VA invalidates non-hypervisor stage-1 TLB entries
// by VA+ASID.  Same S1P guard applies.
//
// BEFORE FIX: compile error / no guard → FAILS.
// AFTER FIX:  CERROR_ILL raised → PASSES.

/// BUG-AUDIT-NEW-03: `TlbiNhVa` when `IDR0.S1P == 0` must raise
/// `CERROR_ILL` and set `GERROR.CMDQ_ERR`.
///
/// ARM §4.4.2.3: CMD_TLBI_NH_VA requires stage-1; CERROR_ILL when S1P=0.
///
/// BEFORE FIX: compile error and no guard → FAILS.
/// AFTER FIX:  guard fires → PASSES.
#[test]
fn tlbi_nh_va_s1p_disabled_raises_cerror_ill() {
    // §4.4.2.3: TlbiNhVa with IDR0.S1P==0 must raise CERROR_ILL.
    let smmu = make_smmu();

    // BEFORE FIX: does not compile.
    smmu.set_s1p_supported(false);

    smmu.clear_gerror(smmu.get_gerror() ^ smmu.get_gerrorn());

    let mut cmd = CommandEntry::new(CommandType::TlbiNhVa, 0, 0);
    cmd.vmid          = 3;
    cmd.asid          = 20;
    cmd.start_address = 0x0004_0000;
    cmd.ril           = false; // Use single-VA form to keep test focused on S1P guard

    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-AUDIT-NEW-03: TlbiNhVa when IDR0.S1P==0 must raise CERROR_ILL and \
         set GERROR.CMDQ_ERR. \
         ARM §4.4.2.3: CMD_TLBI_NH_VA invalidates stage-1 entries by VA+ASID; \
         when S1P=0 stage-1 is not implemented → command is illegal. \
         Current code: no S1P guard in TlbiNhVa handler → executes silently. \
         GERROR=0x{:08X}",
        smmu.get_gerror()
    );
}

// ============================================================================
// BUG-AUDIT-NEW-02 (§4.3.3): CfgiCd / CfgiCdAll global IDR0.S1P guard
// ============================================================================
//
// The corrected tests in test_bugs_new17.rs verify the NO-error path
// (bypass/stage-2-only stream with IDR0.S1P==1 → silent no-op).
//
// These two tests verify the CERROR_ILL path: IDR0.S1P==0 → error, regardless
// of what stream the command targets.

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: CfgiCd when IDR0.S1P==0 → CERROR_ILL
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.3.3 line 5362: "This command raises CERROR_ILL when stage 1 is not
// implemented."  "Stage 1 is not implemented" ≡ IDR0.S1P == 0.
//
// BEFORE FIX: `set_s1p_supported(false)` does not compile → FAILS.
// AFTER FIX:  IDR0.S1P==0 guard in CfgiCd handler → CERROR_ILL → PASSES.

/// BUG-AUDIT-NEW-02: `CfgiCd` when `IDR0.S1P == 0` must raise `CERROR_ILL`
/// and set `GERROR.CMDQ_ERR`.
///
/// ARM §4.3.3 line 5362: CERROR_ILL when stage-1 is not implemented globally.
///
/// BEFORE FIX: `set_s1p_supported` does not exist (compile error) → FAILS.
/// AFTER FIX:  IDR0.S1P==0 guard fires → CERROR_ILL → PASSES.
#[test]
fn cfgi_cd_s1p_disabled_raises_cerror_ill() {
    // §4.3.3: CfgiCd with IDR0.S1P==0 → CERROR_ILL (stage-1 not implemented globally).
    let smmu = make_smmu();

    // Configure a stage-1-only stream so the command is not an unknown-StreamID no-op.
    let stream_id: u32 = 0x80;
    smmu.configure_stream(sid(stream_id), stage1_only_config())
        .expect("configure_stream should succeed");
    smmu.enable_stream(sid(stream_id))
        .expect("enable_stream should succeed");

    // BEFORE FIX: does not compile.
    smmu.set_s1p_supported(false);

    smmu.clear_gerror(smmu.get_gerror() ^ smmu.get_gerrorn());
    smmu.clear_event_queue();

    let cmd = CommandEntry::new(CommandType::CfgiCd, stream_id, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-AUDIT-NEW-02: CfgiCd when IDR0.S1P==0 must raise CERROR_ILL and \
         set GERROR.CMDQ_ERR. \
         ARM §4.3.3 line 5362: 'This command raises CERROR_ILL when stage 1 is \
         not implemented.' Stage-1 not implemented ≡ IDR0.S1P==0. \
         Current code checks per-stream is_stage1_enabled() — not the IDR0.S1P flag. \
         After BUG-AUDIT-NEW-02 fix: guard uses IDR0.S1P==0 → CERROR_ILL. \
         GERROR=0x{:08X}",
        smmu.get_gerror()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: CfgiCdAll when IDR0.S1P==0 → CERROR_ILL
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.3.4 line 5388: same rule as CfgiCd applies to CfgiCdAll.
//
// BEFORE FIX: compile error → FAILS.
// AFTER FIX:  IDR0.S1P==0 guard fires → CERROR_ILL → PASSES.

/// BUG-AUDIT-NEW-02: `CfgiCdAll` when `IDR0.S1P == 0` must raise `CERROR_ILL`
/// and set `GERROR.CMDQ_ERR`.
///
/// ARM §4.3.4 line 5388: CERROR_ILL when stage-1 is not implemented globally.
///
/// BEFORE FIX: `set_s1p_supported` does not exist (compile error) → FAILS.
/// AFTER FIX:  IDR0.S1P==0 guard fires → CERROR_ILL → PASSES.
#[test]
fn cfgi_cd_all_s1p_disabled_raises_cerror_ill() {
    // §4.3.4: CfgiCdAll with IDR0.S1P==0 → CERROR_ILL.
    let smmu = make_smmu();

    // Configure a stage-1-only stream so the command is not an unknown-StreamID no-op.
    let stream_id: u32 = 0x81;
    smmu.configure_stream(sid(stream_id), stage1_only_config())
        .expect("configure_stream should succeed");
    smmu.enable_stream(sid(stream_id))
        .expect("enable_stream should succeed");

    // BEFORE FIX: does not compile.
    smmu.set_s1p_supported(false);

    smmu.clear_gerror(smmu.get_gerror() ^ smmu.get_gerrorn());
    smmu.clear_event_queue();

    let cmd = CommandEntry::new(CommandType::CfgiCdAll, stream_id, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-AUDIT-NEW-02: CfgiCdAll when IDR0.S1P==0 must raise CERROR_ILL and \
         set GERROR.CMDQ_ERR. \
         ARM §4.3.4 line 5388: 'This command raises CERROR_ILL when stage 1 is \
         not implemented.' Stage-1 not implemented ≡ IDR0.S1P==0. \
         Current code checks per-stream is_stage1_enabled() — not IDR0.S1P. \
         After BUG-AUDIT-NEW-02 fix: guard uses IDR0.S1P==0 → CERROR_ILL. \
         GERROR=0x{:08X}",
        smmu.get_gerror()
    );
}
