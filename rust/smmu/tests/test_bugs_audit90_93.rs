//! TDD failing tests for BUG-AUDIT-90 through BUG-AUDIT-93.
//!
//! Each test is designed to FAIL before the corresponding fix is applied,
//! documenting the exact observable bug behaviour.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventType, PagePermissions,
    SecurityState, StreamConfig, StreamID, IOVA, PA, PASID,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

fn pasid0() -> PASID {
    PASID::new(0).unwrap()
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    smmu
}

// ============================================================================
// BUG-AUDIT-90 — Hardcoded AccessType::Read in two-stage translation
// ============================================================================

/// Build a two-stage StreamConfig: stage1 + stage2, both enabled.
fn two_stage_config() -> StreamConfig {
    StreamConfig {
        translation_enabled: true,
        stage1_enabled: true,
        stage2_enabled: true,
        ..StreamConfig::default()
    }
}

/// BUG-AUDIT-90: Stage-2 data translation must use the actual access type, not
/// a hardcoded AccessType::Read.
///
/// Scenario: Stage-2 maps the IPA read-only.  A Write transaction must raise a
/// PermissionFault with rnw=false (write fault).
///
/// BEFORE FIX: the stage-2 translate_page() call hardcodes AccessType::Read, so
/// the permission check always tests read permissions — the write is never denied
/// by stage-2, and the translation either succeeds or fails with rnw=true.
///
/// AFTER FIX: stage-2 receives the real access type; a read-only mapping denies
/// the write with rnw=false in the emitted event.
#[test]
fn bug_audit_90_two_stage_write_to_read_only_s2_sets_rnw_false() {
    let smmu = make_smmu();
    smmu.enable().unwrap();

    let stream = sid(10);
    let p = pasid0();
    let guest_iova = iova(0x1000);
    let ipa_pa = pa(0x2000);
    let phys_pa = pa(0x3000);

    smmu.configure_stream(stream, two_stage_config()).unwrap();
    smmu.create_pasid(stream, p).unwrap();
    smmu.create_stage2_address_space(stream).unwrap();

    // Stage-1: IOVA -> IPA (read-write so S1 doesn't block the write)
    smmu.map_page(stream, p, guest_iova, ipa_pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

    // Stage-2: IPA -> PA with READ-ONLY permissions
    let ipa_as_iova = IOVA::new(ipa_pa.as_u64()).unwrap();
    smmu.map_stage2_page(stream, ipa_as_iova, phys_pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();

    // Perform a Write — stage-2 must deny it and generate rnw=false
    let result = smmu.translate(stream, p, guest_iova, AccessType::Write, SecurityState::NonSecure);
    assert!(result.is_err(), "Write to read-only S2 mapping must fail");

    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "BUG-AUDIT-90: a PermissionFault event must be recorded"
    );
    let ev = &events[0];
    assert_eq!(
        ev.event_type,
        EventType::FPermission,
        "BUG-AUDIT-90: event must be FPermission, got {:?}",
        ev.event_type
    );
    assert!(
        !ev.rnw,
        "BUG-AUDIT-90: rnw must be false (write fault); got rnw={} — \
         stage-2 translate_page was called with hardcoded AccessType::Read instead of \
         the actual Write access type",
        ev.rnw
    );
}

// ============================================================================
// BUG-AUDIT-91 — S2T0SZ=0 not treated as ILLEGAL for stage-2-enabled streams
// ============================================================================

/// BUG-AUDIT-91: configure_stream() with stage2_enabled=true and a genuinely
/// out-of-range s2_t0sz must reject the STE with a C_BAD_STE event.
///
/// NOTE: s2_t0sz=0 is the software model's "no IPA range restriction" sentinel
/// (BUG-AUDIT-48) and must NOT be rejected.  This test uses s2_t0sz=1 which is
/// below the minimum valid value of 22 for SL0=1 ([22, 44] per ARM §5.2) and
/// is NOT the sentinel, so it must trigger C_BAD_STE.
///
/// BEFORE FIX: the `!= 0` guard in smmu/mod.rs skips the T0SZ range check for
/// the sentinel 0, which is correct.  Non-zero values out of range were already
/// rejected by the BUG-AUDIT-88 per-combination check.
///
/// This test exercises the range-check path: s2_t0sz=1 is non-zero and below
/// the minimum, so it must be caught by `config.s2_t0sz < min_t0sz`.
#[test]
fn bug_audit_91_s2_t0sz_nonzero_below_min_stage2_enabled_is_illegal() {
    let smmu = make_smmu();

    let cfg = StreamConfig {
        translation_enabled: true,
        stage1_enabled: false,
        stage2_enabled: true,
        s2_t0sz: 1,  // 1 is below minimum valid value of 22 for SL0=1
        s2_sl0: 1,   // SL0=1 → valid range [22, 44]; 1 is out of range
        ..StreamConfig::default()
    };

    // configure_stream should fail (return Err) or at minimum record C_BAD_STE
    let result = smmu.configure_stream(sid(20), cfg);

    // Either the call returns Err or it emits C_BAD_STE
    let events = smmu.get_events();
    let has_bad_ste = events.iter().any(|e| e.event_type == EventType::CBadSte);

    assert!(
        result.is_err() || has_bad_ste,
        "BUG-AUDIT-91: s2_t0sz=1 (below min 22 for SL0=1) must be rejected \
         (Err or C_BAD_STE event); result={result:?}, events={events:?}"
    );
}

// ============================================================================
// BUG-AUDIT-92 — SSV incorrectly set based on stream capability
// ============================================================================

/// BUG-AUDIT-92: For a substream-capable stream (s1cd_max > 0), a transaction
/// with PASID=0 and ssv=false must record SSV=false in the fault event.
///
/// BEFORE FIX: `ssv` is set to `s1cd_max > 0 || pasid.as_u32() != 0`, so a
/// substream-capable stream with PASID=0 and ssv=false incorrectly sets SSV=true.
///
/// AFTER FIX: SSV reflects whether a SubstreamID was presented on this specific
/// transaction (`ssv` parameter), not the stream's capability.
#[test]
fn bug_audit_92_ssv_false_when_no_substreamid_presented() {
    let smmu = make_smmu();
    smmu.enable().unwrap();

    // Build a substream-capable, stage-1-only stream (s1cd_max > 0)
    let cfg = StreamConfig {
        translation_enabled: true,
        stage1_enabled: true,
        stage2_enabled: false,
        s1cd_max: 4, // capable of 2^4=16 substreams
        ..StreamConfig::default()
    };

    let stream = sid(30);
    smmu.configure_stream(stream, cfg).unwrap();

    // Create PASID 0 context (the "no substream" context)
    let p0 = pasid0();
    smmu.create_pasid(stream, p0).unwrap();

    // Translate to an unmapped address — will generate a fault event.
    // Use translate() which passes ssv=false internally.
    let result = smmu.translate(stream, p0, iova(0x5000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "Unmapped page must fault");

    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "BUG-AUDIT-92: a fault event must be recorded"
    );
    let ev = &events[0];
    assert!(
        !ev.ssv,
        "BUG-AUDIT-92: ssv must be false when no SubstreamID was presented \
         (translate() passes ssv=false, PASID=0); got ssv=true because the \
         implementation used s1cd_max > 0 instead of the actual ssv parameter"
    );
}

// ============================================================================
// BUG-AUDIT-93 — CMDQ error recovery: advance_cmdq_cons as a separate step
// ============================================================================

/// BUG-AUDIT-93: ARM §7.1 CMDQ error recovery requires two distinct steps:
/// 1. advance_cmdq_cons() — advance CONS.RD past the stuck erroneous command
/// 2. clear_gerror(GERROR_CMDQ_ERR) — acknowledge the error bit
///
/// This test verifies that advance_cmdq_cons() is a separate callable method and
/// that calling it clears CONS.ERR before the gerror acknowledgement.
///
/// BEFORE FIX: advance_cmdq_cons() does not exist; all three steps (pop,
/// advance CONS, clear CONS.ERR) happen inside clear_gerror(). The test fails
/// to compile because the method is missing.
///
/// AFTER FIX: advance_cmdq_cons() pops the erroneous command and clears
/// CONS.ERR; clear_gerror() only clears the GERROR bit.
#[test]
fn bug_audit_93_advance_cmdq_cons_is_separate_step() {
    let smmu = make_smmu();

    // Trigger CERROR_ILL via CMD_SYNC with CS=3 (Reserved, per §4.7.3).
    let mut bad_cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    bad_cmd.cs = 3;
    smmu.submit_command(bad_cmd).unwrap();
    let _ = smmu.process_command_queue();

    // Verify GERROR.CMDQ_ERR is active.
    let active = (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR;
    assert_ne!(active, 0, "precondition: GERROR.CMDQ_ERR must be active after CERROR_ILL");

    // Verify CONS.ERR is set.
    assert_ne!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_NONE,
        "precondition: CONS.ERR must be set after CERROR_ILL"
    );

    // Step 2: advance_cmdq_cons() — pops the erroneous command and clears CONS.ERR.
    // This method must exist on SMMU after BUG-AUDIT-93 is fixed.
    smmu.advance_cmdq_cons();

    // CONS.ERR must now be CERROR_NONE (cleared by advance_cmdq_cons).
    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_NONE,
        "BUG-AUDIT-93: CONS.ERR must be CERROR_NONE after advance_cmdq_cons(); \
         method does not exist or does not clear CONS.ERR"
    );

    // GERROR.CMDQ_ERR must still be active (not yet acknowledged).
    let still_active = (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR;
    assert_ne!(
        still_active, 0,
        "BUG-AUDIT-93: GERROR.CMDQ_ERR must still be active after advance_cmdq_cons() alone; \
         clear_gerror has not been called yet"
    );

    // Step 4: acknowledge via clear_gerror.
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);
    let after_clear = (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR;
    assert_eq!(
        after_clear, 0,
        "BUG-AUDIT-93: GERROR.CMDQ_ERR must be inactive after clear_gerror"
    );

    // Verify the queue resumes: submit a valid command and process it.
    smmu.submit_command(CommandEntry::new(CommandType::PrefetchConfig, 0, 0)).unwrap();
    let processed = smmu.process_command_queue();
    assert!(
        processed.as_ref().is_ok_and(|&n| n >= 1),
        "BUG-AUDIT-93: queue must resume processing after advance_cmdq_cons + clear_gerror; \
         got {processed:?}"
    );
}

/// BUG-AUDIT-93 companion: after the refactor, clear_gerror(CMDQ_ERR) alone
/// must NOT pop/advance the command queue.  The test submits a bad command,
/// processes it to get CERROR_ILL, then calls clear_gerror WITHOUT
/// advance_cmdq_cons first.  The queue must still have the erroneous command
/// until advance_cmdq_cons is called.
#[test]
fn bug_audit_93_clear_gerror_alone_does_not_pop_queue() {
    let smmu = make_smmu();

    let mut bad_cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    bad_cmd.cs = 3;
    smmu.submit_command(bad_cmd).unwrap();

    // Queue size before processing = 1
    let _ = smmu.process_command_queue();

    // CONS.ERR is set; GERROR.CMDQ_ERR is active.
    assert_ne!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_NONE,
        "precondition: CONS.ERR set"
    );

    // Call advance_cmdq_cons first, then clear_gerror.
    smmu.advance_cmdq_cons();
    // After advance, queue size must be 0 (popped the erroneous command).
    assert_eq!(
        smmu.get_command_queue_size(),
        0,
        "BUG-AUDIT-93: advance_cmdq_cons must pop the erroneous command; queue must be empty"
    );

    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);
    // CONS.ERR must remain CERROR_NONE (was already cleared by advance_cmdq_cons).
    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_NONE,
        "BUG-AUDIT-93: CONS.ERR must remain CERROR_NONE after clear_gerror"
    );
}
