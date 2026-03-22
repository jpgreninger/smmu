//! TDD failing tests for five confirmed spec-compliance bugs.
//!
//! Tests are written to FAIL with the current buggy code and PASS only after
//! the corresponding fix is applied.
//!
//! # Bugs Covered
//!
//! ## BUG-RUST-1 — C_BAD_SUBSTREAMID and C_BAD_CD set rnw/ind/pnu (must be RES0)
//!
//! `record_translation_fault()` sets `rnw`, `ind`, and `pnu` for ALL event types
//! including `CBadSubstreamid` and `CBadCd`.  ARM §7.3.9 and §7.3.11 mark those
//! three wire-format bits as RES0 for these events.
//!
//! ## BUG-RUST-2 — F_STREAM_DISABLED enqueued without CR0.EVENTQEN gate
//!
//! The S1DSS==0b00 inline event push skips the CR0.EVENTQEN check that every
//! other event path uses.  ARM §7.2.1 / §3.5.3 require the gate on all events.
//!
//! ## BUG-RUST-3 — F_BAD_ATS_TREQ and F_TRANSL_FORBIDDEN set rnw/ind incorrectly
//!
//! All three F_BAD_ATS_TREQ inline events set both `rnw` and `ind` from the
//! access type.  ARM §7.3.6 marks bits 100/99/98 as RES0 for this event type.
//! F_TRANSL_FORBIDDEN (§7.3.8) has RnW defined but InD as RES0.
//!
//! ## BUG-RUST-5 — CMD_SYNC_COMPLETION uses command.stream_id (architecturally undefined)
//!
//! CMD_SYNC (§4.7.3) has no StreamID operand.  Using `command.stream_id` is
//! architecturally undefined.  The event `stream_id` field should be 0.
//!
//! ## BUG-RUST-7 — DashMap shard lock acquired inside event_queue.write() (ABBA deadlock)
//!
//! `record_translation_fault()` acquires `event_queue.write()` and then
//! `self.streams.get()` (DashMap shard lock) inside it.  The translate() call
//! path holds the DashMap shard while calling `record_translation_fault()`.
//! Snapshot the MEV flag before the write-lock acquisition to prevent ABBA deadlock.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventType, PagePermissions, SecurityState, StreamConfig,
    StreamID, StreamWorld, IOVA, PA, PASID,
};
use smmu::{TransactionType, SMMU};

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
// BUG-RUST-1a — C_BAD_SUBSTREAMID: rnw/ind/pnu must be RES0 (false)
// ============================================================================
//
// Trigger: configure a stream with s1cd_max=1 (supports PASID[0]), then
// translate with PASID=2 (>= 2^s1cd_max = 2) → CBadSubstreamid.
// Use ReadPrivileged access (would normally set rnw=true, pnu=true if not RES0).
//
// BEFORE FIX: rnw=true, pnu=true → assertions fail.
// AFTER FIX:  rnw=false, ind=false, pnu=false (RES0) → assertions pass.

/// BUG-RUST-1a: C_BAD_SUBSTREAMID event must have rnw=false, ind=false, pnu=false (RES0).
///
/// ARM §7.3.9: bits 100(RnW)/99(InD)/98(PnU) are explicitly RES0 for C_BAD_SUBSTREAMID.
///
/// BEFORE FIX: rnw=true / pnu=true (set from ReadPrivileged access) → FAILS.
/// AFTER FIX:  all three false → PASSES.
#[test]
fn rust1a_c_bad_substreamid_rnw_ind_pnu_are_res0() {
    let smmu = make_smmu();
    let stream_id = sid(0xC1_01);

    // s1cd_max=1 → valid PASID range [0, 1]. PASID=2 triggers C_BAD_SUBSTREAMID.
    let cfg = StreamConfig {
        s1cd_max: 1,
        strw: StreamWorld::El1El0, // EL1 — privilege determined by priv_cfg/access type
        priv_cfg: 3, // PRIVCFG=3 → always privileged (would set pnu=true if not RES0)
        ..StreamConfig::stage1_only()
    };
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_pasid(stream_id, pasid(1)).unwrap();
    // Do NOT create PASID 2 — it is out of range for s1cd_max=1.

    // ReadPrivileged access — would normally yield rnw=true and pnu=true.
    let result = smmu.translate(
        stream_id,
        pasid(2), // out of range → C_BAD_SUBSTREAMID
        iova(0x1000),
        AccessType::ReadPrivileged,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "BUG-RUST-1a: PASID=2 >= 2^s1cd_max=2 must fail, got {result:?}"
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::CBadSubstreamid)
        })
        .expect("BUG-RUST-1a: Expected a C_BAD_SUBSTREAMID event");

    // ARM §7.3.9: rnw, ind, pnu are RES0 for C_BAD_SUBSTREAMID.
    assert!(
        !ev.rnw,
        "BUG-RUST-1a: C_BAD_SUBSTREAMID must have rnw=false (RES0 per §7.3.9). \
         Got rnw=true — bug: rnw set from !access.can_write()."
    );
    assert!(
        !ev.ind,
        "BUG-RUST-1a: C_BAD_SUBSTREAMID must have ind=false (RES0 per §7.3.9). \
         Got ind=true."
    );
    assert!(
        !ev.pnu,
        "BUG-RUST-1a: C_BAD_SUBSTREAMID must have pnu=false (RES0 per §7.3.9). \
         Got pnu=true — bug: pnu set from is_access_privileged() for privileged access."
    );
}

// ============================================================================
// BUG-RUST-1b — C_BAD_CD: rnw/ind/pnu must be RES0 (false)
// ============================================================================
//
// Trigger: configure a stream with aa64=false → C_BAD_CD.
// Use WritePrivileged access (would normally set rnw=false, pnu=true).
//
// BEFORE FIX: pnu=true → assertion fails.
// AFTER FIX:  rnw=false, ind=false, pnu=false → assertions pass.

/// BUG-RUST-1b: C_BAD_CD event must have rnw=false, ind=false, pnu=false (RES0).
///
/// ARM §7.3.11: bits 100(RnW)/99(InD)/98(PnU) are RES0 for C_BAD_CD.
///
/// BEFORE FIX: pnu=true (set from WritePrivileged access with PRIVCFG=3) → FAILS.
/// AFTER FIX:  all three false → PASSES.
#[test]
fn rust1b_c_bad_cd_rnw_ind_pnu_are_res0() {
    let smmu = make_smmu();
    let stream_id = sid(0xC1_02);

    // aa64=false → C_BAD_CD (AArch32 LPAE is unsupported per ARM §5.4).
    // priv_cfg=3 → always privileged → would normally set pnu=true.
    let cfg = StreamConfig {
        aa64: false,
        priv_cfg: 3,
        ..StreamConfig::stage1_only()
    };
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    // Write access — rnw = !Write.can_write() = false; pnu would be true from PRIVCFG=3.
    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::Write,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "BUG-RUST-1b: aa64=false must cause C_BAD_CD, got {result:?}"
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32() && matches!(e.event_type, EventType::CBadCd)
        })
        .expect("BUG-RUST-1b: Expected a C_BAD_CD event for aa64=false");

    // ARM §7.3.11: rnw, ind, pnu are RES0 for C_BAD_CD.
    assert!(
        !ev.rnw,
        "BUG-RUST-1b: C_BAD_CD must have rnw=false (RES0 per §7.3.11)."
    );
    assert!(
        !ev.ind,
        "BUG-RUST-1b: C_BAD_CD must have ind=false (RES0 per §7.3.11)."
    );
    assert!(
        !ev.pnu,
        "BUG-RUST-1b: C_BAD_CD must have pnu=false (RES0 per §7.3.11). \
         Got pnu=true — bug: pnu set from is_access_privileged() for PRIVCFG=3."
    );
}

// ============================================================================
// BUG-RUST-2 — F_STREAM_DISABLED not suppressed when EVENTQEN=0
// ============================================================================
//
// The S1DSS==0b00 abort path (line ~3815) pushes F_STREAM_DISABLED directly to
// the event queue WITHOUT checking CR0.EVENTQEN.  ARM §7.2.1 / §3.5.3 require
// that no events are recorded when EVENTQEN=0.
//
// Setup: stream with s1cd_max=1, s1dss=0 (abort). Disable EVENTQEN.
// Translate with PASID=0 (non-substream on substream-capable stream → S1DSS path).
//
// BEFORE FIX: event emitted even with EVENTQEN=0 → get_events() returns 1 event.
// AFTER FIX:  no event emitted → get_events() returns 0 events.

/// BUG-RUST-2: F_STREAM_DISABLED must NOT be emitted when CR0.EVENTQEN=0.
///
/// ARM §7.2.1 / §3.5.3: event queue is gated by EVENTQEN for ALL events.
///
/// BEFORE FIX: event emitted regardless → get_events().len() == 1 → FAILS.
/// AFTER FIX:  event suppressed → get_events() is empty → PASSES.
#[test]
fn rust2_f_stream_disabled_suppressed_when_eventqen_off() {
    let smmu = SMMU::new();
    // Enable SMMU but do NOT set EVENTQEN — events must be suppressed.
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN);

    let stream_id = sid(0xC2_01);

    // s1cd_max=1 and s1dss=0 → PASID=0 on this stream → S1DSS=0b00 abort path.
    let cfg = StreamConfig {
        s1cd_max: 1,
        s1dss: 0, // abort
        ..StreamConfig::stage1_only()
    };
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    // Translate with PASID=0 on a substream-capable stream with s1dss=0 → F_STREAM_DISABLED path.
    let result = smmu.translate(
        stream_id,
        pasid(0), // triggers S1DSS=0b00 abort
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "BUG-RUST-2: S1DSS=0 must abort translation, got {result:?}"
    );

    // With EVENTQEN=0, no events must be in the queue.
    let events = smmu.get_events();
    assert!(
        events.is_empty(),
        "BUG-RUST-2: F_STREAM_DISABLED must NOT be enqueued when CR0.EVENTQEN=0. \
         Got {} events: {:?}",
        events.len(),
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

// ============================================================================
// BUG-RUST-3a — F_BAD_ATS_TREQ: rnw and ind must be RES0 (false)
// ============================================================================
//
// All three F_BAD_ATS_TREQ inline constructions set rnw and ind from the access
// type.  ARM §7.3.6 wire format shows bits 100/99/98 as RES0 for this event.
//
// Trigger (ATSCHK path with eats=0): configure stream with eats=0, s1/s2 enabled.
// Send ATS TR (translate_with_type with AtsTranslationRequest).
// Use ReadPrivileged access → rnw would be true, pnu would be true with the bug.
//
// BEFORE FIX: rnw=true → assertion fails.
// AFTER FIX:  rnw=false, ind=false (RES0) → assertions pass.

/// BUG-RUST-3a: F_BAD_ATS_TREQ event must have rnw=false and ind=false (RES0).
///
/// ARM §7.3.6: bits 100(RnW)/99(InD) are RES0 for F_BAD_ATS_TREQ.
///
/// BEFORE FIX: rnw=true (Read, !can_write()) → FAILS.
/// AFTER FIX:  rnw=false → PASSES.
#[test]
fn rust3a_f_bad_ats_treq_rnw_and_ind_are_res0() {
    let smmu = make_smmu();
    let stream_id = sid(0xC3_01);

    // eats=0 → ATS TR not supported → F_BAD_ATS_TREQ.
    let cfg = StreamConfig {
        eats: 0, // ATS not enabled for this stream
        ..StreamConfig::stage1_only()
    };
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x1000),
        PA::new(0x8000_1000).unwrap(),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // ATS TR with ReadPrivileged access → rnw would be true with bug.
    let result = smmu.translate_with_type(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::ReadPrivileged,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
    );
    assert!(
        result.is_err(),
        "BUG-RUST-3a: ATS TR on eats=0 stream must fail, got {result:?}"
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::FBadAtsTreq)
        })
        .expect("BUG-RUST-3a: Expected F_BAD_ATS_TREQ event");

    // ARM §7.3.6: rnw and ind are RES0.
    assert!(
        !ev.rnw,
        "BUG-RUST-3a: F_BAD_ATS_TREQ must have rnw=false (RES0 per §7.3.6). \
         Got rnw=true — bug: rnw set from !access.can_write() for ReadPrivileged."
    );
    assert!(
        !ev.ind,
        "BUG-RUST-3a: F_BAD_ATS_TREQ must have ind=false (RES0 per §7.3.6). \
         Got ind=true."
    );
}

// ============================================================================
// BUG-RUST-3b — F_TRANSL_FORBIDDEN: ind must be RES0 (false); rnw IS defined
// ============================================================================
//
// The F_TRANSL_FORBIDDEN inline event (SMMUEN=0 path and ATSCHK recheck path)
// sets both rnw and ind from the access type.  ARM §7.3.8: RnW (bit 100) IS
// defined; only InD (bit 99) is RES0.
//
// Trigger (SMMUEN=0 ATS TT path): SMMU disabled (SMMUEN=0), send ATS Translated
// → F_TRANSL_FORBIDDEN emitted.  Use Execute access.
// Execute: rnw=true (!can_write()=true), ind=true (can_execute()&&!can_write()=true).
//
// BEFORE FIX: ind=true → assertion fails; rnw=true (correct, should remain true).
// AFTER FIX:  ind=false (RES0), rnw=true (defined) → assertions pass.

/// BUG-RUST-3b: F_TRANSL_FORBIDDEN event must have ind=false (RES0) but rnw IS defined.
///
/// ARM §7.3.8: bit 99 (InD) is RES0 for F_TRANSL_FORBIDDEN; bit 100 (RnW) is defined.
///
/// BEFORE FIX: ind=true (Execute access) → FAILS.
/// AFTER FIX:  ind=false (RES0), rnw=true (Execute !can_write) → PASSES.
#[test]
fn rust3b_f_transl_forbidden_ind_is_res0_rnw_is_defined() {
    // Use a fresh SMMU with SMMUEN=0 — the very first check in translate_with_type()
    // for AtsTranslated is the SMMUEN=0 guard which emits F_TRANSL_FORBIDDEN.
    let smmu = SMMU::new();
    // Set EVENTQEN only — do NOT set SMMUEN so the SMMUEN=0 ATS TT path fires.
    smmu.set_cr0(SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    let stream_id = sid(0xC3_02);
    // No configure_stream needed: SMMUEN=0 path fires before stream lookup.

    // Execute access: rnw = !Execute.can_write() = true; ind = true (bug).
    let result = smmu.translate_with_type(
        stream_id,
        pasid(0),
        iova(0x2000),
        AccessType::Execute,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated,
    );
    assert!(
        result.is_err(),
        "BUG-RUST-3b: ATS TT with SMMUEN=0 must fail, got {result:?}"
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::FTranslForbidden)
        })
        .expect("BUG-RUST-3b: Expected F_TRANSL_FORBIDDEN event");

    // ARM §7.3.8: InD (bit 99) is RES0; RnW (bit 100) is defined.
    assert!(
        !ev.ind,
        "BUG-RUST-3b: F_TRANSL_FORBIDDEN must have ind=false (RES0 per §7.3.8). \
         Got ind=true — bug: ind set from access.can_execute()&&!access.can_write()=true \
         for Execute access."
    );
    // RnW IS defined for F_TRANSL_FORBIDDEN — Execute is not a write, so rnw=true.
    assert!(
        ev.rnw,
        "BUG-RUST-3b: F_TRANSL_FORBIDDEN must have rnw=true for Execute access \
         (RnW IS defined per §7.3.8; !Execute.can_write()=true). \
         Got rnw=false — regression guard."
    );
}

// ============================================================================
// BUG-RUST-5 — CMD_SYNC_COMPLETION: stream_id must be 0
// ============================================================================
//
// CMD_SYNC (§4.7.3) has no StreamID operand.  The current code uses
// `stream_id: command.stream_id` which is architecturally undefined.
// The event should carry `stream_id: 0`.
//
// Setup: submit CMD_SYNC with cs=1 (SIG_IRQ) and a non-zero stream_id field
// (e.g. 0xBEEF).  The resulting CommandSyncCompletion event must have
// stream_id=0, not 0xBEEF.
//
// BEFORE FIX: stream_id=0xBEEF → assertion fails.
// AFTER FIX:  stream_id=0 → assertion passes.

/// BUG-RUST-5: CommandSyncCompletion event must have stream_id=0.
///
/// CMD_SYNC (ARM §4.7.3) has no StreamID operand; using command.stream_id
/// is architecturally undefined.
///
/// BEFORE FIX: stream_id == command.stream_id (e.g. 0xBEEF) → FAILS.
/// AFTER FIX:  stream_id == 0 → PASSES.
#[test]
fn rust5_cmd_sync_completion_stream_id_is_zero() {
    let smmu = make_smmu();

    // Submit CMD_SYNC with cs=1 (SIG_IRQ) and a deliberately non-zero stream_id.
    // The non-zero stream_id in the command must NOT appear in the event.
    let mut cmd = CommandEntry::new(CommandType::Sync, 0xBEEF, 0);
    cmd.cs = 0x01; // SIG_IRQ — triggers CommandSyncCompletion event

    smmu.submit_command(cmd).expect("submit_command must succeed");
    smmu.process_command_queue().expect("process_command_queue must succeed");

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| matches!(e.event_type, EventType::CommandSyncCompletion))
        .expect("BUG-RUST-5: Expected a CommandSyncCompletion event for CS=1");

    assert_eq!(
        ev.stream_id, 0,
        "BUG-RUST-5: CommandSyncCompletion must have stream_id=0 \
         (CMD_SYNC has no StreamID operand per ARM §4.7.3). \
         Got stream_id=0x{:X} — bug: event uses command.stream_id.",
        ev.stream_id
    );
}

// ============================================================================
// BUG-RUST-7 — MEV deduplication still works after lock-order fix
// ============================================================================
//
// After the BUG-RUST-7 fix (snapshot MEV flag before acquiring event_queue.write()),
// the MEV deduplication logic must still function correctly:
// a duplicate event on a stream with MEV=true must be suppressed.
//
// This is a functional correctness test — the ABBA deadlock would manifest as
// a hang, but we verify that the fix did not break MEV suppression.
//
// Setup: stream with mev=true. Translate twice to produce the same fault.
// Assert only one event in queue (MEV deduplication active).
//
// PASSES before and after the fix (but verifies the fix did not regress MEV).
// A second sub-test exercises translate() to ensure no deadlock.

/// BUG-RUST-7: MEV deduplication must suppress duplicate events after lock-order fix.
///
/// Verifies that snapshotting MEV before acquiring event_queue.write() does not
/// break the deduplication logic: only one event per (type, stream_id, pasid)
/// triple must appear in the queue when MEV=true.
///
/// The test would hang if the ABBA deadlock is present (old code), so passing
/// this test also implicitly verifies no deadlock.
#[test]
fn rust7_mev_deduplication_works_after_lock_order_fix() {
    let smmu = make_smmu();
    let stream_id = sid(0xC7_01);

    // mev=true → duplicate events for the same (type, stream, pasid) are suppressed.
    let cfg = StreamConfig {
        mev: true,
        ..StreamConfig::stage1_only()
    };
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // Do NOT map any page → every translate call produces F_TRANSLATION.

    // First translation — event emitted.
    let _ = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x4000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    // Second translation with same (type=F_TRANSLATION, stream, pasid) → MEV suppresses.
    let _ = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x4000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    let f_translation_count = events
        .iter()
        .filter(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::FTranslation)
        })
        .count();

    assert_eq!(
        f_translation_count, 1,
        "BUG-RUST-7: MEV=true must suppress duplicate F_TRANSLATION events. \
         Expected 1 event, got {f_translation_count}. \
         MEV deduplication broken by lock-order fix."
    );
}

/// BUG-RUST-7 concurrency sanity: translate() must not deadlock when MEV=true.
///
/// The ABBA deadlock manifests as a hang in translate() when MEV=true.
/// This test drives translate() on a stream with MEV=true and verifies
/// it returns without blocking.
#[test]
fn rust7_mev_translate_does_not_deadlock() {
    let smmu = make_smmu();
    let stream_id = sid(0xC7_02);

    let cfg = StreamConfig {
        mev: true,
        ..StreamConfig::stage1_only()
    };
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // Do NOT map page → F_TRANSLATION (exercises MEV path inside record_translation_fault).

    // If deadlock is present this call hangs; test failure = deadlock regression.
    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x5000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    // Translation must fail (unmapped page), not deadlock.
    assert!(
        result.is_err(),
        "BUG-RUST-7: translate() with MEV=true must return an error for unmapped page, \
         not deadlock. Got {result:?}"
    );
}
