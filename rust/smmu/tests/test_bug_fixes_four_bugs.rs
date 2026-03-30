//! TDD tests for four confirmed Rust SMMU bugs.
//!
//! - BUG-RUST-1: FaultType::BadSTE maps to EventType::FTranslation instead of EventType::CBadSte
//! - BUG-RUST-3: RIL SCALE field not clamped to 39, causing shift overflow
//! - BUG-RUST-2: S2AFFD/S2HA not loaded in stage-2 AF check
//! - BUG-RUST-5: PRIVCFG=3 doesn't promote compound execute access types
#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventType, FaultMode, PagePermissions,
    SecurityState, StreamConfig, StreamID, IOVA, PA, PASID,
};
use smmu::SMMU;

fn make_smmu() -> SMMU {
    let s = SMMU::new();
    s.enable().unwrap();
    s
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

fn pasid(id: u32) -> PASID {
    PASID::new(id).unwrap()
}

fn sid(id: u32) -> StreamID {
    StreamID::new(id).unwrap()
}

// ─── BUG-RUST-1: FaultType::BadSTE must map to EventType::CBadSte ────────────

/// BUG-RUST-1 integration test: trigger a C_BAD_STE event via two-stage translation
/// where the stage-2 address space is absent (translate_two_stage_with_ipa returns
/// StreamNotConfigured → map_translation_error_to_fault_type → FaultType::BadSTE →
/// map_fault_type_to_event_type → should be CBadSte, not FTranslation).
#[test]
fn bug_rust1_integration_bad_ste_event_from_missing_stage2() {
    let smmu = make_smmu();
    let stream_id = sid(42);

    // Two-stage stream; stage-2 address space NOT created.
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .s2_t0sz(16)
        .s2_ps(5)
        .build()
        .unwrap();
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // Map a stage-1 page; stage-2 address space intentionally absent.
    smmu.map_page(
        stream_id, pasid(0), iova(0x1000), pa(0x2000),
        PagePermissions::read_only(), SecurityState::NonSecure,
    ).unwrap();

    let result = smmu.translate(
        stream_id, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure,
    );
    assert!(result.is_err(), "Expected translation error for missing stage-2");

    // The event queue must contain a CBadSte event, not FTranslation
    let events = smmu.get_events();
    let bad_ste_event = events.iter().find(|e| {
        e.event_type == EventType::CBadSte && e.stream_id == stream_id.as_u32()
    });
    let f_translation_for_bad_ste = events.iter().find(|e| {
        e.event_type == EventType::FTranslation && e.stream_id == stream_id.as_u32()
    });

    assert!(
        bad_ste_event.is_some(),
        "BUG-RUST-1: Expected CBadSte event in queue for missing stage-2 address space. \
         Got events: {events:?}"
    );
    assert!(
        f_translation_for_bad_ste.is_none(),
        "BUG-RUST-1: FTranslation event was emitted for a BadSTE condition. Events: {events:?}"
    );
}

// ─── BUG-RUST-3: RIL SCALE clamping to 39 ────────────────────────────────────

/// BUG-RUST-3: TLBI with RIL and scale=40 must not panic.
/// ARM spec: SCALE values >39 must be treated as 39.
/// Before fix: shift of 5*40=200 panics in debug builds (overflow) during
/// process_command_queue() when the RIL invalidation path is reached.
///
/// IMPORTANT: CR2.PTM must be set AND process_command_queue() called to exercise
/// the buggy shift computation. submit_command() only enqueues; it does not execute.
#[test]
fn bug_rust3_ril_scale_40_does_not_panic() {
    let smmu = make_smmu();
    let stream_id = sid(10);
    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(stream_id, cfg).unwrap();

    // Set CR2.PTM (bit 2) so the RIL range-invalidation path is actually executed
    // when process_command_queue() runs.
    smmu.set_cr2(SMMU::CR2_PTM);

    // Build a TlbiNhVa command with RIL=true and scale=40 (out-of-spec value)
    let mut cmd = CommandEntry::new(CommandType::TlbiNhVa, stream_id.as_u32(), 0);
    cmd.ril = true;
    cmd.scale = 40; // Must be clamped to 39 before the 5*scale shift
    cmd.num = 0;
    cmd.tg = 0;     // 4096-byte granule
    cmd.start_address = 0x1000;
    cmd.asid = 0;

    smmu.submit_command(cmd).unwrap();
    // Process the command: this is where the shift is computed.
    // Must not panic; process_command_queue must succeed.
    let result = smmu.process_command_queue();
    assert!(
        result.is_ok(),
        "BUG-RUST-3: process_command_queue failed or panicked with scale=40: {result:?}"
    );
}

/// BUG-RUST-3: scale=255 (maximum u8) must not panic.
#[test]
fn bug_rust3_ril_scale_255_does_not_panic() {
    let smmu = make_smmu();
    let stream_id = sid(11);
    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(stream_id, cfg).unwrap();

    smmu.set_cr2(SMMU::CR2_PTM);

    let mut cmd = CommandEntry::new(CommandType::TlbiNhVa, stream_id.as_u32(), 0);
    cmd.ril = true;
    cmd.scale = 255; // Extreme value — must be clamped to 39
    cmd.num = 31;
    cmd.tg = 0;
    cmd.start_address = 0x0;
    cmd.asid = 0;

    smmu.submit_command(cmd).unwrap();
    let result = smmu.process_command_queue();
    assert!(
        result.is_ok(),
        "BUG-RUST-3: process_command_queue panicked with scale=255: {result:?}"
    );
}

/// BUG-RUST-3: scale=3 (5*3=15 shift, blocks=(num+1)<<15, granule=4096=2^12).
/// Worst case: (31+1) << 15 * 4096 = 32 * 32768 * 4096 = 4 GiB — fits in u64.
/// This is the safe small-scale regression test.
#[test]
fn bug_rust3_ril_small_scale_no_overflow() {
    let smmu = make_smmu();
    let stream_id = sid(12);
    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(stream_id, cfg).unwrap();

    smmu.set_cr2(SMMU::CR2_PTM);

    let mut cmd = CommandEntry::new(CommandType::TlbiNhVa, stream_id.as_u32(), 0);
    cmd.ril = true;
    cmd.scale = 3;  // 5*3=15 shift — safely fits in u64 even with max num
    cmd.num = 31;
    cmd.tg = 0;
    cmd.start_address = 0x0;
    cmd.asid = 0;

    smmu.submit_command(cmd).unwrap();
    let result = smmu.process_command_queue();
    assert!(
        result.is_ok(),
        "BUG-RUST-3: process_command_queue failed for safe scale=3: {result:?}"
    );
}

// ─── BUG-RUST-2: S2AFFD/S2HA must govern stage-2 AF fault checking ───────────

/// BUG-RUST-2 test 1: S2AFFD=true on a stage-2-only stream must suppress F_ACCESS
/// even when a page has AF=0.
///
/// Before fix: the stage-2 AF check is entirely absent in translate_stage2_only,
/// OR if it existed, it would use self.affd (stage-1 AFFD) instead of self.s2affd.
/// After fix: stage-2 AF check uses self.s2affd + self.s2ha.
#[test]
fn bug_rust2_s2affd_true_suppresses_af_fault_on_stage2_only() {
    let smmu = make_smmu();
    let stream_id = sid(20);

    // Stage-2-only stream with S2AFFD=true (AF faults disabled for stage-2).
    // Stage-1 AFFD=false is irrelevant — stage-2-only streams don't do stage-1.
    let mut cfg = StreamConfig::builder()
        .stage1_enabled(false)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .s2_t0sz(16)
        .s2_ps(5)
        .build()
        .unwrap();
    cfg.s2affd = true;
    cfg.s2ha = false;
    cfg.ha = false;
    cfg.affd = false; // Stage-1 AFFD false — must NOT affect stage-2 behaviour
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();

    // Map a stage-2 page with AF=0 (unaccessed). IPA == IOVA for stage-2-only streams.
    smmu.map_stage2_page_unaccessed(
        stream_id,
        iova(0x1000),
        pa(0x2000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    ).unwrap();

    let result = smmu.translate(
        stream_id, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure,
    );
    assert!(
        result.is_ok(),
        "BUG-RUST-2: S2AFFD=true should suppress F_ACCESS on stage-2-only AF=0 page, \
         got {result:?}"
    );
    let events = smmu.get_events();
    let af_fault = events.iter().find(|e| {
        e.event_type == EventType::FAccess && e.stream_id == stream_id.as_u32()
    });
    assert!(
        af_fault.is_none(),
        "BUG-RUST-2: Unexpected F_ACCESS event with S2AFFD=true: {events:?}"
    );
}

/// BUG-RUST-2 test 2: S2AFFD=false on a stage-2-only stream must generate F_ACCESS
/// when a page has AF=0.
#[test]
fn bug_rust2_s2affd_false_generates_af_fault_on_stage2_only() {
    let smmu = make_smmu();
    let stream_id = sid(21);

    let mut cfg = StreamConfig::builder()
        .stage1_enabled(false)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .s2_t0sz(16)
        .s2_ps(5)
        .build()
        .unwrap();
    cfg.s2affd = false;
    cfg.s2ha = false;
    cfg.ha = false;
    cfg.affd = true; // Stage-1 AFFD=true — must NOT suppress the stage-2 AF fault
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();

    // Map a stage-2 page with AF=0 (unaccessed).
    smmu.map_stage2_page_unaccessed(
        stream_id,
        iova(0x3000),
        pa(0x4000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    ).unwrap();

    let result = smmu.translate(
        stream_id, pasid(0), iova(0x3000), AccessType::Read, SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "BUG-RUST-2: S2AFFD=false with AF=0 must generate F_ACCESS, got Ok"
    );

    let events = smmu.get_events();
    let af_fault = events.iter().find(|e| {
        e.event_type == EventType::FAccess && e.stream_id == stream_id.as_u32()
    });
    assert!(
        af_fault.is_some(),
        "BUG-RUST-2: Expected F_ACCESS event with S2AFFD=false + AF=0: {events:?}"
    );
}

/// BUG-RUST-2 test 3: Two-stage stream with S2AFFD=true must not generate F_ACCESS
/// for stage-2 AF=0 pages.
#[test]
fn bug_rust2_s2affd_true_suppresses_af_fault_on_two_stage() {
    let smmu = make_smmu();
    let stream_id = sid(22);

    // Two-stage stream: S2AFFD=true suppresses stage-2 AF faults.
    let mut cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .s2_t0sz(16)
        .s2_ps(5)
        .build()
        .unwrap();
    cfg.s2affd = true;
    cfg.s2ha = false;
    cfg.ha = false;
    cfg.affd = false; // Stage-1 AFFD=false — stage-1 AF faults are active
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();

    // Map stage-1 page with AF=true (no stage-1 AF fault).
    smmu.map_page(
        stream_id, pasid(0), iova(0x5000), pa(0x6000),
        PagePermissions::read_only(), SecurityState::NonSecure,
    ).unwrap();
    // Map stage-2 page with AF=0. The IPA from stage-1 is 0x6000.
    smmu.map_stage2_page_unaccessed(
        stream_id,
        iova(0x6000),
        pa(0x7000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    ).unwrap();

    let result = smmu.translate(
        stream_id, pasid(0), iova(0x5000), AccessType::Read, SecurityState::NonSecure,
    );
    assert!(
        result.is_ok(),
        "BUG-RUST-2: S2AFFD=true should suppress stage-2 F_ACCESS in two-stage, \
         got {result:?}"
    );
    let events = smmu.get_events();
    let af_fault = events.iter().find(|e| {
        e.event_type == EventType::FAccess && e.stream_id == stream_id.as_u32()
    });
    assert!(
        af_fault.is_none(),
        "BUG-RUST-2: Unexpected F_ACCESS in two-stage with S2AFFD=true: {events:?}"
    );
}

// ─── BUG-RUST-5: PRIVCFG=3 must promote compound execute access types ─────────

/// BUG-RUST-5 test 1: PRIVCFG=3 (Force Privileged) must promote AccessType::ReadExecute
/// to its privileged equivalent so privileged-only pages can be accessed.
///
/// The bug: PRIVCFG=3 match arm does not include ReadExecute, WriteExecute, ReadWriteExecute
/// so they fall through to `other => other` — they remain unprivileged access types.
/// The privileged_only check at the end of translate_stage1_only uses
/// `effective_priv_suppresses_check()` which already returns true for PRIVCFG=3
/// regardless of access type, so the privileged_only page check passes.
///
/// The actual failure is in `is_access_effectively_privileged(access_type)` which is
/// used for UWXN and pnu computation, and in `effective_access_type()` which is
/// called before translate() for IND/RNW event fields. When `ReadExecute` is not
/// promoted, `is_privileged()` on it returns false (no bit-3), causing:
/// - incorrect pnu=false in events (bug)
/// - UWXN may trigger on the wrong access type (bug)
///
/// To test the actual permission-check failure specifically, we test a STRW=EL1
/// stream where the STRW check (not PRIVCFG) would reject the unpromoted type.
/// But the simplest observable failure is through the `effective_access_type()`
/// path: with PRIVCFG=3, effective_access_type(ReadExecute) should return
/// ReadExecutePrivileged (with bit-3 set), but the current code leaves it as
/// ReadExecute (bit-3 clear). This means is_privileged() returns false.
///
/// We verify via the pnu field in the emitted event:
/// - With PRIVCFG=3 and ReadExecute: pnu must be true (access is privileged).
/// - Without fix: pnu is false (ReadExecute.is_privileged() == false).
#[test]
fn bug_rust5_privcfg3_promotes_read_execute() {
    // With PRIVCFG=3 and ReadExecute on a normal (non-privileged-only) page:
    // - effective_access_type(ReadExecute) should give ReadExecutePrivileged
    // - pnu in the fault event should be true
    // - The translation itself should succeed (page has R+X)
    {
        let smmu = make_smmu();
        let stream_id = sid(30);
        let mut cfg = StreamConfig::stage1_only();
        cfg.priv_cfg = 3; // Force Privileged
        smmu.configure_stream(stream_id, cfg).unwrap();
        smmu.create_pasid(stream_id, pasid(0)).unwrap();
        // Map a read+execute page (accessible, non-privileged-only).
        smmu.map_page(
            stream_id, pasid(0), iova(0x8000), pa(0x9000),
            PagePermissions::read_execute(),
            SecurityState::NonSecure,
        ).unwrap();

        // Translation should succeed.
        let result = smmu.translate(
            stream_id, pasid(0), iova(0x8000), AccessType::ReadExecute,
            SecurityState::NonSecure,
        );
        assert!(
            result.is_ok(),
            "BUG-RUST-5: PRIVCFG=3 + ReadExecute on accessible page must succeed. \
             Got {result:?}"
        );
    }

    // Test that PRIVCFG=3 makes the effective access type privileged:
    // map a privileged-only execute+read page. With PRIVCFG=3 and ReadExecute,
    // effective_priv_suppresses_check()=true → privileged_only check passes.
    // Without fix: the access type passed is ReadExecute (not promoted), but
    // effective_priv_suppresses_check() still returns true for PRIVCFG=3,
    // so the privileged_only page check passes regardless. The remaining
    // observable failure is via is_access_effectively_privileged() returning false.
    //
    // We verify the promotion through the event pnu field when a fault occurs:
    // Map a write-only page (no R/X), then access with ReadExecute + PRIVCFG=3.
    // The fault event's pnu should reflect that the access is privileged.
    {
        let smmu = make_smmu();
        let stream_id = sid(31);
        let mut cfg = StreamConfig::stage1_only();
        cfg.priv_cfg = 3; // Force Privileged
        smmu.configure_stream(stream_id, cfg).unwrap();
        smmu.create_pasid(stream_id, pasid(0)).unwrap();
        // Map a write-only page — ReadExecute will fail with PermissionViolation.
        smmu.map_page(
            stream_id, pasid(0), iova(0x8000), pa(0x9000),
            PagePermissions::write_only(),
            SecurityState::NonSecure,
        ).unwrap();

        let result = smmu.translate(
            stream_id, pasid(0), iova(0x8000), AccessType::ReadExecute,
            SecurityState::NonSecure,
        );
        assert!(result.is_err(), "Expected PermissionViolation on write-only page");

        // BUG-RUST-5: With PRIVCFG=3, the promoted access type is privileged.
        // The event pnu field must be true (privilege bit set in effective access type).
        let events = smmu.get_events();
        let ev = events.iter().rev()
            .find(|e| e.stream_id == stream_id.as_u32())
            .expect("Expected a fault event");
        assert!(
            ev.pnu,
            "BUG-RUST-5: PRIVCFG=3 must make ReadExecute privileged (pnu=true). \
             Got pnu=false. Event: {ev:?}"
        );
    }
}

/// BUG-RUST-5 test 2: PRIVCFG=3 must make WriteExecute privileged (pnu=true in events).
#[test]
fn bug_rust5_privcfg3_promotes_write_execute() {
    let smmu = make_smmu();
    let stream_id = sid(32);
    let mut cfg = StreamConfig::stage1_only();
    cfg.priv_cfg = 3; // Force Privileged
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // Map a read-only page — WriteExecute fails → fault → check pnu
    smmu.map_page(
        stream_id, pasid(0), iova(0xA000), pa(0xB000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    ).unwrap();

    let result = smmu.translate(
        stream_id, pasid(0), iova(0xA000), AccessType::WriteExecute,
        SecurityState::NonSecure,
    );
    assert!(result.is_err(), "Expected PermissionViolation");

    let events = smmu.get_events();
    let ev = events.iter().rev()
        .find(|e| e.stream_id == stream_id.as_u32())
        .expect("Expected a fault event");
    assert!(
        ev.pnu,
        "BUG-RUST-5: PRIVCFG=3 must make WriteExecute privileged (pnu=true). \
         Got pnu=false. Event: {ev:?}"
    );
}

/// BUG-RUST-5 test 3: PRIVCFG=3 must make ReadWriteExecute privileged (pnu=true in events).
#[test]
fn bug_rust5_privcfg3_promotes_read_write_execute() {
    let smmu = make_smmu();
    let stream_id = sid(33);
    let mut cfg = StreamConfig::stage1_only();
    cfg.priv_cfg = 3; // Force Privileged
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // Map an execute-only page — ReadWriteExecute fails → fault → check pnu
    smmu.map_page(
        stream_id, pasid(0), iova(0xC000), pa(0xD000),
        PagePermissions::execute_only(),
        SecurityState::NonSecure,
    ).unwrap();

    let result = smmu.translate(
        stream_id, pasid(0), iova(0xC000), AccessType::ReadWriteExecute,
        SecurityState::NonSecure,
    );
    assert!(result.is_err(), "Expected PermissionViolation");

    let events = smmu.get_events();
    let ev = events.iter().rev()
        .find(|e| e.stream_id == stream_id.as_u32())
        .expect("Expected a fault event");
    assert!(
        ev.pnu,
        "BUG-RUST-5: PRIVCFG=3 must make ReadWriteExecute privileged (pnu=true). \
         Got pnu=false. Event: {ev:?}"
    );
}
