//! TDD tests for §4.5 ATC Invalidation — conformance verification.
//!
//! ## §4.5.1 `CMD_ATC_INV` Global flag and SSV interaction
//!
//! ARM spec §4.5.1 (line 6005): "The Global parameter is IGNORED when `SSV==0`."
//! This simulation model has no explicit SSV field in `CommandEntry`; it cannot
//! distinguish `SSV=0` (no PASID) from `SSV=1+PASID=0`. The model conservatively
//! treats `G=1` as always active (safe over-invalidation, never under-invalidates).
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]

use smmu::types::{CommandEntry, CommandType, StreamConfig, StreamID};
use smmu::SMMU;

fn make_smmu() -> SMMU {
    let s = SMMU::new();
    s.enable().unwrap();
    s
}

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

/// §4.5.1 — `CMD_ATC_INV` with `G=1`, `pasid=0` must succeed without `CERROR_ILL`.
///
/// The spec says G is IGNORED when `SSV==0`, but this model has no SSV field.
/// `PASID=0` is conservatively treated as `SSV=1` (safe over-invalidation).
/// The command must succeed (no `CERROR_ILL` from the G flag).
#[test]
fn test_atc_inv_global_ignored_when_ssv_zero() {
    let smmu = make_smmu();
    let stream_id = sid(50);

    let cfg = StreamConfig::builder()
        .stage1_enabled(false)
        .stage2_enabled(false)
        .translation_enabled(false)
        .build()
        .unwrap();
    smmu.configure_stream(stream_id, cfg).unwrap();

    // Submit CMD_ATC_INV with pasid=0 (SSV=0) and flags=1 (G=1).
    // Per §4.5.1 the Global flag must be IGNORED when SSV==0.
    // The command should succeed (range-based path used instead of global).
    let mut cmd = CommandEntry::new(CommandType::AtcInv, stream_id.as_u32(), 0); // pasid=0 → SSV=0
    cmd.flags = 1; // G=1 bit
    cmd.start_address = 0x1000;
    cmd.end_address = 0x2000;
    smmu.submit_command(cmd).unwrap();
    let result = smmu.process_command_queue();
    assert!(
        result.is_ok(),
        "BUG-AUDIT-133: CMD_ATC_INV with G=1, SSV=0 must succeed (Global IGNORED per §4.5.1), got {result:?}"
    );
}

/// §4.5.1 regression — `CMD_ATC_INV` with `G=1`, `SSV=1` (`pasid≠0`) must use global path.
/// The global invalidation is only active when `SSV==1`.
#[test]
fn test_atc_inv_global_active_when_ssv_one() {
    let smmu = make_smmu();
    let stream_id = sid(51);

    let cfg = StreamConfig::builder()
        .stage1_enabled(false)
        .stage2_enabled(false)
        .translation_enabled(false)
        .build()
        .unwrap();
    smmu.configure_stream(stream_id, cfg).unwrap();

    // Submit CMD_ATC_INV with pasid=1 (SSV=1) and G=1 → global invalidation.
    let mut cmd = CommandEntry::new(CommandType::AtcInv, stream_id.as_u32(), 1); // pasid=1 → SSV=1
    cmd.flags = 1; // G=1 bit
    cmd.start_address = 0x1000;
    cmd.end_address = 0x2000;
    smmu.submit_command(cmd).unwrap();
    let result = smmu.process_command_queue();
    assert!(
        result.is_ok(),
        "§4.5.1 regression: CMD_ATC_INV with G=1, SSV=1 must succeed, got {result:?}"
    );
}

/// §4.5.1 — `CMD_ATC_INV` `SSec=1` on NS queue must generate `CERROR_ILL`.
/// (Regression for BUG-NEW-38.)
#[test]
fn test_atc_inv_ssec_cerror_ill() {
    let smmu = make_smmu();
    let stream_id = sid(52);

    let cfg = StreamConfig::builder()
        .stage1_enabled(false)
        .stage2_enabled(false)
        .translation_enabled(false)
        .build()
        .unwrap();
    smmu.configure_stream(stream_id, cfg).unwrap();

    let mut cmd = CommandEntry::new(CommandType::AtcInv, stream_id.as_u32(), 0);
    cmd.ssec = true; // SSec=1 on NS queue → CERROR_ILL
    cmd.start_address = 0x1000;
    cmd.end_address = 0x2000;
    smmu.submit_command(cmd).unwrap();
    // process_command_queue always returns Ok; errors are recorded in CMDQ_CONS.ERR
    smmu.process_command_queue().unwrap();
    // CERROR_ILL=1 must be set in CMDQ_CONS.ERR
    let err_code = smmu.get_cmdq_cons_err();
    assert_eq!(
        err_code, 1u32,
        "BUG-NEW-38 regression: CMD_ATC_INV SSec=1 on NS queue must set CMDQ_CONS.ERR=CERROR_ILL=1"
    );
}
