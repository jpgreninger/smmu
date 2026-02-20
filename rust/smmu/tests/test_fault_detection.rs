#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]

//! Tests for fault security state propagation.
//!
//! ARM SMMU v3 spec §7.2.3: fault records and event queue entries must carry the
//! security state of the originating transaction, not a hard-coded NonSecure value.

use smmu::types::{AccessType, SecurityState, StreamConfig, StreamID, IOVA, PASID};
use smmu::SMMU;

// ============================================================================
// Helper
// ============================================================================

fn default_smmu() -> SMMU {
    let smmu = SMMU::new();
    // SMMUEN must be set (§6.3.9) for translations to proceed past bypass.
    smmu.enable().unwrap();
    smmu
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).expect("valid IOVA")
}

fn pasid(v: u32) -> PASID {
    PASID::new(v).unwrap()
}

fn stream_id(v: u32) -> StreamID {
    StreamID::new(v).unwrap()
}

// ============================================================================
// Fault security state propagation — translation fault path
// ============================================================================

/// FINDING-M-07 (TDD): A faulting Secure transaction must record `SecurityState::Secure`
/// in the fault record, not the hard-coded NonSecure.
#[test]
fn test_translation_fault_records_secure_security_state() {
    let smmu = default_smmu();
    let sid = stream_id(1);

    // Configure a Stage-1-only stream with no pages mapped — any translate will fault
    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(sid, cfg).unwrap();

    // Issue a Secure translation that must fault (no page mapped)
    let _ = smmu.translate(sid, pasid(0), iova(0x1000), AccessType::Read, SecurityState::Secure);

    // The fault record must carry the Secure security state from the request
    let faults = smmu.get_faults();
    assert!(!faults.is_empty(), "expected at least one fault record");
    let fault = &faults[0];
    assert_eq!(
        fault.security_state(),
        SecurityState::Secure,
        "fault record security_state must reflect the transaction's SecurityState::Secure, not NonSecure"
    );
}

/// FINDING-M-07 (TDD): A faulting Realm transaction must record `SecurityState::Realm`
/// in the fault record.
#[test]
fn test_translation_fault_records_realm_security_state() {
    let smmu = default_smmu();
    let sid = stream_id(2);

    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(sid, cfg).unwrap();

    let _ = smmu.translate(sid, pasid(0), iova(0x2000), AccessType::Read, SecurityState::Realm);

    let faults = smmu.get_faults();
    assert!(!faults.is_empty(), "expected at least one fault record");
    let fault = &faults[0];
    assert_eq!(
        fault.security_state(),
        SecurityState::Realm,
        "fault record security_state must reflect the transaction's SecurityState::Realm, not NonSecure"
    );
}

/// FINDING-M-07 (TDD): A stream-not-found fault for a Secure transaction must record
/// `SecurityState::Secure` in the fault record.
#[test]
fn test_stream_not_found_fault_records_secure_security_state() {
    let smmu = default_smmu();
    // Do NOT configure the stream — this triggers the stream-not-found path
    let sid = stream_id(99);

    let _ = smmu.translate(sid, pasid(0), iova(0x3000), AccessType::Write, SecurityState::Secure);

    let faults = smmu.get_faults();
    assert!(!faults.is_empty(), "expected at least one fault record for unconfigured stream");
    let fault = &faults[0];
    assert_eq!(
        fault.security_state(),
        SecurityState::Secure,
        "stream-not-found fault security_state must reflect the transaction's SecurityState::Secure, not NonSecure"
    );
}

// ============================================================================
// Event queue security state propagation
// ============================================================================

/// FINDING-M-07 (TDD): Event queue entries must also carry the correct security state.
#[test]
fn test_event_queue_records_secure_security_state() {
    let smmu = default_smmu();
    let sid = stream_id(3);

    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(sid, cfg).unwrap();

    let _ = smmu.translate(sid, pasid(0), iova(0x4000), AccessType::Read, SecurityState::Secure);

    let events = smmu.get_events();
    assert!(!events.is_empty(), "expected at least one event queue entry");
    let event = &events[0];
    assert_eq!(
        event.security_state,
        SecurityState::Secure,
        "event queue entry security_state must reflect the transaction's SecurityState::Secure, not NonSecure"
    );
}

/// FINDING-M-07 (TDD): NonSecure transactions must still record NonSecure correctly.
#[test]
fn test_translation_fault_records_nonsecure_security_state() {
    let smmu = default_smmu();
    let sid = stream_id(4);

    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(sid, cfg).unwrap();

    let _ = smmu.translate(sid, pasid(0), iova(0x5000), AccessType::Read, SecurityState::NonSecure);

    let faults = smmu.get_faults();
    assert!(!faults.is_empty(), "expected at least one fault record");
    assert_eq!(faults[0].security_state(), SecurityState::NonSecure);
}
