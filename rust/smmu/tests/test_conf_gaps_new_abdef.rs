//! Tests for ARM SMMU v3 conformance gaps:
//! - GAP-NEW-G: STE.S1STALLD forces abort even when CD.S=1
//! - GAP-NEW-D: IDR registers (IDR0–IDR5, AIDR, IIDR)
//! - GAP-NEW-A: Structure-fetch fault injection (SteFetch, CdFetch, FWalkEabt)
//! - GAP-NEW-E: STATUSR / IRQ_CTRL / CTRLACK registers
//! - GAP-NEW-F: GATOS address translation wrapper
#![allow(missing_docs)]
#![allow(clippy::doc_markdown)]
use smmu::types::{
    AccessType, FaultMode, SecurityState, StreamConfig, StreamID, IOVA, PASID, PA,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn enable_smmu(smmu: &SMMU) {
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

fn make_sid(id: u32) -> StreamID {
    StreamID::new(id).unwrap()
}

fn make_pasid(id: u32) -> PASID {
    PASID::new(id).unwrap()
}

fn make_iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn make_pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

// ============================================================================
// GAP-NEW-G: STE.S1STALLD — stall disable override
// ============================================================================

/// GAP-NEW-G test 1: s1_stalld=false, CD.S=1 (stall mode) — stall is active.
/// A translation fault on an unmapped page should return a Stalled result.
#[test]
fn test_gap_new_g_stall_active_when_s1stalld_false() {
    let smmu = SMMU::new();
    enable_smmu(&smmu);

    let sid = make_sid(1);
    let pasid = make_pasid(0);

    // Configure stream with stall mode enabled, s1_stalld=false (default)
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .fault_mode(FaultMode::Stall)
        .s1_stalld(false)
        .build()
        .unwrap();

    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, pasid).unwrap();
    // Do NOT map any page — force a translation fault on an unmapped address

    let result = smmu.translate(sid, pasid, make_iova(0x1000), AccessType::Read, SecurityState::NonSecure);

    // With stall mode active and s1_stalld=false, we expect a Stalled error
    match result {
        Err(smmu::types::TranslationError::Stalled { .. }) => {
            // correct — stall is active
        }
        other => panic!("Expected Stalled, got: {other:?}"),
    }
}

/// GAP-NEW-G test 2 (BUG-NEW-F corrected): s1_stalld=true + CD.S=1 is ILLEGAL per
/// ARM §5.5 CdIllegal() line 9748 — configure_stream() now emits C_BAD_CD and
/// rejects that combination.  The legal configuration is fault_mode=Terminate (CD.S=0).
/// This test verifies the legal config works: translation faults are non-stall errors.
#[test]
fn test_gap_new_g_s1stalld_with_terminate_produces_translation_error() {
    let smmu = SMMU::new();
    enable_smmu(&smmu);

    let sid = make_sid(2);
    let pasid = make_pasid(0);

    // s1_stalld=true + fault_mode=Terminate (CD.S=0) is the spec-legal configuration.
    // s1_stalld=true + fault_mode=Stall (CD.S=1) is ILLEGAL → C_BAD_CD (BUG-NEW-F fix).
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .s1_stalld(true)
        .build()
        .unwrap();

    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, pasid).unwrap();
    // Do NOT map any page — force a translation fault

    let result = smmu.translate(sid, pasid, make_iova(0x1000), AccessType::Read, SecurityState::NonSecure);

    // s1_stalld=true + Terminate: faults must be non-stall errors (F_TRANSLATION).
    match result {
        Err(smmu::types::TranslationError::Stalled { .. }) => {
            panic!("Got Stalled but s1_stalld=true + Terminate should not stall");
        }
        Err(_) => {
            // Any non-stall error is correct (F_TRANSLATION expected)
        }
        Ok(_) => panic!("Expected translation fault, got success"),
    }
}

// ============================================================================
// GAP-NEW-D: IDR registers
// ============================================================================

/// GAP-NEW-D test 1: get_idr0() returns nonzero (S2P at bit 0, S1P at bit 1).
#[test]
fn test_gap_new_d_idr0_nonzero_s1p_set() {
    let smmu = SMMU::new();
    let idr0 = smmu.get_idr0();

    // ARM IHI0070G.b §6.3.1: bit 0 = S2P, bit 1 = S1P
    assert_ne!(idr0, 0, "IDR0 must be nonzero");
    assert_ne!(idr0 & (1 << 1), 0, "IDR0 bit 1 (S1P) must be set");
    assert_ne!(idr0 & (1 << 0), 0, "IDR0 bit 0 (S2P) must be set");
}

/// GAP-NEW-D test 2: get_idr1() returns nonzero (SIDSIZE=32 in bits[5:0]).
#[test]
fn test_gap_new_d_idr1_sidsize_32() {
    let smmu = SMMU::new();
    let idr1 = smmu.get_idr1();

    assert_ne!(idr1, 0, "IDR1 must be nonzero");
    // bits[5:0] = SIDSIZE = 32 (0x20)
    assert_eq!(idr1 & 0x3F, 0x20, "IDR1 SIDSIZE (bits[5:0]) must be 32");
}

/// GAP-NEW-D test 3: get_idr2() returns 0 (only BA_VATOS[9:0], VATOS not implemented).
#[test]
fn test_gap_new_d_idr2_returns_zero() {
    let smmu = SMMU::new();
    let idr2 = smmu.get_idr2();

    // ARM IHI0070G.b §6.3.3: IDR2 only has BA_VATOS[9:0]; IAS/OAS are NOT in IDR2.
    // This model does not implement VATOS, so IDR2 must be 0.
    assert_eq!(idr2, 0, "IDR2 must be 0 (no IAS/OAS fields; only BA_VATOS which is unused)");
}

/// GAP-NEW-D test 4: get_idr5() — OAS in bits[2:0], granule flags in bits[6:4].
#[test]
fn test_gap_new_d_idr5_granules() {
    let smmu = SMMU::new();
    let idr5 = smmu.get_idr5();

    // ARM IHI0070G.b §6.3.6:
    //   bits[2:0] = OAS = 5 (48-bit)
    //   bit  4    = GRAN4K
    //   bit  5    = GRAN16K
    //   bit  6    = GRAN64K
    assert_eq!(idr5 & 0b111, 5, "IDR5 OAS (bits[2:0]) must be 5 (48-bit)");
    assert_ne!(idr5 & (1 << 4), 0, "IDR5 GRAN4K (bit 4) must be set");
    // BUG-AUDIT-52 fix: GRAN16K and GRAN64K must be 0 — only 4KB granule is implemented.
    assert_eq!(idr5 & (1 << 5), 0, "IDR5 GRAN16K (bit 5) must be 0 — not implemented (BUG-AUDIT-52)");
    assert_eq!(idr5 & (1 << 6), 0, "IDR5 GRAN64K (bit 6) must be 0 — not implemented (BUG-AUDIT-52)");
}

/// GAP-NEW-D test 5: get_aidr() and get_iidr() return expected values.
#[test]
fn test_gap_new_d_aidr_iidr() {
    let smmu = SMMU::new();
    // AIDR must be 0x02 (SMMUv3.2) since RIL/FWB/T0SZ features are implemented.
    let aidr = smmu.get_aidr();
    assert_eq!(aidr, 0x02, "AIDR must be 0x02 (SMMUv3.2)");
    // IIDR is 0 (no implementer code assigned)
    let iidr = smmu.get_iidr();
    assert_eq!(iidr, 0x0, "IIDR must be 0");
}

/// GAP-P2: IDR3 must encode RIL(bit10), FWB(bit8), BBML=0b01(bit11), HAD(bit2).
#[test]
fn test_gap_p2_idr3_capability_bits() {
    let smmu = SMMU::new();
    let idr3 = smmu.get_idr3();
    assert_ne!(idr3 & (1 << 2),  0, "IDR3 bit 2 (HAD) must be set");
    assert_ne!(idr3 & (1 << 8),  0, "IDR3 bit 8 (FWB) must be set");
    assert_ne!(idr3 & (1 << 10), 0, "IDR3 bit 10 (RIL) must be set");
    assert_ne!(idr3 & (1 << 11), 0, "IDR3 bit 11 (BBML[0]) must be set");
    assert_eq!(idr3 & (1 << 12), 0, "IDR3 bit 12 (BBML[1]) must be clear (BBML=0b01)");
}

/// GAP-P1: AIDR must return 0x02 (SMMUv3.2) since RIL/FWB/T0SZ features are implemented.
#[test]
fn test_gap_p1_aidr_smmuv32() {
    let smmu = SMMU::new();
    assert_eq!(smmu.get_aidr(), 0x02, "AIDR must be 0x02 (SMMUv3.2)");
}

// ============================================================================
// GAP-NEW-A: Structure-fetch fault injection
// ============================================================================

/// GAP-NEW-A test 1: inject_ste_fetch_abort() produces FSteFetch event in queue.
#[test]
fn test_gap_new_a_ste_fetch_abort_event() {
    let smmu = SMMU::new();
    // Enable EVENTQEN so events are recorded
    smmu.set_cr0(SMMU::CR0_EVENTQEN);

    let sid = make_sid(10);
    smmu.inject_ste_fetch_abort(sid);

    let events = smmu.get_events();
    assert!(!events.is_empty(), "Event queue must be non-empty after STE fetch abort injection");
    assert_eq!(
        events[0].event_type,
        smmu::types::EventType::FSteFetch,
        "Injected event must be FSteFetch"
    );
    assert_eq!(events[0].stream_id, 10, "Event stream_id must match injected stream_id");
}

/// GAP-NEW-A test 2: inject_ste_fetch_abort() with EVENTQEN=0 produces no event.
#[test]
fn test_gap_new_a_ste_fetch_abort_no_event_when_eventqen_zero() {
    let smmu = SMMU::new();
    // Do NOT set EVENTQEN — CR0 stays at 0
    smmu.set_cr0(0);

    let sid = make_sid(11);
    smmu.inject_ste_fetch_abort(sid);

    let events = smmu.get_events();
    assert!(events.is_empty(), "No events must be recorded when EVENTQEN=0");
}

/// GAP-NEW-A test 3: inject_cd_fetch_abort() produces FCdFetch event.
#[test]
fn test_gap_new_a_cd_fetch_abort_event() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_EVENTQEN);

    let sid = make_sid(12);
    let pasid = make_pasid(5);
    smmu.inject_cd_fetch_abort(sid, pasid);

    let events = smmu.get_events();
    assert!(!events.is_empty(), "Event queue must be non-empty after CD fetch abort injection");
    assert_eq!(
        events[0].event_type,
        smmu::types::EventType::FCdFetch,
        "Injected event must be FCdFetch"
    );
    assert_eq!(events[0].stream_id, 12);
    assert_eq!(events[0].pasid, 5);
}

/// GAP-NEW-A test 4: inject_walk_eabt() produces FWalkEabt event.
#[test]
fn test_gap_new_a_walk_eabt_event() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_EVENTQEN);

    let sid = make_sid(13);
    let pasid = make_pasid(0);
    let iova = make_iova(0xDEAD_0000);
    smmu.inject_walk_eabt(sid, pasid, iova, false, 1);

    let events = smmu.get_events();
    assert!(!events.is_empty(), "Event queue must be non-empty after walk EABT injection");
    assert_eq!(
        events[0].event_type,
        smmu::types::EventType::FWalkEabt,
        "Injected event must be FWalkEabt"
    );
    assert_eq!(events[0].stream_id, 13);
    assert_eq!(events[0].address, 0xDEAD_0000);
}

// ============================================================================
// GAP-NEW-E: STATUSR / IRQ_CTRL / CTRLACK registers
// ============================================================================

/// GAP-NEW-E test 1: get_statusr() returns 0 (DORMANT=0) when SMMU is running.
#[test]
fn test_gap_new_e_statusr_zero_when_running() {
    let smmu = SMMU::new();
    enable_smmu(&smmu);
    let statusr = smmu.get_statusr();
    assert_eq!(statusr & 1, 0, "STATUSR.DORMANT must be 0 when SMMU is running");
}

/// GAP-NEW-E test 2: get_statusr() returns DORMANT=1 after shutdown.
#[test]
fn test_gap_new_e_statusr_dormant_after_shutdown() {
    let smmu = SMMU::new();
    smmu.shutdown().unwrap();
    let statusr = smmu.get_statusr();
    assert_eq!(statusr & 1, 1, "STATUSR.DORMANT (bit 0) must be 1 after shutdown");
}

/// GAP-NEW-E test 3: set_irq_ctrl(v) then get_irq_ctrlack() returns v.
#[test]
fn test_gap_new_e_irq_ctrl_ctrlack_handshake() {
    let smmu = SMMU::new();

    let test_value: u32 = 0b0000_0111; // enable all three IRQ lines
    smmu.set_irq_ctrl(test_value);
    let ctrlack = smmu.get_irq_ctrlack();
    assert_eq!(
        ctrlack, test_value,
        "IRQ_CTRLACK must mirror IRQ_CTRL after write (synchronous model)"
    );
}

/// GAP-NEW-E test 4: get_irq_ctrlack() starts at 0.
#[test]
fn test_gap_new_e_irq_ctrlack_initial_zero() {
    let smmu = SMMU::new();
    assert_eq!(smmu.get_irq_ctrlack(), 0, "IRQ_CTRLACK must reset to 0");
}

// ============================================================================
// GAP-NEW-F: GATOS address translation
// ============================================================================

// ARM IHI0070G.b §6.3.40: bits[55:12] are the PA address field in GATOS_PAR.
// ATTR[63:56] and SH[9:8] overlay the upper/middle bits; this mask isolates [55:12].
const PAR_ADDR_MASK: u64 = 0x00FF_FFFF_FFFF_F000;

/// GAP-NEW-F test 1: gatos_translate on a successfully mapped page returns PA.
#[test]
fn test_gap_new_f_gatos_success_returns_pa() {
    let smmu = SMMU::new();
    enable_smmu(&smmu);

    let sid = make_sid(20);
    let pasid = make_pasid(0);

    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .build()
        .unwrap();

    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, pasid).unwrap();
    smmu.map_page(sid, pasid, make_iova(0x2000), make_pa(0xABC0_0000), smmu::types::PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

    let par = smmu.gatos_translate(sid, pasid, make_iova(0x2000), AccessType::Read, SecurityState::NonSecure);

    // On success bit 0 must be 0 (no fault)
    assert_eq!(par & 1, 0, "GATOS_PAR bit 0 must be 0 (no fault) on success");
    assert_eq!(
        par & PAR_ADDR_MASK,
        0xABC0_0000,
        "GATOS_PAR bits[55:12] must hold the PA"
    );
}

/// GAP-NEW-F test 2: gatos_translate on an unmapped address returns FAULT (bit 0 = 1).
#[test]
fn test_gap_new_f_gatos_fault_on_unmapped() {
    let smmu = SMMU::new();
    enable_smmu(&smmu);

    let sid = make_sid(21);
    let pasid = make_pasid(0);

    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .build()
        .unwrap();

    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, pasid).unwrap();
    // No page mapped at 0x5000

    let par = smmu.gatos_translate(sid, pasid, make_iova(0x5000), AccessType::Read, SecurityState::NonSecure);

    // On fault bit 0 must be 1
    assert_eq!(par & 1, 1, "GATOS_PAR bit 0 must be 1 (FAULT) on translation failure");
    // FAULTCODE[11:4] = 0x10 (F_TRANSLATION)
    assert_eq!((par >> 4) & 0xFF, 0x10, "GATOS_PAR FAULTCODE[11:4] must be 0x10");
    // REASON[2:1] = 0b00
    assert_eq!((par >> 1) & 0x3, 0, "GATOS_PAR REASON[2:1] must be 0b00");
}

// ============================================================================
// GAP-P9: IDR0.STALL_MODEL[25:24] consistent with SEV=1
// ============================================================================

/// GAP-P9: IDR0.STALL_MODEL[25:24] must be 0b00 (both stall and terminate models supported).
#[test]
fn test_gap_p9_idr0_stall_model_consistent_with_sev() {
    let smmu = SMMU::new();
    let idr0 = smmu.get_idr0();
    // BUG-AUDIT-55 fix: IDR0.SEV (bit 14) is now 0 by default — WFE-wake not implemented.
    // Software must call set_sev_supported(true) to enable it.
    assert_eq!(idr0 & (1 << 14), 0, "IDR0.SEV (bit 14) must be 0 by default (BUG-AUDIT-55)");
    // STALL_MODEL=0b00: both stall and terminate models supported.
    assert_eq!((idr0 >> 24) & 0x3, 0, "IDR0 STALL_MODEL[25:24] must be 0b00");
}

// ============================================================================
// GAP-R01: IDR0.STALL_MODEL encoding
// ============================================================================

/// GAP-R01: IDR0.STALL_MODEL[25:24] must be 0b00 (both models supported).
#[test]
fn test_gap_r01_idr0_stall_model_zero() {
    let smmu = SMMU::new();
    let idr0 = smmu.get_idr0();
    assert_eq!((idr0 >> 24) & 0x3, 0, "IDR0.STALL_MODEL[25:24] must be 0b00");
    // BUG-AUDIT-55 fix: IDR0.SEV (bit 14) is now 0 by default — WFE-wake not implemented.
    assert_eq!(idr0 & (1 << 14), 0, "IDR0.SEV (bit 14) must be 0 by default (BUG-AUDIT-55)");
}

// ============================================================================
// GAP-R06: IDR5.STALL_MAX non-zero
// ============================================================================

/// GAP-R06: IDR5.STALL_MAX[31:16] must be non-zero when stall is supported.
#[test]
fn test_gap_r06_idr5_stall_max_nonzero() {
    let smmu = SMMU::new();
    let idr5 = smmu.get_idr5();
    let stall_max = (idr5 >> 16) & 0xFFFF;
    assert_ne!(stall_max, 0, "IDR5.STALL_MAX[31:16] must be non-zero when stall is supported");
}

// ============================================================================
// GAP-R02: IDR0.Hyp (bit 9)
// ============================================================================

/// GAP-R02: IDR0.Hyp (bit 9) must be set — mandatory for SMMUv3.2 with S1P+S2P.
#[test]
fn test_gap_r02_idr0_hyp_set() {
    let smmu = SMMU::new();
    let idr0 = smmu.get_idr0();
    assert_ne!(idr0 & (1 << 9), 0, "IDR0.Hyp (bit 9) must be set for SMMUv3.2");
}

// ============================================================================
// GAP-R03: IDR3.XNX (bit 4)
// ============================================================================

/// GAP-R03: IDR3.XNX (bit 4) must be set — mandatory for SMMUv3.1+ when S2P==1.
#[test]
fn test_gap_r03_idr3_xnx_set() {
    let smmu = SMMU::new();
    let idr3 = smmu.get_idr3();
    assert_ne!(idr3 & (1 << 4), 0, "IDR3.XNX (bit 4) must be set for SMMUv3.1+ with S2P");
}

// ============================================================================
// GAP-P8: IDR1.ATTR_PERMS_OVR (bit 26)
// ============================================================================

/// GAP-P8: IDR1.ATTR_PERMS_OVR (bit 26) must be set — INSTCFG+PRIVCFG implemented.
#[test]
fn test_gap_p8_idr1_attr_perms_ovr_set() {
    let smmu = SMMU::new();
    let idr1 = smmu.get_idr1();
    assert_ne!(idr1 & (1 << 26), 0, "IDR1 bit 26 (ATTR_PERMS_OVR) must be set");
}

// ============================================================================
// GAP-P5: GATOS_PAR fault syndrome (§6.3.40)
// ============================================================================

/// GAP-P5: GATOS_PAR fault syndrome has FAULTCODE[11:4]=0x10 and REASON[2:1]=0b00.
#[test]
fn test_gap_p5_gatos_fault_par_has_faultcode() {
    let smmu = SMMU::new();
    enable_smmu(&smmu);

    let sid = make_sid(22);
    let pasid = make_pasid(0);
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .build()
        .unwrap();
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, pasid).unwrap();
    // No page mapped — F_TRANSLATION fault.

    let par = smmu.gatos_translate(sid, pasid, make_iova(0x9000), AccessType::Read, SecurityState::NonSecure);
    assert_eq!(par & 1, 1, "GATOS_PAR bit 0 must be 1 (FAULT)");
    assert_eq!((par >> 4) & 0xFF, 0x10, "GATOS_PAR FAULTCODE[11:4] must be 0x10 (F_TRANSLATION)");
    assert_eq!((par >> 1) & 0x3, 0, "GATOS_PAR REASON[2:1] must be 0b00");
}

// ============================================================================
// GAP-R05/R08: F_UUT, F_TRANSL_FORBIDDEN, F_BAD_ATS_TREQ must have event_class=0
// ============================================================================

/// GAP-R05/R08: F_UUT, F_TRANSL_FORBIDDEN, F_BAD_ATS_TREQ must have event_class=0.
///
/// The ARM spec wire formats for these event types have no CLASS field — those
/// bit positions are RES0. event_class must be 0, not 2.
#[test]
fn test_gap_r05_r08_ats_uut_forbidden_event_class_zero() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    // Trigger F_UUT via report_unsupported_transaction
    let sid = make_sid(0xAB);
    let pasid = make_pasid(0);
    let iova = make_iova(0x1000);
    smmu.report_unsupported_transaction(sid, pasid, iova, AccessType::Read, smmu::types::SecurityState::NonSecure).unwrap();

    let events = smmu.get_events();
    assert!(!events.is_empty(), "Expected at least one event from report_unsupported_transaction");
    let ev = &events[events.len() - 1];
    assert_eq!(
        ev.event_type,
        smmu::types::EventType::FUut,
        "Expected FUut event"
    );
    assert_eq!(ev.event_class, 0, "F_UUT must have event_class=0 (CLASS field is RES0)");
}

/// GAP-R05/R08: F_TRANSL_FORBIDDEN (SMMUEN=0 ATS TT path) must have event_class=0.
#[test]
fn test_gap_r05_r08_transl_forbidden_smmuen0_event_class_zero() {
    use smmu::types::TransactionType;
    let smmu = SMMU::new();
    // SMMUEN=0, EVENTQEN=1 only
    smmu.set_cr0(SMMU::CR0_EVENTQEN);

    let sid = make_sid(0xCD);
    let pasid = make_pasid(0);
    let iova = make_iova(0x2000);

    // ATS TT with SMMUEN=0 must emit F_TRANSL_FORBIDDEN (always permitted per spec)
    let _ = smmu.translate_with_type(sid, pasid, iova, AccessType::Read, smmu::types::SecurityState::NonSecure, TransactionType::AtsTranslated);

    let events = smmu.get_events();
    let forbidden = events.iter().find(|e| e.event_type == smmu::types::EventType::FTranslForbidden);
    assert!(forbidden.is_some(), "Expected F_TRANSL_FORBIDDEN event for ATS TT when SMMUEN=0");
    assert_eq!(
        forbidden.unwrap().event_class, 0,
        "F_TRANSL_FORBIDDEN must have event_class=0 (CLASS field is RES0)"
    );
}

/// GAP-R05/R08: F_BAD_ATS_TREQ (SMMUEN=0 ATS TR path) must have event_class=0.
#[test]
fn test_gap_r05_r08_bad_ats_treq_smmuen0_event_class_zero() {
    use smmu::types::TransactionType;
    let smmu = SMMU::new();
    // SMMUEN=0, EVENTQEN=1, REC_CFG_ATS=1 (bit 0 of CR2)
    smmu.set_cr0(SMMU::CR0_EVENTQEN);
    smmu.set_cr2(SMMU::CR2_REC_CFG_ATS);

    let sid = make_sid(0xDE);
    let pasid = make_pasid(0);
    let iova = make_iova(0x3000);

    // ATS TR with SMMUEN=0 must emit F_BAD_ATS_TREQ (gated on CR2.REC_CFG_ATS=1)
    let _ = smmu.translate_with_type(sid, pasid, iova, AccessType::Read, smmu::types::SecurityState::NonSecure, TransactionType::AtsTranslationRequest);

    let events = smmu.get_events();
    let treq_ev = events.iter().find(|e| e.event_type == smmu::types::EventType::FBadAtsTreq);
    assert!(treq_ev.is_some(), "Expected F_BAD_ATS_TREQ event for ATS TR when SMMUEN=0 and REC_CFG_ATS=1");
    assert_eq!(
        treq_ev.unwrap().event_class, 0,
        "F_BAD_ATS_TREQ must have event_class=0 (CLASS field is RES0)"
    );
}

// ============================================================================
// GAP-R04: IDR0.ATSRECERR (bit 23) must be set
// ============================================================================

/// GAP-R04: IDR0.ATSRECERR (bit 23) must be set — REC_CFG_ATS is implemented.
#[test]
fn test_gap_r04_idr0_atsrecerr_set() {
    let smmu = SMMU::new();
    let idr0 = smmu.get_idr0();
    assert_ne!(idr0 & (1 << 23), 0, "IDR0.ATSRECERR (bit 23) must be set");
}

// ============================================================================
// GAP-R07: IDR0.BTM (bit 5) must be set
// ============================================================================

/// GAP-R07: IDR0.BTM (bit 5) must be set — broadcast TLBI is implemented.
#[test]
fn test_gap_r07_idr0_btm_set() {
    let smmu = SMMU::new();
    let idr0 = smmu.get_idr0();
    assert_ne!(idr0 & (1 << 5), 0, "IDR0.BTM (bit 5) must be set");
}

// ============================================================================
// GAP-NEW-S1: IDR1.ATTR_TYPES_OVR (bit 27)
// ============================================================================

/// GAP-NEW-S1: IDR1.ATTR_TYPES_OVR (bit 27) must be set — MTCFG/SHCFG/ALLOCCFG implemented.
#[test]
fn test_gap_new_s1_idr1_attr_types_ovr_set() {
    let smmu = SMMU::new();
    let idr1 = smmu.get_idr1();
    assert_ne!(idr1 & (1 << 27), 0,
        "IDR1.ATTR_TYPES_OVR (bit 27) must be set — MTCFG/SHCFG/ALLOCCFG are implemented");
}

// ============================================================================
// GAP-NEW-S2: IDR0.TERM_MODEL (bit 26)
// ============================================================================

/// GAP-NEW-S2 (updated by Bug-2 fix): IDR0.TERM_MODEL (bit 26) must be 0.
///
/// Bug 2 fix: TERM_MODEL=1 would require C_BAD_CD when CD.A=0, which the model
/// does not implement.  Clearing bit 26 correctly reports that RAZ/WI termination
/// IS supported (ARM IHI0070G.b §6.3.1).
#[test]
fn test_gap_new_s2_idr0_term_model_set() {
    let smmu = SMMU::new();
    let idr0 = smmu.get_idr0();
    assert_eq!(idr0 & (1 << 26), 0,
        "IDR0.TERM_MODEL (bit 26) must be 0 — Bug-2 fix: model supports RAZ/WI termination \
         and does not validate CD.A=1 (ARM IHI0070G.b §6.3.1)");
}

// ============================================================================
// GAP-NEW-S3: CR2.E2H gates STRW=El2E2h validity (§6.3.12, §3.17.5)
// ============================================================================

/// GAP-NEW-S3: CR2_E2H constant must be defined as bit 0 (value 1).
#[test]
fn test_gap_new_s3_cr2_e2h_constant_defined() {
    assert_eq!(SMMU::CR2_E2H, 1u32, "CR2_E2H must be bit 0 per §6.3.12");
}

/// GAP-NEW-S3: CR2.E2H defaults to 0; can be set and read back.
#[test]
fn test_gap_new_s3_cr2_e2h_readback() {
    let smmu = SMMU::new();
    // Default: CR2.E2H=0
    assert_eq!(smmu.get_cr2() & SMMU::CR2_E2H, 0, "CR2.E2H must be 0 by default");
    // Set E2H=1
    smmu.set_cr2(smmu.get_cr2() | SMMU::CR2_E2H);
    assert_eq!(
        smmu.get_cr2() & SMMU::CR2_E2H,
        SMMU::CR2_E2H,
        "CR2.E2H must read back 1 after set"
    );
}

/// GAP-NEW-S3: With CR2.E2H=0, a stream configured STRW=El2E2h behaves as NS-EL2.
/// Translation must still succeed (downgrade is transparent to caller).
#[test]
fn test_gap_new_s3_strw_el2e2h_downgrades_when_e2h_zero() {
    use smmu::types::StreamWorld;
    let smmu = SMMU::new();
    enable_smmu(&smmu);

    let sid = make_sid(50);
    let pasid = make_pasid(0);

    // CR2.E2H = 0 (default — E2H bit not set)
    assert_eq!(smmu.get_cr2() & SMMU::CR2_E2H, 0, "CR2.E2H must be 0 by default");

    // Configure stream with STRW=El2E2h
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .strw(StreamWorld::El2E2h)
        .build()
        .unwrap();

    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, pasid).unwrap();
    smmu.map_page(
        sid,
        pasid,
        make_iova(0x6000),
        make_pa(0xE000_0000),
        smmu::types::PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Translation must succeed — downgrade is transparent to caller
    let result = smmu.translate(
        sid,
        pasid,
        make_iova(0x6000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_ok(),
        "Translation must succeed even with CR2.E2H=0 + STRW=El2E2h: {result:?}"
    );
}

// ============================================================================
// GAP-J: IDR0.TTF=0b10 makes AA64=0 rejection consistent (§5.4, §6.3.1)
// ============================================================================

/// GAP-J: IDR0.TTF[3:2]=0b10 (VMSAv8-64 only) makes AA64=0 rejection with C_BAD_CD
/// spec-consistent per §6.3.1. Software that reads TTF=0b10 must not configure AA64=0 CDs.
#[test]
fn test_gap_j_idr0_ttf_vmsa64_only_consistent_with_aa64_rejection() {
    let smmu = SMMU::new();
    let idr0 = smmu.get_idr0();
    let ttf = (idr0 >> 2) & 0x3;
    assert_eq!(
        ttf, 2u32,
        "IDR0.TTF[3:2] must be 0b10 (VMSAv8-64 only) — makes AA64=0 rejection spec-consistent per §6.3.1"
    );
}

// ============================================================================
// GAP-N: C_BAD_SUBSTREAMID event must have ssv=true (§7.3.9)
// ============================================================================

/// GAP-N: C_BAD_SUBSTREAMID event must always have ssv=true per §7.3.9.
/// The spec states: "In this event, SubstreamID is always valid (there is no SSV qualifier)."
/// Trigger by presenting a PASID >= 2^S1CDMax on a substream-capable stage-1 stream.
#[test]
fn test_gap_n_c_bad_substreamid_ssv_always_true() {
    let smmu = SMMU::new();
    enable_smmu(&smmu);

    let sid = make_sid(60);
    // s1cd_max=1 → valid PASIDs are 0 and 1; PASID=2 is out of range → C_BAD_SUBSTREAMID
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .s1cd_max(1)
        .build()
        .unwrap();
    smmu.configure_stream(sid, cfg).unwrap();

    // Use PASID=2 which is >= 2^1=2, triggering C_BAD_SUBSTREAMID
    let pasid_out_of_range = make_pasid(2);
    smmu.create_pasid(sid, make_pasid(0)).unwrap();

    let _ = smmu.translate(
        sid,
        pasid_out_of_range,
        make_iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    let bad_ss = events.iter().find(|e| e.event_type == smmu::types::EventType::CBadSubstreamid);
    assert!(bad_ss.is_some(), "Expected C_BAD_SUBSTREAMID event for out-of-range PASID");
    assert!(
        bad_ss.unwrap().ssv,
        "C_BAD_SUBSTREAMID event must have ssv=true per §7.3.9 \
         (SubstreamID is always valid in this event — no SSV qualifier)"
    );
}
