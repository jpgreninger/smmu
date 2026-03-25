//! TDD failing tests for BUG-QA-12, BUG-QA-13, and BUG-QA-14 (Rust implementation).
//!
//! Each test is written to FAIL with the current buggy code and PASS only
//! after the corresponding fix is applied.  No fixes are included here.
//!
//! NOTE: BUG-QA-11 is C++ only (Rust already correctly calls apply_output_attrs
//! in translate_bypass).
//!
//! # Bug Summary
//!
//! ## BUG-QA-12 — §5.5 STE.S2S Stage-2 Stall
//!
//! STE.S2S controls stall-vs-terminate for stage-2 faults independently of the
//! stage-1 CD.S (FaultMode::Stall).  StreamConfig had no `s2_stall` field; the
//! stall check used only `stall_mode` (CD.S / FaultMode::Stall).
//!
//! Also: STALL_MODEL==0b01 (terminate-only) AND s2_stall==true → C_BAD_STE.
//!
//! ## BUG-QA-13 — §5.5 STE.S2R Stage-2 Record
//!
//! STE.S2R controls whether stage-2 fault events are recorded in the event queue.
//! When S2R=0 AND S2S=0, stage-2 fault events must be suppressed entirely (not
//! recorded), analogous to CD.R for stage-1.  StreamConfig had no `s2_record` field.
//!
//! ## BUG-QA-14 — §4.4.4.1 CMD_TLBI_NSNH_ALL over-invalidates
//!
//! CMD_TLBI_NSNH_ALL should only invalidate Non-Secure Non-Hyp (EL1_EL0) entries,
//! preserving NS-EL2 (StreamWorld::El2) and NS-EL2-E2H (StreamWorld::El2E2h) entries.
//! Previously it called invalidate_all() which also evicted EL2 entries.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventType, FaultMode, SecurityState, StreamConfig,
    StreamID, StreamWorld, IOVA, PASID,
};
use smmu::{PagePermissions, SMMU};

// ============================================================================
// Helpers
// ============================================================================

fn sid(id: u32) -> StreamID {
    StreamID::new(id).unwrap()
}

fn pasid() -> PASID {
    PASID::new(0).unwrap()
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

/// Build an SMMU with SMMUEN + CMDQEN + EVENTQEN enabled.
fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    smmu
}

// ============================================================================
// BUG-QA-12: STE.S2S — Stage-2 Stall (§5.5)
// ============================================================================

/// BUG-QA-12 (primary): stage-2-only stream with s2_stall=true and
/// FaultMode::Terminate causes a stage-2 fault to STALL (not terminate).
///
/// BEFORE FIX: s2_stall field does not exist; stall_mode=false (Terminate)
/// → fault returns Err(TranslationError::*) not Err(Stalled { stag }).
///
/// AFTER FIX: s2_stall=true gates stall for stage-2 faults independently of
/// CD.S/FaultMode.  The missing page fault is Stalled { stag > 0 }.
#[test]
fn bug_qa12_s2s_stage2_stall_enabled() {
    let smmu = make_smmu();

    // Stage-2-only stream with s2_stall=true, FaultMode=Terminate (CD.S=0).
    let mut cfg = StreamConfig::stage2_only();
    cfg.s2_stall = true;
    cfg.fault_mode = FaultMode::Terminate; // CD.S=0 — must NOT drive stall
    smmu.configure_stream(sid(1), cfg).unwrap();
    smmu.create_stage2_address_space(sid(1)).unwrap();
    // Do NOT map 0x1000 → missing page → stage-2 F_TRANSLATION fault

    let result = smmu.translate(
        sid(1), pasid(), iova(0x1000), AccessType::Read, SecurityState::NonSecure,
    );

    // Must be Stalled (not a plain translation error) because s2_stall=true.
    assert!(
        result.is_err(),
        "BUG-QA-12: translate on unmapped page must fail"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, smmu::TranslationError::Stalled { stag } if stag != 0),
        "BUG-QA-12: stage-2 fault with s2_stall=true must return Stalled {{stag>0}}, got {:?}",
        err
    );
}

/// BUG-QA-12 (negative): stage-2-only stream with s2_stall=false and
/// FaultMode::Stall (CD.S=1) must NOT stall on a stage-2 fault.
///
/// s2_stall governs stage-2 stall independently of FaultMode.  When s2_stall=false
/// the fault must terminate even if CD.S=1.
///
/// BEFORE FIX: stall_mode=true (from FaultMode::Stall) would incorrectly stall.
///
/// AFTER FIX: s2_stall=false → stage-2 fault terminates (no Stalled result).
#[test]
fn bug_qa12_s2s_false_stage2_fault_terminates() {
    let smmu = make_smmu();

    // Stage-2-only stream with s2_stall=false, FaultMode::Stall (CD.S=1).
    let mut cfg = StreamConfig::stage2_only();
    cfg.s2_stall = false;
    cfg.fault_mode = FaultMode::Stall; // CD.S=1 — must NOT drive stage-2 stall
    smmu.configure_stream(sid(2), cfg).unwrap();
    smmu.create_stage2_address_space(sid(2)).unwrap();
    // Do NOT map 0x2000 → missing page → stage-2 fault

    let result = smmu.translate(
        sid(2), pasid(), iova(0x2000), AccessType::Read, SecurityState::NonSecure,
    );

    // Must NOT be Stalled because s2_stall=false.
    assert!(result.is_err(), "BUG-QA-12 neg: translate on unmapped page must fail");
    let err = result.unwrap_err();
    assert!(
        !matches!(err, smmu::TranslationError::Stalled { .. }),
        "BUG-QA-12 neg: s2_stall=false must NOT stall stage-2 fault, got {:?}",
        err
    );
}

/// BUG-QA-12 (STALL_MODEL validation): STALL_MODEL==0b01 (terminate-only) AND
/// s2_stall=true in stream config → configure_stream must fail with C_BAD_STE.
///
/// BEFORE FIX: no s2_stall field → configure_stream always succeeds.
///
/// AFTER FIX: validation rejects the combination and records C_BAD_STE event.
#[test]
fn bug_qa12_s2s_stall_model_terminate_only_bad_ste() {
    let smmu = make_smmu();
    smmu.set_stall_model(0x01); // IDR0.STALL_MODEL = terminate-only

    let mut cfg = StreamConfig::stage2_only();
    cfg.s2_stall = true; // Conflicts with terminate-only model

    let result = smmu.configure_stream(sid(3), cfg);

    // Must be rejected with an error.
    assert!(
        result.is_err(),
        "BUG-QA-12 validation: STALL_MODEL=terminate-only + s2_stall=true must be rejected"
    );

    // C_BAD_STE event must have been recorded.
    let events = smmu.get_events();
    let has_bad_ste = events
        .iter()
        .any(|e| e.event_type == EventType::CBadSte);
    assert!(
        has_bad_ste,
        "BUG-QA-12 validation: C_BAD_STE event must be recorded when STALL_MODEL=terminate-only + s2_stall=true"
    );
}

// ============================================================================
// BUG-QA-13: STE.S2R — Stage-2 Record (§5.5)
// ============================================================================

/// BUG-QA-13 (primary): stage-2-only stream with s2_record=false AND s2_stall=false
/// must suppress the stage-2 fault event from the event queue.
///
/// BEFORE FIX: no s2_record field → event is always recorded in queue.
///
/// AFTER FIX: S2R=0 && S2S=0 → event suppressed; error still returned to caller.
#[test]
fn bug_qa13_s2r_false_s2s_false_suppresses_event() {
    let smmu = make_smmu();

    // Stage-2-only stream with s2_record=false, s2_stall=false.
    let mut cfg = StreamConfig::stage2_only();
    cfg.s2_record = false;
    cfg.s2_stall = false;
    smmu.configure_stream(sid(10), cfg).unwrap();
    smmu.create_stage2_address_space(sid(10)).unwrap();
    // Do NOT map 0x3000 → stage-2 fault

    let result = smmu.translate(
        sid(10), pasid(), iova(0x3000), AccessType::Read, SecurityState::NonSecure,
    );

    // Translation must fail (error returned to caller).
    assert!(result.is_err(), "BUG-QA-13: translate on unmapped page must fail");

    // Event queue must be EMPTY (S2R=0 suppresses the event).
    assert!(
        !smmu.has_events(),
        "BUG-QA-13: S2R=0 && S2S=0 must suppress stage-2 fault event; event queue must be empty"
    );
}

/// BUG-QA-13 (negative): stage-2-only stream with s2_record=true (default) must
/// record the stage-2 fault event in the event queue.
///
/// This is the normal path — confirming that s2_record=true preserves existing behavior.
#[test]
fn bug_qa13_s2r_true_records_event() {
    let smmu = make_smmu();

    // Stage-2-only stream with s2_record=true (default), s2_stall=false.
    let mut cfg = StreamConfig::stage2_only();
    cfg.s2_record = true; // explicit, same as default
    cfg.s2_stall = false;
    smmu.configure_stream(sid(11), cfg).unwrap();
    smmu.create_stage2_address_space(sid(11)).unwrap();
    // Do NOT map 0x4000 → stage-2 fault

    let result = smmu.translate(
        sid(11), pasid(), iova(0x4000), AccessType::Read, SecurityState::NonSecure,
    );

    // Translation must fail.
    assert!(result.is_err(), "BUG-QA-13 pos: translate on unmapped page must fail");

    // Event must be recorded in the queue (S2R=1).
    assert!(
        smmu.has_events(),
        "BUG-QA-13 pos: S2R=1 must record stage-2 fault event in queue"
    );
}

/// BUG-QA-13 (stall path): when s2_stall=true the event must still be recorded
/// (stall path always records the event regardless of S2R).
#[test]
fn bug_qa13_s2r_false_s2s_true_records_event() {
    let smmu = make_smmu();

    // s2_record=false but s2_stall=true → stall path, event is recorded.
    let mut cfg = StreamConfig::stage2_only();
    cfg.s2_record = false;
    cfg.s2_stall = true;
    smmu.configure_stream(sid(12), cfg).unwrap();
    smmu.create_stage2_address_space(sid(12)).unwrap();
    // Do NOT map 0x5000 → stage-2 fault

    let result = smmu.translate(
        sid(12), pasid(), iova(0x5000), AccessType::Read, SecurityState::NonSecure,
    );

    // Must stall.
    assert!(
        matches!(result, Err(smmu::TranslationError::Stalled { .. })),
        "BUG-QA-13 stall: s2_stall=true must stall, got {:?}",
        result
    );

    // Event must be recorded (stall path always records).
    assert!(
        smmu.has_events(),
        "BUG-QA-13 stall: s2_stall=true must record event in queue"
    );
}

// ============================================================================
// BUG-QA-14: CMD_TLBI_NSNH_ALL over-invalidates (§4.4.4.1)
// ============================================================================

/// BUG-QA-14 (primary): CMD_TLBI_NSNH_ALL must evict EL1_EL0 entries but
/// PRESERVE NS-EL2 (StreamWorld::El2) entries.
///
/// BEFORE FIX: NSNH_ALL calls invalidate_all() → El2 entry is also evicted.
///
/// AFTER FIX: NSNH_ALL calls invalidate_nsnh_all() → only El1El0 entries evicted;
/// El2 entry survives and serves a TLB hit on the next translate.
#[test]
fn bug_qa14_nsnh_all_evicts_el1el0_preserves_el2() {
    let smmu = make_smmu();

    // Stream 20: STRW=El1El0 (default) — this entry MUST be evicted by NSNH_ALL.
    let mut cfg_el1 = StreamConfig::stage2_only();
    cfg_el1.vmid = 10;
    // strw is recorded on TLB entries via the stream context — El1El0 is the default.
    smmu.configure_stream(sid(20), cfg_el1).unwrap();
    smmu.create_stage2_address_space(sid(20)).unwrap();
    smmu.map_stage2_page(
        sid(20), iova(0xA000), smmu::PA::new(0xB000).unwrap(),
        PagePermissions::read_write(), SecurityState::NonSecure,
    ).unwrap();

    // Stream 21: STRW=El2 — this entry MUST be preserved by NSNH_ALL.
    // NS-EL2 stream: strw=El2, stage2-only.
    // Note: STRW validation only applies when stage1_enabled=true.
    // For stage2-only streams, strw is stored but validation is skipped (strw_unused=true).
    let mut cfg_el2 = StreamConfig::stage2_only();
    cfg_el2.vmid = 11;
    cfg_el2.strw = StreamWorld::El2;
    smmu.configure_stream(sid(21), cfg_el2).unwrap();
    smmu.create_stage2_address_space(sid(21)).unwrap();
    smmu.map_stage2_page(
        sid(21), iova(0xC000), smmu::PA::new(0xD000).unwrap(),
        PagePermissions::read_write(), SecurityState::NonSecure,
    ).unwrap();

    // Warm up both TLB entries.
    let r1 = smmu.translate(sid(20), pasid(), iova(0xA000), AccessType::Read, SecurityState::NonSecure);
    assert!(r1.is_ok(), "BUG-QA-14 setup: El1El0 stream translate must succeed");

    let r2 = smmu.translate(sid(21), pasid(), iova(0xC000), AccessType::Read, SecurityState::NonSecure);
    assert!(r2.is_ok(), "BUG-QA-14 setup: El2 stream translate must succeed");

    // Verify second translate for El2 stream is a TLB hit before invalidation.
    let stats_before_inv = smmu.get_cache_statistics();
    let _ = smmu.translate(sid(21), pasid(), iova(0xC000), AccessType::Read, SecurityState::NonSecure);
    let stats_after_second_el2 = smmu.get_cache_statistics();
    assert!(
        stats_after_second_el2.tlb_hits() > stats_before_inv.tlb_hits(),
        "BUG-QA-14 setup: El2 entry must be cached before NSNH_ALL \
         (hits before={}, after={})",
        stats_before_inv.tlb_hits(), stats_after_second_el2.tlb_hits()
    );

    // Issue CMD_TLBI_NSNH_ALL — must evict El1El0 entries only.
    let cmd = CommandEntry::new(CommandType::TlbiNsnhAll, 0, 0);
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // El1El0 entry (stream 20) must be evicted → next translate is a TLB miss.
    let stats_before_el1 = smmu.get_cache_statistics();
    let r3 = smmu.translate(sid(20), pasid(), iova(0xA000), AccessType::Read, SecurityState::NonSecure);
    assert!(r3.is_ok(), "BUG-QA-14: El1El0 stream translate after NSNH_ALL must succeed");
    let stats_after_el1 = smmu.get_cache_statistics();
    assert!(
        stats_after_el1.tlb_misses() > stats_before_el1.tlb_misses(),
        "BUG-QA-14: NSNH_ALL must evict El1El0 entry (miss before={}, after={})",
        stats_before_el1.tlb_misses(), stats_after_el1.tlb_misses()
    );

    // El2 entry (stream 21) must be PRESERVED → next translate is still a TLB hit.
    let stats_before_el2 = smmu.get_cache_statistics();
    let r4 = smmu.translate(sid(21), pasid(), iova(0xC000), AccessType::Read, SecurityState::NonSecure);
    assert!(r4.is_ok(), "BUG-QA-14: El2 stream translate after NSNH_ALL must succeed");
    let stats_after_el2 = smmu.get_cache_statistics();
    assert!(
        stats_after_el2.tlb_hits() > stats_before_el2.tlb_hits(),
        "BUG-QA-14: NSNH_ALL must PRESERVE El2 entry (hits before={}, after={})",
        stats_before_el2.tlb_hits(), stats_after_el2.tlb_hits()
    );
}

// ============================================================================
// QA-AUDIT-FIX-A: TLBI_NH_ALL must be VMID-scoped EL1_EL0 invalidation
// ============================================================================

/// QA-AUDIT-FIX-A: CMD_TLBI_NH_ALL scoped to VMID=1 must NOT evict TLB entries
/// tagged with VMID=2.
///
/// ARM §4.4.2.1 — TLBI_NH_ALL invalidates EL1_EL0 entries scoped to the VMID
/// field in the command.  Entries for a different VMID must be preserved.
///
/// BEFORE FIX: TlbiNhAll calls invalidate_all() → evicts ALL entries including VMID=2.
///
/// AFTER FIX: TlbiNhAll calls invalidate_nh_by_vmid(vmid) → only VMID=1 El1El0
/// entries are evicted; VMID=2 entries survive and produce a TLB hit on next translate.
#[test]
fn qa_audit_fix_a_nh_all_vmid_scope_preserves_other_vmid() {
    let smmu = make_smmu();

    // Stream 0x40: VMID=1, stage-2-only (default strw=El1El0).
    let mut cfg_a = StreamConfig::stage2_only();
    cfg_a.vmid = 1;
    cfg_a.strw = StreamWorld::El1El0;
    cfg_a.security_state = SecurityState::NonSecure;
    smmu.configure_stream(sid(0x40), cfg_a).unwrap();
    smmu.create_stage2_address_space(sid(0x40)).unwrap();
    smmu.map_stage2_page(
        sid(0x40),
        iova(0x1000),
        smmu::PA::new(0x1000).unwrap(),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    // Stream 0x41: VMID=2, stage-2-only — must NOT be evicted by NH_ALL(VMID=1).
    let mut cfg_b = StreamConfig::stage2_only();
    cfg_b.vmid = 2;
    cfg_b.strw = StreamWorld::El1El0;
    cfg_b.security_state = SecurityState::NonSecure;
    smmu.configure_stream(sid(0x41), cfg_b).unwrap();
    smmu.create_stage2_address_space(sid(0x41)).unwrap();
    smmu.map_stage2_page(
        sid(0x41),
        iova(0x2000),
        smmu::PA::new(0x2000).unwrap(),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    // Warm up both TLB entries.
    let r1 = smmu.translate(sid(0x40), pasid(), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(r1.is_ok(), "QA-AUDIT-FIX-A setup: VMID=1 translation must succeed");
    let r2 = smmu.translate(sid(0x41), pasid(), iova(0x2000), AccessType::Read, SecurityState::NonSecure);
    assert!(r2.is_ok(), "QA-AUDIT-FIX-A setup: VMID=2 translation must succeed");

    // Confirm VMID=2 entry is cached (second translate is a hit).
    let stats_before_inv = smmu.get_cache_statistics();
    let _ = smmu.translate(sid(0x41), pasid(), iova(0x2000), AccessType::Read, SecurityState::NonSecure);
    let stats_after_second = smmu.get_cache_statistics();
    assert!(
        stats_after_second.tlb_hits() > stats_before_inv.tlb_hits(),
        "QA-AUDIT-FIX-A setup: VMID=2 entry must be cached before invalidation"
    );

    // Issue TLBI_NH_ALL scoped to VMID=1.
    let mut cmd = CommandEntry::new(CommandType::TlbiNhAll, 0, 0);
    cmd.vmid = 1;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // VMID=2 translation must still be a TLB hit (entry preserved).
    let stats_before_vmid2 = smmu.get_cache_statistics();
    let r2b = smmu.translate(sid(0x41), pasid(), iova(0x2000), AccessType::Read, SecurityState::NonSecure);
    assert!(r2b.is_ok(), "QA-AUDIT-FIX-A: VMID=2 translation must succeed after NH_ALL(VMID=1)");
    let stats_after_vmid2 = smmu.get_cache_statistics();
    assert!(
        stats_after_vmid2.tlb_hits() > stats_before_vmid2.tlb_hits(),
        "QA-AUDIT-FIX-A: TLBI_NH_ALL(VMID=1) must NOT evict VMID=2 entries (hits before={}, after={})",
        stats_before_vmid2.tlb_hits(), stats_after_vmid2.tlb_hits()
    );
}

// ============================================================================
// QA-AUDIT-FIX-B: STALL_MODEL validation must be guarded by stage2_enabled
// ============================================================================

/// QA-AUDIT-FIX-B: A bypass stream with s2_stall=true must NOT trigger C_BAD_STE
/// when STALL_MODEL==0b01, because s2_stall is meaningless without stage-2.
///
/// ARM §5.5 — S2S is only relevant when stage-2 is active.  The STALL_MODEL
/// conflict check (STALL_MODEL=terminate-only AND S2S=1 → C_BAD_STE) must only
/// fire when stage2_enabled=true.
///
/// BEFORE FIX: configure_stream checks s2_stall without guarding on stage2_enabled
/// → bypass streams with s2_stall=true are incorrectly rejected.
///
/// AFTER FIX: the check is guarded by `config.stage2_enabled`, so bypass/stage1-only
/// streams with s2_stall=true are accepted without error.
#[test]
fn qa_audit_fix_b_stall_model_guard_bypass_stream() {
    let smmu = make_smmu();
    smmu.set_stall_model(0x01); // IDR0.STALL_MODEL = terminate-only

    // Bypass stream with s2_stall=true — must NOT raise C_BAD_STE because
    // there is no stage-2 to stall.
    let mut cfg = StreamConfig::bypass();
    cfg.s2_stall = true; // ignored because stage2_enabled=false
    cfg.security_state = SecurityState::NonSecure;

    let result = smmu.configure_stream(sid(0x50), cfg);
    assert!(
        result.is_ok(),
        "QA-AUDIT-FIX-B: bypass stream with s2_stall=true must not trigger C_BAD_STE, got {:?}",
        result
    );

    let events = smmu.get_events();
    let has_bad_ste = events.iter().any(|e| e.event_type == EventType::CBadSte);
    assert!(
        !has_bad_ste,
        "QA-AUDIT-FIX-B: C_BAD_STE must not be generated for bypass stream with s2_stall=true"
    );
}

/// BUG-QA-14 (El2E2h): CMD_TLBI_NSNH_ALL must also preserve El2E2h entries.
///
/// BEFORE FIX: invalidate_all() evicts El2E2h entries.
///
/// AFTER FIX: invalidate_nsnh_all() only evicts El1El0; El2E2h entries survive.
#[test]
fn bug_qa14_nsnh_all_preserves_el2e2h() {
    let smmu = make_smmu();

    // Enable CR2.E2H so El2E2h STRW is valid (some implementations require this).
    // For stage2-only streams, strw validation is skipped, so we can always set strw=El2E2h.

    // Stream 30: STRW=El1El0 → evicted by NSNH_ALL.
    let mut cfg_el1 = StreamConfig::stage2_only();
    cfg_el1.vmid = 20;
    smmu.configure_stream(sid(30), cfg_el1).unwrap();
    smmu.create_stage2_address_space(sid(30)).unwrap();
    smmu.map_stage2_page(
        sid(30), iova(0xE000), smmu::PA::new(0xF000).unwrap(),
        PagePermissions::read_write(), SecurityState::NonSecure,
    ).unwrap();

    // Stream 31: STRW=El2E2h → preserved by NSNH_ALL.
    let mut cfg_e2h = StreamConfig::stage2_only();
    cfg_e2h.vmid = 21;
    cfg_e2h.strw = StreamWorld::El2E2h;
    smmu.configure_stream(sid(31), cfg_e2h).unwrap();
    smmu.create_stage2_address_space(sid(31)).unwrap();
    smmu.map_stage2_page(
        sid(31), iova(0x10000), smmu::PA::new(0x11000).unwrap(),
        PagePermissions::read_write(), SecurityState::NonSecure,
    ).unwrap();

    // Warm up both TLB entries.
    let r1 = smmu.translate(sid(30), pasid(), iova(0xE000), AccessType::Read, SecurityState::NonSecure);
    assert!(r1.is_ok(), "BUG-QA-14 E2H setup: El1El0 translate must succeed");
    let r2 = smmu.translate(sid(31), pasid(), iova(0x10000), AccessType::Read, SecurityState::NonSecure);
    assert!(r2.is_ok(), "BUG-QA-14 E2H setup: El2E2h translate must succeed");

    // Verify El2E2h entry is cached.
    let stats_before = smmu.get_cache_statistics();
    let _ = smmu.translate(sid(31), pasid(), iova(0x10000), AccessType::Read, SecurityState::NonSecure);
    let stats_cached = smmu.get_cache_statistics();
    assert!(
        stats_cached.tlb_hits() > stats_before.tlb_hits(),
        "BUG-QA-14 E2H setup: El2E2h entry must be cached before NSNH_ALL"
    );

    // Issue CMD_TLBI_NSNH_ALL.
    let cmd = CommandEntry::new(CommandType::TlbiNsnhAll, 0, 0);
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // El1El0 entry must be evicted → miss.
    let stats_before_el1 = smmu.get_cache_statistics();
    let r3 = smmu.translate(sid(30), pasid(), iova(0xE000), AccessType::Read, SecurityState::NonSecure);
    assert!(r3.is_ok(), "BUG-QA-14 E2H: El1El0 translate after NSNH_ALL must succeed");
    let stats_after_el1 = smmu.get_cache_statistics();
    assert!(
        stats_after_el1.tlb_misses() > stats_before_el1.tlb_misses(),
        "BUG-QA-14 E2H: NSNH_ALL must evict El1El0 entry"
    );

    // El2E2h entry must be PRESERVED → hit.
    let stats_before_e2h = smmu.get_cache_statistics();
    let r4 = smmu.translate(sid(31), pasid(), iova(0x10000), AccessType::Read, SecurityState::NonSecure);
    assert!(r4.is_ok(), "BUG-QA-14 E2H: El2E2h translate after NSNH_ALL must succeed");
    let stats_after_e2h = smmu.get_cache_statistics();
    assert!(
        stats_after_e2h.tlb_hits() > stats_before_e2h.tlb_hits(),
        "BUG-QA-14 E2H: NSNH_ALL must PRESERVE El2E2h entry (hits before={}, after={})",
        stats_before_e2h.tlb_hits(), stats_after_e2h.tlb_hits()
    );
}
