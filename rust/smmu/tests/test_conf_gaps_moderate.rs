//! Tests for ARM SMMU v3 conformance gaps (moderate set)
//!
//! CONF-GAP-23: CR0.PRIQEN set by `enable()`
//! CONF-GAP-24: `CMD_RESUME` outcome observable
//! CONF-GAP-16: `SteIllegal()` validation (STRW=EL3 for NS, S2TTB OAS)
//! CONF-GAP-20: `EventEntry` wire format fields
//! CONF-GAP-7: `TLBI_S2_IPA` IPA operand
//! CONF-GAP-25: Two-stage ASID+VMID TLB tagging
//!
//! Each test is written BEFORE implementation so it fails first (TDD).

use smmu::types::{
    AccessType, CommandEntry, CommandType, SecurityState, StreamConfig, StreamID,
    StreamWorld,
};
use smmu::SMMU;

// ============================================================================
// CONF-GAP-23: enable() sets CR0_PRIQEN (§6.3.9)
// ============================================================================

#[test]
fn test_conf_gap23_enable_sets_priqen() {
    let smmu = SMMU::new();
    // Before enable: CR0 is 0.
    assert_eq!(smmu.get_cr0() & SMMU::CR0_PRIQEN, 0, "CR0.PRIQEN must be 0 before enable()");

    smmu.enable().unwrap();

    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_PRIQEN,
        0,
        "CR0.PRIQEN must be set after enable() per §6.3.9"
    );
}

#[test]
fn test_conf_gap23_enable_sets_all_required_bits() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    let cr0 = smmu.get_cr0();
    assert_ne!(cr0 & SMMU::CR0_SMMUEN, 0, "SMMUEN must be set by enable()");
    assert_ne!(cr0 & SMMU::CR0_PRIQEN, 0, "PRIQEN must be set by enable()");
    assert_ne!(cr0 & SMMU::CR0_EVENTQEN, 0, "EVENTQEN must be set by enable()");
    assert_ne!(cr0 & SMMU::CR0_CMDQEN, 0, "CMDQEN must be set by enable()");
}

// ============================================================================
// CONF-GAP-24: CMD_RESUME outcome observable (§3.12.2)
// ============================================================================

#[test]
fn test_conf_gap24_resume_retry_outcome() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    // Configure a stall-mode stream.
    let sid = StreamID::new(1).unwrap();
    let mut cfg = StreamConfig::stage1_only();
    cfg.fault_mode = smmu::types::FaultMode::Stall;
    smmu.configure_stream(sid, cfg).unwrap();
    let pasid0 = smmu::PASID::new(0).unwrap();
    smmu.create_pasid(sid, pasid0).unwrap();

    // Trigger a stall fault (no page mapped → F_TRANSLATION).
    let iova = smmu::IOVA::new(0x1000).unwrap();
    let res = smmu.translate(sid, smmu::PASID::new(0).unwrap(), iova, AccessType::Read, SecurityState::NonSecure);
    let stag = match res {
        Err(smmu::types::TranslationError::Stalled { stag }) => stag,
        other => panic!("expected Stalled, got {other:?}"),
    };

    // Issue CMD_RESUME with Ac=1 (Retry).
    let mut cmd = CommandEntry::new(CommandType::Resume, sid.as_u32(), 0);
    cmd.stag = stag;
    cmd.action = true;
    cmd.abort = false;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // The outcome should be observable.
    let outcome = smmu.get_resume_outcome(stag);
    assert!(outcome.is_some(), "CONF-GAP-24: get_resume_outcome must return Some after CMD_RESUME Ac=1");
    let outcome = outcome.unwrap();
    assert_eq!(
        outcome,
        smmu::ResumeOutcome::Retry,
        "CONF-GAP-24: Ac=1 must produce ResumeOutcome::Retry"
    );
}

#[test]
fn test_conf_gap24_resume_terminate_outcome() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = StreamID::new(2).unwrap();
    let mut cfg = StreamConfig::stage1_only();
    cfg.fault_mode = smmu::types::FaultMode::Stall;
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, smmu::PASID::new(0).unwrap()).unwrap();

    let iova = smmu::IOVA::new(0x2000).unwrap();
    let res = smmu.translate(sid, smmu::PASID::new(0).unwrap(), iova, AccessType::Read, SecurityState::NonSecure);
    let stag = match res {
        Err(smmu::types::TranslationError::Stalled { stag }) => stag,
        other => panic!("expected Stalled, got {other:?}"),
    };

    // Issue CMD_RESUME with Ac=0, Ab=0 (Terminate).
    let mut cmd = CommandEntry::new(CommandType::Resume, sid.as_u32(), 0);
    cmd.stag = stag;
    cmd.action = false;
    cmd.abort = false;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    let outcome = smmu.get_resume_outcome(stag);
    assert!(outcome.is_some(), "get_resume_outcome must return Some after CMD_RESUME Ac=0,Ab=0");
    assert_eq!(
        outcome.unwrap(),
        smmu::ResumeOutcome::Terminate,
        "Ac=0,Ab=0 must produce ResumeOutcome::Terminate"
    );
}

#[test]
fn test_conf_gap24_resume_abort_outcome() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = StreamID::new(3).unwrap();
    let mut cfg = StreamConfig::stage1_only();
    cfg.fault_mode = smmu::types::FaultMode::Stall;
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, smmu::PASID::new(0).unwrap()).unwrap();

    let iova = smmu::IOVA::new(0x3000).unwrap();
    let res = smmu.translate(sid, smmu::PASID::new(0).unwrap(), iova, AccessType::Read, SecurityState::NonSecure);
    let stag = match res {
        Err(smmu::types::TranslationError::Stalled { stag }) => stag,
        other => panic!("expected Stalled, got {other:?}"),
    };

    // Issue CMD_RESUME with Ac=0, Ab=1 (Abort).
    let mut cmd = CommandEntry::new(CommandType::Resume, sid.as_u32(), 0);
    cmd.stag = stag;
    cmd.action = false;
    cmd.abort = true;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    let outcome = smmu.get_resume_outcome(stag);
    assert!(outcome.is_some(), "get_resume_outcome must return Some after CMD_RESUME Ac=0,Ab=1");
    assert_eq!(
        outcome.unwrap(),
        smmu::ResumeOutcome::Abort,
        "Ac=0,Ab=1 must produce ResumeOutcome::Abort"
    );
}

#[test]
fn test_conf_gap24_clear_resume_outcomes() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = StreamID::new(4).unwrap();
    let mut cfg = StreamConfig::stage1_only();
    cfg.fault_mode = smmu::types::FaultMode::Stall;
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, smmu::PASID::new(0).unwrap()).unwrap();

    let iova = smmu::IOVA::new(0x4000).unwrap();
    let res = smmu.translate(sid, smmu::PASID::new(0).unwrap(), iova, AccessType::Read, SecurityState::NonSecure);
    let stag = match res {
        Err(smmu::types::TranslationError::Stalled { stag }) => stag,
        other => panic!("expected Stalled, got {other:?}"),
    };

    let mut cmd = CommandEntry::new(CommandType::Resume, sid.as_u32(), 0);
    cmd.stag = stag;
    cmd.action = true;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // Confirm outcome recorded.
    assert!(smmu.get_resume_outcome(stag).is_some());

    // After get_resume_outcome (which removes), it should be gone.
    assert!(smmu.get_resume_outcome(stag).is_none(), "get_resume_outcome must remove the entry");
}

#[test]
fn test_conf_gap24_clear_all_resume_outcomes() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = StreamID::new(5).unwrap();
    let mut cfg = StreamConfig::stage1_only();
    cfg.fault_mode = smmu::types::FaultMode::Stall;
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, smmu::PASID::new(0).unwrap()).unwrap();

    let iova = smmu::IOVA::new(0x5000).unwrap();
    let res = smmu.translate(sid, smmu::PASID::new(0).unwrap(), iova, AccessType::Read, SecurityState::NonSecure);
    let stag = match res {
        Err(smmu::types::TranslationError::Stalled { stag }) => stag,
        other => panic!("expected Stalled, got {other:?}"),
    };

    let mut cmd = CommandEntry::new(CommandType::Resume, sid.as_u32(), 0);
    cmd.stag = stag;
    cmd.action = true;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    smmu.clear_resume_outcomes();
    assert!(smmu.get_resume_outcome(stag).is_none(), "clear_resume_outcomes must remove all outcomes");
}

// ============================================================================
// CONF-GAP-16: SteIllegal() validation (§5.2.2)
// ============================================================================

#[test]
fn test_conf_gap16_ns_stream_el3_strw_rejected() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = StreamID::new(10).unwrap();
    let mut cfg = StreamConfig::stage1_only();
    // NS stream with STRW=EL3 — must be rejected per §5.2.2.
    cfg.security_state = SecurityState::NonSecure;
    cfg.strw = StreamWorld::El3;

    let result = smmu.configure_stream(sid, cfg);
    assert!(
        result.is_err(),
        "CONF-GAP-16: NS stream with STRW=EL3 must be rejected by configure_stream()"
    );
}

#[test]
fn test_conf_gap16_ns_stream_other_strw_allowed() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = StreamID::new(11).unwrap();
    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.strw = StreamWorld::El1El0; // allowed for NS
    assert!(smmu.configure_stream(sid, cfg).is_ok(), "NS stream with STRW=El1El0 must be allowed");
}

#[test]
fn test_conf_gap16_s2ttb_oas_bounds_checked() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = StreamID::new(12).unwrap();
    let mut cfg = StreamConfig::two_stage();
    // s2_ps=0 → 32-bit OAS → max valid S2TTB address < 2^32 = 0x1_0000_0000
    cfg.s2_ps = 0;
    // Set S2TTB to an address that exceeds 32-bit OAS.
    cfg.s2_ttb = 0x1_0000_0000; // exactly 2^32 → out of range

    let result = smmu.configure_stream(sid, cfg);
    assert!(
        result.is_err(),
        "CONF-GAP-16: S2TTB exceeding OAS must be rejected"
    );
}

#[test]
fn test_conf_gap16_s2ttb_within_oas_allowed() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = StreamID::new(13).unwrap();
    let mut cfg = StreamConfig::two_stage();
    // s2_ps=5 → 48-bit OAS → 0x0000_1000_0000_0000 is well within range.
    cfg.s2_ps = 5;
    cfg.s2_ttb = 0x0000_1000_0000_0000;

    assert!(
        smmu.configure_stream(sid, cfg).is_ok(),
        "CONF-GAP-16: S2TTB within OAS must be accepted"
    );
}

// ============================================================================
// CONF-GAP-20: EventEntry wire format fields (§7.3)
// ============================================================================

#[test]
fn test_conf_gap20_event_entry_has_ipa_field() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = StreamID::new(20).unwrap();
    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, smmu::PASID::new(0).unwrap()).unwrap();

    // Trigger a fault to generate an event.
    let iova = smmu::IOVA::new(0xABCD_0000).unwrap();
    let _ = smmu.translate(sid, smmu::PASID::new(0).unwrap(), iova, AccessType::Read, SecurityState::NonSecure);

    let events = smmu.get_events();
    assert!(!events.is_empty(), "Must have generated at least one event");
    // Verify ipa field exists (defaults to 0 for S1-only).
    let ev = &events[0];
    let _ = ev.ipa; // field must exist; value 0 is correct for S1-only
}

#[test]
fn test_conf_gap20_event_entry_rnw_write() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = StreamID::new(21).unwrap();
    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, smmu::PASID::new(0).unwrap()).unwrap();

    let iova = smmu::IOVA::new(0xABCD_1000).unwrap();
    let _ = smmu.translate(sid, smmu::PASID::new(0).unwrap(), iova, AccessType::Write, SecurityState::NonSecure);

    let events = smmu.get_events();
    assert!(!events.is_empty());
    assert!(events[0].rnw, "rnw must be true for Write access");
}

#[test]
fn test_conf_gap20_event_entry_rnw_read() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = StreamID::new(22).unwrap();
    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, smmu::PASID::new(0).unwrap()).unwrap();

    let iova = smmu::IOVA::new(0xABCD_2000).unwrap();
    let _ = smmu.translate(sid, smmu::PASID::new(0).unwrap(), iova, AccessType::Read, SecurityState::NonSecure);

    let events = smmu.get_events();
    assert!(!events.is_empty());
    assert!(!events[0].rnw, "rnw must be false for Read access");
}

#[test]
fn test_conf_gap20_event_entry_ind_execute() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = StreamID::new(23).unwrap();
    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, smmu::PASID::new(0).unwrap()).unwrap();

    let iova = smmu::IOVA::new(0xABCD_3000).unwrap();
    let _ = smmu.translate(sid, smmu::PASID::new(0).unwrap(), iova, AccessType::Execute, SecurityState::NonSecure);

    let events = smmu.get_events();
    assert!(!events.is_empty());
    assert!(events[0].ind, "ind must be true for Execute access");
}

#[test]
fn test_conf_gap20_event_entry_ssv_nonzero_pasid() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = StreamID::new(24).unwrap();
    let mut cfg = StreamConfig::stage1_only();
    cfg.pasid_enabled = true;
    cfg.max_pasid = 1023;
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, smmu::PASID::new(5).unwrap()).unwrap();

    let iova = smmu::IOVA::new(0xABCD_4000).unwrap();
    let pasid = smmu::PASID::new(5).unwrap();
    let _ = smmu.translate(sid, pasid, iova, AccessType::Read, SecurityState::NonSecure);

    let events = smmu.get_events();
    assert!(!events.is_empty());
    assert!(events[0].ssv, "ssv must be true when pasid != 0");
}

#[test]
fn test_conf_gap20_event_entry_ssv_zero_pasid() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = StreamID::new(25).unwrap();
    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, smmu::PASID::new(0).unwrap()).unwrap();

    let iova = smmu::IOVA::new(0xABCD_5000).unwrap();
    let _ = smmu.translate(sid, smmu::PASID::new(0).unwrap(), iova, AccessType::Read, SecurityState::NonSecure);

    let events = smmu.get_events();
    assert!(!events.is_empty());
    assert!(!events[0].ssv, "ssv must be false when pasid == 0");
}

#[test]
fn test_conf_gap20_event_class_translation_fault() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = StreamID::new(26).unwrap();
    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, smmu::PASID::new(0).unwrap()).unwrap();

    let iova = smmu::IOVA::new(0xABCD_6000).unwrap();
    let _ = smmu.translate(sid, smmu::PASID::new(0).unwrap(), iova, AccessType::Read, SecurityState::NonSecure);

    let events = smmu.get_events();
    assert!(!events.is_empty());
    // F_TRANSLATION is event_class = 0 (translation fault).
    assert_eq!(events[0].event_class, 0, "F_TRANSLATION must have event_class=0");
}

// ============================================================================
// CONF-GAP-7: TLBI_S2_IPA IPA operand (§4.4)
// ============================================================================

#[test]
fn test_conf_gap7_tlbi_s2_ipa_selective_invalidation() {
    use smmu::cache::{CacheEntry, CacheKey, ReplacementPolicy, TlbCache};
    use smmu::{IOVA, PA, PASID, StreamID as SID};
    use smmu::types::PagePermissions;

    let cache = TlbCache::new(64, ReplacementPolicy::Lru);
    let sid = SID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();
    let security = SecurityState::NonSecure;

    // Insert an entry with vmid=7 and ipa=0x2000.
    let iova1 = IOVA::new(0x1000).unwrap();
    let pa1   = PA::new(0x3000).unwrap();
    let mut entry1 = CacheEntry::new_with_tags(iova1, pa1, PagePermissions::read_write(), security, 0, 7, 0);
    entry1.ipa = 0x2000;
    let key1 = CacheKey::new(sid, pasid, iova1, security);
    cache.insert(key1, entry1);

    // Insert a second entry with vmid=7 but ipa=0x5000 (different IPA).
    let iova2 = IOVA::new(0x4000).unwrap();
    let pa2   = PA::new(0x6000).unwrap();
    let mut entry2 = CacheEntry::new_with_tags(iova2, pa2, PagePermissions::read_write(), security, 0, 7, 0);
    entry2.ipa = 0x5000;
    let key2 = CacheKey::new(sid, pasid, iova2, security);
    cache.insert(key2, entry2);

    // Selective invalidation: only entries with ipa in [0x2000, 0x2FFF].
    cache.invalidate_by_vmid_and_ipa(7, 0xFFFF, 0x2000, 0x2FFF);

    // entry1 (ipa=0x2000) must be gone; entry2 (ipa=0x5000) must survive.
    assert!(cache.lookup(&key1).is_none(), "CONF-GAP-7: entry with matching IPA must be invalidated");
    assert!(cache.lookup(&key2).is_some(), "CONF-GAP-7: entry with non-matching IPA must survive");
}

#[test]
fn test_conf_gap7_tlbi_s2_ipa_zero_ipa_not_matched() {
    use smmu::cache::{CacheEntry, CacheKey, ReplacementPolicy, TlbCache};
    use smmu::{IOVA, PA, PASID, StreamID as SID};
    use smmu::types::PagePermissions;

    let cache = TlbCache::new(64, ReplacementPolicy::Lru);
    let sid = SID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();
    let security = SecurityState::NonSecure;

    // S1-only entry: ipa=0 (not a two-stage entry).
    let iova = IOVA::new(0x1000).unwrap();
    let pa   = PA::new(0x2000).unwrap();
    let entry = CacheEntry::new_with_tags(iova, pa, PagePermissions::read_write(), security, 5, 7, 0);
    // entry.ipa is 0 by default — this should NOT be matched by IPA-targeted TLBI.
    let key = CacheKey::new(sid, pasid, iova, security);
    cache.insert(key, entry);

    // IPA invalidation — entry.ipa == 0 must not be matched (it's not a two-stage entry).
    cache.invalidate_by_vmid_and_ipa(7, 0xFFFF, 0x0, 0xFFF);

    assert!(cache.lookup(&key).is_some(), "CONF-GAP-7: S1-only entry (ipa=0) must not be matched by IPA TLBI");
}

// ============================================================================
// CONF-GAP-25: Two-stage ASID+VMID TLB tagging (§3.17)
// ============================================================================

#[test]
fn test_conf_gap25_stage1_only_asid_nonzero_vmid_zero() {
    use smmu::cache::{CacheKey, ReplacementPolicy, TlbCache};
    use smmu::{IOVA, PA, PagePermissions, PASID};

    // Create a cache and simulate S1-only insertion.
    let cache = TlbCache::new(64, ReplacementPolicy::Lru);
    let sid = StreamID::new(30).unwrap();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x10000).unwrap();
    let pa   = PA::new(0x20000).unwrap();
    let security = SecurityState::NonSecure;

    // Stage-1 only: asid=42, vmid=0.
    let entry = smmu::cache::CacheEntry::new_with_tags(iova, pa, PagePermissions::read_write(), security, 42, 0, 0);
    let key = CacheKey::new(sid, pasid, iova, security);
    cache.insert(key, entry);

    let hit = cache.lookup(&key).expect("entry must be cached");
    assert_eq!(hit.asid, 42, "CONF-GAP-25: S1 entry must have non-zero ASID");
    assert_eq!(hit.vmid, 0, "CONF-GAP-25: S1 entry must have vmid=0");
}

#[test]
fn test_conf_gap25_stage2_only_asid_zero_vmid_nonzero() {
    use smmu::cache::{CacheKey, ReplacementPolicy, TlbCache};
    use smmu::{IOVA, PA, PagePermissions, PASID};

    let cache = TlbCache::new(64, ReplacementPolicy::Lru);
    let sid = StreamID::new(31).unwrap();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x10000).unwrap();
    let pa   = PA::new(0x30000).unwrap();
    let security = SecurityState::NonSecure;

    // Stage-2 only: asid=0, vmid=77.
    let entry = smmu::cache::CacheEntry::new_with_tags(iova, pa, PagePermissions::read_write(), security, 0, 77, 0);
    let key = CacheKey::new(sid, pasid, iova, security);
    cache.insert(key, entry);

    let hit = cache.lookup(&key).expect("entry must be cached");
    assert_eq!(hit.asid, 0, "CONF-GAP-25: S2-only entry must have asid=0");
    assert_eq!(hit.vmid, 77, "CONF-GAP-25: S2-only entry must have non-zero VMID");
}

#[test]
fn test_conf_gap25_two_stage_asid_and_vmid_nonzero() {
    use smmu::cache::{CacheKey, ReplacementPolicy, TlbCache};
    use smmu::{IOVA, PA, PagePermissions, PASID};

    let cache = TlbCache::new(64, ReplacementPolicy::Lru);
    let sid = StreamID::new(32).unwrap();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x10000).unwrap();
    let pa   = PA::new(0x40000).unwrap();
    let security = SecurityState::NonSecure;

    // Two-stage: asid=10 (S1 ASID), vmid=20 (S2 VMID).
    let entry = smmu::cache::CacheEntry::new_with_tags(iova, pa, PagePermissions::read_write(), security, 10, 20, 0);
    let key = CacheKey::new(sid, pasid, iova, security);
    cache.insert(key, entry);

    let hit = cache.lookup(&key).expect("entry must be cached");
    assert_eq!(hit.asid, 10, "CONF-GAP-25: Two-stage entry must have S1 ASID");
    assert_eq!(hit.vmid, 20, "CONF-GAP-25: Two-stage entry must have S2 VMID");
}

#[test]
fn test_conf_gap25_two_stage_tlbi_s12_vmall_invalidates_by_vmid() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = StreamID::new(33).unwrap();
    let mut cfg = StreamConfig::two_stage();
    cfg.vmid = 55;
    smmu.configure_stream(sid, cfg).unwrap();

    // Set up stage-2 and stage-1 address spaces.
    smmu.create_stage2_address_space(sid).unwrap();
    let pasid = smmu::PASID::new(0).unwrap();
    smmu.create_pasid(sid, pasid).unwrap();

    let iova = smmu::IOVA::new(0x5000).unwrap();
    smmu.map_page(sid, pasid, iova, smmu::PA::new(0x8000).unwrap(), smmu::types::PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    smmu.map_stage2_page(sid, smmu::IOVA::new(0x8000).unwrap(), smmu::PA::new(0xA000).unwrap(), smmu::types::PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

    // Do a successful translation to populate TLB.
    let _ = smmu.translate(sid, pasid, iova, AccessType::Read, SecurityState::NonSecure);

    // Issue CMD_TLBI_S12_VMALL with vmid=55.
    let mut cmd = CommandEntry::new(CommandType::TlbiS12Vmall, 0, 0);
    cmd.vmid = 55;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // The TLB entry should now be gone — next translation is a miss.
    // (We can't directly inspect TLB, but we can verify invalidation_count incremented.)
    let stats = smmu.get_cache_statistics();
    assert!(stats.invalidation_count() >= 1, "CONF-GAP-25: TLBI_S12_VMALL must increment invalidation count");
}
