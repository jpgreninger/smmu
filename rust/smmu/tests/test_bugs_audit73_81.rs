//! TDD tests for AUDIT-73 through AUDIT-81.
//!
//! Each section documents the bug, the expected failure before the fix,
//! and the expected pass after the fix.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventEntry, EventType, PagePermissions, PRIEntry,
    SMMUConfig, SecurityState, StreamConfig, StreamID, StreamWorld, TransactionType, IOVA, PA,
    PASID,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

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

// ============================================================================
// AUDIT-73 — set_cr0() must clear PRIQEN when PRI is not supported
// ============================================================================

/// AUDIT-73 test: set_cr0(SMMUEN|PRIQEN) on an SMMU with pri_supported=false
/// must silently discard the PRIQEN bit from both CR0 and CR0ACK.
///
/// BEFORE FIX: get_cr0() and get_cr0ack() both return a value with PRIQEN set.
/// AFTER FIX:  PRIQEN is cleared from both registers.
#[test]
fn audit_73_set_cr0_clears_priqen_when_pri_not_supported() {
    let smmu = SMMU::new();
    smmu.set_pri_supported(false);

    // Write CR0 with both SMMUEN and PRIQEN set.
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_PRIQEN);

    let cr0 = smmu.get_cr0();
    let cr0ack = smmu.get_cr0ack();

    assert_eq!(
        cr0 & SMMU::CR0_PRIQEN,
        0,
        "AUDIT-73: CR0.PRIQEN must be cleared by set_cr0() when PRI is not supported (got cr0=0x{:x})",
        cr0
    );
    assert_eq!(
        cr0ack & SMMU::CR0_PRIQEN,
        0,
        "AUDIT-73: CR0ACK.PRIQEN must be cleared by set_cr0() when PRI is not supported (got cr0ack=0x{:x})",
        cr0ack
    );
    // SMMUEN must still be set.
    assert_ne!(
        cr0 & SMMU::CR0_SMMUEN,
        0,
        "AUDIT-73: CR0.SMMUEN must be preserved after set_cr0() PRI guard"
    );
}

/// Positive control: when pri_supported=true, PRIQEN must be preserved.
#[test]
fn audit_73_set_cr0_preserves_priqen_when_pri_supported() {
    let smmu = SMMU::new();
    smmu.set_pri_supported(true);
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_PRIQEN);

    let cr0 = smmu.get_cr0();
    let cr0ack = smmu.get_cr0ack();

    assert_ne!(
        cr0 & SMMU::CR0_PRIQEN,
        0,
        "AUDIT-73 positive: CR0.PRIQEN must be preserved when PRI is supported"
    );
    assert_ne!(
        cr0ack & SMMU::CR0_PRIQEN,
        0,
        "AUDIT-73 positive: CR0ACK.PRIQEN must be preserved when PRI is supported"
    );
}

// ============================================================================
// AUDIT-74 — reconfigure_stream() must run full SteIllegal() checks
// ============================================================================

/// AUDIT-74 test: reconfigure_stream() with STRW=El3 (0b11) for a NonSecure
/// stage-1 stream must return an error (C_BAD_STE path).
///
/// BEFORE FIX: reconfigure_stream() only calls config.validate() which does
///   not check STRW; the function returns Ok(()) — wrong.
/// AFTER FIX:  The STRW check fires; reconfigure_stream() returns Err.
#[test]
fn audit_74_reconfigure_stream_rejects_illegal_strw_el3_nonsecure() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let stream = sid(1);

    // Configure the stream with a valid config first.
    let valid_config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .strw(StreamWorld::El1El0)
        .security_state(SecurityState::NonSecure)
        .build()
        .unwrap();
    smmu.configure_stream(stream, valid_config).unwrap();

    // Now try to reconfigure with illegal STRW=El3 for a NonSecure stream.
    // El3 is 0b11 — both conditions: NonSecure && bit[0]=1 fires first,
    // but the El3 check also fires for Secure. Use a Secure stream to hit El3.
    let illegal_config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .strw(StreamWorld::El3)
        .security_state(SecurityState::Secure)
        .build()
        .unwrap();
    let result = smmu.reconfigure_stream(stream, illegal_config);
    assert!(
        result.is_err(),
        "AUDIT-74: reconfigure_stream() must reject STRW=El3 for Secure stream, got Ok"
    );
}

/// AUDIT-74 test: reconfigure_stream() with invalid S2TG (non-zero) must return error.
#[test]
fn audit_74_reconfigure_stream_rejects_invalid_s2tg() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let stream = sid(2);

    // Configure the stream with a valid stage-2 config first.
    let valid_config = StreamConfig::builder()
        .translation_enabled(true)
        .stage2_enabled(true)
        .s2_aa64(true)
        .security_state(SecurityState::NonSecure)
        .build()
        .unwrap();
    smmu.configure_stream(stream, valid_config).unwrap();

    // Try to reconfigure with invalid S2TG=1 (64KB not supported by IDR5).
    let invalid_s2tg_config = StreamConfig::builder()
        .translation_enabled(true)
        .stage2_enabled(true)
        .s2_aa64(true)
        .s2_tg(1) // 64KB — not supported
        .security_state(SecurityState::NonSecure)
        .build()
        .unwrap();
    let result = smmu.reconfigure_stream(stream, invalid_s2tg_config);
    assert!(
        result.is_err(),
        "AUDIT-74: reconfigure_stream() must reject invalid S2TG=1 (64KB unsupported), got Ok"
    );
}

/// AUDIT-74 test: reconfigure_stream() with S1P=false must reject stage-1 config.
#[test]
fn audit_74_reconfigure_stream_rejects_s1_when_s1p_false() {
    let smmu = SMMU::new();
    smmu.set_s1p_supported(true);
    smmu.set_s2p_supported(true);
    smmu.enable().unwrap();

    let stream = sid(3);

    // Configure a valid stage-1 stream.
    let valid_config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .strw(StreamWorld::El1El0)
        .security_state(SecurityState::NonSecure)
        .build()
        .unwrap();
    smmu.configure_stream(stream, valid_config).unwrap();

    // Now disable S1P and try to reconfigure with stage-1.
    smmu.set_s1p_supported(false);
    let s1_config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .strw(StreamWorld::El1El0)
        .security_state(SecurityState::NonSecure)
        .build()
        .unwrap();
    let result = smmu.reconfigure_stream(stream, s1_config);
    assert!(
        result.is_err(),
        "AUDIT-74: reconfigure_stream() must reject stage-1 when S1P=false, got Ok"
    );
}

// ============================================================================
// AUDIT-75 — submit_event() must be gated by CR0.EVENTQEN
// ============================================================================

/// AUDIT-75 test: submit_event() called when EVENTQEN=0 must not post the event.
///
/// BEFORE FIX: submit_event() posts without checking EVENTQEN.
/// AFTER FIX:  submit_event() returns early (Ok or Err) without posting
///             when EVENTQEN=0.
#[test]
fn audit_75_submit_event_no_op_when_eventqen_zero() {
    let smmu = SMMU::new();
    // Enable SMMUEN and CMDQEN only — no EVENTQEN.
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN);

    assert_eq!(
        smmu.get_cr0() & SMMU::CR0_EVENTQEN,
        0,
        "AUDIT-75 precondition: EVENTQEN must be 0"
    );

    let event = EventEntry {
        event_type: EventType::CBadSte,
        stream_id: 1,
        security_state: SecurityState::NonSecure,
        timestamp: 0,
        stall: false,
        ..EventEntry::zeroed()
    };

    // Even if submit_event returns Ok, the event must not appear in the queue.
    let _ = smmu.submit_event(event);

    let events = smmu.get_events();
    assert!(
        events.is_empty(),
        "AUDIT-75: submit_event() must not post events when EVENTQEN=0, got {} event(s)",
        events.len()
    );
}

// ============================================================================
// AUDIT-76 — set_gbpa_abort() must also update gbpa_config.abort
// ============================================================================

/// AUDIT-76 test: set_gbpa_abort(true) must update both the atomic AND
/// the gbpa_config struct's abort field.
///
/// BEFORE FIX: gbpa_config.abort remains false after set_gbpa_abort(true).
/// AFTER FIX:  gbpa_config.abort == true.
#[test]
fn audit_76_set_gbpa_abort_true_updates_gbpa_config() {
    let smmu = SMMU::new();
    smmu.set_gbpa_abort(true);

    assert!(
        smmu.is_gbpa_abort(),
        "AUDIT-76: is_gbpa_abort() must return true after set_gbpa_abort(true)"
    );
    assert!(
        smmu.get_gbpa_config().abort,
        "AUDIT-76: get_gbpa_config().abort must be true after set_gbpa_abort(true)"
    );
}

/// AUDIT-76 negative: set_gbpa_abort(false) must clear gbpa_config.abort.
#[test]
fn audit_76_set_gbpa_abort_false_clears_gbpa_config() {
    let smmu = SMMU::new();
    // Set to true first, then clear.
    smmu.set_gbpa_abort(true);
    smmu.set_gbpa_abort(false);

    assert!(
        !smmu.is_gbpa_abort(),
        "AUDIT-76: is_gbpa_abort() must be false after set_gbpa_abort(false)"
    );
    assert!(
        !smmu.get_gbpa_config().abort,
        "AUDIT-76: get_gbpa_config().abort must be false after set_gbpa_abort(false)"
    );
}

// ============================================================================
// AUDIT-77 — ATS early-return paths must increment total_translations
// ============================================================================

/// AUDIT-77 test: ATS Translation Request with SMMUEN=0 must increment
/// total_translations (not just failed_translations).
///
/// BEFORE FIX: only failed_translations is incremented; total stays 0.
/// AFTER FIX:  total == successful + failed (total >= 1 after one attempt).
#[test]
fn audit_77_ats_treq_smmuen0_increments_total_translations() {
    let smmu = SMMU::new();
    // Do NOT enable SMMU — SMMUEN=0 for this test.

    smmu.reset_translation_stats();

    let stream = sid(1);
    // The stream does not need to exist; the SMMUEN=0 early-return fires first.
    let _ = smmu.translate_with_type(
        stream,
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
    );

    let (total, successful, failed) = smmu.get_translation_stats();
    assert_eq!(
        total,
        successful + failed,
        "AUDIT-77: total_translations ({}) must equal successful ({}) + failed ({})",
        total,
        successful,
        failed
    );
    assert!(
        total >= 1,
        "AUDIT-77: total_translations must be at least 1 after one ATS TR attempt"
    );
}

/// AUDIT-77 test: ATS Translated (TT) with SMMUEN=0 must also increment
/// total_translations.
#[test]
fn audit_77_ats_tt_smmuen0_increments_total_translations() {
    let smmu = SMMU::new();
    // Do NOT enable SMMU.

    smmu.reset_translation_stats();

    let stream = sid(1);
    let _ = smmu.translate_with_type(
        stream,
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated,
    );

    let (total, successful, failed) = smmu.get_translation_stats();
    assert_eq!(
        total,
        successful + failed,
        "AUDIT-77: ATS TT total ({}) must equal successful ({}) + failed ({})",
        total,
        successful,
        failed
    );
    assert!(
        total >= 1,
        "AUDIT-77: total_translations must be >= 1 after ATS TT attempt with SMMUEN=0"
    );
}

// ============================================================================
// AUDIT-78 — process_pri_queue() must check effective PRIQEN
// ============================================================================

/// AUDIT-78 test: process_pri_queue() when SMMUEN=0 must return 0 and not emit
/// events, even when the PRI queue contains un-emitted entries.
///
/// To get an un-emitted entry in the queue, we use a tiny event queue (size=4)
/// so that submit_page_request's immediate emission fails (event queue full),
/// leaving priq_emitted behind queue.len().
///
/// BEFORE FIX: process_pri_queue() processes entries and emits events regardless
///   of SMMUEN.
/// AFTER FIX:  Returns Ok(0) immediately; no new events emitted when SMMUEN=0.
#[test]
fn audit_78_process_pri_queue_no_op_when_smmuen_zero() {
    // Small event queue so we can fill it easily.
    let mut cfg = SMMUConfig::default();
    cfg.queue_config.event_queue_size = 4;
    cfg.queue_config.pri_queue_size = 16;
    let smmu = SMMU::with_config(cfg);

    // Enable with all relevant bits so submit_page_request works.
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);

    // Fill the event queue completely so that the next submission's immediate
    // emit fails and priq_emitted stays at 0 while the PRI queue has 1 entry.
    for i in 0u32..4 {
        let event = EventEntry {
            event_type: EventType::CBadSte,
            stream_id: 100 + i,
            timestamp: u64::from(i),
            security_state: SecurityState::NonSecure,
            stall: false,
            ..EventEntry::zeroed()
        };
        let _ = smmu.submit_event(event);
    }
    // Event queue must now be full.
    assert_eq!(smmu.get_events().len(), 4, "AUDIT-78 precondition: event queue must be full");

    // Submit a PRI entry. The immediate emission attempt fails (queue full),
    // so priq_emitted stays at 0 while pri_queue has 1 entry.
    let entry = PRIEntry::with_address(1, 0, 0x1000, AccessType::Read);
    let _ = smmu.submit_page_request(entry);
    assert_eq!(smmu.priq_emitted_count(), 0, "AUDIT-78 precondition: priq_emitted must be 0");
    assert_eq!(smmu.get_pri_queue_size(), 1, "AUDIT-78 precondition: PRI queue must have 1 entry");

    // Now disable the SMMU (SMMUEN=0).
    smmu.set_cr0(0);
    assert_eq!(smmu.get_cr0() & SMMU::CR0_SMMUEN, 0, "AUDIT-78 precondition: SMMUEN must be 0");

    // Clear the event queue so we can detect new emissions.
    smmu.clear_event_queue();
    smmu.set_cr0(SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN); // EVENTQEN=1 but SMMUEN=0

    let before_events = smmu.get_events().len();
    let processed = smmu.process_pri_queue().unwrap();
    let after_events = smmu.get_events().len();

    assert_eq!(
        processed, 0,
        "AUDIT-78: process_pri_queue() must return 0 when SMMUEN=0, returned {}",
        processed
    );
    assert_eq!(
        after_events,
        before_events,
        "AUDIT-78: process_pri_queue() must not emit events when SMMUEN=0 \
         (before={}, after={})",
        before_events,
        after_events
    );
}

/// AUDIT-78 test: process_pri_queue() when PRIQEN=0 (SMMUEN=1) must return 0
/// and not emit events for un-emitted PRI queue entries.
#[test]
fn audit_78_process_pri_queue_no_op_when_priqen_zero() {
    // Small event queue so we can fill it easily.
    let mut cfg = SMMUConfig::default();
    cfg.queue_config.event_queue_size = 4;
    cfg.queue_config.pri_queue_size = 16;
    let smmu = SMMU::with_config(cfg);

    // Enable with PRIQEN=1 to submit a PRI entry.
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);

    // Fill the event queue so the PRI submit's immediate emission fails.
    for i in 0u32..4 {
        let event = EventEntry {
            event_type: EventType::CBadSte,
            stream_id: 200 + i,
            timestamp: u64::from(i),
            security_state: SecurityState::NonSecure,
            stall: false,
            ..EventEntry::zeroed()
        };
        let _ = smmu.submit_event(event);
    }

    let entry = PRIEntry::with_address(1, 0, 0x1000, AccessType::Read);
    let _ = smmu.submit_page_request(entry);
    assert_eq!(smmu.priq_emitted_count(), 0, "AUDIT-78 precondition: priq_emitted must be 0");
    assert_eq!(smmu.get_pri_queue_size(), 1, "AUDIT-78 precondition: PRI queue must have 1 entry");

    // Clear the event queue and then set CR0: SMMUEN=1 but PRIQEN=0.
    smmu.clear_event_queue();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    assert_eq!(
        smmu.get_cr0() & SMMU::CR0_PRIQEN,
        0,
        "AUDIT-78 precondition: PRIQEN must be 0"
    );
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_SMMUEN,
        0,
        "AUDIT-78 precondition: SMMUEN must be 1"
    );

    let before_events = smmu.get_events().len();
    let processed = smmu.process_pri_queue().unwrap();
    let after_events = smmu.get_events().len();

    assert_eq!(
        processed, 0,
        "AUDIT-78: process_pri_queue() must return 0 when PRIQEN=0, returned {}",
        processed
    );
    assert_eq!(
        after_events,
        before_events,
        "AUDIT-78: process_pri_queue() must not emit events when PRIQEN=0 \
         (before={}, after={})",
        before_events,
        after_events
    );
}

// ============================================================================
// AUDIT-79 — STRW EL2/EL3 promotion must apply in two-stage paths
// ============================================================================

/// AUDIT-79 test: a two-stage stream with STRW=El2 must promote an unprivileged
/// access type to privileged, so a privileged-only stage-1 page mapping is
/// accessible.
///
/// The two-stage setup uses:
///   - Stage-1: IOVA 0x1000 → IPA 0x5000 (privileged-only page)
///   - Stage-2: IPA 0x5000 → PA 0xA000 (read/write)
///
/// An unprivileged Read to IOVA 0x1000 on a stream with STRW=El2 must be
/// promoted to ReadPrivileged and succeed.
///
/// BEFORE FIX: The `!stage2_enabled` guard in translate() skips STRW promotion
///   for two-stage streams; unprivileged access to the privileged-only page
///   → permission fault.
/// AFTER FIX:  The guard is removed; STRW=El2 promotes the access type to
///   ReadPrivileged regardless of stage-2 being active; the translation succeeds.
#[test]
fn audit_79_strw_el2_promotes_access_in_two_stage_stream() {
    let smmu = SMMU::new();
    smmu.set_s1p_supported(true);
    smmu.set_s2p_supported(true);
    smmu.enable().unwrap();

    let stream = sid(42);

    // Configure a two-stage stream with STRW=El2 (only valid for Secure streams).
    // Per ARM §5.2: STRW=El2 (0b01) is legal only for Secure streams.
    // This is the valid two-stage STRW=El2 scenario to test the promotion path.
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .stage2_enabled(true)
        .strw(StreamWorld::El2)
        .s2_aa64(true)
        .security_state(SecurityState::Secure)
        .s2_t0sz(16)
        .build()
        .unwrap();
    smmu.configure_stream(stream, cfg).unwrap();

    // Stage-1 PASID 0: IOVA 0x1000 → IPA 0x5000 (privileged-only mapping).
    smmu.create_pasid(stream, pasid(0)).unwrap();
    smmu.map_page(
        stream,
        pasid(0),
        iova(0x1000),
        pa(0x5000),
        PagePermissions::read_only_privileged(),
        SecurityState::Secure,
    )
    .unwrap();

    // Stage-2: IPA 0x5000 → PA 0xA000.
    smmu.create_stage2_address_space(stream).unwrap();
    smmu.map_stage2_page(
        stream,
        iova(0x5000),
        pa(0xA000),
        PagePermissions::read_write(),
        SecurityState::Secure,
    )
    .unwrap();

    // Issue an unprivileged Read. STRW=El2 must promote it to ReadPrivileged,
    // which succeeds through the privileged-only stage-1 mapping.
    let result = smmu.translate(
        stream,
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::Secure,
    );
    assert!(
        result.is_ok(),
        "AUDIT-79: STRW=El2 two-stage must promote unprivileged Read to privileged; \
         expected Ok, got: {:?}",
        result
    );
}

// ============================================================================
// AUDIT-80 — CS=0b10 doc comment fix (no behavioral test needed)
// ============================================================================
// The fix is a documentation-only change to command_entry.rs.
// Correctness is verified by cargo clippy --all-targets -- -D warnings passing.

// ============================================================================
// AUDIT-81 — CMD_ATC_INV must be ignored when SMMUEN=0
// ============================================================================

/// AUDIT-81 test: CMD_ATC_INV submitted when SMMUEN=0 must be silently ignored
/// — no completion event, returns success/ignored (not an error).
///
/// BEFORE FIX: CMD_ATC_INV is processed fully and emits a completion event.
/// AFTER FIX:  CMD_ATC_INV is ignored when SMMUEN=0; no event emitted, Ok(()).
#[test]
fn audit_81_cmd_atc_inv_ignored_when_smmuen_zero() {
    let smmu = SMMU::new();
    // Do NOT enable SMMU — SMMUEN=0.

    // Enable EVENTQEN so we can detect if any event is incorrectly emitted.
    smmu.set_cr0(SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    // Verify SMMUEN is still 0.
    assert_eq!(
        smmu.get_cr0() & SMMU::CR0_SMMUEN,
        0,
        "AUDIT-81 precondition: SMMUEN must be 0"
    );

    let before_events = smmu.get_events().len();

    // Submit a CMD_ATC_INV command directly via process_single_command or
    // via the command queue.
    let cmd = CommandEntry {
        cmd_type: CommandType::AtcInv,
        stream_id: 1,
        pasid: 0,
        start_address: 0x1000,
        end_address: 0x2000,
        flags: 0,
        ssec: false,
        ..CommandEntry::default()
    };

    // process_command expects the queue to be set up; use submit + process.
    smmu.submit_command(cmd).unwrap();
    // process_command_queue processes one or all commands.
    let _ = smmu.process_command_queue();

    let after_events = smmu.get_events().len();

    assert_eq!(
        after_events,
        before_events,
        "AUDIT-81: CMD_ATC_INV must not emit a completion event when SMMUEN=0 \
         (before={}, after={})",
        before_events,
        after_events
    );
}
