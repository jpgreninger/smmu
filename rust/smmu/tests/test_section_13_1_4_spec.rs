#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

//! TDD conformance tests for ARM SMMU v3 §13.1.4
//!
//! Bugs covered:
//! - BUG-13.1.4-A: ATOS must ignore INSTCFG and PRIVCFG (Table 13.4).
//! - BUG-13.1.4-D: Device/Normal-iNC-oNC memory type forces OSH regardless of SHCFG.

use smmu::types::{AccessType, PagePermissions, SecurityState, StreamConfig, StreamID, IOVA, PA, PASID};
use smmu::SMMU;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

fn pasid0() -> PASID {
    PASID::new(0).unwrap()
}

/// Build an enabled SMMU with a stage-1-only stream on `stream_n` whose
/// INSTCFG=3 (Force Instruction).  A single page is mapped at IOVA=0x1000
/// with Read+Write (no Execute) permissions.
fn make_smmu_instcfg3_rw_stream(stream_n: u32) -> SMMU {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let mut cfg = StreamConfig::stage1_only();
    cfg.inst_cfg = 3; // INSTCFG = 0b11 (Force Instruction)
    smmu.configure_stream(sid(stream_n), cfg).unwrap();
    smmu.create_pasid(sid(stream_n), pasid0()).unwrap();

    // Map a page with Read+Write but NOT Execute.
    smmu.map_page(
        sid(stream_n),
        pasid0(),
        IOVA::new(0x1000).unwrap(),
        PA::new(0x8000_1000).unwrap(),
        PagePermissions::new(true, true, false),
        SecurityState::NonSecure,
    )
    .unwrap();

    smmu
}

/// Build an enabled SMMU with a stage-1-only stream on `stream_n` whose
/// MTCFG=1, MemAttr=0x00 (Device-nGnRnE), SHCFG=3 (ISH override).
/// A single Normal page is mapped at IOVA=0x1000 (page_attr=0xFF).
fn make_smmu_device_memattr_ish_stream(stream_n: u32) -> SMMU {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let mut cfg = StreamConfig::stage1_only();
    cfg.mt_cfg = true;
    cfg.mem_attr = 0x00; // Device-nGnRnE
    cfg.sh_cfg = 3;       // ISH override (0b11)
    smmu.configure_stream(sid(stream_n), cfg).unwrap();
    smmu.create_pasid(sid(stream_n), pasid0()).unwrap();

    smmu.map_page(
        sid(stream_n),
        pasid0(),
        IOVA::new(0x1000).unwrap(),
        PA::new(0x9000_1000).unwrap(),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    smmu
}

// ---------------------------------------------------------------------------
// BUG-13.1.4-A: ATOS must ignore INSTCFG and PRIVCFG
// ---------------------------------------------------------------------------

/// §13.1.4 Table 13.4: For ATOS operations, INSTCFG is ignored ("InD taken
/// from ATOS_ADDR").
///
/// Setup: stream with INSTCFG=3 (Force Instruction), page mapped R+W (no X).
///
/// With INSTCFG=3, ordinary translate(Read) converts Read→Execute, then fails
/// permission check because the page has no Execute permission.
///
/// gatos_translate(Read) must succeed because INSTCFG is ignored for ATOS —
/// the raw Read access is used, which the page permits.
#[test]
fn test_bug_13_1_4_a_gatos_ignores_instcfg3() {
    let smmu = make_smmu_instcfg3_rw_stream(2001);
    let iova = IOVA::new(0x1000).unwrap();

    // Ordinary translate(Read) with INSTCFG=3: Read → Execute → permission fail.
    let ordinary_result = smmu.translate(
        sid(2001),
        pasid0(),
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        ordinary_result.is_err(),
        "ordinary translate(Read) with INSTCFG=3 must fail (Read promoted to Execute, no X perm)"
    );

    // ATOS gatos_translate(Read): INSTCFG must be ignored → plain Read → success.
    let par = smmu.gatos_translate(
        sid(2001),
        pasid0(),
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert_eq!(
        par & 1,
        0,
        "gatos_translate(Read) with INSTCFG=3 must succeed (INSTCFG ignored for ATOS); PAR=0x{:016x}",
        par
    );

    // PA must be the expected physical address.
    let pa_bits = (par >> 12) & 0x0000_0FFF_FFFF_FFFF_u64;
    let expected_pa_bits = (0x8000_1000_u64) >> 12;
    assert_eq!(
        pa_bits,
        expected_pa_bits,
        "ATOS PAR must contain the correct physical address; PAR=0x{:016x}",
        par
    );
}

// ---------------------------------------------------------------------------
// BUG-13.1.4-D: Device/Normal-iNC-oNC always OSH regardless of SHCFG
// ---------------------------------------------------------------------------

/// §13.1.4: "Device or Normal-iNC-oNC memory types always OSH regardless of
/// any SHCFG override."
///
/// Setup: MTCFG=1, MemAttr=0x00 (Device-nGnRnE), SHCFG=3 (ISH).
/// Translate a mapped Normal page. Even though MemAttr overrides to Device,
/// the shareability in TranslationData must be OSH (0b10), not ISH (0b11).
#[test]
fn test_bug_13_1_4_d_device_memattr_forces_osh() {
    let smmu = make_smmu_device_memattr_ish_stream(2002);
    let iova = IOVA::new(0x1000).unwrap();

    let result = smmu.translate(
        sid(2002),
        pasid0(),
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(result.is_ok(), "Translation must succeed: {:?}", result);

    let data = result.unwrap();
    assert_eq!(
        data.shareability(),
        0b10_u8,
        "Device memtype must force OSH (0b10) regardless of SHCFG=3 (ISH); got shareability={}",
        data.shareability()
    );
}
