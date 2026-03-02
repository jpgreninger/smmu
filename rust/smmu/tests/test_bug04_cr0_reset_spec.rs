#![allow(clippy::doc_markdown)]
#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

//! BUG-04/SPEC-01: CR0 Reset Value must be 0 (ARM IHI0070G.b §6.3.9)
//!
//! Spec: ARM IHI0070G.b §6.3.9 — ALL CR0 bits reset to 0, including
//! CMDQEN, EVENTQEN, PRIQEN, and SMMUEN.
//!
//! The previous implementation incorrectly initialised CR0 with queue-enable
//! bits set (PRIQEN|EVENTQEN|CMDQEN == 1), which is non-conformant.

use smmu::SMMU;

/// §6.3.9 / BUG-04: After reset ALL CR0 bits must be 0.
///
/// This test FAILS before the fix because CR0 is initialised with
/// PRIQEN|EVENTQEN|CMDQEN bits set.
#[test]
fn bug04_cr0_all_bits_zero_after_reset() {
    let smmu = SMMU::new();
    assert_eq!(
        smmu.get_cr0(),
        0,
        "BUG-04/§6.3.9: CR0 must be 0 after reset (all bits including PRIQEN, \
         EVENTQEN, CMDQEN, and SMMUEN must be clear)"
    );
}

/// §6.3.9 / BUG-04: SMMUEN must be 0 after reset.
#[test]
fn bug04_cr0_smmuen_clear_after_reset() {
    let smmu = SMMU::new();
    assert_eq!(
        smmu.get_cr0() & SMMU::CR0_SMMUEN,
        0,
        "§6.3.9: SMMUEN must be 0 after reset"
    );
}

/// §6.3.9 / BUG-04: PRIQEN must be 0 after reset.
#[test]
fn bug04_cr0_priqen_clear_after_reset() {
    let smmu = SMMU::new();
    assert_eq!(
        smmu.get_cr0() & SMMU::CR0_PRIQEN,
        0,
        "§6.3.9: PRIQEN must be 0 after reset"
    );
}

/// §6.3.9 / BUG-04: EVENTQEN must be 0 after reset.
#[test]
fn bug04_cr0_eventqen_clear_after_reset() {
    let smmu = SMMU::new();
    assert_eq!(
        smmu.get_cr0() & SMMU::CR0_EVENTQEN,
        0,
        "§6.3.9: EVENTQEN must be 0 after reset"
    );
}

/// §6.3.9 / BUG-04: CMDQEN must be 0 after reset.
#[test]
fn bug04_cr0_cmdqen_clear_after_reset() {
    let smmu = SMMU::new();
    assert_eq!(
        smmu.get_cr0() & SMMU::CR0_CMDQEN,
        0,
        "§6.3.9: CMDQEN must be 0 after reset"
    );
}

/// §6.3.9: After enable(), SMMUEN and queue enables must all be set.
///
/// This is the positive complement: enable() should activate SMMUEN +
/// CMDQEN + EVENTQEN + PRIQEN so all queues are usable.
#[test]
fn bug04_enable_sets_queue_enables_and_smmuen() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    let cr0 = smmu.get_cr0();
    assert_ne!(
        cr0 & SMMU::CR0_SMMUEN,
        0,
        "enable() must set SMMUEN"
    );
    assert_ne!(
        cr0 & SMMU::CR0_CMDQEN,
        0,
        "enable() must set CMDQEN"
    );
    assert_ne!(
        cr0 & SMMU::CR0_EVENTQEN,
        0,
        "enable() must set EVENTQEN"
    );
    assert_ne!(
        cr0 & SMMU::CR0_PRIQEN,
        0,
        "enable() must set PRIQEN"
    );
}

/// §6.3.9: When CR0 is 0 (reset), process_command_queue must be a no-op
/// (CMDQEN=0 gates command processing).
///
/// After BUG-04 fix, callers who want command processing must explicitly
/// set CMDQEN via set_cr0() or call enable().
#[test]
fn bug04_cmdqen_zero_prevents_command_processing() {
    use smmu::types::{CommandEntry, CommandType};

    let smmu = SMMU::new();
    // CR0 = 0 after reset (BUG-04 fix), so CMDQEN=0.
    assert_eq!(smmu.get_cr0() & SMMU::CR0_CMDQEN, 0, "precondition: CMDQEN=0");

    smmu.submit_command(CommandEntry::new(CommandType::TlbiNhAll, 0, 0)).unwrap();
    let count = smmu.process_command_queue().unwrap();
    assert_eq!(
        count,
        0,
        "§4.1.2: process_command_queue must be a no-op when CMDQEN=0"
    );
    // The command must still be in the queue (not consumed).
    assert_eq!(smmu.get_command_queue_size(), 1, "command must remain in queue when CMDQEN=0");
}

/// §7.2.1: When CR0.EVENTQEN=0 (reset), events must not be recorded.
///
/// After BUG-04 fix, event recording is suppressed until EVENTQEN is set.
#[test]
fn bug04_eventqen_zero_suppresses_events() {
    use smmu::types::{AccessType, FaultMode, SecurityState, StreamConfig, StreamID, IOVA, PASID};

    let smmu = SMMU::new();
    // CR0 = 0 (reset default), SMMUEN=0 so SMMU is in bypass mode.
    // We must set SMMUEN to trigger event-generating paths.
    smmu.set_cr0(SMMU::CR0_SMMUEN); // EVENTQEN=0

    let stream_id = StreamID::new(1).unwrap();
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, PASID::new(0).unwrap()).unwrap();

    // Translate unmapped address — generates translation fault.
    let _ = smmu.translate(
        stream_id,
        PASID::new(0).unwrap(),
        IOVA::new(0x5000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert_eq!(
        smmu.get_events().len(),
        0,
        "§7.2.1: No events must be queued when EVENTQEN=0"
    );
}
