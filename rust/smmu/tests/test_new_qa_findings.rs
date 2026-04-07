#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

//! TDD tests for three ARM SMMU v3 conformance gaps:
//!
//! - GAP NEW-1: §7.3 EVENT CLASS field encoding
//! - GAP NEW-2: §7.3.13 S2/IPA fields not populated for two-stage faults
//! - GAP NEW-5: §3.4 Stage-2-bypass OAS truncate vs abort

use smmu::types::{
    AccessType, EventType, FaultMode, PagePermissions, SecurityState, StreamConfig, StreamID,
    IOVA, PA, PASID,
};
use smmu::SMMU;

// ── helpers ──────────────────────────────────────────────────────────────────

fn make_smmu() -> SMMU {
    let s = SMMU::new();
    s.enable().unwrap();
    s
}

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

fn pasid(n: u32) -> PASID {
    PASID::new(n).unwrap()
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

// ─────────────────────────────────────────────────────────────────────────────
// GAP NEW-1: §7.3 EVENT CLASS field encoding
// ─────────────────────────────────────────────────────────────────────────────

/// NEW-1 Test A: C_BAD_SUBSTREAMID is a configuration event — CLASS must be 0.
///
/// ARM IHI0070G.b §7.3: CLASS is only defined for translation-related F_* events.
/// C_* configuration events (C_BAD_STREAMID, C_BAD_STE, C_BAD_SUBSTREAMID, C_BAD_CD,
/// F_CFG_CONFLICT) must leave CLASS at 0.
///
/// BEFORE FIX: C_BAD_SUBSTREAMID event has event_class==1 (wrong config class encoding).
/// AFTER FIX:  C_BAD_SUBSTREAMID event has event_class==0.
#[test]
fn new1_c_config_event_class_is_zero() {
    let smmu = make_smmu();

    // Trigger C_BAD_SUBSTREAMID by presenting a non-zero PASID on a
    // stage-2-only stream. Per §7.3.9 this is always C_BAD_SUBSTREAMID.
    let cfg = StreamConfig::builder()
        .stage1_enabled(false)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xC0), cfg).unwrap();
    smmu.create_stage2_address_space(sid(0xC0)).unwrap();

    let result = smmu.translate(
        sid(0xC0),
        pasid(1), // non-zero PASID on stage-2-only stream → C_BAD_SUBSTREAMID
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "Non-zero PASID on stage-2-only stream must fail with C_BAD_SUBSTREAMID"
    );

    let events = smmu.get_events();
    let config_event = events
        .iter()
        .find(|e| e.event_type == EventType::CBadSubstreamid)
        .expect("Expected a C_BAD_SUBSTREAMID event");

    assert_eq!(
        config_event.event_class, 0,
        "C_BAD_SUBSTREAMID (C_* configuration event) must have event_class==0 per ARM §7.3; \
         got event_class=={} for event type {:?}",
        config_event.event_class, config_event.event_type
    );
}

/// NEW-1 Test B: F_TRANSLATION event must have CLASS==2 (IN — fault on input address).
///
/// ARM IHI0070G.b §7.3: For F_* translation faults in a software model, CLASS==0b10
/// (IN = fault on the input address itself) because the SW model does not perform
/// real hardware page-table walks — faults are detected on the input address.
///
/// BEFORE FIX: F_TRANSLATION event has event_class==0 (wrong — numerically "CD").
/// AFTER FIX:  F_TRANSLATION event has event_class==2 (IN).
#[test]
fn new1_f_translation_event_class_is_in() {
    let smmu = make_smmu();

    // Configure a valid stage-1-only stream
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0x11), cfg).unwrap();
    smmu.create_pasid(sid(0x11), pasid(0)).unwrap();
    // Deliberately do NOT map any page — translation will fail with F_TRANSLATION.

    let result = smmu.translate(
        sid(0x11),
        pasid(0),
        iova(0x5000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "Translation to unmapped page must fail with F_TRANSLATION"
    );

    let events = smmu.get_events();
    let f_trans = events
        .iter()
        .find(|e| e.event_type == EventType::FTranslation)
        .expect("Expected an F_TRANSLATION event");

    assert_eq!(
        f_trans.event_class, 2,
        "F_TRANSLATION event must have event_class==2 (IN per ARM §7.3.13); \
         got event_class=={}",
        f_trans.event_class
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// GAP NEW-2: §7.3.13 S2/IPA fields not populated for two-stage faults
// ─────────────────────────────────────────────────────────────────────────────

/// NEW-2 Test A: A stage-2 fault must set S2=true in the event record.
///
/// ARM IHI0070G.b §7.3.13: When a fault occurs at stage-2 translation,
/// the event record must have S2=1.
///
/// Setup: two-stage stream; stage-1 maps IOVA→IPA but stage-2 has NO mapping
/// for that IPA.  Translate → F_TRANSLATION with s2==true.
///
/// BEFORE FIX: event.s2 == false (bug — S2 field not populated).
/// AFTER FIX:  event.s2 == true.
#[test]
fn new2_stage2_fault_has_s2_true() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0x22), cfg).unwrap();
    smmu.create_pasid(sid(0x22), pasid(0)).unwrap();

    // Stage-1: IOVA 0x1000 → IPA 0x8000 (stage-1 succeeds)
    smmu.map_page(
        sid(0x22),
        pasid(0),
        iova(0x1000),
        pa(0x8000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Stage-2: create the address space but do NOT map IPA 0x8000 → any PA.
    // This forces a stage-2 translation fault.
    smmu.create_stage2_address_space(sid(0x22)).unwrap();

    let result = smmu.translate(
        sid(0x22),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "Two-stage translation with unmapped IPA must fail"
    );

    let events = smmu.get_events();
    let fault_event = events
        .iter()
        .find(|e| e.event_type == EventType::FTranslation)
        .expect("Expected an F_TRANSLATION event for stage-2 fault");

    assert!(
        fault_event.s2,
        "F_TRANSLATION for a stage-2 fault must have s2==true per ARM §7.3.13; \
         got s2=={}",
        fault_event.s2
    );
}

/// NEW-2 Test B: A stage-2 fault must carry the correct IPA in the event record.
///
/// ARM IHI0070G.b §7.3.13: IPA must hold the intermediate physical address
/// (the stage-1 output address) when S2==1.
///
/// The IPA in the event must equal the PA that stage-1 resolved (0x8000 here).
///
/// BEFORE FIX: event.ipa == 0 (bug — IPA field not populated).
/// AFTER FIX:  event.ipa == 0x8000.
#[test]
fn new2_stage2_fault_has_correct_ipa() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0x23), cfg).unwrap();
    smmu.create_pasid(sid(0x23), pasid(0)).unwrap();

    // Stage-1: IOVA 0x2000 → IPA 0x9000
    let ipa_addr: u64 = 0x9000;
    smmu.map_page(
        sid(0x23),
        pasid(0),
        iova(0x2000),
        pa(ipa_addr),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Stage-2: create address space but leave IPA 0x9000 unmapped.
    smmu.create_stage2_address_space(sid(0x23)).unwrap();

    let result = smmu.translate(
        sid(0x23),
        pasid(0),
        iova(0x2000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(result.is_err(), "Must fail — IPA unmapped in stage-2");

    let events = smmu.get_events();
    let fault_event = events
        .iter()
        .find(|e| e.event_type == EventType::FTranslation)
        .expect("Expected an F_TRANSLATION event");

    assert_eq!(
        fault_event.ipa, ipa_addr,
        "F_TRANSLATION for a stage-2 fault must carry IPA=={:#x} per ARM §7.3.13; \
         got ipa=={:#x}",
        ipa_addr, fault_event.ipa
    );
}

/// NEW-2 Test C: A stage-1-only fault must have S2=false and IPA=0.
///
/// When stage-2 is not involved, the event record S2 and IPA fields must remain
/// at their zero/false defaults.
///
/// EXPECTED: Passes both before and after fix (regression guard).
#[test]
fn new2_stage1_fault_has_s2_false() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0x24), cfg).unwrap();
    smmu.create_pasid(sid(0x24), pasid(0)).unwrap();
    // No page mapped — triggers stage-1 F_TRANSLATION.

    let result = smmu.translate(
        sid(0x24),
        pasid(0),
        iova(0x3000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(result.is_err(), "Stage-1 fault must return error");

    let events = smmu.get_events();
    let fault_event = events
        .iter()
        .find(|e| e.event_type == EventType::FTranslation)
        .expect("Expected an F_TRANSLATION event");

    assert!(
        !fault_event.s2,
        "Stage-1-only fault must have s2==false; got s2=={}",
        fault_event.s2
    );
    assert_eq!(
        fault_event.ipa, 0,
        "Stage-1-only fault must have ipa==0; got ipa=={:#x}",
        fault_event.ipa
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// GAP NEW-5: §3.4 Stage-2-bypass OAS: truncate vs abort
// ─────────────────────────────────────────────────────────────────────────────

/// BUG-AUDIT-114 correction: §3.4 line 1635 requires silent truncation, not F_ADDR_SIZE.
///
/// ARM IHI0070G.b §3.4 line 1635: "If bypassing stage 2 (because STE.Config == 0b10x
/// or if unimplemented), the IPA is presented directly as the PA output address.  If
/// the IPA is outside the range of the OAS, the address is silently truncated to fit
/// the OAS."
///
/// The earlier BUG-AUDIT-84 interpretation (raise F_ADDR_SIZE per §7.3.14) was incorrect
/// for the stage-2-bypass case.  §3.4 line 1635 is the authoritative rule and mandates
/// silent truncation with no event.
///
/// Setup:
///   - OAS = 36 bits (max_pa_bits = 36) → OAS mask = 0x0F_FFFF_FFFF
///   - stage-1 stream maps IOVA→PA where PA > OAS limit
///   - translate → must return Ok with truncated PA AND no F_ADDR_SIZE event
///
/// BEFORE BUG-AUDIT-114 fix: translate returns Err(AddressSizeError) and records F_ADDR_SIZE.
/// AFTER BUG-AUDIT-114 fix:  translate returns Ok with OAS-truncated PA; no event.
#[test]
fn new5_stage2_bypass_oas_truncates_not_aborts() {
    use smmu::types::{AddressConfig, EventType, SMMUConfig};

    // Build an SMMU with 36-bit OAS so overflow is easy to trigger.
    let addr_cfg = AddressConfig::builder()
        .max_pa_bits(36_u8)
        .build()
        .unwrap();
    let smmu_cfg = SMMUConfig::builder()
        .address_config(addr_cfg)
        .build()
        .unwrap();
    let smmu = SMMU::with_config(smmu_cfg);
    smmu.enable().unwrap();

    // Stage-1-only stream (stage2_enabled=false — stage-2 is bypassed).
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0x55), cfg).unwrap();
    smmu.create_pasid(sid(0x55), pasid(0)).unwrap();

    // OAS = 36 bits → limit = 0x10_0000_0000 (64 GiB).
    // Map IOVA → PA where PA is above the 36-bit limit.
    let oas_bits: u64 = 36;
    let oas_limit: u64 = 1u64 << oas_bits;
    let high_pa: u64 = oas_limit + 0x1000; // 0x10_0000_1000 — above OAS

    // Map the page in stage-1 with the high PA.
    smmu.map_page(
        sid(0x55),
        pasid(0),
        iova(0x4000),
        pa(high_pa),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        sid(0x55),
        pasid(0),
        iova(0x4000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    // §3.4 line 1635: must return Ok with PA silently truncated to OAS width.
    let oas_mask: u64 = (1u64 << oas_bits) - 1;
    let expected_truncated_pa = high_pa & oas_mask; // 0x10_0000_1000 & 0x0F_FFFF_FFFF = 0x1000

    assert!(
        result.is_ok(),
        "BUG-AUDIT-114: §3.4 line 1635 requires silent PA truncation to OAS width, \
         not F_ADDR_SIZE. Got: {:?}",
        result.err()
    );

    let data = result.unwrap();
    assert_eq!(
        data.physical_address().as_u64(),
        expected_truncated_pa,
        "BUG-AUDIT-114: PA must be truncated to OAS width. Expected {:#x}, got {:#x}",
        expected_truncated_pa,
        data.physical_address().as_u64()
    );

    // No F_ADDR_SIZE event must be recorded — truncation is silent.
    let events = smmu.get_events();
    let has_addr_size = events
        .iter()
        .any(|e| e.event_type == EventType::FAddrSize);
    assert!(
        !has_addr_size,
        "BUG-AUDIT-114: §3.4 line 1635 requires SILENT truncation — \
         no F_ADDR_SIZE event must be queued. Events: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// GAP-B — §5.2 STE.Config reserved values 0b001/0b010/0b011
// ─────────────────────────────────────────────────────────────────────────────

/// GAP-B-1: translation_enabled=true with no stage selected → reserved encoding, must be rejected.
#[test]
fn gap_b_reserved_config_translation_on_no_stage_rejected() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = StreamID::new(0xB0).unwrap();
    let cfg = StreamConfig {
        translation_enabled: true,
        stage1_enabled: false,  // no stage → reserved STE.Config
        stage2_enabled: false,
        ..StreamConfig::default()
    };

    let result = smmu.configure_stream(sid, cfg);
    assert!(result.is_err(),
        "GAP-B: §5.2 — reserved STE.Config (translation_enabled=true, no stage) must be rejected");
}

/// GAP-B-2: all valid STE.Config encodings must still be accepted (regression guard).
#[test]
fn gap_b_valid_configs_still_accepted() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    // 0b000 disabled
    let sid1 = StreamID::new(0xB1).unwrap();
    let cfg1 = StreamConfig::default();
    assert!(smmu.configure_stream(sid1, cfg1).is_ok(), "0b000 disabled must be accepted");

    // 0b100 bypass
    let sid2 = StreamID::new(0xB2).unwrap();
    let cfg2 = StreamConfig::bypass();
    assert!(smmu.configure_stream(sid2, cfg2).is_ok(), "0b100 bypass must be accepted");

    // 0b101 stage-1 only
    let sid3 = StreamID::new(0xB3).unwrap();
    let cfg3 = StreamConfig::stage1_only();
    assert!(smmu.configure_stream(sid3, cfg3).is_ok(), "0b101 S1-only must be accepted");

    // 0b110 stage-2 only
    let sid4 = StreamID::new(0xB4).unwrap();
    let cfg4 = StreamConfig {
        translation_enabled: true,
        stage1_enabled: false,
        stage2_enabled: true,
        ..StreamConfig::default()
    };
    assert!(smmu.configure_stream(sid4, cfg4).is_ok(), "0b110 S2-only must be accepted");

    // 0b111 both stages
    let sid5 = StreamID::new(0xB5).unwrap();
    let cfg5 = StreamConfig {
        translation_enabled: true,
        stage1_enabled: true,
        stage2_enabled: true,
        ..StreamConfig::default()
    };
    assert!(smmu.configure_stream(sid5, cfg5).is_ok(), "0b111 both stages must be accepted");
}
