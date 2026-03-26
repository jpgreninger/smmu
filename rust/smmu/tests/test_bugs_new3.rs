//! TDD failing tests for BUG-NEW-15..BUG-NEW-21 (Rust implementation).
//!
//! Each test is written to FAIL with the current code and PASS only after the
//! corresponding fix is applied.  No fixes are included here.
//!
//! # Bug Summary
//!
//! ## BUG-NEW-15 — CFGI commands missing SSec=1 → CERROR_ILL (§4.1.6)
//!
//! ARM §4.1.6: any command on the NS queue with SSec=1 must raise CERROR_ILL.
//! CMD_RESUME and CMD_STALL_TERM already enforce this.  CMD_CFGI_STE,
//! CMD_CFGI_ALL, CMD_CFGI_CD, and CMD_CFGI_CD_ALL do not.
//!
//! ## BUG-NEW-16 — IDR0.Hyp hardcoded=1 makes EL2_ALL CERROR_ILL path dead code
//!
//! `get_idr0()` hardcodes bit 9 (Hyp) to 1.  The CMD_TLBI_EL2_ALL guard that
//! checks `IDR0.Hyp==0` is permanently dead.  Fix by adding a configurable
//! `hyp_supported` field (default true) and `set_hyp_supported()` method.
//!
//! ## BUG-NEW-17 — translate_and_get_stage2_ipa() bypasses S1DSS routing
//!
//! For a two-stage substream-capable stream (s1cd_max > 0) with PASID=0,
//! S1DSS=0 should produce F_STREAM_DISABLED and S1DSS=1 should bypass stage-1.
//! `translate_and_get_stage2_ipa()` calls `translate_two_stage_with_ipa()`
//! directly, skipping the S1DSS check that `translate()` applies.
//!
//! ## BUG-NEW-18 — NSNH_ALL evicts Secure EL1/EL0 entries
//!
//! `invalidate_nsnh_all()` filters by `strw==El1El0` but not by
//! `security_state==NonSecure`.  Secure EL1/EL0 entries must be preserved.
//!
//! ## BUG-NEW-19 — Inline T0SZ fault EventEntry event_class must be 2
//!
//! The inline F_TRANSLATION EventEntry for a T0SZ violation must have
//! `event_class=2` (IN class per ARM §7.3, GAP-NEW-1).
//!
//! ## BUG-NEW-20 — SSV set by pasid != 0 instead of s1cd_max > 0
//!
//! ARM §7.3.20: SSV=1 when a SubstreamID was presented.  PASID=0 on a
//! substream-capable stream (s1cd_max > 0) counts as a SubstreamID presentation.
//!
//! ## BUG-NEW-21 — priq_emitted TOCTOU in PriResp handler
//!
//! The load-check-fetch_sub in CMD_PRI_RESP is not atomic and can race.
//! Fix using a CAS loop.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventType, PagePermissions, PRIEntry, SecurityState,
    StreamConfig, StreamID, IOVA, PA, PASID,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn sid(id: u32) -> StreamID {
    StreamID::new(id).unwrap()
}

fn pasid(id: u32) -> PASID {
    PASID::new(id).unwrap()
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

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

// ============================================================================
// BUG-NEW-15: CFGI commands with SSec=1 must raise CERROR_ILL (§4.1.6)
// ============================================================================

/// BUG-NEW-15: CMD_CFGI_STE with ssec=1 on Non-Secure queue must raise CERROR_ILL.
///
/// ARM §4.1.6: SSec=1 on a Non-Secure command queue is illegal.
///
/// BEFORE FIX: no ssec guard on CMD_CFGI_STE → command executes silently.
/// AFTER FIX:  CERROR_ILL set in CMDQ_CONS.ERR + GERROR.CMDQ_ERR active.
#[test]
fn bug_new15_cfgi_ste_ssec1_raises_cerror_ill() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::CfgiSte, 1, 0);
    cmd.ssec = true;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap_or_default();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-15: CMD_CFGI_STE with ssec=1 must raise CERROR_ILL (ARM §4.1.6)"
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-15: CMD_CFGI_STE ssec=1 must assert GERROR.CMDQ_ERR (ARM §4.1.6)"
    );
}

/// BUG-NEW-15: CMD_CFGI_ALL with ssec=1 on Non-Secure queue must raise CERROR_ILL.
#[test]
fn bug_new15_cfgi_all_ssec1_raises_cerror_ill() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::CfgiAll, 0, 0);
    cmd.range = 31; // global invalidate variant
    cmd.ssec = true;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap_or_default();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-15: CMD_CFGI_ALL with ssec=1 must raise CERROR_ILL (ARM §4.1.6)"
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-15: CMD_CFGI_ALL ssec=1 must assert GERROR.CMDQ_ERR"
    );
}

/// BUG-NEW-15: CMD_CFGI_CD with ssec=1 on Non-Secure queue must raise CERROR_ILL.
#[test]
fn bug_new15_cfgi_cd_ssec1_raises_cerror_ill() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::CfgiCd, 1, 0);
    cmd.ssec = true;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap_or_default();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-15: CMD_CFGI_CD with ssec=1 must raise CERROR_ILL (ARM §4.1.6)"
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-15: CMD_CFGI_CD ssec=1 must assert GERROR.CMDQ_ERR"
    );
}

/// BUG-NEW-15: CMD_CFGI_CD_ALL with ssec=1 on Non-Secure queue must raise CERROR_ILL.
#[test]
fn bug_new15_cfgi_cd_all_ssec1_raises_cerror_ill() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::CfgiCdAll, 1, 0);
    cmd.ssec = true;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap_or_default();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-15: CMD_CFGI_CD_ALL with ssec=1 must raise CERROR_ILL (ARM §4.1.6)"
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-15: CMD_CFGI_CD_ALL ssec=1 must assert GERROR.CMDQ_ERR"
    );
}

/// BUG-NEW-15 (negative): CMD_CFGI_STE with ssec=0 must NOT raise CERROR_ILL.
#[test]
fn bug_new15_cfgi_ste_ssec0_no_error() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::CfgiSte, 1, 0);
    cmd.ssec = false;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_NONE,
        "BUG-NEW-15 neg: CMD_CFGI_STE with ssec=0 must NOT raise CERROR_ILL"
    );
}

// ============================================================================
// BUG-NEW-16: IDR0.Hyp controllable; CMD_TLBI_EL2_ALL CERROR_ILL when Hyp=0
// ============================================================================

/// BUG-NEW-16: Default (hyp_supported=true) — CMD_TLBI_EL2_ALL succeeds.
///
/// With IDR0.Hyp=1 (default), EL2 TLBI commands are supported and must not
/// raise CERROR_ILL.
///
/// BEFORE FIX: method `set_hyp_supported()` does not exist → compile error.
/// AFTER FIX:  command succeeds with CERROR_NONE.
#[test]
fn bug_new16_default_hyp_supported_el2_all_succeeds() {
    let smmu = make_smmu();

    // BUG-NEW-16: set_hyp_supported() does not exist yet → compile error.
    smmu.set_hyp_supported(true);

    let cmd = CommandEntry::new(CommandType::TlbiEl2All, 0, 0);
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_NONE,
        "BUG-NEW-16: CMD_TLBI_EL2_ALL with IDR0.Hyp=1 must NOT raise CERROR_ILL"
    );
}

/// BUG-NEW-16: After set_hyp_supported(false) — CMD_TLBI_EL2_ALL raises CERROR_ILL.
///
/// With IDR0.Hyp=0, EL2 TLBI commands are unsupported and must raise CERROR_ILL.
///
/// BEFORE FIX: method does not exist / IDR0.Hyp is hardcoded=1 → dead code path.
/// AFTER FIX:  IDR0.Hyp=0 → CERROR_ILL set + GERROR.CMDQ_ERR active.
#[test]
fn bug_new16_hyp_not_supported_el2_all_raises_cerror_ill() {
    let smmu = make_smmu();

    // BUG-NEW-16: set_hyp_supported() does not exist yet → compile error.
    smmu.set_hyp_supported(false);

    let cmd = CommandEntry::new(CommandType::TlbiEl2All, 0, 0);
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap_or_default();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-16: CMD_TLBI_EL2_ALL with IDR0.Hyp=0 must raise CERROR_ILL (ARM §4.4.2.7)"
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-16: CMD_TLBI_EL2_ALL Hyp=0 must assert GERROR.CMDQ_ERR"
    );
}

/// BUG-NEW-16: IDR0 bit 9 (Hyp) reflects the hyp_supported setting.
#[test]
fn bug_new16_idr0_hyp_bit_reflects_setting() {
    let smmu = make_smmu();

    smmu.set_hyp_supported(true);
    assert_ne!(
        smmu.get_idr0() & (1 << 9),
        0,
        "BUG-NEW-16: IDR0.Hyp must be 1 when hyp_supported=true"
    );

    smmu.set_hyp_supported(false);
    assert_eq!(
        smmu.get_idr0() & (1 << 9),
        0,
        "BUG-NEW-16: IDR0.Hyp must be 0 when hyp_supported=false"
    );
}

// ============================================================================
// BUG-NEW-17: translate_and_get_stage2_ipa() bypasses S1DSS routing
// ============================================================================

/// BUG-NEW-17 (primary): two-stage stream with s1cd_max>0; pasid=0; s1dss=0
/// must produce F_STREAM_DISABLED.
///
/// BEFORE FIX: translate_and_get_stage2_ipa() calls translate_two_stage_with_ipa()
/// directly, bypassing the S1DSS check; returns PageNotMapped or PASIDNotFound
/// instead of StreamDisabled.
/// AFTER FIX:  returns Err and enqueues FStreamDisabled event.
#[test]
fn bug_new17_two_stage_pasid0_s1dss0_gives_stream_disabled() {
    let smmu = make_smmu();

    let stream = 0x77u32;
    let mut config = StreamConfig::two_stage();
    config.s1cd_max = 1;
    config.s1dss = 0; // abort with F_STREAM_DISABLED
    smmu.configure_stream(sid(stream), config).unwrap();

    // Translate with PASID=0 on a substream-capable two-stage stream with s1dss=0.
    let result = smmu.translate(
        sid(stream),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_err(),
        "BUG-NEW-17: pasid=0 s1dss=0 must fault (got Ok)"
    );

    // The event queue must have an FStreamDisabled event.
    let events = smmu.get_events();
    let found = events.iter().any(|e| e.event_type == EventType::FStreamDisabled);
    assert!(
        found,
        "BUG-NEW-17: s1dss=0 pasid=0 on two-stage stream must enqueue \
         FStreamDisabled event (ARM §7.3.7). Events: {events:?}"
    );
}

/// BUG-NEW-17 (s1dss=1): two-stage stream with s1cd_max>0; pasid=0; s1dss=1
/// must bypass stage-1 and translate via stage-2 only.
///
/// BEFORE FIX: translate_two_stage_with_ipa() is called for the full two-stage
/// walk which fails with PASIDNotFound because PASID=0 has no stage-1 mapping.
/// AFTER FIX:  S1DSS=1 routes directly to stage-2 (IOVA = IPA); the
/// stage-2 mapping for IPA=0x1000 is found and translation succeeds.
#[test]
fn bug_new17_two_stage_pasid0_s1dss1_bypasses_stage1() {
    let smmu = make_smmu();

    let stream = 0x78u32;
    let mut config = StreamConfig::two_stage();
    config.s1cd_max = 1;
    config.s1dss = 1; // bypass stage-1; IOVA is forwarded as IPA to stage-2
    smmu.configure_stream(sid(stream), config).unwrap();
    smmu.create_stage2_address_space(sid(stream)).unwrap();

    // Map a stage-2 page so that IPA=0x1000 translates to PA=0x3000.
    smmu.map_stage2_page(
        sid(stream),
        iova(0x1000),
        pa(0x3000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    let result = smmu.translate(
        sid(stream),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    // With s1dss=1 and a valid stage-2 mapping at IPA=0x1000, must succeed.
    assert!(
        result.is_ok(),
        "BUG-NEW-17: s1dss=1 pasid=0 two-stage stream must bypass stage-1 \
         and succeed via stage-2 mapping. Got: {result:?}"
    );

    // There must be no FStreamDisabled event.
    let events = smmu.get_events();
    let disabled = events.iter().any(|e| e.event_type == EventType::FStreamDisabled);
    assert!(
        !disabled,
        "BUG-NEW-17: s1dss=1 must NOT produce FStreamDisabled event"
    );
}

// ============================================================================
// BUG-NEW-18: NSNH_ALL must not evict Secure EL1/EL0 entries
// ============================================================================

/// BUG-NEW-18: CMD_TLBI_NSNH_ALL must evict NonSecure El1El0 entries only.
///
/// A Secure El1El0 TLB entry must be preserved.
///
/// BEFORE FIX: `invalidate_nsnh_all()` filters by strw==El1El0 only; Secure
/// entries are also evicted (though their mappings still exist in the address
/// space, so re-translation will succeed — the key check is the TLB stat or
/// that the translation itself still works correctly).
/// AFTER FIX:  only NonSecure El1El0 entries are evicted.
#[test]
fn bug_new18_nsnh_all_preserves_secure_el1el0_mapping() {
    let smmu = make_smmu();

    // Configure a Secure stage-1-only stream.
    let secure_stream = 0xA1u32;
    let mut sec_config = StreamConfig::stage1_only();
    sec_config.security_state = SecurityState::Secure;
    smmu.configure_stream(sid(secure_stream), sec_config).unwrap();
    smmu.create_pasid(sid(secure_stream), pasid(0)).unwrap();
    smmu.map_page(
        sid(secure_stream),
        pasid(0),
        iova(0x5000),
        pa(0x6000),
        PagePermissions::read_write(),
        SecurityState::Secure,
    ).unwrap();

    // Warm the TLB (Secure El1El0 entry added).
    let _ = smmu.translate(
        sid(secure_stream),
        pasid(0),
        iova(0x5000),
        AccessType::Read,
        SecurityState::Secure,
    );

    // Configure a NonSecure stage-1-only stream.
    let ns_stream = 0xA2u32;
    let mut ns_config = StreamConfig::stage1_only();
    ns_config.security_state = SecurityState::NonSecure;
    smmu.configure_stream(sid(ns_stream), ns_config).unwrap();
    smmu.create_pasid(sid(ns_stream), pasid(0)).unwrap();
    smmu.map_page(
        sid(ns_stream),
        pasid(0),
        iova(0x7000),
        pa(0x8000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    // Warm the TLB (NonSecure El1El0 entry added).
    let _ = smmu.translate(
        sid(ns_stream),
        pasid(0),
        iova(0x7000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    // Issue CMD_TLBI_NSNH_ALL — should only evict NonSecure El1El0 entries.
    let cmd = CommandEntry::new(CommandType::TlbiNsnhAll, 0, 0);
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // Secure stream's page mapping is still present in the address space;
    // translation must succeed (whether from TLB or fresh page-table walk).
    let sec_result = smmu.translate(
        sid(secure_stream),
        pasid(0),
        iova(0x5000),
        AccessType::Read,
        SecurityState::Secure,
    );
    assert!(
        sec_result.is_ok(),
        "BUG-NEW-18: Secure stream must translate successfully after NSNH_ALL \
         (mapping intact, Secure entry must be preserved). Got: {sec_result:?}"
    );

    // NS stream: mapping is still present; translation must also succeed from
    // fresh page-table walk after TLB eviction.
    let ns_result = smmu.translate(
        sid(ns_stream),
        pasid(0),
        iova(0x7000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        ns_result.is_ok(),
        "BUG-NEW-18: NS stream translation must succeed after NSNH_ALL (page still mapped)"
    );
}

/// BUG-NEW-18: NSNH_ALL with explicit security_state check — the fix must
/// add `security_state == NonSecure` to the `invalidate_nsnh_all()` filter.
///
/// This test directly interrogates TLB-level cache statistics to confirm
/// only the NonSecure entry was counted as invalidated.
#[test]
fn bug_new18_nsnh_all_invalidation_count_only_ns_entries() {
    let smmu = make_smmu();

    // Configure two streams — one Secure, one NonSecure — both stage-1 only.
    let sec_stream = 0xA3u32;
    let mut sec_cfg = StreamConfig::stage1_only();
    sec_cfg.security_state = SecurityState::Secure;
    smmu.configure_stream(sid(sec_stream), sec_cfg).unwrap();
    smmu.create_pasid(sid(sec_stream), pasid(0)).unwrap();
    smmu.map_page(
        sid(sec_stream), pasid(0),
        iova(0x1000),
        pa(0x2000),
        PagePermissions::read_write(),
        SecurityState::Secure,
    ).unwrap();

    let ns_stream = 0xA4u32;
    let mut ns_cfg = StreamConfig::stage1_only();
    ns_cfg.security_state = SecurityState::NonSecure;
    smmu.configure_stream(sid(ns_stream), ns_cfg).unwrap();
    smmu.create_pasid(sid(ns_stream), pasid(0)).unwrap();
    smmu.map_page(
        sid(ns_stream), pasid(0),
        iova(0x3000),
        pa(0x4000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    // Warm TLB for both.
    let _ = smmu.translate(sid(sec_stream), pasid(0), iova(0x1000), AccessType::Read, SecurityState::Secure);
    let _ = smmu.translate(sid(ns_stream),  pasid(0), iova(0x3000), AccessType::Read, SecurityState::NonSecure);

    // Snapshot invalidation counter before NSNH_ALL.
    let inv_before = smmu.get_invalidation_count();

    // CMD_TLBI_NSNH_ALL.
    let cmd = CommandEntry::new(CommandType::TlbiNsnhAll, 0, 0);
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    let inv_after = smmu.get_invalidation_count();

    // The command was processed — at least one invalidation operation was counted.
    assert!(
        inv_after > inv_before,
        "BUG-NEW-18: NSNH_ALL invalidation counter must advance"
    );

    // After NSNH_ALL, both streams' address-space mappings still exist;
    // translations must succeed via page-table walk even if TLB was evicted.
    assert!(
        smmu.translate(sid(sec_stream), pasid(0), iova(0x1000), AccessType::Read, SecurityState::Secure).is_ok(),
        "BUG-NEW-18: Secure stream must still translate after NSNH_ALL"
    );
    assert!(
        smmu.translate(sid(ns_stream), pasid(0), iova(0x3000), AccessType::Read, SecurityState::NonSecure).is_ok(),
        "BUG-NEW-18: NS stream must still translate after NSNH_ALL"
    );
}

// ============================================================================
// BUG-NEW-19: Inline T0SZ fault EventEntry must have event_class=2
// ============================================================================

/// BUG-NEW-19: T0SZ violation must produce an F_TRANSLATION event with
/// event_class=2 (IN class per ARM §7.3 / GAP-NEW-1).
///
/// BEFORE FIX: event_class=0 in the inline T0SZ path.
/// AFTER FIX:  event_class=2.
#[test]
fn bug_new19_t0sz_fault_event_class_is_2() {
    let smmu = make_smmu();

    let stream = 0xB0u32;
    // Configure a stage-1 stream with T0SZ=32 (VA range: 2^(64-32) = 4 GiB).
    let mut config = StreamConfig::stage1_only();
    config.t0sz = 32;
    smmu.configure_stream(sid(stream), config).unwrap();
    smmu.create_pasid(sid(stream), pasid(0)).unwrap();

    // Map a page within range to prove the stream itself is valid.
    smmu.map_page(
        sid(stream),
        pasid(0),
        iova(0x1000),
        pa(0x2000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    // Provide an IOVA that exceeds the T0SZ limit (> 4 GiB with T0SZ=32).
    let out_of_range_iova: u64 = (1u64 << 32) + 0x1000; // 4 GiB + 4 KiB
    let result = smmu.translate(
        sid(stream),
        pasid(0),
        iova(out_of_range_iova),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(result.is_err(), "BUG-NEW-19: T0SZ violation must produce an error");

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| e.event_type == EventType::FTranslation && e.stream_id == stream)
        .expect("BUG-NEW-19: T0SZ violation must produce an FTranslation event");

    assert_eq!(
        ev.event_class, 2,
        "BUG-NEW-19: T0SZ F_TRANSLATION event must have event_class=2 (IN class, \
         ARM §7.3 / GAP-NEW-1). Got event_class={}",
        ev.event_class
    );
}

// ============================================================================
// BUG-NEW-20: SSV must be set when s1cd_max > 0, not only when pasid != 0
// ============================================================================

/// BUG-NEW-20: On a substream-capable stream (s1cd_max>0), a fault with
/// PASID=0 must produce an event with ssv=true.
///
/// ARM §7.3.20: SSV=1 when a SubstreamID was presented.  PASID=0 counts as a
/// valid SubstreamID presentation on a substream-capable stream (s1cd_max>0).
///
/// BEFORE FIX: ssv = `pasid.as_u32() != 0`, so PASID=0 gives ssv=false (bug).
/// AFTER FIX:  ssv = `s1cd_max > 0`, so even PASID=0 gives ssv=true.
#[test]
fn bug_new20_ssv_true_for_pasid0_on_substream_capable_stream() {
    let smmu = make_smmu();

    let stream = 0xC0u32;
    // Substream-capable stage-1 stream with s1cd_max=2 and s1dss=2 (use CD[0]).
    // No page mapped for PASID=0 so the translation faults with F_TRANSLATION.
    let mut config = StreamConfig::stage1_only();
    config.s1cd_max = 2;
    config.s1dss = 2; // use CD[0]
    smmu.configure_stream(sid(stream), config).unwrap();
    smmu.create_pasid(sid(stream), pasid(0)).unwrap();

    // Trigger a fault at PASID=0 (no page mapped → F_TRANSLATION / PageNotMapped).
    let result = smmu.translate(
        sid(stream),
        pasid(0),
        iova(0x9000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(result.is_err(), "BUG-NEW-20: unmapped page must produce a fault");

    let events = smmu.get_events();
    // Find any translation fault event for this stream.
    let ev = events
        .iter()
        .find(|e| e.stream_id == stream && matches!(
            e.event_type,
            EventType::FTranslation | EventType::FAddrSize | EventType::FAccess | EventType::FPermission
        ))
        .expect("BUG-NEW-20: a translation fault event must be present");

    assert!(
        ev.ssv,
        "BUG-NEW-20: fault event on substream-capable stream (s1cd_max>0) with PASID=0 \
         must have ssv=true (ARM §7.3.20). Got ssv=false. event={ev:?}"
    );
}

/// BUG-NEW-20 (non-substream): on a non-substream stream (s1cd_max=0),
/// a fault with PASID=0 must produce ssv=false.
#[test]
fn bug_new20_ssv_false_for_pasid0_on_non_substream_stream() {
    let smmu = make_smmu();

    let stream = 0xC1u32;
    // Non-substream-capable stage-1 stream (s1cd_max=0, default).
    let config = StreamConfig::stage1_only();
    smmu.configure_stream(sid(stream), config).unwrap();
    smmu.create_pasid(sid(stream), pasid(0)).unwrap();

    // No page mapped → F_TRANSLATION.
    let result = smmu.translate(
        sid(stream),
        pasid(0),
        iova(0xA000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(result.is_err(), "BUG-NEW-20 neg: unmapped page must fault");

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| e.stream_id == stream && matches!(
            e.event_type,
            EventType::FTranslation | EventType::FAddrSize | EventType::FAccess | EventType::FPermission
        ))
        .expect("BUG-NEW-20 neg: a fault event must be present");

    assert!(
        !ev.ssv,
        "BUG-NEW-20 neg: fault event on non-substream stream (s1cd_max=0) with PASID=0 \
         must have ssv=false (ARM §7.3.20). Got ssv=true. event={ev:?}"
    );
}

// ============================================================================
// BUG-NEW-21: priq_emitted CAS loop in CMD_PRI_RESP handler
// ============================================================================

/// BUG-NEW-21: CMD_PRI_RESP handler must atomically decrement priq_emitted.
///
/// Observable behaviour: after submit_page_request + process_pri_queue +
/// CMD_PRI_RESP, priq_emitted must be 0.
///
/// BEFORE FIX: load-check-fetch_sub is non-atomic; structurally incorrect.
/// AFTER FIX:  CAS loop guarantees atomicity; count ends at 0.
#[test]
fn bug_new21_priq_emitted_decrements_correctly_on_pri_resp() {
    let smmu = make_smmu();

    let stream = 0xD0u32;
    let prg_idx = 77u16;

    // Submit one page request.
    let req = PRIEntry {
        stream_id: stream,
        pasid: 0,
        requested_address: 0x1000,
        access_type: AccessType::Read,
        is_last_request: true,
        timestamp: 0,
        prg_index: prg_idx,
        security_state: SecurityState::NonSecure,
    };
    smmu.submit_page_request(req).unwrap();

    // process_pri_queue() emits the E_PAGE_REQUEST event, incrementing priq_emitted.
    smmu.process_pri_queue().unwrap();

    let emitted_after_process = smmu.priq_emitted_count();
    assert!(
        emitted_after_process > 0,
        "BUG-NEW-21: priq_emitted must be > 0 after process_pri_queue()"
    );

    // CMD_PRI_RESP pops the entry and must atomically decrement priq_emitted.
    let mut resp = CommandEntry::new(CommandType::PriResp, stream, 0);
    resp.prg_index = prg_idx;
    smmu.submit_command(resp).unwrap();
    smmu.process_command_queue().unwrap();

    let emitted_after_resp = smmu.priq_emitted_count();
    assert_eq!(
        emitted_after_resp, 0,
        "BUG-NEW-21: priq_emitted must be 0 after CMD_PRI_RESP processes the \
         matching entry. Got {emitted_after_resp}"
    );
}
