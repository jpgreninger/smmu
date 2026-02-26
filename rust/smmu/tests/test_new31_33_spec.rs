#![allow(clippy::doc_markdown)]
//! ARM SMMU v3 FINDING-NEW-31, NEW-32, NEW-33 spec tests
//!
//! Tests for:
//! - FINDING-NEW-31: AccessFlagFault maps to wrong event type (§7.3.15)
//! - FINDING-NEW-32: E_PAGE_REQUEST hardcodes NonSecure security state (§7.3.19)
//! - FINDING-NEW-33: CMD_SYNC CS=0b11 reserved not rejected with CERROR_ILL (§4.8)

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventType, FaultMode, PagePermissions,
    PRIEntry, SecurityState, StreamConfig, StreamID, PASID, IOVA, PA,
};
use smmu::SMMU;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu
}

fn sid(n: u32) -> StreamID { StreamID::new(n).unwrap() }
fn pasid(n: u32) -> PASID { PASID::new(n).unwrap() }
fn iova(addr: u64) -> IOVA { IOVA::new(addr).unwrap() }

fn count_events_of_type(smmu: &SMMU, event_type: EventType) -> usize {
    smmu.get_events()
        .iter()
        .filter(|e| e.event_type == event_type)
        .count()
}

// ── FINDING-NEW-31: AccessFlagFault → FAccess event (§7.3.15) ────────────────
//
// Note: AccessFlagFault cannot be triggered through the public translate() API
// because the Rust model uses HA=1 mode (hardware access flag update), which
// automatically sets the access flag on first access rather than faulting.
//
// The mapping is verified via a unit test embedded in smmu/mod.rs (see
// #[cfg(test)] block). Here we verify the regression: if a translation fault
// does occur (via unmapped page), the event type is F_TRANSLATION, not F_ACCESS —
// confirming AccessFlagFault and TranslationFault produce distinct event types.

/// Regression guard: TranslationFault must still produce F_TRANSLATION (not F_ACCESS).
#[test]
fn translation_fault_produces_f_translation_not_f_access() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0x60), cfg).unwrap();
    smmu.create_pasid(sid(0x60), pasid(0)).unwrap();

    // Trigger a TranslationFault (unmapped address)
    let result = smmu.translate(
        sid(0x60),
        pasid(0),
        iova(0x5000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(result.is_err(), "expected translation fault for unmapped page");

    let f_translation_count = count_events_of_type(&smmu, EventType::FTranslation);
    let f_access_count = count_events_of_type(&smmu, EventType::FAccess);

    assert_eq!(
        f_translation_count, 1,
        "TranslationFault must produce exactly one F_TRANSLATION event"
    );
    assert_eq!(
        f_access_count, 0,
        "TranslationFault must NOT produce an F_ACCESS event"
    );
}

/// Regression guard: PermissionFault must produce F_PERMISSION (not F_ACCESS).
#[test]
fn permission_fault_produces_f_permission_not_f_access() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0x61), cfg).unwrap();
    smmu.create_pasid(sid(0x61), pasid(0)).unwrap();

    // Map a read-only page
    let pa = PA::new(0x1000_0000).unwrap();
    smmu.map_page(
        sid(0x61),
        pasid(0),
        iova(0x5000),
        pa,
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Attempt write → PermissionFault
    let result = smmu.translate(
        sid(0x61),
        pasid(0),
        iova(0x5000),
        AccessType::Write,
        SecurityState::NonSecure,
    );
    assert!(result.is_err(), "expected permission fault for write to read-only page");

    let f_permission_count = count_events_of_type(&smmu, EventType::FPermission);
    let f_access_count = count_events_of_type(&smmu, EventType::FAccess);

    assert_eq!(
        f_permission_count, 1,
        "PermissionFault must produce exactly one F_PERMISSION event"
    );
    assert_eq!(
        f_access_count, 0,
        "PermissionFault must NOT produce an F_ACCESS event"
    );
}

// ── FINDING-NEW-32: E_PAGE_REQUEST must preserve the request's security state ─

/// A Secure PRI request must generate an E_PAGE_REQUEST event with SecurityState::Secure.
#[test]
fn pri_request_secure_generates_secure_event() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0x70), cfg).unwrap();
    smmu.create_pasid(sid(0x70), pasid(0)).unwrap();

    // Submit a Secure page request (security_state field added by FINDING-NEW-32 fix)
    let req = PRIEntry::with_address(0x70, 0, 0x8000, AccessType::Read)
        .with_security_state(SecurityState::Secure);
    smmu.submit_page_request(req).unwrap();
    smmu.process_pri_queue().unwrap();

    // Find the E_PAGE_REQUEST event
    let events = smmu.get_events();
    let pri_event = events.iter().find(|e| e.event_type == EventType::EPageRequest);
    assert!(
        pri_event.is_some(),
        "FINDING-NEW-32: an E_PAGE_REQUEST event must be emitted after process_pri_queue()"
    );
    assert_eq!(
        pri_event.unwrap().security_state,
        SecurityState::Secure,
        "§7.3.19 / FINDING-NEW-32: E_PAGE_REQUEST event must carry the request's security state (Secure)"
    );
}

/// A NonSecure PRI request must still generate an E_PAGE_REQUEST event with SecurityState::NonSecure.
#[test]
fn pri_request_nonsecure_generates_nonsecure_event() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0x71), cfg).unwrap();
    smmu.create_pasid(sid(0x71), pasid(0)).unwrap();

    let req = PRIEntry::with_address(0x71, 0, 0x9000, AccessType::Read)
        .with_security_state(SecurityState::NonSecure);
    smmu.submit_page_request(req).unwrap();
    smmu.process_pri_queue().unwrap();

    let events = smmu.get_events();
    let pri_event = events.iter().find(|e| e.event_type == EventType::EPageRequest);
    assert!(pri_event.is_some(), "FINDING-NEW-32: an E_PAGE_REQUEST event must be emitted");
    assert_eq!(
        pri_event.unwrap().security_state,
        SecurityState::NonSecure,
        "§7.3.19 / FINDING-NEW-32: E_PAGE_REQUEST event must carry NonSecure state for NonSecure request"
    );
}

// ── FINDING-NEW-33 / BUG-02: CMD_SYNC CS=0b11 reserved → CERROR_ILL ──────────

/// CMD_SYNC with CS=0b11 is Reserved per §4.8 — must generate CERROR_ILL:
/// sets GERROR.CMDQ_ERR (bit[0] toggled), halts queue, generates no
/// CommandSyncCompletion event.  Command errors are NOT reported via the
/// Event queue (§4.1.3 / §7.1).
#[test]
fn cmd_sync_cs_reserved_generates_no_completion_event() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 0b11; // Reserved value — must cause CERROR_ILL per §4.7.3
    smmu.submit_command(cmd).unwrap();

    // process_command_queue must return Err (CERROR_ILL halts processing)
    let result = smmu.process_command_queue();
    assert!(
        result.is_err(),
        "§4.7.3 / BUG-02: process_command_queue must return Err for CMD_SYNC CS=0b11"
    );

    // GERROR.CMDQ_ERR must be toggled per §7.5
    assert_ne!(
        smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR,
        0,
        "§4.7.3 / BUG-02: GERROR.CMDQ_ERR (bit[0]) must be set after CERROR_ILL"
    );

    // No CommandSyncCompletion event — command errors use GERROR, not Event queue
    let sync_events = count_events_of_type(&smmu, EventType::CommandSyncCompletion);
    assert_eq!(
        sync_events, 0,
        "§4.8 / BUG-02: CMD_SYNC CS=0b11 must NOT generate a CommandSyncCompletion event"
    );
}

/// CMD_SYNC with CS=0b01 (SIG_IRQ) must still generate a COMMAND_SYNC_COMPLETION event
/// — regression guard ensuring the CS=0b11 fix does not break valid CS values.
#[test]
fn cmd_sync_cs_sig_irq_generates_completion_event() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 0b01; // SIG_IRQ
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    let sync_events = count_events_of_type(&smmu, EventType::CommandSyncCompletion);
    assert_eq!(
        sync_events, 1,
        "§4.8: CMD_SYNC CS=0b01 (SIG_IRQ) must generate exactly one CommandSyncCompletion event"
    );
}
