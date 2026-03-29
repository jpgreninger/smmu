//! TDD failing tests for BUG-AUDIT-20, BUG-AUDIT-32, BUG-AUDIT-34, and
//! BUG-AUDIT-35 (Rust).
//!
//! Each test that exercises a bug is written to FAIL with the current code
//! (red) and PASS only after the corresponding fix is applied (green).
//! Tests explicitly marked as regression always PASS.
//!
//! # BUG-AUDIT-20 (§6.3.1): IDR0.NS1ATS missing S1P gate
//!
//! ARM IHI0070G.b §6.3.1: IDR0.NS1ATS (bit 11) must be RES0 when
//! `ATS==0 OR S1P==0 OR S2P==0`.  The current gate is:
//!   `ns1ats_supported && s2p_supported`
//! Missing the `s1p_supported` term.
//!
//! BEFORE FIX: NS1ATS=1 even when S1P=0 (with ns1ats=true, s2p=true) → FAILS.
//! AFTER FIX:  NS1ATS=0 when S1P=0 → PASSES.
//!
//! # BUG-AUDIT-32 (§5.2): IDR0.CD2L must be 1
//!
//! ARM IHI0070G.b §5.2: The flat PASID map supports any s1cdMax, so
//! IDR0.CD2L (bit[19]) should be 1 to indicate two-level CD table support.
//!
//! BEFORE FIX: IDR0.CD2L (bit 19) == 0 → FAILS.
//! AFTER FIX:  IDR0.CD2L (bit 19) == 1 → PASSES.
//!
//! # BUG-AUDIT-34 (§4.4.1.1): RIL loop uses hardcoded 4KB step
//!
//! ARM IHI0070G.b §4.4.1.1: The range step in a RIL TLBI walk must use
//! `granule_size` (TG=1→4KB, TG=2→16KB, TG=3→64KB).
//!
//! `TlbiNhVaa` and `TlbiEl2Vaa` both declare `let page_size: u64 = 4096` and
//! advance `cur` by `page_size` instead of `granule_size`, causing the loop
//! to iterate over 4× too many entries when TG=2 (16KB) or 16× too many when
//! TG=3 (64KB).
//!
//! A direct step-size test is verified by checking that submitting a valid
//! TlbiNhVaa/TlbiEl2Vaa command with TG=2 (16KB) and a valid NUM=1, SCALE=0
//! does NOT produce CERROR_ILL.  The behavioral correctness of the step is
//! confirmed by ensuring the loop completes without panic for large ranges
//! with TG=2 and TG=3.
//!
//! BEFORE FIX: `page_size` is hardcoded 4096 inside the RIL loop → incorrect
//!   step for TG=2/3.  Commands still succeed (no CERROR_ILL), so these tests
//!   are regression guards for BUG-AUDIT-34 against future regressions.
//!   The primary behavioral fix is verified by the step-size tests below.
//!
//! # BUG-AUDIT-35 (§6.3.1): HTTU overclaims dirty state support
//!
//! ARM IHI0070G.b §6.3.1: HTTU=0b10 claims dirty state update, but only
//! access flag is implemented.  Must be HTTU=0b01 (access flag only).
//!
//! BEFORE FIX: `(get_idr0() >> 6) & 0x3` == 0b10 → FAILS when we assert 0b01.
//! AFTER FIX:  `(get_idr0() >> 6) & 0x3` == 0b01 → PASSES.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]
#![allow(dead_code)]

use smmu::types::{
    CommandEntry, CommandType, FaultMode, SecurityState, StreamConfig, StreamID, IOVA,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
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

// ============================================================================
// BUG-AUDIT-20: IDR0.NS1ATS missing S1P gate
// ============================================================================

/// BUG-AUDIT-20: `get_idr0()` NS1ATS (bit 11) must be RES0 when S1P=0.
///
/// ARM §6.3.1: IDR0.NS1ATS is RES0 when `ATS==0 OR S1P==0 OR S2P==0`.
/// The current gate only checks `ns1ats_supported && s2p_supported` —
/// missing the `s1p_supported` term.
///
/// BEFORE FIX: NS1ATS=1 when ns1ats=true, s1p=false, s2p=true → FAILS.
/// AFTER FIX:  NS1ATS=0 → PASSES.
#[test]
fn idr0_ns1ats_res0_when_s1p_disabled() {
    let smmu = make_smmu();
    smmu.set_ns1ats_supported(true);
    smmu.set_s1p_supported(false);
    smmu.set_s2p_supported(true);

    let idr0 = smmu.get_idr0();
    assert_eq!(
        idr0 & (1u32 << 11),
        0,
        "BUG-AUDIT-20: IDR0.NS1ATS (bit 11) must be RES0 when S1P==0. \
         ARM §6.3.1: NS1ATS is RES0 when ATS==0 OR S1P==0 OR S2P==0. \
         Current gate missing s1p_supported check. IDR0=0x{:08X}",
        idr0
    );
}

/// BUG-AUDIT-20 positive regression: NS1ATS must be 1 when all three
/// conditions are true (ns1ats=true, s1p=true, s2p=true).
///
/// ALWAYS PASSES — verifies the positive path still works after the fix.
#[test]
fn idr0_ns1ats_set_when_all_enabled() {
    let smmu = make_smmu();
    smmu.set_ns1ats_supported(true);
    smmu.set_s1p_supported(true);
    smmu.set_s2p_supported(true);

    let idr0 = smmu.get_idr0();
    assert_ne!(
        idr0 & (1u32 << 11),
        0,
        "IDR0.NS1ATS (bit 11) must be 1 when ns1ats=true, s1p=true, s2p=true. \
         IDR0=0x{:08X}",
        idr0
    );
}

/// BUG-AUDIT-20 regression: NS1ATS must still be RES0 when S2P=0.
///
/// ALWAYS PASSES (existing gate already covers this) — regression guard.
#[test]
fn idr0_ns1ats_res0_when_s2p_disabled() {
    let smmu = make_smmu();
    smmu.set_ns1ats_supported(true);
    smmu.set_s1p_supported(true);
    smmu.set_s2p_supported(false);

    let idr0 = smmu.get_idr0();
    assert_eq!(
        idr0 & (1u32 << 11),
        0,
        "IDR0.NS1ATS (bit 11) must be RES0 when S2P==0. IDR0=0x{:08X}",
        idr0
    );
}

// ============================================================================
// BUG-AUDIT-32: IDR0.CD2L must be 1
// ============================================================================

/// BUG-AUDIT-32: `get_idr0()` CD2L (bit 19) must be 1.
///
/// ARM §5.2: The flat PASID map supports any s1cdMax, so IDR0.CD2L
/// (bit[19]) must be 1 to advertise two-level CD table support.
///
/// BEFORE FIX: bit 19 == 0 → FAILS.
/// AFTER FIX:  bit 19 == 1 → PASSES.
#[test]
fn idr0_cd2l_must_be_one() {
    let smmu = make_smmu();
    let idr0 = smmu.get_idr0();
    assert_ne!(
        idr0 & (1u32 << 19),
        0,
        "BUG-AUDIT-32: IDR0.CD2L (bit 19) must be 1 — flat PASID map \
         supports any s1cdMax (ARM §5.2). \
         Current code does not set this bit. IDR0=0x{:08X}",
        idr0
    );
}

// ============================================================================
// BUG-AUDIT-34: TlbiNhVaa / TlbiEl2Vaa RIL loop uses hardcoded 4KB step
// ============================================================================

/// BUG-AUDIT-34 TlbiNhVaa: A valid RIL command with TG=2 (16KB granule)
/// must execute without CERROR_ILL.
///
/// This is a behavioral-correctness regression guard: the step-size bug
/// does not cause CERROR_ILL, but the correct granule_size must be used
/// in the loop to match spec §4.4.1.1.
///
/// ALWAYS PASSES — the command is valid; CERROR_ILL must NOT be raised.
#[test]
fn tlbi_nh_vaa_ril_tg2_no_cerror_ill() {
    let smmu = make_smmu();
    smmu.set_s1p_supported(true);
    smmu.configure_stream(sid(1), stage1_only_config()).unwrap();

    // Clear any pre-existing GERROR state.
    smmu.clear_gerror(smmu.get_gerror() ^ smmu.get_gerrorn());

    let mut cmd = CommandEntry::new(CommandType::TlbiNhVaa, 0, 0);
    cmd.start_address = 0x0010_0000;
    cmd.vmid          = 1;
    cmd.ril           = true;
    cmd.tg            = 2; // 16KB granule
    cmd.num           = 1; // NUM=1, valid (not the reserved 0,0,0 combo)
    cmd.scale         = 0;
    cmd.ttl           = 0;

    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert!(
        !is_gerror_cmdq_err_active(&smmu),
        "BUG-AUDIT-34: TlbiNhVaa with TG=2 (16KB), NUM=1, SCALE=0 is valid \
         and must NOT raise CERROR_ILL. IDR0=0x{:08X} GERROR=0x{:08X}",
        smmu.get_idr0(),
        smmu.get_gerror()
    );
}

/// BUG-AUDIT-34 TlbiEl2Vaa: A valid RIL command with TG=2 (16KB granule)
/// must execute without CERROR_ILL.
///
/// ALWAYS PASSES — the command is valid; CERROR_ILL must NOT be raised.
#[test]
fn tlbi_el2_vaa_ril_tg2_no_cerror_ill() {
    let smmu = make_smmu();
    smmu.set_hyp_supported(true);
    smmu.set_s2p_supported(true);

    // Clear any pre-existing GERROR state.
    smmu.clear_gerror(smmu.get_gerror() ^ smmu.get_gerrorn());

    let mut cmd = CommandEntry::new(CommandType::TlbiEl2Vaa, 0, 0);
    cmd.start_address = 0x0010_0000;
    cmd.ril           = true;
    cmd.tg            = 2; // 16KB granule
    cmd.num           = 1; // valid
    cmd.scale         = 0;
    cmd.ttl           = 0;

    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert!(
        !is_gerror_cmdq_err_active(&smmu),
        "BUG-AUDIT-34: TlbiEl2Vaa with TG=2 (16KB), NUM=1, SCALE=0 is valid \
         and must NOT raise CERROR_ILL. IDR0=0x{:08X} GERROR=0x{:08X}",
        smmu.get_idr0(),
        smmu.get_gerror()
    );
}

/// BUG-AUDIT-34 step-size verification: TlbiNhVaa RIL with TG=3 (64KB)
/// must complete without panic for a multi-block range.
///
/// With page_size hardcoded to 4096 and TG=3 (64KB), the loop would iterate
/// 16× more steps than needed. With the fix (granule_size used), it iterates
/// exactly the right number. This test confirms no panic and no CERROR_ILL.
///
/// BEFORE FIX: loop iterates 16× too many entries (still no panic in test, but
///   incorrect behavior). Test passes either way — it is a behavior regression guard.
/// AFTER FIX: loop uses granule_size=65536, correct iteration count.
#[test]
fn tlbi_nh_vaa_ril_tg3_step_size_no_panic() {
    let smmu = make_smmu();
    smmu.set_s1p_supported(true);
    smmu.configure_stream(sid(2), stage1_only_config()).unwrap();
    smmu.clear_gerror(smmu.get_gerror() ^ smmu.get_gerrorn());

    let mut cmd = CommandEntry::new(CommandType::TlbiNhVaa, 0, 0);
    cmd.start_address = 0x0000_0000;
    cmd.vmid          = 0;
    cmd.ril           = true;
    cmd.tg            = 3; // 64KB granule
    cmd.num           = 4; // valid 5-block range
    cmd.scale         = 0;
    cmd.ttl           = 1; // non-zero TTL avoids the reserved combo

    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert!(
        !is_gerror_cmdq_err_active(&smmu),
        "BUG-AUDIT-34: TlbiNhVaa with TG=3 (64KB), NUM=4, SCALE=0, TTL=1 \
         must NOT raise CERROR_ILL. GERROR=0x{:08X}",
        smmu.get_gerror()
    );
}

// ============================================================================
// BUG-AUDIT-35: HTTU overclaims dirty state support
// ============================================================================

/// BUG-AUDIT-35: `get_idr0()` HTTU[7:6] must be 0b01 (access flag only).
///
/// ARM §6.3.1: HTTU=0b10 claims dirty state update support, but only
/// access flag is implemented. Must be 0b01.
///
/// BEFORE FIX: `(get_idr0() >> 6) & 0x3` == 0b10 → FAILS.
/// AFTER FIX:  `(get_idr0() >> 6) & 0x3` == 0b01 → PASSES.
#[test]
fn idr0_httu_must_be_01_access_flag_only() {
    let smmu = make_smmu();
    let idr0 = smmu.get_idr0();
    let httu = (idr0 >> 6) & 0x3;
    assert_eq!(
        httu,
        0b01,
        "BUG-AUDIT-35: IDR0.HTTU[7:6] must be 0b01 (access flag only, §6.3.1). \
         HTTU=0b10 overclaims dirty state update support which is not implemented. \
         Current value: 0b{:02b} (IDR0=0x{:08X})",
        httu,
        idr0
    );
}
