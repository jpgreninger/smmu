//! Regression tests for bugs fixed on 2026-03-21.
//!
//! - BUG-RUST-1: `get_events()` must NOT advance `EVENTQ_CONS` — it is a
//!   non-destructive snapshot.
//! - BUG-RUST-4 (INSTCFG): INSTCFG=2 must demote `ReadExecute → Read` and
//!   `ReadExecutePrivileged → ReadPrivileged`, not only `Execute → Read`.
//! - BUG-RUST-4 (STRW) + BUG-RUST-7: STRW=El2/El3 must promote the incoming
//!   access type to its privileged variant before INSTCFG/PRIVCFG.
//!   `WriteExecute → WriteExecutePrivileged` must be included.
//! - BUG-RUST-8: `clear_all_pasids()` must also clear `stage2_address_space`.
//! - BUG-RUST-3: `toggle_ovflg_once()` must be bounded (max-retry loop).
//! - BUG-RUST-6: `get_events()` should release write lock before read snapshot.
//!
//! Each test is designed to FAIL before the corresponding fix is applied.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

use smmu::types::{
    AccessType, EventEntry, EventType, IOVA, PagePermissions, PASID, QueueConfig, SMMUConfig,
    SecurityState, StreamConfig, StreamID, StreamWorld, PA,
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

/// Configure a basic stage-1-only stream with no VA range restriction and
/// PASID 0 ready to use.  IOVA 0x1000 is left **unmapped** so that any
/// translation to it produces an F_TRANSLATION fault.
fn setup_fault_stream(smmu: &SMMU, stream_id: StreamID) {
    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // deliberately no map_page — fault on translate
}

// ============================================================================
// BUG-RUST-1: get_events() must NOT advance EVENTQ_CONS
// ============================================================================

/// After triggering one fault, `get_events()` must return the event but leave
/// `EVENTQ_CONS` unchanged.  A second call to `get_events()` must return the
/// same single event again.
///
/// Before the fix, `get_events()` advances CONS for each returned event,
/// causing `eventq_cons_index()` to increase and the second call to observe
/// a mismatched PROD/CONS that could lose events.
#[test]
fn bug_rust1_get_events_does_not_advance_cons() {
    let smmu = make_smmu();
    let stream_id = sid(1);
    setup_fault_stream(&smmu, stream_id);

    // Record CONS before any translation.
    let cons_initial = smmu.eventq_cons_index();

    // Trigger a fault — this should enqueue one event.
    let _ = smmu.translate(stream_id, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);

    // PROD must have advanced (one event enqueued).
    assert!(
        smmu.get_event_queue_size() > 0,
        "expected at least one event after fault"
    );

    // Read CONS before first get_events().
    let cons_before = smmu.eventq_cons_index();

    // Call get_events() — must return the event.
    let events1 = smmu.get_events();
    assert_eq!(events1.len(), 1, "first get_events() must return 1 event");

    // BUG-RUST-1 assertion: CONS must NOT have changed.
    let cons_after_first = smmu.eventq_cons_index();
    assert_eq!(
        cons_before, cons_after_first,
        "BUG-RUST-1: get_events() must not advance EVENTQ_CONS (was {cons_before}, \
         now {cons_after_first})"
    );

    // CONS should also equal its initial value (no advancement at all).
    assert_eq!(
        cons_initial, cons_after_first,
        "EVENTQ_CONS must remain at initial value {cons_initial}"
    );

    // Second call must still return the same event.
    let events2 = smmu.get_events();
    assert_eq!(
        events2.len(),
        1,
        "second get_events() must still return 1 event (snapshot is non-destructive)"
    );

    // CONS still must not have changed.
    let cons_after_second = smmu.eventq_cons_index();
    assert_eq!(
        cons_before, cons_after_second,
        "BUG-RUST-1: EVENTQ_CONS must not advance after second get_events()"
    );
}

/// Enqueue multiple events and confirm that multiple get_events() calls all
/// return the full set without CONS advancing.
#[test]
fn bug_rust1_multiple_events_cons_stable() {
    let smmu = make_smmu();

    for id in 1u32..=3 {
        let stream_id = sid(id);
        setup_fault_stream(&smmu, stream_id);
        let _ = smmu.translate(stream_id, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    }

    assert_eq!(smmu.get_event_queue_size(), 3, "expected 3 events");

    let cons_before = smmu.eventq_cons_index();

    for call in 0..3 {
        let events = smmu.get_events();
        assert_eq!(
            events.len(),
            3,
            "call {call}: expected 3 events from snapshot"
        );
        let cons_now = smmu.eventq_cons_index();
        assert_eq!(
            cons_before, cons_now,
            "call {call}: CONS must not change (was {cons_before}, now {cons_now})"
        );
    }
}

// ============================================================================
// BUG-RUST-4 (INSTCFG): ReadExecute / ReadExecutePrivileged demotion
// ============================================================================

/// INSTCFG=2 (Force-Data) must demote `ReadExecute → Read`.
/// A page with read-only permission (no execute) must succeed with
/// `ReadExecute` + INSTCFG=2 because the execute component is stripped.
/// Before the fix the `ReadExecute` arm is absent so it passes through
/// unchanged and the execute permission check fails.
#[test]
fn bug_rust4_instcfg2_read_execute_demoted_to_read() {
    let smmu = make_smmu();
    let stream_id = sid(10);

    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz = 0;
    cfg.inst_cfg = 2; // Force-Data
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    // Map the page read-only (no execute permission).
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x2000),
        pa(0x3000),
        PagePermissions::read_only(), // no execute
        SecurityState::NonSecure,
    )
    .unwrap();

    // With INSTCFG=2, ReadExecute → Read; page has read perm → MUST succeed.
    let result = smmu.translate(stream_id, pasid(0), iova(0x2000), AccessType::ReadExecute, SecurityState::NonSecure);
    assert!(
        result.is_ok(),
        "BUG-RUST-4 INSTCFG: ReadExecute with INSTCFG=2 must succeed on read-only page \
         (demoted to Read), got {result:?}"
    );
}

/// INSTCFG=2 (Force-Data) must demote `ReadExecutePrivileged → ReadPrivileged`.
/// A page that is privileged-only read (no execute) with STRW=El2 (suppresses
/// privileged_only) must succeed.
#[test]
fn bug_rust4_instcfg2_read_execute_privileged_demoted_to_read_privileged() {
    let smmu = make_smmu();
    let stream_id = sid(11);

    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz = 0;
    cfg.inst_cfg = 2; // Force-Data
    cfg.strw = StreamWorld::El2; // suppress privileged_only check
    // STRW=El2 with NonSecure is rejected (BUG-RUST-2a fix, ARM §5.2 bit[0]=1 rule).
    cfg.security_state = SecurityState::Secure;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    // Map page with read + privileged_only (no execute).
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x2000),
        pa(0x3000),
        PagePermissions::read_only_privileged(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // ReadExecutePrivileged with INSTCFG=2 → ReadPrivileged.
    // STRW=El2 promotes incoming (so ReadExecutePrivileged stays privileged from STRW),
    // then INSTCFG=2 must strip the execute component → ReadPrivileged.
    // privileged_only is suppressed by El2 → success.
    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x2000),
        AccessType::ReadExecutePrivileged,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_ok(),
        "BUG-RUST-4 INSTCFG: ReadExecutePrivileged with INSTCFG=2 must succeed on \
         privileged-read-only page (demoted to ReadPrivileged), got {result:?}"
    );
}

/// INSTCFG=2 must demote ReadExecute on a two-stage (translate_and_get_stage2_ipa) path too.
/// Set up a two-stage stream: stage-1 maps IOVA→IPA, stage-2 maps IPA→PA with read-only perms.
/// ReadExecute + INSTCFG=2 must succeed (demoted to Read).
#[test]
fn bug_rust4_instcfg2_read_execute_two_stage_path() {
    let smmu = make_smmu();
    let stream_id = sid(12);

    let mut cfg = StreamConfig::two_stage();
    cfg.t0sz = 0;
    cfg.s2_t0sz = 25; // valid S2T0SZ for 4KB/SL0=1 per §5.2 (BUG-AUDIT-88: min=22 for SL0=1)
    cfg.inst_cfg = 2; // Force-Data
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();

    // Stage-1: IOVA 0x4000 → IPA 0x4000
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x4000),
        pa(0x4000), // IPA
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Stage-2: IPA 0x4000 → PA 0x5000 (read-only, no execute)
    smmu.map_stage2_page(
        stream_id,
        iova(0x4000),
        pa(0x5000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // With INSTCFG=2, ReadExecute → Read. Both stages have read perm → success.
    let result = smmu.translate(stream_id, pasid(0), iova(0x4000), AccessType::ReadExecute, SecurityState::NonSecure);
    assert!(
        result.is_ok(),
        "BUG-RUST-4 INSTCFG two-stage: ReadExecute with INSTCFG=2 must succeed on \
         read-only pages, got {result:?}"
    );
}

// ============================================================================
// BUG-RUST-4 (STRW) + BUG-RUST-7: STRW promotion before INSTCFG/PRIVCFG
// ============================================================================

/// STRW=El2 must promote `WriteExecute → WriteExecutePrivileged` before the
/// permission check.  A privileged-only page with write+execute must succeed.
///
/// Before the fix, `WriteExecute` is never promoted, so the `privileged_only`
/// check fails even with STRW=El2.
#[test]
fn bug_rust7_strw_write_execute_promoted_to_privileged() {
    let smmu = make_smmu();
    let stream_id = sid(20);

    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz = 0;
    cfg.strw = StreamWorld::El2;
    // STRW=El2 with NonSecure is rejected (BUG-RUST-2a fix, ARM §5.2 bit[0]=1 rule).
    cfg.security_state = SecurityState::Secure;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    // Map a page that is write+execute + privileged-only.
    let perms = PagePermissions::new(false, true, true).with_privileged_only(true);
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x4000),
        pa(0x5000),
        perms,
        SecurityState::NonSecure,
    )
    .unwrap();

    // With STRW=El2, WriteExecute must be promoted to WriteExecutePrivileged,
    // satisfying the privileged_only constraint.
    let result = smmu.translate(stream_id, pasid(0), iova(0x4000), AccessType::WriteExecute, SecurityState::NonSecure);
    assert!(
        result.is_ok(),
        "BUG-RUST-7: WriteExecute with STRW=El2 must succeed on privileged-only \
         write+execute page (promoted to WriteExecutePrivileged), got {result:?}"
    );
}

/// STRW=El2 must promote `Read → ReadPrivileged` to satisfy `privileged_only`.
/// This validates the basic STRW promotion path for the simple Read case.
#[test]
fn bug_rust4_strw_read_promoted_to_read_privileged() {
    let smmu = make_smmu();
    let stream_id = sid(21);

    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz = 0;
    cfg.strw = StreamWorld::El2;
    // STRW=El2 with NonSecure is rejected (BUG-RUST-2a fix, ARM §5.2 bit[0]=1 rule).
    cfg.security_state = SecurityState::Secure;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    // Privileged-only read page.
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x4000),
        pa(0x5000),
        PagePermissions::read_only_privileged(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Read with STRW=El2 → ReadPrivileged → passes privileged_only check.
    let result = smmu.translate(stream_id, pasid(0), iova(0x4000), AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_ok(),
        "BUG-RUST-4 STRW: Read with STRW=El2 must succeed on privileged-only page \
         (promoted to ReadPrivileged), got {result:?}"
    );
}

/// STRW=El2 must promote access types (same semantics as El3).
/// STRW=EL2 is valid for Secure streams (ARM §5.2).
/// Note: STRW=El3 is now forbidden even for Secure streams per BUG-RUST-2b fix.
#[test]
fn bug_rust4_strw_el3_promotes_write_execute() {
    let smmu = make_smmu();
    let stream_id = sid(22);

    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz = 0;
    cfg.strw = StreamWorld::El2; // El3 is now forbidden; El2 has same promotion semantics
    cfg.security_state = SecurityState::Secure;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    let perms = PagePermissions::new(false, true, true).with_privileged_only(true);
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x6000),
        pa(0x7000),
        perms,
        SecurityState::Secure,
    )
    .unwrap();

    let result = smmu.translate(stream_id, pasid(0), iova(0x6000), AccessType::WriteExecute, SecurityState::Secure);
    assert!(
        result.is_ok(),
        "BUG-RUST-7: WriteExecute with STRW=El2 must succeed on privileged-only page, \
         got {result:?}"
    );
}

/// STRW=El1El0 must NOT promote access types (baseline — no regression).
#[test]
fn bug_rust4_strw_el1el0_does_not_promote() {
    let smmu = make_smmu();
    let stream_id = sid(23);

    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz = 0;
    cfg.strw = StreamWorld::El1El0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    // Privileged-only read page: unprivileged Read must FAIL.
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x8000),
        pa(0x9000),
        PagePermissions::read_only_privileged(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(stream_id, pasid(0), iova(0x8000), AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_err(),
        "STRW=El1El0 must NOT promote Read; should fail on privileged-only page, \
         got {result:?}"
    );
}

/// STRW=El2 + INSTCFG=2: promotion must happen BEFORE INSTCFG (matching C++ order).
/// Write with STRW=El2 → WritePrivileged (promoted).
/// INSTCFG=2 doesn't touch Write variants → WritePrivileged stays.
/// privileged_only satisfied → success.
#[test]
fn bug_rust4_strw_promotion_before_instcfg() {
    let smmu = make_smmu();
    let stream_id = sid(24);

    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz = 0;
    cfg.strw = StreamWorld::El2;
    // STRW=El2 with NonSecure is rejected (BUG-RUST-2a fix, ARM §5.2 bit[0]=1 rule).
    cfg.security_state = SecurityState::Secure;
    cfg.inst_cfg = 2; // Force-Data
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    // Map a write-privileged page (no read/execute needed).
    let perms = PagePermissions::write_only().with_privileged_only(true);
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0xA000),
        pa(0xB000),
        perms,
        SecurityState::NonSecure,
    )
    .unwrap();

    // Write with STRW=El2 → WritePrivileged (promoted).
    // INSTCFG=2 doesn't touch Write variants → WritePrivileged stays.
    // privileged_only satisfied → success.
    let result = smmu.translate(stream_id, pasid(0), iova(0xA000), AccessType::Write, SecurityState::NonSecure);
    assert!(
        result.is_ok(),
        "STRW=El2 + INSTCFG=2: Write must be promoted to WritePrivileged and succeed \
         on privileged-only write page, got {result:?}"
    );
}

// ============================================================================
// BUG-RUST-8: clear_all_pasids() must clear stage2_address_space
// ============================================================================

/// After `clear_all_pasids()`, any previously set stage2 address space must be
/// cleared.  A subsequent two-stage translation (stage2 enabled but no stage2
/// AS set) must fail — not succeed with stale data.
///
/// Before the fix, the stale stage2 address space remains, so translations
/// through it may succeed incorrectly.
#[test]
fn bug_rust8_clear_all_pasids_clears_stage2() {
    use smmu::address_space::AddressSpace;
    use smmu::stream_context::StreamContext;
    use std::sync::Arc;

    let ctx = StreamContext::new();
    ctx.enable();
    ctx.set_stage1_enabled(true);
    ctx.set_stage2_enabled(true);

    // Bring in a real AddressSpace as stage2.
    let s2 = Arc::new(AddressSpace::new());
    // Map IPA 0x1000 → PA 0x2000 in stage2.
    s2.map_page(
        IOVA::new(0x1000).unwrap(),
        PA::new(0x2000).unwrap(),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();
    ctx.set_stage2_address_space(Some(s2));

    // Create PASID 0 with stage1 mapping IOVA 0x1000 → PA 0x1000 (IPA == PA).
    ctx.create_pasid(PASID::new(0).unwrap()).unwrap();
    ctx.map_page(
        PASID::new(0).unwrap(),
        IOVA::new(0x1000).unwrap(),
        PA::new(0x1000).unwrap(),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Sanity: two-stage translation succeeds before clear.
    let before = ctx.translate(
        PASID::new(0).unwrap(),
        IOVA::new(0x1000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(before.is_ok(), "two-stage translate must succeed before clear");

    // Clear all PASIDs — must also clear stage2_address_space.
    ctx.clear_all_pasids().unwrap();

    // Re-create PASID 0 with a stage1 mapping, but do NOT set stage2 AS.
    ctx.create_pasid(PASID::new(0).unwrap()).unwrap();
    ctx.map_page(
        PASID::new(0).unwrap(),
        IOVA::new(0x1000).unwrap(),
        PA::new(0x1000).unwrap(),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Two-stage is still enabled but stage2 AS is now None.
    // Translation must FAIL (no stage2 AS configured after clear).
    let after = ctx.translate(
        PASID::new(0).unwrap(),
        IOVA::new(0x1000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        after.is_err(),
        "BUG-RUST-8: after clear_all_pasids(), two-stage translate must fail \
         (stage2 AS cleared), but got {after:?}"
    );
}

// ============================================================================
// BUG-RUST-3: toggle_ovflg_once() must be bounded (max-retry loop)
// ============================================================================

/// Exercises `toggle_ovflg_once()` indirectly by filling the event queue
/// and triggering an overflow.  Verifies that OVFLG is set and that
/// `acknowledge_eventq_overflow()` clears it, confirming the bounded CAS
/// path works correctly.
#[test]
fn bug_rust3_ovflg_toggle_bounded() {
    // Build SMMU with a 4-slot event queue so we can overflow it quickly.
    let config = SMMUConfig::builder()
        .queue_config(
            QueueConfig::builder()
                .event_queue_size(4)
                .build()
                .unwrap(),
        )
        .build()
        .unwrap();
    let smmu = SMMU::with_config(config);
    smmu.enable().unwrap();

    // Configure 6 separate streams so we can trigger 6 faults.
    for id in 1u32..=6 {
        setup_fault_stream(&smmu, sid(id));
    }

    // Trigger 6 faults: first 4 fill the queue; faults 5 & 6 overflow.
    for id in 1u32..=6 {
        let _ = smmu.translate(sid(id), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    }

    // OVFLG must be active (PROD.bit31 != CONS.bit31).
    let prod = smmu.get_eventq_prod();
    let cons = smmu.eventq_cons_index();
    let ovflg    = (prod >> 31) & 1;
    let ovackflg = (cons >> 31) & 1;
    assert_ne!(
        ovflg, ovackflg,
        "BUG-RUST-3: OVFLG must be active after queue overflow \
         (PROD={prod:#010x}, CONS={cons:#010x})"
    );

    // Acknowledge the overflow — clears OVFLG condition.
    smmu.acknowledge_eventq_overflow();
    let prod_after = smmu.get_eventq_prod();
    let cons_after = smmu.eventq_cons_index();
    let ovflg_after    = (prod_after >> 31) & 1;
    let ovackflg_after = (cons_after >> 31) & 1;
    assert_eq!(
        ovflg_after, ovackflg_after,
        "OVFLG must be cleared after acknowledge_eventq_overflow()"
    );
}

// ============================================================================
// BUG-RUST-6: get_events() lock scope (nice-to-have — behaviour test only)
// ============================================================================

/// Verifies that `get_events()` correctly returns a snapshot after the stall-
/// pending drain.  This is a behavioural test: if the lock refactoring is
/// correct, the result is the same as before; the test catches any regression
/// where the split lock acquisition returns fewer events.
#[test]
fn bug_rust6_get_events_returns_full_snapshot_after_drain() {
    let smmu = make_smmu();

    // Enqueue 3 events directly via submit_event so we control the queue.
    for i in 0u32..3 {
        let ev = EventEntry {
            event_type: EventType::FTranslation,
            stream_id: i,
            ..EventEntry::zeroed()
        };
        smmu.submit_event(ev).unwrap();
    }

    assert_eq!(smmu.get_event_queue_size(), 3);

    let events = smmu.get_events();
    assert_eq!(
        events.len(),
        3,
        "get_events() must return all 3 events in the snapshot"
    );

    // Queue size must be unchanged (non-destructive snapshot).
    assert_eq!(
        smmu.get_event_queue_size(),
        3,
        "queue size must be unchanged after get_events()"
    );
}
