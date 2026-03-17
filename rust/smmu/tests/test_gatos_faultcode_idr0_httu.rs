//! Tests for ARM SMMU v3 conformance gaps:
//! - GAP-2: gatos_translate() FAULTCODE must reflect actual fault type (§9.1.5)
//! - GAP-3: IDR0.HTTU[7:6] must advertise HA/HD support (§6.3.1)
#![allow(missing_docs)]
#![allow(clippy::doc_markdown)]
use smmu::types::{
    AccessType, FaultMode, SecurityState, StreamConfig, StreamID, IOVA, PASID,
};
use smmu::SMMU;

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

/// Extracts FAULTCODE from GATOS_PAR bits[11:4].
const fn extract_faultcode(par: u64) -> u64 {
    (par >> 4) & 0xFF
}

// ============================================================================
// GAP-2 Test 1: C_BAD_SUBSTREAMID → FAULTCODE must be 0x08
//
// Setup: stage-1 stream with s1cd_max=4 (valid PASIDs: 0..15).
// Call gatos_translate with PASID=16 (out of range) → C_BAD_SUBSTREAMID event.
// ============================================================================

#[test]
fn test_gap2_gatos_c_bad_substreamid_faultcode_is_0x08() {
    let smmu = SMMU::new();
    enable_smmu(&smmu);

    let sid = make_sid(0x50);
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .s1cd_max(4) // valid PASIDs: 0..15
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();

    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, make_pasid(0)).unwrap();

    // PASID=16 >= 2^4=16 → C_BAD_SUBSTREAMID
    let par = smmu.gatos_translate(
        sid,
        make_pasid(16),
        make_iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert_eq!(par & 1, 1, "GATOS_PAR bit 0 must be 1 (FAULT)");
    let fc = extract_faultcode(par);
    assert_eq!(
        fc,
        0x08,
        "FAULTCODE must be 0x08 (C_BAD_SUBSTREAMID) per ARM §9.1.5; got 0x{fc:02x}"
    );
}

// ============================================================================
// GAP-2 Test 2: C_BAD_STREAMID → FAULTCODE must be 0x02
//
// Setup: set CR2.RECINVSID=1, call gatos_translate with unconfigured stream.
// translate() generates C_BAD_STREAMID when stream not found + RECINVSID=1.
// ============================================================================

#[test]
fn test_gap2_gatos_c_bad_streamid_faultcode_is_0x02() {
    let smmu = SMMU::new();
    enable_smmu(&smmu);
    // Enable RECINVSID so C_BAD_STREAMID event is written to queue
    smmu.set_cr2(smmu.get_cr2() | SMMU::CR2_RECINVSID);

    // Call with unconfigured stream — no configure_stream called for SID=0x9999
    let par = smmu.gatos_translate(
        make_sid(0x9999),
        make_pasid(0),
        make_iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert_eq!(par & 1, 1, "GATOS_PAR bit 0 must be 1 (FAULT)");
    let fc = extract_faultcode(par);
    assert_eq!(
        fc,
        0x02,
        "FAULTCODE must be 0x02 (C_BAD_STREAMID) per ARM §9.1.5; got 0x{fc:02x}"
    );
}

// ============================================================================
// GAP-2 Test 3: F_TRANSLATION → FAULTCODE must be 0x10 (regression check)
//
// Ensure the fix doesn't break the existing F_TRANSLATION case.
// ============================================================================

#[test]
fn test_gap2_gatos_f_translation_faultcode_is_0x10() {
    let smmu = SMMU::new();
    enable_smmu(&smmu);

    let sid = make_sid(0x51);
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .build()
        .unwrap();
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, make_pasid(0)).unwrap();
    // No page mapped at 0x3000

    let par = smmu.gatos_translate(
        sid,
        make_pasid(0),
        make_iova(0x3000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert_eq!(par & 1, 1, "GATOS_PAR bit 0 must be 1 (FAULT)");
    let fc = extract_faultcode(par);
    assert_eq!(
        fc,
        0x10,
        "FAULTCODE must be 0x10 (F_TRANSLATION); got 0x{fc:02x}"
    );
}

// ============================================================================
// GAP-3 Test: IDR0.HTTU[7:6] must be 0b10
// ============================================================================

#[test]
fn test_gap3_idr0_httu_bits_are_0b10() {
    let smmu = SMMU::new();
    let idr0 = smmu.get_idr0();
    let httu = (idr0 >> 6) & 0x3;
    assert_eq!(
        httu,
        0b10,
        "IDR0.HTTU[7:6] must be 0b10 (access flag + dirty state update, §6.3.1); got {httu:#b}"
    );
}
