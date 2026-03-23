//! Tests for open items resolved on 2026-03-23.
//!
//! ## OPEN-3: CMDQ_CONS ERR field comment correction (documentation only)
//!
//! ARM §6.3.28 specifies that the CMDQ_CONS ERR field occupies bits **[30:24]**
//! (a 7-bit field).  Bit 31 is RES0.  Comments that said "bits [31:24]" were
//! wrong and have been corrected to "bits [30:24]" with an explanatory note:
//! "Bit 31 of CMDQ_CONS is RES0 per ARM §6.3.28; ERR occupies only bits[30:24]."
//!
//! There is no behavioural change for OPEN-3 — `CMDQ_CONS_ERR_SHIFT = 24` is
//! architecturally correct regardless of whether the field is described as
//! 7-bit [30:24] or 8-bit [31:24].  The constant value is unchanged.
//!
//! ## OPEN-6: SecurityViolation must produce FaultType::PermissionFault
//!
//! ARM §7.3.16 defines the F_PERMISSION event (event code 0x13) for permission
//! and security-state violations.  `FaultType::SecurityFault` carries the
//! numeric value 0x12, which collides with the ARM F_ACCESS event code
//! (§7.3.15, event code 0x12).  Using `FaultType::SecurityFault` for a
//! security-state mismatch therefore encodes the wrong event code on the wire.
//!
//! The correct mapping is:
//!   `TranslationError::SecurityViolation → FaultType::PermissionFault`
//!
//! Both the unit mapping (`map_translation_error_to_fault_type`) and the
//! end-to-end path (translate() → get_faults()) must reflect this.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

use smmu::types::{
    AccessType, EventType, FaultType, PagePermissions, SecurityState, StreamConfig, StreamID,
    IOVA, PA, PASID,
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

// ============================================================================
// OPEN-3: CMDQ_CONS ERR field comment verification (no behavioural change)
// ============================================================================

/// OPEN-3 — CMDQ_CONS_ERR_SHIFT=24 is correct for a 7-bit field at bits[30:24].
///
/// ARM §6.3.28: ERR is at bits [30:24] (7 bits); bit 31 is RES0.
/// The shift value of 24 is architecturally correct.  This test verifies
/// the constant value is unchanged (documentation fix only).
#[test]
fn open3_cmdq_cons_err_shift_is_24() {
    // CMDQ_CONS_ERR_SHIFT must be 24: the ERR field starts at bit 24.
    // Bit 31 of CMDQ_CONS is RES0 per ARM §6.3.28 — ERR occupies only bits[30:24].
    assert_eq!(
        SMMU::CMDQ_CONS_ERR_SHIFT,
        24,
        "OPEN-3: CMDQ_CONS_ERR_SHIFT must be 24 (ERR at bits[30:24], ARM §6.3.28)"
    );
}

/// OPEN-3 — CERROR_* constants have the correct values (§4.7.3 / §6.3.28).
#[test]
fn open3_cerror_constants_correct() {
    assert_eq!(SMMU::CERROR_NONE, 0, "CERROR_NONE must be 0");
    assert_eq!(SMMU::CERROR_ILL, 1, "CERROR_ILL must be 1");
    assert_eq!(SMMU::CERROR_ABT, 2, "CERROR_ABT must be 2");
    assert_eq!(SMMU::CERROR_ATC_INV_SYNC, 3, "CERROR_ATC_INV_SYNC must be 3");
}

// ============================================================================
// OPEN-6: SecurityViolation → PermissionFault mapping
// ============================================================================

// NOTE: The direct unit test for map_translation_error_to_fault_type() cannot
// be placed here because that function is private (fn, not pub fn).  The
// corresponding internal unit test lives in src/smmu/mod.rs inside the
// #[cfg(test)] module as `open6_security_violation_maps_to_permission_fault`.
// The end-to-end tests below exercise the mapping through the public API.

/// OPEN-6 — End-to-end: a security-state mismatch during translate() must
/// produce a FaultRecord with fault_type == FaultType::PermissionFault.
///
/// Setup:
/// - Configure a stream with stage-1-only translation (NonSecure).
/// - Map a page with SecurityState::Secure.
/// - Translate the same IOVA with SecurityState::NonSecure.
/// - The security-state mismatch triggers TranslationError::SecurityViolation
///   inside AddressSpace::translate_page().
///
/// Expected: get_faults() returns a FaultRecord whose fault_type is
/// FaultType::PermissionFault (not SecurityFault).
#[test]
fn open6_security_violation_fault_record_has_permission_fault_type() {
    let smmu = make_smmu();
    let stream_id = sid(1);

    // Configure a NonSecure stage-1-only stream.
    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz = 0;
    cfg.security_state = SecurityState::NonSecure;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    // Map the page with SecurityState::Secure (deliberate mismatch).
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x1000),
        pa(0x2000),
        PagePermissions::read_write(),
        SecurityState::Secure,
    )
    .unwrap();

    // Translate with NonSecure — should trigger SecurityViolation.
    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "OPEN-6: translation with security-state mismatch must fail"
    );

    // The FaultRecord must carry PermissionFault, not SecurityFault.
    let faults = smmu.get_faults();
    assert!(
        !faults.is_empty(),
        "OPEN-6: a FaultRecord must be recorded for the security violation"
    );
    let ft = faults[0].fault_type();
    assert_eq!(
        ft,
        FaultType::PermissionFault,
        "OPEN-6: FaultRecord.fault_type must be PermissionFault for security violation \
         (ARM §7.3.16); got {ft:?}"
    );
    assert_ne!(
        ft,
        FaultType::SecurityFault,
        "OPEN-6: FaultRecord.fault_type must NOT be SecurityFault (0x12 collides with \
         ARM F_ACCESS §7.3.15)"
    );
}

/// OPEN-6 — Event queue: a security-state mismatch must produce an event
/// of type EventType::FPermission in the event queue.
///
/// This test verifies the full pipeline:
///   TranslationError::SecurityViolation
///     → FaultType::PermissionFault   (map_translation_error_to_fault_type)
///       → EventType::FPermission     (map_fault_type_to_event_type)
///
/// NOTE: FaultType::SecurityFault ALSO maps to EventType::FPermission (the
/// second BUG-RUST-M01 test in mod.rs verifies this independently), so the
/// event-queue outcome is the same regardless of which FaultType is used.
/// The key requirement is that the FaultRecord and the direct mapping both
/// use PermissionFault so the fault-type numeric value is correct (0x13
/// rather than the colliding 0x12).
#[test]
fn open6_security_violation_emits_f_permission_event() {
    let smmu = make_smmu();
    let stream_id = sid(2);

    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz = 0;
    cfg.security_state = SecurityState::NonSecure;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    // Map with Secure; translate as NonSecure.
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x4000),
        pa(0x8000),
        PagePermissions::read_only(),
        SecurityState::Secure,
    )
    .unwrap();

    let _ = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x4000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "OPEN-6: an EventEntry must be enqueued for the security violation"
    );
    let et = events[0].event_type;
    assert_eq!(
        et,
        EventType::FPermission,
        "OPEN-6: EventEntry.event_type must be FPermission for security violation \
         (ARM §7.3.16); got {et:?}"
    );
}
