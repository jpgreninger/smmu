//! FINDING-NEW-44: `ATC_INVALIDATE_COMPLETION` and `COMMAND_SYNC_COMPLETION` events
//! hardcode `security_state: SecurityState::NonSecure`.
//!
//! ARM IHI0070G.b §7.3.21 (IMPDEF completion events): the security state recorded
//! in a completion event should reflect the security state of the stream that
//! originated the command, not a hardcoded NonSecure value.
//!
//! Before the fix, both `AtcInvalidateCompletion` and `CommandSyncCompletion`
//! events always carried `SecurityState::NonSecure` regardless of the stream's
//! configured security state.  After the fix, they use the stream's configured
//! security state.
//!
//! Root cause: `StreamConfig` had no `security_state` field.  Added
//! `pub security_state: SecurityState` (default `NonSecure`) and propagated
//! it through `configure_stream()` → `StreamContext` → completion event.

use smmu::types::{
    CommandEntry, CommandType, EventType, SecurityState, StreamConfig, StreamID, PASID,
};
use smmu::SMMU;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu
}

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

/// Build a stage-1 stream config with an explicit security state.
fn secure_stage1_config() -> StreamConfig {
    StreamConfig {
        security_state: SecurityState::Secure,
        ..StreamConfig::stage1_only()
    }
}

// ── Test 1: AtcInvalidateCompletion must use the stream's security state ─────

/// Configure a stream with `SecurityState::Secure`, send `CMD_ATC_INV`, retrieve
/// events, find `AtcInvalidateCompletion`, and assert its `security_state` is
/// `Secure` — not `NonSecure`.
#[test]
fn test_atc_completion_uses_stream_security_state() {
    let smmu = make_smmu();
    let stream_id = sid(0xA0);

    // Configure stream with Secure state
    smmu.configure_stream(stream_id, secure_stage1_config()).unwrap();

    // PASID 0 is implicitly available on stage1_only streams (or create it)
    let _ = smmu.create_pasid(stream_id, PASID::new(0).unwrap());

    // Submit CMD_ATC_INV for this stream
    let cmd = CommandEntry {
        cmd_type: CommandType::AtcInv,
        stream_id: 0xA0,
        pasid: 0,
        start_address: 0x1000,
        end_address: 0x2000,
        flags: 0,
        timestamp: 0,
        asid: 0,
        vmid: 0,
        stag: 0,
        prg_index: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        security_state: SecurityState::NonSecure,
    };
    smmu.submit_command(cmd).expect("submit_command must succeed");
    smmu.process_command_queue().expect("process_command_queue must succeed");

    // Find the AtcInvalidateCompletion event
    let events = smmu.get_events();
    let completion = events
        .iter()
        .find(|e| e.event_type == EventType::AtcInvalidateCompletion)
        .expect("AtcInvalidateCompletion event must be present");

    assert_eq!(
        completion.security_state,
        SecurityState::Secure,
        "FINDING-NEW-44: AtcInvalidateCompletion.security_state must match the \
         stream's configured security state (Secure), not hardcoded NonSecure. \
         Got: {:?}",
        completion.security_state
    );
}

// ── Test 2: CommandSyncCompletion must use the stream's security state ────────

/// Configure a stream with `SecurityState::Secure`, send `CMD_SYNC` (CS=0x01 to
/// generate an event), retrieve events, find `CommandSyncCompletion`, and assert
/// its `security_state` is `Secure` — not `NonSecure`.
#[test]
fn test_sync_completion_uses_stream_security_state() {
    let smmu = make_smmu();
    let stream_id = sid(0xB0);

    // Configure stream with Secure state
    smmu.configure_stream(stream_id, secure_stage1_config()).unwrap();

    // Submit CMD_SYNC with CS=0x01 (SIG_IRQ) so a completion event is generated
    // The CMD_SYNC stream_id associates the command with this stream.
    let mut cmd = CommandEntry::new(CommandType::Sync, 0xB0, 0);
    cmd.cs = 0x01; // SIG_IRQ — causes CommandSyncCompletion event

    smmu.submit_command(cmd).expect("submit_command must succeed");
    smmu.process_command_queue().expect("process_command_queue must succeed");

    // Find the CommandSyncCompletion event
    let events = smmu.get_events();
    let completion = events
        .iter()
        .find(|e| e.event_type == EventType::CommandSyncCompletion)
        .expect("CommandSyncCompletion event must be present");

    assert_eq!(
        completion.security_state,
        SecurityState::Secure,
        "FINDING-NEW-44: CommandSyncCompletion.security_state must match the \
         stream's configured security state (Secure), not hardcoded NonSecure. \
         Got: {:?}",
        completion.security_state
    );
}

// ── Test 3: NonSecure stream still gets NonSecure completion events ───────────

/// Confirm backward compatibility: a stream configured with the default
/// `SecurityState::NonSecure` still produces `NonSecure` completion events.
#[test]
fn test_atc_completion_nonsecure_stream_stays_nonsecure() {
    let smmu = make_smmu();
    let stream_id = sid(0xC0);

    // Default stage1_only() — security_state defaults to NonSecure
    smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    let _ = smmu.create_pasid(stream_id, PASID::new(0).unwrap());

    let cmd = CommandEntry {
        cmd_type: CommandType::AtcInv,
        stream_id: 0xC0,
        pasid: 0,
        start_address: 0x1000,
        end_address: 0x2000,
        flags: 0,
        timestamp: 0,
        asid: 0,
        vmid: 0,
        stag: 0,
        prg_index: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        security_state: SecurityState::NonSecure,
    };
    smmu.submit_command(cmd).expect("submit_command must succeed");
    smmu.process_command_queue().expect("process_command_queue must succeed");

    let events = smmu.get_events();
    let completion = events
        .iter()
        .find(|e| e.event_type == EventType::AtcInvalidateCompletion)
        .expect("AtcInvalidateCompletion event must be present");

    assert_eq!(
        completion.security_state,
        SecurityState::NonSecure,
        "Backward compat: NonSecure stream must produce NonSecure completion events. \
         Got: {:?}",
        completion.security_state
    );
}

// ── Test 4: StreamConfig builder security_state method ───────────────────────

/// Confirm the builder method `security_state()` stores the value correctly
/// and that `StreamConfig::default()` has `SecurityState::NonSecure`.
#[test]
fn test_stream_config_security_state_builder() {
    // Default must be NonSecure (backward compatible)
    let default_config = StreamConfig::builder().build().unwrap();
    assert_eq!(
        default_config.security_state,
        SecurityState::NonSecure,
        "StreamConfig default security_state must be NonSecure"
    );

    // Builder method must set the value
    let secure_config = StreamConfig::builder()
        .security_state(SecurityState::Secure)
        .build()
        .unwrap();
    assert_eq!(
        secure_config.security_state,
        SecurityState::Secure,
        "Builder security_state(Secure) must store Secure"
    );
}
