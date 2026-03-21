//! Regression tests for two bugs: BUG-RUST-A and BUG-RUST-I.
//!
//! - BUG-RUST-A: On the S1DSS=0b01 + stage-2 fault path, `nsipa` is
//!   hardcoded `false` in the `record_translation_fault` call.  ARM
//!   IHI0070G.b §7.3 requires nsipa=true when s2=true and the IPA is in
//!   Non-Secure PA space (i.e., when security_state==NonSecure).
//!
//! - BUG-RUST-I: `map_range()`, `map_pages()`, and `map_pages_batched()`
//!   in `AddressSpace` create entries WITHOUT `.with_access_flag(true)`,
//!   causing AF=0 for all bulk-mapped pages.  With ha=false and affd=false
//!   (the default `StreamConfig`), any translation of a bulk-mapped page
//!   triggers a spurious F_ACCESS fault instead of succeeding.
//!
//! Each test is designed to FAIL before the corresponding fix is applied and
//! PASS after it.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

use smmu::address_space::AddressSpace;
use smmu::stream_context::StreamContext;
use smmu::types::{
    AccessType, EventType, PagePermissions, SecurityState, StreamConfig, StreamID, IOVA, PA,
    PASID, PAGE_SIZE,
};
use smmu::SMMU;
use std::sync::Arc;

// ============================================================================
// Helpers
// ============================================================================

fn make_smmu() -> SMMU {
    let s = SMMU::new();
    s.enable().unwrap();
    s
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn sid(id: u32) -> StreamID {
    StreamID::new(id).unwrap()
}

fn pasid(id: u32) -> PASID {
    PASID::new(id).unwrap()
}

// ============================================================================
// BUG-RUST-A: nsipa must match security_state on S1DSS=0b01 + stage-2 fault
// ============================================================================
//
// The S1DSS=0b01 + stage-2 path in smmu/mod.rs calls record_translation_fault
// with nsipa hardcoded to `false`.  ARM §7.3 requires:
//   nsipa = s2 && (security_state == NonSecure)
//
// Setup: stream with stage1_enabled=true, s1cd_max>0 (substream-capable),
//        s1dss=1, stage2_enabled=true.  PASID=0 hits the S1DSS=0b01 arm.
//        Stage-2 has no mapping → stage-2 fault (s2=true).
//
// BEFORE FIX: nsipa is always false, even for NonSecure streams.
// AFTER FIX:  nsipa = (security_state == NonSecure).

/// BUG-RUST-A: NonSecure stream with S1DSS=0b01 + stage-2 fault → nsipa=true.
///
/// BEFORE FIX: the event has nsipa=false → test FAILS.
/// AFTER FIX:  the event has nsipa=true → test PASSES.
#[test]
fn bug_rust_a_s1dss01_stage2_fault_nssec_stream_nsipa_is_true() {
    let smmu = make_smmu();
    // Use a stream ID not used by other tests.
    let stream_id = sid(0xDA_01);

    // Configure: stage1+stage2, s1dss=1 (bypass-S1), s1cd_max=1 (substream-capable).
    // PASID=0 on a substream-capable stream (s1cd_max>0) with s1dss=1
    // takes the S1DSS=0b01 arm → bypasses stage-1, forwards IOVA as IPA to stage-2.
    let mut cfg = StreamConfig::two_stage();
    cfg.t0sz = 0;
    cfg.s2_t0sz = 0;
    cfg.s1dss = 1;           // S1DSS=0b01: bypass stage-1 for PASID=0
    cfg.s1cd_max = 1;        // Substream-capable → S1DSS block is reached for PASID=0
    cfg.security_state = SecurityState::NonSecure;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // Create stage-2 address space but leave it empty (no IPA→PA mappings).
    // S1DSS=0b01 forwards the IOVA as an IPA to stage-2; no mapping → fault.
    smmu.create_stage2_address_space(stream_id).unwrap();

    // Translate: S1DSS=0b01 uses IOVA as IPA; stage-2 has no mapping → fault.
    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "BUG-RUST-A: S1DSS=0b01 + empty stage-2 must produce a fault, got {result:?}"
    );

    // Find the event for this stream.
    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| e.stream_id == stream_id.as_u32())
        .expect("BUG-RUST-A: Expected a fault event for this stream");

    // The fault is a stage-2 fault → s2 must be true.
    assert!(
        ev.s2,
        "BUG-RUST-A: S1DSS=0b01 + stage-2 fault must have s2=true, got s2={}",
        ev.s2
    );

    // BUG-RUST-A assertion: nsipa must be true for a NonSecure stream.
    assert!(
        ev.nsipa,
        "BUG-RUST-A: NonSecure stream S1DSS=0b01 + stage-2 fault must have nsipa=true, \
         got nsipa=false (hardcoded false bug)"
    );
}

/// BUG-RUST-A regression guard: Secure stream with S1DSS=0b01 + stage-2 fault → nsipa=false.
///
/// Even after the fix, a Secure stream must produce nsipa=false because
/// nsipa = s2 && (security_state == NonSecure), and NonSecure is false here.
///
/// BEFORE FIX: nsipa is always false → this test passes before the fix
/// (regression guard — must continue to pass after the fix).
#[test]
fn bug_rust_a_s1dss01_stage2_fault_secure_stream_nsipa_is_false() {
    let smmu = make_smmu();
    let stream_id = sid(0xDA_02);

    // Secure two-stage stream with S1DSS=0b01.
    let mut cfg = StreamConfig::two_stage();
    cfg.t0sz = 0;
    cfg.s2_t0sz = 0;
    cfg.s1dss = 1;
    cfg.s1cd_max = 1;
    cfg.security_state = SecurityState::Secure;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();

    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::Secure,
    );
    assert!(
        result.is_err(),
        "BUG-RUST-A: Secure S1DSS=0b01 + empty stage-2 must produce a fault, got {result:?}"
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| e.stream_id == stream_id.as_u32())
        .expect("BUG-RUST-A: Expected a fault event for this stream");

    assert!(
        ev.s2,
        "BUG-RUST-A: Secure S1DSS=0b01 + stage-2 fault must have s2=true, got s2={}",
        ev.s2
    );

    // Regression guard: nsipa must be false for Secure stream.
    assert!(
        !ev.nsipa,
        "BUG-RUST-A: Secure stream S1DSS=0b01 + stage-2 fault must have nsipa=false, \
         got nsipa=true"
    );
}

/// BUG-RUST-A: Event type must be F_TRANSLATION on the S1DSS=0b01 + stage-2 fault path.
///
/// Verifies that the fault event type is correct alongside the nsipa check.
#[test]
fn bug_rust_a_s1dss01_stage2_fault_event_type_is_f_translation() {
    let smmu = make_smmu();
    let stream_id = sid(0xDA_03);

    let mut cfg = StreamConfig::two_stage();
    cfg.t0sz = 0;
    cfg.s2_t0sz = 0;
    cfg.s1dss = 1;
    cfg.s1cd_max = 1;
    cfg.security_state = SecurityState::NonSecure;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();

    let _ = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x2000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| e.stream_id == stream_id.as_u32())
        .expect("BUG-RUST-A: Expected a fault event for this stream");

    // The stage-2 fault for a missing IPA mapping should be F_TRANSLATION.
    assert!(
        matches!(ev.event_type, EventType::FTranslation),
        "BUG-RUST-A: S1DSS=0b01 + stage-2 fault must be F_TRANSLATION, got {:?}",
        ev.event_type
    );
}

// ============================================================================
// BUG-RUST-I: map_range / map_pages / map_pages_batched miss .with_access_flag(true)
// ============================================================================
//
// map_page() correctly sets AF=true on the created PageEntry.  The three bulk
// mapping functions (map_range, map_pages, map_pages_batched) forget to chain
// .with_access_flag(true), so all bulk-mapped pages have AF=0.
//
// StreamContext::translate() checks: if !ha && !affd && AF==0 → F_ACCESS.
// The default StreamConfig has ha=false and affd=false, so any translation of
// a bulk-mapped page triggers a spurious F_ACCESS fault.
//
// BEFORE FIX: translate returns Err(AccessFlagFault) / F_ACCESS → tests FAIL.
// AFTER FIX:  translate returns Ok(...)                           → tests PASS.
//
// Note on ownership:
// - map_range(&mut self): tested via plain AddressSpace (non-Arc) + get_page_access_flag.
//   Also tested via StreamContext where we call map_range on a separate AS that is
//   then attached as stage-2 (stage-2 AS created separately, map_range on it before attach).
// - map_pages(&self): tested via Arc<AddressSpace> obtained from get_pasid_address_space().
// - map_pages_batched(&self): tested via Arc<AddressSpace> obtained from get_pasid_address_space().

/// BUG-RUST-I: map_range() must set AF=true on each inserted page entry.
///
/// Test uses AddressSpace::get_page_access_flag() to directly inspect the AF
/// bit of each page inserted by map_range().
///
/// BEFORE FIX: AF=false for all range-mapped entries → test FAILS.
/// AFTER FIX:  AF=true  for all range-mapped entries → test PASSES.
#[test]
fn bug_rust_i_map_range_sets_access_flag_direct() {
    let mut addr_space = AddressSpace::new();

    // Map 3 pages: IOVA 0x1000, 0x2000, 0x3000 → PA 0x4000, 0x5000, 0x6000.
    let start_iova = IOVA::new(0x1000).unwrap();
    let end_iova = IOVA::new(0x1000 + 2 * PAGE_SIZE).unwrap(); // inclusive end at 0x3000
    let start_pa = PA::new(0x4000).unwrap();

    addr_space
        .map_range(
            start_iova,
            end_iova,
            start_pa,
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .unwrap();

    // Verify AF=true for each page in the range.
    for page_idx in 0u64..3 {
        let test_iova = IOVA::new(0x1000 + page_idx * PAGE_SIZE).unwrap();
        let af = addr_space.get_page_access_flag(test_iova);
        assert_eq!(
            af,
            Some(true),
            "BUG-RUST-I: map_range() page IOVA=0x{:x} must have AF=true, got {:?} \
             (AF=false means spurious F_ACCESS faults will occur)",
            0x1000 + page_idx * PAGE_SIZE,
            af
        );
    }
}

/// BUG-RUST-I: map_range() bug causes F_ACCESS in StreamContext translate.
///
/// Uses StreamContext with a stage-2 AddressSpace populated via map_range.
/// Stage-2 AF=0 → translate produces F_ACCESS error (s2 AF check fires).
///
/// BEFORE FIX: two-stage translate Err (AccessFlagFault) → test FAILS.
/// AFTER FIX:  two-stage translate Ok                    → test PASSES.
#[test]
fn bug_rust_i_map_range_stage2_causes_f_access_fault() {
    let ctx = StreamContext::new();
    ctx.enable();
    ctx.set_stage1_enabled(true);
    ctx.set_stage2_enabled(true);

    ctx.create_pasid(PASID::new(0).unwrap()).unwrap();

    // Map a stage-1 page: IOVA 0x1000 → IPA 0x2000 (using map_page which is AF=true).
    ctx.map_page(
        PASID::new(0).unwrap(),
        IOVA::new(0x1000).unwrap(),
        PA::new(0x2000).unwrap(),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Create a separate stage-2 AddressSpace and use map_range on it.
    // map_range requires &mut self so we use it before wrapping in Arc.
    let mut s2 = AddressSpace::new();
    s2.map_range(
        IOVA::new(0x2000).unwrap(), // IPA
        IOVA::new(0x2000).unwrap(), // single-page range (start == end)
        PA::new(0x3000).unwrap(),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Wrap and attach.
    ctx.set_stage2_address_space(Some(Arc::new(s2)));

    // Two-stage translate: IOVA 0x1000 → IPA 0x2000 → PA 0x3000.
    // Stage-2 lookup of IPA 0x2000: if map_range set AF=0, s2 AF check → F_ACCESS error.
    let result = ctx.translate(
        PASID::new(0).unwrap(),
        IOVA::new(0x1000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_ok(),
        "BUG-RUST-I: map_range() must set AF=true; two-stage translate must succeed, \
         got {result:?} (likely AccessFlagFault from stage-2 AF=0 bug)"
    );
}

/// BUG-RUST-I: map_pages() must set AF=true so that translate succeeds.
///
/// Uses StreamContext: obtains Arc<AddressSpace> for PASID 0 (map_pages takes &self,
/// so it works on Arc<AddressSpace>), calls map_pages, then translates.
///
/// BEFORE FIX: AF=0 → F_ACCESS fault → test FAILS.
/// AFTER FIX:  AF=true → translate succeeds → test PASSES.
#[test]
fn bug_rust_i_map_pages_sets_access_flag() {
    let ctx = StreamContext::new();
    ctx.enable();
    ctx.set_stage1_enabled(true);
    ctx.set_stage2_enabled(false);

    ctx.create_pasid(PASID::new(0).unwrap()).unwrap();

    // Obtain Arc<AddressSpace> for PASID 0. map_pages takes &self → works on Arc.
    let addr_space = ctx
        .get_pasid_address_space(PASID::new(0).unwrap())
        .expect("BUG-RUST-I: PASID 0 address space must exist after create_pasid");

    // Map pages via map_pages() (bulk IOVA→PA pairs).
    let mappings = vec![
        (IOVA::new(0x5000).unwrap(), PA::new(0x8000).unwrap()),
        (IOVA::new(0x6000).unwrap(), PA::new(0x9000).unwrap()),
        (IOVA::new(0x7000).unwrap(), PA::new(0xA000).unwrap()),
    ];
    addr_space
        .map_pages(&mappings, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    // Verify AF directly via get_page_access_flag.
    for &(test_iova, _) in &mappings {
        let af = addr_space.get_page_access_flag(test_iova);
        assert_eq!(
            af,
            Some(true),
            "BUG-RUST-I: map_pages() IOVA {test_iova:?} must have AF=true, got {af:?}"
        );
    }

    // Also verify translate succeeds (AF check in StreamContext).
    for &(test_iova, _) in &mappings {
        let result = ctx.translate(
            PASID::new(0).unwrap(),
            test_iova,
            AccessType::Read,
            SecurityState::NonSecure,
        );
        assert!(
            result.is_ok(),
            "BUG-RUST-I: map_pages() must set AF=true; translate of IOVA {test_iova:?} must \
             succeed, got {result:?} (likely AccessFlagFault from AF=0 bug)"
        );
    }
}

/// BUG-RUST-I: map_pages_batched() must set AF=true so that translate succeeds.
///
/// Uses StreamContext: obtains Arc<AddressSpace> for PASID 0 and calls
/// map_pages_batched (takes &self → works on Arc).
///
/// BEFORE FIX: AF=0 → F_ACCESS fault → test FAILS.
/// AFTER FIX:  AF=true → translate succeeds → test PASSES.
#[test]
fn bug_rust_i_map_pages_batched_sets_access_flag() {
    let ctx = StreamContext::new();
    ctx.enable();
    ctx.set_stage1_enabled(true);
    ctx.set_stage2_enabled(false);

    ctx.create_pasid(PASID::new(0).unwrap()).unwrap();

    let addr_space = ctx
        .get_pasid_address_space(PASID::new(0).unwrap())
        .expect("BUG-RUST-I: PASID 0 address space must exist");

    // Map pages via map_pages_batched().
    let mappings = vec![
        (IOVA::new(0xB000).unwrap(), PA::new(0xC000).unwrap()),
        (IOVA::new(0xD000).unwrap(), PA::new(0xE000).unwrap()),
    ];
    addr_space
        .map_pages_batched(&mappings, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    // Verify AF directly.
    for &(test_iova, _) in &mappings {
        let af = addr_space.get_page_access_flag(test_iova);
        assert_eq!(
            af,
            Some(true),
            "BUG-RUST-I: map_pages_batched() IOVA {test_iova:?} must have AF=true, got {af:?}"
        );
    }

    // Verify translate succeeds.
    for &(test_iova, _) in &mappings {
        let result = ctx.translate(
            PASID::new(0).unwrap(),
            test_iova,
            AccessType::Read,
            SecurityState::NonSecure,
        );
        assert!(
            result.is_ok(),
            "BUG-RUST-I: map_pages_batched() must set AF=true; translate of IOVA {test_iova:?} \
             must succeed, got {result:?} (likely AccessFlagFault from AF=0 bug)"
        );
    }
}

/// BUG-RUST-I regression guard: map_page() (single-page) must continue to set AF=true.
///
/// This verifies the existing correct behaviour is not broken by any related fix.
/// This test PASSES before and after the fix.
#[test]
fn bug_rust_i_regression_map_page_af_still_correct() {
    let ctx = StreamContext::new();
    ctx.enable();
    ctx.set_stage1_enabled(true);
    ctx.set_stage2_enabled(false);

    ctx.create_pasid(PASID::new(0).unwrap()).unwrap();

    // Use the StreamContext's map_page (which already calls AddressSpace::map_page with AF=true).
    ctx.map_page(
        PASID::new(0).unwrap(),
        IOVA::new(0xF000).unwrap(),
        PA::new(0x10000).unwrap(),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = ctx.translate(
        PASID::new(0).unwrap(),
        IOVA::new(0xF000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_ok(),
        "Regression guard: map_page() must set AF=true; translate must succeed, got {result:?}"
    );
}
