#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

//! TDD conformance tests for ARM SMMU v3 §13.1.5 MemAttr combining.
//!
//! Bug covered:
//! - BUG-13.1.5-A: translate_two_stage / translate_two_stage_with_ipa discard
//!   stage-1 page_attr instead of combining with stage-2 per §13.1.5 strength ordering.
//!
//! These tests exercise StreamContext::translate() directly (which calls translate_two_stage)
//! so that stage-1 device memory (mapped via AddressSpace::map_page_device) can be tested.
//! The SMMU public API does not expose a stage-1 device page mapper, so StreamContext
//! is used directly for the failing-before-fix tests.

use smmu::stream_context::StreamContext;
use smmu::types::{AccessType, PagePermissions, SecurityState, StreamConfig, StreamID, IOVA, PA, PASID};
use smmu::SMMU;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

fn pasid0() -> PASID {
    PASID::new(0).unwrap()
}

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

/// Build a two-stage `StreamConfig` with safe S2 parameters.
fn two_stage_cfg() -> StreamConfig {
    let mut cfg = StreamConfig::two_stage();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s2ptw = false;
    cfg.t0sz = 0;
    cfg.s2_t0sz = 25;
    cfg.s2_ps = 5;
    cfg
}

// ---------------------------------------------------------------------------
// BUG-13.1.5-A tests using StreamContext directly
//
// StreamContext::translate() dispatches to translate_two_stage() for (S1,S2)=(true,true).
// translate_two_stage() creates result via TranslationData::new() which defaults
// page_attr=0xFF, ignoring both s1_data.page_attr and s2_data.page_attr entirely.
//
// Strength ordering: Device-nGnRnE (0x00) > Normal-WB (0xFF).
// The combine rule takes the stronger (more restrictive) type.
// ---------------------------------------------------------------------------

/// Build a two-stage StreamContext with:
///   Stage-1: IOVA 0x1000 → IPA 0x5000 as Device-nGnRnE (page_attr=0x00)
///   Stage-2: IPA  0x5000 → PA  0xA000 as Normal-WB     (page_attr=0xFF)
///
/// The combined result should be Device-nGnRnE (0x00).
///
/// BEFORE FIX: translate_two_stage creates result_data without setting page_attr,
/// so it defaults to 0xFF (Normal). Test fails because 0xFF != 0x00.
///
/// AFTER FIX: combine_mem_type(0x00, 0xFF) == 0x00. Test passes.
#[test]
fn test_13_1_5_device_s1_normal_s2_gives_device() {
    let ctx = StreamContext::new();
    ctx.enable();
    ctx.set_stage1_enabled(true);
    ctx.set_stage2_enabled(true);
    ctx.create_pasid(pasid0()).unwrap();
    ctx.create_stage2_address_space().unwrap();

    // Stage-1: map IOVA 0x1000 → PA(IPA) 0x5000 as Device-nGnRnE
    let s1_as = ctx.get_pasid_address_space(pasid0()).unwrap();
    s1_as
        .map_page_device(
            iova(0x1000),
            pa(0x5000),
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .unwrap();

    // Stage-2: map IPA 0x5000 → PA 0xA000 as Normal-WB (default page_attr=0xFF)
    ctx.map_stage2_page(iova(0x5000), pa(0xA000), PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let result = ctx
        .translate(pasid0(), iova(0x1000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();

    assert_eq!(
        result.page_attr, 0x00,
        "§13.1.5: Device(S1,0x00) + Normal(S2,0xFF) must combine to Device (0x00), got {:#04x}",
        result.page_attr
    );
}

/// Build a two-stage StreamContext with:
///   Stage-1: IOVA 0x1000 → IPA 0x5000 as Normal-WB     (page_attr=0xFF)
///   Stage-2: IPA  0x5000 → PA  0xA000 as Device-nGnRnE (page_attr=0x00)
///
/// BEFORE FIX: translate_two_stage defaults page_attr=0xFF (ignores S2 device attr).
/// Result is 0xFF — WRONG (device is stronger, result should be 0x00).
///
/// AFTER FIX: combine_mem_type(0xFF, 0x00) == 0x00. Test passes.
#[test]
fn test_13_1_5_normal_s1_device_s2_gives_device_via_stream_context() {
    let ctx = StreamContext::new();
    ctx.enable();
    ctx.set_stage1_enabled(true);
    ctx.set_stage2_enabled(true);
    ctx.create_pasid(pasid0()).unwrap();
    ctx.create_stage2_address_space().unwrap();

    // Stage-1: Normal-WB (default page_attr=0xFF)
    ctx.map_page(
        pasid0(),
        iova(0x1000),
        pa(0x5000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Stage-2: Device-nGnRnE (page_attr=0x00)
    ctx.map_stage2_device_page(
        iova(0x5000),
        pa(0xA000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = ctx
        .translate(pasid0(), iova(0x1000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();

    assert_eq!(
        result.page_attr, 0x00,
        "§13.1.5: Normal(S1,0xFF) + Device(S2,0x00) must combine to Device (0x00), got {:#04x}",
        result.page_attr
    );
}

/// Both stages Normal-WB: combined result must remain Normal-WB (0xFF).
///
/// This test must pass both before and after the fix (it validates no regression).
#[test]
fn test_13_1_5_normal_s1_normal_s2_gives_normal_via_stream_context() {
    let ctx = StreamContext::new();
    ctx.enable();
    ctx.set_stage1_enabled(true);
    ctx.set_stage2_enabled(true);
    ctx.create_pasid(pasid0()).unwrap();
    ctx.create_stage2_address_space().unwrap();

    ctx.map_page(
        pasid0(),
        iova(0x1000),
        pa(0x5000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    ctx.map_stage2_page(iova(0x5000), pa(0xA000), PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let result = ctx
        .translate(pasid0(), iova(0x1000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();

    assert_eq!(
        result.page_attr, 0xFF,
        "§13.1.5: Normal(S1,0xFF) + Normal(S2,0xFF) must remain Normal (0xFF), got {:#04x}",
        result.page_attr
    );
}

// ---------------------------------------------------------------------------
// §13.1.5 test via SMMU public API (translate_two_stage_with_ipa path)
//
// The SMMU slow path calls translate_and_get_stage2_ipa → translate_two_stage_with_ipa.
// That function sets result.page_attr = s2_data.page_attr (no S1 combining).
// We cannot map S1 device pages through the SMMU public API, so we verify the
// gatos_translate PAR ATTR field for Normal-S1 + Device-S2 (exercises the
// translate_two_stage_with_ipa fix indirectly — attr must be 0x00).
// ---------------------------------------------------------------------------

/// Normal (S1) + Device-nGnRnE (S2): gatos PAR ATTR bits[63:56] must be 0x00.
///
/// This test passes before the fix because translate_two_stage_with_ipa already
/// carries s2_data.page_attr = 0x00 correctly for this direction. It validates
/// the SMMU path is not broken by the combining fix.
#[test]
fn test_13_1_5_gatos_normal_s1_device_s2_attr_is_device() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let stream = sid(13_151);
    smmu.configure_stream(stream, two_stage_cfg()).unwrap();
    smmu.create_pasid(stream, pasid0()).unwrap();
    smmu.create_stage2_address_space(stream).unwrap();

    smmu.map_page(
        stream,
        pasid0(),
        iova(0x1000),
        pa(0x5000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    smmu.map_stage2_device_page(
        stream,
        iova(0x5000),
        pa(0xA000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let par = smmu.gatos_translate(
        stream,
        pasid0(),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert_eq!(
        par & 1,
        0,
        "§13.1.5: gatos_translate must succeed (FAULT=0). par=0x{:016X}",
        par
    );

    let attr = (par >> 56) & 0xFF;
    assert_eq!(
        attr, 0x00,
        "§13.1.5: gatos PAR ATTR bits[63:56] must be 0x00 (Device-nGnRnE) \
         when S2 is device memory. Got attr=0x{:02X}",
        attr
    );
}

// ---------------------------------------------------------------------------
// Unit tests for combine_mem_type strength ordering
//
// These tests verify the combining logic independently by exercising it through
// the two-stage StreamContext translate path with known attribute combinations.
// ---------------------------------------------------------------------------

/// Device-nGnRnE (0x00) + Device-nGnRE (0x04): stronger (0x00) wins.
///
/// Map S1 as Device-nGnRnE (0x00) via map_page_device.
/// Map S2 as Normal-WB (0xFF) via map_stage2_page (we use 0x00 as our known stronger val).
///
/// Actually: we test that the minimum-rank attr is returned for both orderings.
/// Use the two-stage path with S1=Device(0x00) and S2=Normal(0xFF), then swap.
/// Both should give 0x00.
#[test]
fn test_13_1_5_device_wins_regardless_of_stage_order() {
    // Test A: S1=Device(0x00), S2=Normal(0xFF) → combined = 0x00
    {
        let ctx = StreamContext::new();
        ctx.enable();
        ctx.set_stage1_enabled(true);
        ctx.set_stage2_enabled(true);
        ctx.create_pasid(pasid0()).unwrap();
        ctx.create_stage2_address_space().unwrap();

        let s1_as = ctx.get_pasid_address_space(pasid0()).unwrap();
        s1_as
            .map_page_device(
                iova(0x2000),
                pa(0x6000),
                PagePermissions::read_write(),
                SecurityState::NonSecure,
            )
            .unwrap();

        ctx.map_stage2_page(iova(0x6000), pa(0xB000), PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();

        let result = ctx
            .translate(pasid0(), iova(0x2000), AccessType::Read, SecurityState::NonSecure)
            .unwrap();

        assert_eq!(
            result.page_attr, 0x00,
            "§13.1.5: Device(S1) + Normal(S2) → Device(0x00), got {:#04x}",
            result.page_attr
        );
    }

    // Test B: S1=Normal(0xFF), S2=Device(0x00) → combined = 0x00
    {
        let ctx = StreamContext::new();
        ctx.enable();
        ctx.set_stage1_enabled(true);
        ctx.set_stage2_enabled(true);
        ctx.create_pasid(pasid0()).unwrap();
        ctx.create_stage2_address_space().unwrap();

        ctx.map_page(
            pasid0(),
            iova(0x2000),
            pa(0x6000),
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .unwrap();

        ctx.map_stage2_device_page(
            iova(0x6000),
            pa(0xB000),
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .unwrap();

        let result = ctx
            .translate(pasid0(), iova(0x2000), AccessType::Read, SecurityState::NonSecure)
            .unwrap();

        assert_eq!(
            result.page_attr, 0x00,
            "§13.1.5: Normal(S1) + Device(S2) → Device(0x00), got {:#04x}",
            result.page_attr
        );
    }
}
