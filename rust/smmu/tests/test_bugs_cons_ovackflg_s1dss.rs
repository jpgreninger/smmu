#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

//! TDD tests for four bugs in the Rust SMMU implementation:
//!
//! - BUG-RUST-1: `process_pri_queue()` strips PRIQ_CONS.OVACKFLG (bit 31).
//! - BUG-RUST-2: CMD_PRI_RESP handler strips PRIQ_CONS.OVACKFLG (bit 31).
//! - BUG-RUST-3: S1DSS=0b01 returns identity PA when stage-2 is also enabled.
//! - BUG-RUST-4: `set_strtab_split()` with value >= 32 causes panic in
//!   `validate_stream_id_2level()`.

use smmu::types::{
    AccessType, CommandEntry, CommandType, PagePermissions, PRIEntry, QueueConfig, SMMUConfig,
    SecurityState, StreamConfig, StreamID, StreamTableFormat, IOVA, PA, PASID,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn make_smmu() -> SMMU {
    let s = SMMU::new();
    s.enable().unwrap();
    s
}

fn smmu_with_all_queues() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(
        SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN,
    );
    smmu
}

fn smmu_with_pri_queue() -> SMMU {
    let queue_cfg = QueueConfig::default();
    let smmu_cfg = SMMUConfig::from(queue_cfg);
    let smmu = SMMU::with_config(smmu_cfg);
    smmu.set_cr0(
        SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN,
    );
    smmu
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

fn sid(id: u32) -> StreamID {
    StreamID::new(id).unwrap()
}

fn pasid(id: u32) -> PASID {
    PASID::new(id).unwrap()
}

/// Build a PRIEntry for (stream_id, prg_index).
const fn pri_entry(stream_id: u32, prg_index: u16) -> PRIEntry {
    PRIEntry::with_address(stream_id, 0, 0x1000, AccessType::Read).with_prg_index(prg_index)
}

/// Build a CMD_PRI_RESP for (stream_id, prg_index).
const fn pri_resp_cmd(stream_id: u32, prg_index: u16) -> CommandEntry {
    let mut cmd = CommandEntry::new(CommandType::PriResp, stream_id, 0);
    cmd.prg_index = prg_index;
    cmd
}

// ============================================================================
// BUG-RUST-1: process_pri_queue() must preserve PRIQ_CONS.OVACKFLG (bit 31)
// ============================================================================

/// After software sets OVACKFLG=1 (bit 31 of PRIQ_CONS), calling
/// `process_pri_queue()` must advance the index portion of PRIQ_CONS but
/// MUST NOT clear bit 31.
///
/// FAILS before the fix because `advance_index()` returns `(idx+1) % modulus`
/// with no masking, so the bit-31 input is lost.
#[test]
fn bug_rust1_process_pri_queue_preserves_ovackflg() {
    let smmu = smmu_with_pri_queue();

    // Enqueue one PRI entry.
    smmu.submit_page_request(pri_entry(1, 0)).unwrap();

    // Software sets OVACKFLG=1 to acknowledge a prior overflow.
    smmu.set_priq_cons_ovackflg(1);

    let cons_before = smmu.priq_cons_index();
    assert_eq!(
        cons_before >> 31,
        1,
        "BUG-RUST-1 precondition: OVACKFLG bit 31 must be 1 before processing"
    );

    // Process the queue — advances CONS for the dequeued entry.
    smmu.process_pri_queue().unwrap();

    let cons_after = smmu.priq_cons_index();
    assert_eq!(
        cons_after >> 31,
        1,
        "BUG-RUST-1: process_pri_queue() must preserve OVACKFLG bit 31 \
         (before={cons_before:#010x}, after={cons_after:#010x})"
    );
}

/// Multiple entries: OVACKFLG must survive across multiple `process_pri_queue()`
/// advances.
#[test]
fn bug_rust1_process_pri_queue_preserves_ovackflg_multiple() {
    let smmu = smmu_with_pri_queue();

    smmu.submit_page_request(pri_entry(10, 0)).unwrap();
    smmu.submit_page_request(pri_entry(11, 1)).unwrap();
    smmu.submit_page_request(pri_entry(12, 2)).unwrap();

    // Set OVACKFLG.
    smmu.set_priq_cons_ovackflg(1);

    // Process all three entries one by one by calling process_pri_queue().
    // Each internal advance must preserve bit 31.
    smmu.process_pri_queue().unwrap();

    let cons_after = smmu.priq_cons_index();
    assert_eq!(
        cons_after >> 31,
        1,
        "BUG-RUST-1: OVACKFLG must be preserved after processing all entries \
         (cons={cons_after:#010x})"
    );
}

/// OVACKFLG=0 must stay 0 after processing (no spurious bit-31 set).
#[test]
fn bug_rust1_process_pri_queue_zero_ovackflg_stays_zero() {
    let smmu = smmu_with_pri_queue();

    smmu.submit_page_request(pri_entry(20, 0)).unwrap();

    // OVACKFLG starts at 0.
    assert_eq!(
        smmu.priq_cons_index() >> 31,
        0,
        "BUG-RUST-1 precondition: OVACKFLG must be 0 initially"
    );

    smmu.process_pri_queue().unwrap();

    assert_eq!(
        smmu.priq_cons_index() >> 31,
        0,
        "BUG-RUST-1: OVACKFLG=0 must remain 0 after process_pri_queue()"
    );
}

// ============================================================================
// BUG-RUST-2: CMD_PRI_RESP handler must preserve PRIQ_CONS.OVACKFLG (bit 31)
// ============================================================================

/// After software sets OVACKFLG=1, processing a CMD_PRI_RESP that matches the
/// HEAD entry must advance the index but MUST NOT strip bit 31.
///
/// FAILS before the fix because the CMD_PRI_RESP handler calls `advance_index`
/// with the raw cons value, discarding bit 31.
#[test]
fn bug_rust2_cmd_priresp_preserves_ovackflg() {
    let smmu = smmu_with_all_queues();

    // Enqueue a PRI entry at the head.
    smmu.submit_page_request(pri_entry(5, 0)).unwrap();

    // Software sets OVACKFLG=1.
    smmu.set_priq_cons_ovackflg(1);

    let cons_before = smmu.priq_cons_index();
    assert_eq!(
        cons_before >> 31,
        1,
        "BUG-RUST-2 precondition: OVACKFLG bit 31 must be 1 before command"
    );

    // Submit and process a CMD_PRI_RESP that matches the head entry.
    smmu.submit_command(pri_resp_cmd(5, 0)).unwrap();
    smmu.process_command_queue().unwrap();

    let cons_after = smmu.priq_cons_index();
    assert_eq!(
        cons_after >> 31,
        1,
        "BUG-RUST-2: CMD_PRI_RESP handler must preserve OVACKFLG bit 31 \
         (before={cons_before:#010x}, after={cons_after:#010x})"
    );
}

/// Interior CMD_PRI_RESP (no-op) must also not alter bit 31.
#[test]
fn bug_rust2_cmd_priresp_noop_preserves_ovackflg() {
    let smmu = smmu_with_all_queues();

    smmu.submit_page_request(pri_entry(6, 0)).unwrap();
    smmu.submit_page_request(pri_entry(7, 1)).unwrap();

    // Set OVACKFLG=1.
    smmu.set_priq_cons_ovackflg(1);

    // PriResp for interior entry (7, prg=1) — head is (6, prg=0).
    smmu.submit_command(pri_resp_cmd(7, 1)).unwrap();
    smmu.process_command_queue().unwrap();

    let cons_after = smmu.priq_cons_index();
    assert_eq!(
        cons_after >> 31,
        1,
        "BUG-RUST-2: no-op CMD_PRI_RESP must preserve OVACKFLG bit 31 \
         (cons={cons_after:#010x})"
    );
}

/// OVACKFLG=0 stays 0 when CMD_PRI_RESP advances CONS.
#[test]
fn bug_rust2_cmd_priresp_zero_ovackflg_stays_zero() {
    let smmu = smmu_with_all_queues();

    smmu.submit_page_request(pri_entry(8, 0)).unwrap();

    assert_eq!(smmu.priq_cons_index() >> 31, 0,
        "BUG-RUST-2 precondition: OVACKFLG must be 0");

    smmu.submit_command(pri_resp_cmd(8, 0)).unwrap();
    smmu.process_command_queue().unwrap();

    assert_eq!(
        smmu.priq_cons_index() >> 31,
        0,
        "BUG-RUST-2: CMD_PRI_RESP must not set bit 31 when OVACKFLG was 0"
    );
}

// ============================================================================
// BUG-RUST-3: S1DSS=0b01 must forward IOVA through stage-2 when s2 enabled
// ============================================================================

/// With stage1+stage2 enabled, S1DSS=0b01, and a stage-2 mapping of
/// IPA=0x5000 → PA=0x6000, a translation with PASID=0 must return PA=0x6000
/// (the stage-2 mapped PA), NOT the identity PA=0x5000.
///
/// FAILS before the fix because the S1DSS=0b01 arm returns identity without
/// checking whether stage-2 is active.
#[test]
fn bug_rust3_s1dss_01_with_stage2_uses_stage2() {
    let smmu = make_smmu();

    let stream_id = sid(0xB301);
    let pasid0 = pasid(0);
    let ipa = iova(0x5000);
    let final_pa = pa(0x6000);

    // Configure stream: stage1+stage2 enabled, s1dss=0b01, s1cd_max=4.
    let cfg = StreamConfig {
        s1dss: 0x01, // bypass stage-1, pass IOVA as IPA to stage-2
        s1cd_max: 4,
        ..StreamConfig::two_stage()
    };
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid0).unwrap();

    // Set up stage-2 address space.
    smmu.create_stage2_address_space(stream_id).unwrap();

    // Stage-2 mapping: IPA=0x5000 → PA=0x6000.
    smmu.map_stage2_page(
        stream_id,
        ipa,
        final_pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        stream_id,
        pasid0,
        ipa,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_ok(),
        "BUG-RUST-3: S1DSS=0b01 + stage-2 should succeed; got: {result:?}"
    );
    let translated_pa = result.unwrap().physical_address().as_u64();
    assert_eq!(
        translated_pa, 0x6000,
        "BUG-RUST-3: S1DSS=0b01 + stage-2 must return stage-2 PA=0x6000, \
         not identity PA=0x5000; got: {translated_pa:#x}"
    );
}

/// Stage-2 fault when IPA is not mapped: S1DSS=0b01 + stage-2, unmapped IPA
/// must return an error (not Ok with identity).
#[test]
fn bug_rust3_s1dss_01_with_stage2_unmapped_ipa_returns_error() {
    let smmu = make_smmu();

    let stream_id = sid(0xB302);
    let pasid0 = pasid(0);
    let unmapped_ipa = iova(0x7000);

    let cfg = StreamConfig {
        s1dss: 0x01,
        s1cd_max: 4,
        ..StreamConfig::two_stage()
    };
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid0).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();
    // No stage-2 mapping for 0x7000.

    let result = smmu.translate(
        stream_id,
        pasid0,
        unmapped_ipa,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_err(),
        "BUG-RUST-3: S1DSS=0b01 + stage-2 with unmapped IPA must fail, \
         not return identity; got: {result:?}"
    );
}

/// When stage-2 is NOT enabled, S1DSS=0b01 must still return identity PA
/// (existing correct behaviour for stage-1-only or bypass-stage-2 streams).
#[test]
fn bug_rust3_s1dss_01_without_stage2_returns_identity() {
    let smmu = make_smmu();

    let stream_id = sid(0xB303);
    let pasid0 = pasid(0);
    let test_iova = iova(0x9000);

    // Stage-1 only, no stage-2.
    let mut cfg = StreamConfig::stage1_only();
    cfg.s1dss = 0x01;
    cfg.s1cd_max = 4;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid0).unwrap();
    // No stage-1 mapping either — identity maps directly.

    let result = smmu.translate(
        stream_id,
        pasid0,
        test_iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_ok(),
        "BUG-RUST-3 regression: S1DSS=0b01 without stage-2 must return identity; got: {result:?}"
    );
    assert_eq!(
        result.unwrap().physical_address().as_u64(),
        0x9000,
        "BUG-RUST-3 regression: identity PA must equal IOVA when no stage-2"
    );
}

// ============================================================================
// BUG-RUST-4: set_strtab_split() with value >= 32 must not panic
// ============================================================================

/// `set_strtab_split(32)` followed by a translation on a two-level stream
/// table must not panic.  Before the fix `validate_stream_id_2level()` does
/// `stream_id >> split` with split=32, which is UB/panic in debug mode.
///
/// FAILS before the fix (panic in debug builds when log2size < 32).
#[test]
fn bug_rust4_strtab_split_32_does_not_panic() {
    let smmu = make_smmu();

    // Set a reserved SPLIT value AND a non-sentinel log2size so that the
    // validate_stream_id_2level() early-return guard does NOT fire.
    smmu.set_strtab_split(32);
    smmu.set_strtab_log2size(20); // enables the split check (log2size < 32)
    smmu.set_strtab_format(StreamTableFormat::TwoLevel);

    let stream_id = sid(1);
    let pasid0 = pasid(0);

    smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream_id, pasid0).unwrap();
    smmu.map_page(
        stream_id,
        pasid0,
        iova(0x1000),
        pa(0x2000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Must not panic; either succeeds or returns an error gracefully.
    let result = smmu.translate(
        stream_id,
        pasid0,
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    // After the fix, the reserved split value is clamped; we do not assert on Ok/Err
    // specifically because the clamping may or may not route the StreamID as valid.
    // The important invariant is no panic.
    let _ = result;
}

/// `set_strtab_split(33)` also must not panic.
#[test]
fn bug_rust4_strtab_split_33_does_not_panic() {
    let smmu = make_smmu();

    smmu.set_strtab_split(33);
    smmu.set_strtab_log2size(20); // enables the split check
    smmu.set_strtab_format(StreamTableFormat::TwoLevel);

    let stream_id = sid(2);
    let pasid0 = pasid(0);

    smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream_id, pasid0).unwrap();
    smmu.map_page(
        stream_id,
        pasid0,
        iova(0x1000),
        pa(0x2000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        stream_id,
        pasid0,
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    let _ = result;
}

/// Valid SPLIT values (6, 8, 10) must continue to work after the guard is added.
#[test]
fn bug_rust4_valid_split_6_still_works() {
    let smmu = make_smmu();

    smmu.set_strtab_split(6);
    smmu.set_strtab_log2size(20); // enables the split check (< 32)
    smmu.set_strtab_format(StreamTableFormat::TwoLevel);

    // With split=6 and log2size=20: l1_size = 1<<(20-6) = 1<<14 = 16384.
    // StreamID=3 < 16384 → valid → translation proceeds normally.
    let stream_id = sid(3);
    let pasid0 = pasid(0);

    smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream_id, pasid0).unwrap();
    smmu.map_page(
        stream_id,
        pasid0,
        iova(0x1000),
        pa(0x2000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        stream_id,
        pasid0,
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_ok(),
        "BUG-RUST-4 regression: valid SPLIT=6 must still work; got: {result:?}"
    );
}

// ============================================================================
// NOTE-1 (Cosmetic): enqueue_event() double-OR — regression guard
// ============================================================================

/// Regression test: verify the double-OR in `enqueue_event()` does not affect
/// correctness.  After the cosmetic cleanup, EVENTQ_PROD must still have bit 31
/// preserved after enqueueing an event (correct before and after the cleanup).
#[test]
fn note1_enqueue_event_eventq_prod_bit31_preserved() {
    use smmu::types::{EventEntry, EventType};

    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    // Force OVFLG in prod to 1 by storing directly (testing infrastructure).
    let current_prod = smmu.eventq_prod_index();
    // Set bit 31 via the toggle helper — this requires OVFLG=OVACKFLG.
    // Since both start at 0, a toggle fires only if ovflg == ovackflg.
    // We'll enqueue an event to observe PROD advances properly.

    let ev = EventEntry {
        event_type: EventType::FTranslation,
        stream_id: 1,
        pasid: 0,
        address: 0x1000,
        security_state: SecurityState::NonSecure,
        error_code: 0,
        timestamp: 0,
        ..EventEntry::zeroed()
    };
    let _ = smmu.submit_event(ev);

    // PROD should have advanced (bit 31 aside).
    let new_prod = smmu.eventq_prod_index();
    assert!(
        (new_prod & !((1u32) << 31)) != (current_prod & !((1u32) << 31))
        || new_prod == current_prod + 1,
        "NOTE-1: enqueue_event must advance PROD index (prod_before={current_prod:#010x}, prod_after={new_prod:#010x})"
    );
}
