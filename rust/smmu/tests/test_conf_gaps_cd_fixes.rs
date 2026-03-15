#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

//! TDD tests for two ARM SMMU v3 conformance gaps:
//!
//! - Gap C: §3.4.1/§5.4 CD.T0SZ VA range enforcement
//! - Gap D: §3.2/§13.5 STE.INSTCFG access type override before permission checks

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
// Gap C: §3.4.1/§5.4 — CD.T0SZ VA range enforcement
// ─────────────────────────────────────────────────────────────────────────────

/// Gap C Test 1: T0SZ=16 (48-bit VA), IOVA at 2^48 boundary — must generate F_TRANSLATION.
///
/// ARM IHI0070G.b §3.4.1: An IOVA at or above 2^(64-T0SZ) is outside the valid VA
/// range and must generate F_TRANSLATION.  With T0SZ=16, the VA limit is 2^48 =
/// 0x0001_0000_0000_0000.  An IOVA equal to this limit must fault.
///
/// BEFORE FIX: Translation succeeds (VA range not checked).
/// AFTER FIX:  Translation fails with F_TRANSLATION event.
#[test]
fn gap_c_t0sz_16_iova_exceeds_range_generates_f_translation() {
    let smmu = make_smmu();

    // T0SZ=16 → 48-bit VA space → limit = 2^48 = 0x0001_0000_0000_0000
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .t0sz(16)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xC1), cfg).unwrap();
    smmu.create_pasid(sid(0xC1), pasid(0)).unwrap();

    // Map a page well inside the valid VA range (regression guard — this must still work
    // in the second sub-test below).  No page is mapped at the boundary.

    let va_limit: u64 = 1u64 << 48; // 0x0001_0000_0000_0000

    let result = smmu.translate(
        sid(0xC1),
        pasid(0),
        iova(va_limit), // exactly at the limit — outside valid range
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_err(),
        "Gap C: IOVA={va_limit:#x} with T0SZ=16 must fail (VA range exceeded); got Ok"
    );

    // An F_TRANSLATION event must be recorded.
    let events = smmu.get_events();
    let f_trans = events
        .iter()
        .find(|e| e.event_type == EventType::FTranslation)
        .expect("Gap C: Expected F_TRANSLATION event for VA range violation; none found");

    assert_eq!(
        f_trans.event_class, 2,
        "Gap C: F_TRANSLATION event must have event_class==2 (IN per ARM §7.3); got {}",
        f_trans.event_class
    );
}

/// Gap C Test 2: T0SZ=16 (48-bit VA), IOVA well inside range — must succeed.
///
/// ARM IHI0070G.b §3.4.1: An IOVA below 2^(64-T0SZ) is within the valid range.
/// With T0SZ=16 the limit is 2^48.  0x0000_FFFF_FFFF_F000 < 2^48 must succeed.
///
/// BEFORE FIX: Passes (no VA range check was performed).
/// AFTER FIX:  Must still pass (regression guard).
#[test]
fn gap_c_t0sz_16_iova_within_range_succeeds() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .t0sz(16)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xC2), cfg).unwrap();
    smmu.create_pasid(sid(0xC2), pasid(0)).unwrap();

    // Map a page inside the 48-bit VA window.
    let valid_iova: u64 = 0x0000_FFFF_FFFF_F000;
    smmu.map_page(
        sid(0xC2),
        pasid(0),
        iova(valid_iova),
        pa(0x1000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        sid(0xC2),
        pasid(0),
        iova(valid_iova),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_ok(),
        "Gap C: IOVA={:#x} with T0SZ=16 must succeed (within VA range); got error: {:?}",
        valid_iova,
        result.err()
    );
}

/// Gap C Test 3: T0SZ=0 (no restriction), large IOVA — must succeed.
///
/// ARM IHI0070G.b §5.4: T0SZ=0 means no address bits are excluded from the top,
/// giving the full 64-bit VA space.  No VA range check should be applied.
///
/// BEFORE FIX: Passes (no VA range check).
/// AFTER FIX:  Must still pass (regression guard).
#[test]
fn gap_c_t0sz_0_no_restriction() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .t0sz(0)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xC3), cfg).unwrap();
    smmu.create_pasid(sid(0xC3), pasid(0)).unwrap();

    let large_iova: u64 = 0x000F_FFFF_FFFF_F000;
    smmu.map_page(
        sid(0xC3),
        pasid(0),
        iova(large_iova),
        pa(0x2000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        sid(0xC3),
        pasid(0),
        iova(large_iova),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_ok(),
        "Gap C: T0SZ=0 imposes no VA restriction; IOVA={:#x} must succeed; got error: {:?}",
        large_iova,
        result.err()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Gap D: §3.2/§13.5 — STE.INSTCFG access type override before permission checks
// ─────────────────────────────────────────────────────────────────────────────

/// Gap D Test 1: inst_cfg=1 (Force-Instruction), page mapped execute, access=Read → success.
///
/// ARM IHI0070G.b §3.2/§13.5: STE.INSTCFG=1 (Force Instruction) overrides the
/// incoming access type: a Read is treated as Execute.  The page must be checked
/// against Execute permission, not Read permission.
///
/// Setup: page has execute permission.  access=Read → override to Execute → allowed.
///
/// BEFORE FIX: Read is checked against Read permission directly.
///             If page has no read permission, this would fail; the override is missing.
/// AFTER FIX:  Read is overridden to Execute before the permission check; succeeds.
#[test]
fn gap_d_instcfg1_read_overridden_to_execute_allowed() {
    let smmu = make_smmu();

    // inst_cfg=1: Force-Instruction (Read → Execute override)
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .inst_cfg(1)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xD1), cfg).unwrap();
    smmu.create_pasid(sid(0xD1), pasid(0)).unwrap();

    // Map the page with execute-only permission: execute allowed, read denied.
    smmu.map_page(
        sid(0xD1),
        pasid(0),
        iova(0x1000),
        pa(0x5000),
        PagePermissions::execute_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // access=Read, but inst_cfg=1 overrides to Execute → page has Execute → success.
    let result = smmu.translate(
        sid(0xD1),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_ok(),
        "Gap D: inst_cfg=1, execute-only page, access=Read must succeed \
         (Read overridden to Execute by INSTCFG=1); got error: {:?}",
        result.err()
    );
}

/// Gap D Test 2: inst_cfg=1 (Force-Instruction), page is read-only (no execute), access=Read → fail.
///
/// ARM IHI0070G.b §3.2/§13.5: STE.INSTCFG=1 overrides Read to Execute.
/// If the page does not have execute permission, the overridden check must deny access.
///
/// Setup: page has read-only permission.  access=Read → override to Execute → denied.
///
/// BEFORE FIX: Read is checked against Read permission → succeeds (wrong).
/// AFTER FIX:  Override to Execute → no execute bit on page → F_PERMISSION.
#[test]
fn gap_d_instcfg1_read_overridden_to_execute_denied() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .inst_cfg(1)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xD2), cfg).unwrap();
    smmu.create_pasid(sid(0xD2), pasid(0)).unwrap();

    // Read-only page: no execute permission.
    smmu.map_page(
        sid(0xD2),
        pasid(0),
        iova(0x2000),
        pa(0x6000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // access=Read, inst_cfg=1 → override to Execute → page lacks Execute → denied.
    let result = smmu.translate(
        sid(0xD2),
        pasid(0),
        iova(0x2000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_err(),
        "Gap D: inst_cfg=1, read-only page, access=Read must fail \
         (Read overridden to Execute, execute not permitted); got Ok"
    );
}

/// Gap D Test 3: inst_cfg=2 (Force-Data), page mapped read, access=Execute → success.
///
/// ARM IHI0070G.b §3.2/§13.5: STE.INSTCFG=2 (Force Data) overrides Execute to Read.
/// The page must be checked against Read permission.
///
/// Setup: page has read-write permission.  access=Execute → override to Read → allowed.
///
/// BEFORE FIX: Execute is checked against Execute permission → fails (page has no execute bit).
/// AFTER FIX:  Override to Read → page has Read → success.
#[test]
fn gap_d_instcfg2_execute_overridden_to_read_allowed() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .inst_cfg(2)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xD3), cfg).unwrap();
    smmu.create_pasid(sid(0xD3), pasid(0)).unwrap();

    // Read-write page: no execute permission.
    smmu.map_page(
        sid(0xD3),
        pasid(0),
        iova(0x3000),
        pa(0x7000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // access=Execute, inst_cfg=2 → override to Read → page has Read → success.
    let result = smmu.translate(
        sid(0xD3),
        pasid(0),
        iova(0x3000),
        AccessType::Execute,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_ok(),
        "Gap D: inst_cfg=2, read-write page, access=Execute must succeed \
         (Execute overridden to Read by INSTCFG=2); got error: {:?}",
        result.err()
    );
}

/// Gap D Test 4: inst_cfg=2 (Force-Data), page is execute-only, access=Execute → fail.
///
/// ARM IHI0070G.b §3.2/§13.5: STE.INSTCFG=2 overrides Execute to Read.
/// If the page does not have read permission, the check must deny access.
///
/// Setup: page has execute-only permission.  access=Execute → override to Read → denied.
///
/// BEFORE FIX: Execute is checked against Execute permission → succeeds (wrong).
/// AFTER FIX:  Override to Read → page lacks Read → F_PERMISSION.
#[test]
fn gap_d_instcfg2_execute_overridden_to_read_denied() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .inst_cfg(2)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xD4), cfg).unwrap();
    smmu.create_pasid(sid(0xD4), pasid(0)).unwrap();

    // Execute-only page: no read permission.
    smmu.map_page(
        sid(0xD4),
        pasid(0),
        iova(0x4000),
        pa(0x8000),
        PagePermissions::execute_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // access=Execute, inst_cfg=2 → override to Read → page lacks Read → denied.
    let result = smmu.translate(
        sid(0xD4),
        pasid(0),
        iova(0x4000),
        AccessType::Execute,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_err(),
        "Gap D: inst_cfg=2, execute-only page, access=Execute must fail \
         (Execute overridden to Read, read not permitted); got Ok"
    );
}

/// Gap D Test 5: inst_cfg=0 (no override) — both Read and Execute pass on RX page.
///
/// ARM IHI0070G.b §3.2/§13.5: STE.INSTCFG=0 means no override is applied.
/// Read and Execute on a read-execute page must both succeed without modification.
///
/// EXPECTED: Passes both before and after fix (regression guard).
#[test]
fn gap_d_instcfg0_no_override() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .inst_cfg(0) // no override
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xD5), cfg).unwrap();
    smmu.create_pasid(sid(0xD5), pasid(0)).unwrap();

    // Read-execute page.
    smmu.map_page(
        sid(0xD5),
        pasid(0),
        iova(0x5000),
        pa(0x9000),
        PagePermissions::read_execute(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let read_result = smmu.translate(
        sid(0xD5),
        pasid(0),
        iova(0x5000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        read_result.is_ok(),
        "Gap D: inst_cfg=0, RX page, access=Read must succeed (no override); got error: {:?}",
        read_result.err()
    );

    let exec_result = smmu.translate(
        sid(0xD5),
        pasid(0),
        iova(0x5000),
        AccessType::Execute,
        SecurityState::NonSecure,
    );

    assert!(
        exec_result.is_ok(),
        "Gap D: inst_cfg=0, RX page, access=Execute must succeed (no override); got error: {:?}",
        exec_result.err()
    );
}
