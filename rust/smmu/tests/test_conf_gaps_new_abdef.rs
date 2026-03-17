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

/// GAP-NEW-G test 2: s1_stalld=true, CD.S=1 — stall is suppressed, abort semantics apply.
/// A translation fault must return a non-stall error (e.g., TranslationFault).
#[test]
fn test_gap_new_g_stall_suppressed_when_s1stalld_true() {
    let smmu = SMMU::new();
    enable_smmu(&smmu);

    let sid = make_sid(2);
    let pasid = make_pasid(0);

    // Configure stream with stall mode requested but s1_stalld=true overrides it
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .fault_mode(FaultMode::Stall)
        .s1_stalld(true)
        .build()
        .unwrap();

    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, pasid).unwrap();
    // Do NOT map any page — force a translation fault

    let result = smmu.translate(sid, pasid, make_iova(0x1000), AccessType::Read, SecurityState::NonSecure);

    // With s1_stalld=true, stall is suppressed — must NOT be Stalled
    match result {
        Err(smmu::types::TranslationError::Stalled { .. }) => {
            panic!("Got Stalled but s1_stalld=true should suppress stall mode");
        }
        Err(_) => {
            // Any non-stall error is correct
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
    assert_ne!(idr5 & (1 << 5), 0, "IDR5 GRAN16K (bit 5) must be set");
    assert_ne!(idr5 & (1 << 6), 0, "IDR5 GRAN64K (bit 6) must be set");
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
    smmu.inject_walk_eabt(sid, pasid, iova);

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
    // All other bits are 0 on fault
    assert_eq!(par & !1u64, 0, "GATOS_PAR bits[63:1] must be 0 on fault");
}
