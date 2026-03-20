#![allow(missing_docs)]
#![allow(clippy::doc_markdown)]

//! TDD test for BUG-2 (MODERATE): TBI masking not applied to address-space lookup.
//!
//! ARM IHI0070G.b §3.4.1 states:
//!   "When TBI is 1, the top byte of the input address is not used for
//!    translation purposes."
//!   "All input address bits are recorded unmodified in SMMU fault event records."
//!
//! The bug: the T0SZ range check correctly strips VA[63:56] when TBI=1, but the
//! subsequent call to translate_and_get_stage2_ipa() receives the original unmasked
//! IOVA.  This means a tagged address such as 0xAB00_0000_0000_1000 fails to find
//! the page that is mapped at 0x0000_0000_0000_1000.
//!
//! The fix must pass `lookup_iova` (with top byte masked) to the address-space walk
//! while keeping the original `iova` for fault record addresses.

use smmu::types::{AccessType, FaultMode, PagePermissions, SecurityState, StreamConfig, StreamID, IOVA, PA, PASID};
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
// BUG-2: TBI masking must be applied to the address-space lookup, not only to
//        the T0SZ range check.
// ─────────────────────────────────────────────────────────────────────────────

/// BUG-2 Test 1: TBI=1 — tagged IOVA translates to the same PA as the clean IOVA.
///
/// Map a page at clean address 0x0000_0000_0000_1000.
/// Present IOVA 0xAB00_0000_0000_1000 (same page, tag byte = 0xAB).
/// With TBI=1 the top byte is stripped before the walk, so the same page is found.
///
/// BEFORE FIX: Translation returns PageNotMapped (lookup uses unmasked address).
/// AFTER FIX:  Translation returns Ok with the expected PA.
#[test]
fn bug2_tbi_1_tagged_iova_translates_same_as_clean() {
    let smmu = make_smmu();

    // t0sz=0 → no VA range restriction (we only want to test the lookup masking)
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .t0sz(0)
        .tbi(true)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xB2_01), cfg).unwrap();
    smmu.create_pasid(sid(0xB2_01), pasid(0)).unwrap();

    let clean_iova: u64 = 0x0000_0000_0000_1000;
    let expected_pa: u64 = 0x0000_0000_DEAD_1000;

    smmu.map_page(
        sid(0xB2_01),
        pasid(0),
        iova(clean_iova),
        pa(expected_pa),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Same page with tag byte 0xAB in bits[63:56]
    let tagged_iova: u64 = 0xAB00_0000_0000_1000;
    let result = smmu.translate(
        sid(0xB2_01),
        pasid(0),
        iova(tagged_iova),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_ok(),
        "BUG-2: TBI=1, tagged IOVA={tagged_iova:#018x} must translate successfully \
         to the page mapped at clean address {clean_iova:#018x}; got {result:?}"
    );
    assert_eq!(
        result.unwrap().physical_address().as_u64(),
        expected_pa,
        "BUG-2: expected PA={expected_pa:#018x} but got a different physical address"
    );
}

/// BUG-2 Test 2: TBI=1 — all-ones tag byte still resolves the correct page.
///
/// Tag byte = 0xFF (worst-case tag value).
///
/// BEFORE FIX: PageNotMapped.
/// AFTER FIX:  Ok with correct PA.
#[test]
fn bug2_tbi_1_all_ones_tag_byte_resolves_page() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .t0sz(0)
        .tbi(true)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xB2_02), cfg).unwrap();
    smmu.create_pasid(sid(0xB2_02), pasid(0)).unwrap();

    let clean_iova: u64 = 0x0000_0000_0002_0000;
    let expected_pa: u64 = 0x0000_0000_CAFE_0000;

    smmu.map_page(
        sid(0xB2_02),
        pasid(0),
        iova(clean_iova),
        pa(expected_pa),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let tagged_iova: u64 = 0xFF00_0000_0002_0000;
    let result = smmu.translate(
        sid(0xB2_02),
        pasid(0),
        iova(tagged_iova),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_ok(),
        "BUG-2: TBI=1, all-ones tag byte {tagged_iova:#018x} must hit page at \
         {clean_iova:#018x}; got {result:?}"
    );
    assert_eq!(
        result.unwrap().physical_address().as_u64(),
        expected_pa,
        "BUG-2: expected PA={expected_pa:#018x}"
    );
}

/// BUG-2 Test 3: TBI=0 — tagged IOVA does NOT match the clean-address page
///               (the top byte is part of the address; page is not mapped there).
///
/// Regression guard: when TBI=0 the masking must NOT be applied to the lookup.
///
/// BEFORE FIX: Already returned PageNotMapped (correct for TBI=0).
/// AFTER FIX:  Must still return an error (PageNotMapped), not Ok.
#[test]
fn bug2_tbi_0_tagged_iova_does_not_match_clean_page() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .t0sz(0)
        .tbi(false) // TBI disabled — full address used for lookup
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xB2_03), cfg).unwrap();
    smmu.create_pasid(sid(0xB2_03), pasid(0)).unwrap();

    // Map the page at the clean address only
    let clean_iova: u64 = 0x0000_0000_0000_3000;
    smmu.map_page(
        sid(0xB2_03),
        pasid(0),
        iova(clean_iova),
        pa(0x0000_0000_FEED_3000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // With TBI=0 the tagged IOVA is a distinct address — not mapped
    let tagged_iova: u64 = 0xAB00_0000_0000_3000;
    let result = smmu.translate(
        sid(0xB2_03),
        pasid(0),
        iova(tagged_iova),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_err(),
        "BUG-2 regression: TBI=0, tagged IOVA={tagged_iova:#018x} must NOT hit \
         the page mapped at clean address {clean_iova:#018x}; got Ok"
    );
}

/// BUG-2 Test 4: TBI=1, T0SZ=16 — tagged IOVA within 48-bit (masked) range
///               translates to the correct PA.
///
/// This combines the T0SZ range-check fix (GAP-E) with the lookup masking fix
/// (BUG-2).  Tagged IOVA = 0xFF00_0000_0000_2000; clean = 0x0000_0000_0000_2000.
/// Masked address 0x0000_0000_0000_2000 < 2^48 → range OK, lookup succeeds.
///
/// BEFORE FIX: Range check passes (GAP-E fix), but lookup fails (BUG-2).
/// AFTER FIX:  Both checks pass; translation returns the correct PA.
#[test]
fn bug2_tbi_1_t0sz16_tagged_iova_full_translation() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .t0sz(16) // 48-bit VA space
        .tbi(true)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xB2_04), cfg).unwrap();
    smmu.create_pasid(sid(0xB2_04), pasid(0)).unwrap();

    let clean_iova: u64 = 0x0000_0000_0000_2000;
    let expected_pa: u64 = 0x0000_0000_BEEF_2000;

    smmu.map_page(
        sid(0xB2_04),
        pasid(0),
        iova(clean_iova),
        pa(expected_pa),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Tag byte 0xFF; masked = 0x0000_0000_0000_2000 < 2^48 — within range
    let tagged_iova: u64 = 0xFF00_0000_0000_2000;
    let result = smmu.translate(
        sid(0xB2_04),
        pasid(0),
        iova(tagged_iova),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_ok(),
        "BUG-2+GAP-E: TBI=1, T0SZ=16, tagged IOVA={tagged_iova:#018x} must translate \
         to the page at clean address {clean_iova:#018x}; got {result:?}"
    );
    assert_eq!(
        result.unwrap().physical_address().as_u64(),
        expected_pa,
        "BUG-2+GAP-E: expected PA={expected_pa:#018x}"
    );
}
