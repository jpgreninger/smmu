#![allow(clippy::doc_markdown)]
//! ARM SMMU v3 FINDING-NEW-29 and FINDING-NEW-30 spec tests
//!
//! Tests for:
//! - FINDING-NEW-29: Two-stage permission intersection absent (§3.3.1)
//! - FINDING-NEW-30: CMD_STALL_TERM uses STAG lookup not StreamID sweep (§4.7.2)

use smmu::types::{
    AccessType, CommandEntry, CommandType, FaultMode, PagePermissions, SecurityState,
    StreamConfig, TranslationError, StreamID, PASID, IOVA, PA,
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

/// Set up a two-stage stream and map a page in stage-1 with the given permissions,
/// then map the IPA → PA in stage-2 with a different set of permissions.
/// Returns (smmu, stage1_iova, stage1_pa, stage2_pa).
fn setup_two_stage_stream(
    stream_n: u32,
    s1_perms: PagePermissions,
    s2_perms: PagePermissions,
) -> SMMU {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(sid(stream_n), cfg).unwrap();
    smmu.create_pasid(sid(stream_n), pasid(0)).unwrap();

    // Stage-1: IOVA 0x1000 → IPA 0x2000 (with s1_perms)
    let ipa = PA::new(0x2000).unwrap();
    smmu.map_page(sid(stream_n), pasid(0), iova(0x1000), ipa, s1_perms, SecurityState::NonSecure)
        .unwrap();

    // Stage-2: IPA 0x2000 → PA 0x3000 (with s2_perms)
    smmu.create_stage2_address_space(sid(stream_n)).unwrap();
    smmu.map_stage2_page(
        sid(stream_n),
        iova(0x2000),
        PA::new(0x3000).unwrap(),
        s2_perms,
        SecurityState::NonSecure,
    )
    .unwrap();

    smmu
}

// ── FINDING-NEW-29: Two-stage permission intersection (§3.3.1) ───────────────

/// Stage-1 read-only, Stage-2 read-write → write must be denied.
/// Before the fix the SMMU returns Stage-2's permissions directly, so write
/// would succeed even though Stage-1 forbids it.
#[test]
fn two_stage_s1_readonly_s2_readwrite_write_denied() {
    let smmu = setup_two_stage_stream(
        0x40,
        PagePermissions::read_only(),
        PagePermissions::read_write(),
    );

    // Read must succeed (both stages allow it)
    let read_result = smmu.translate(
        sid(0x40),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        read_result.is_ok(),
        "§3.3.1: read must succeed when both stages allow read, got: {read_result:?}"
    );

    // Write must fail: Stage-1 is read-only
    let write_result = smmu.translate(
        sid(0x40),
        pasid(0),
        iova(0x1000),
        AccessType::Write,
        SecurityState::NonSecure,
    );
    assert!(
        matches!(write_result, Err(TranslationError::PermissionViolation { .. })),
        "§3.3.1 / FINDING-NEW-29: write must be denied when Stage-1 is read-only, got: {write_result:?}"
    );
}

/// Stage-1 read-write, Stage-2 read-only → write must be denied.
/// Before the fix the SMMU returns Stage-2's permissions, so a read-write
/// Stage-1 + read-only Stage-2 would still allow write (wrong).
#[test]
fn two_stage_s1_readwrite_s2_readonly_write_denied() {
    let smmu = setup_two_stage_stream(
        0x41,
        PagePermissions::read_write(),
        PagePermissions::read_only(),
    );

    // Read must succeed (both stages allow it)
    let read_result = smmu.translate(
        sid(0x41),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        read_result.is_ok(),
        "§3.3.1: read must succeed when both stages allow read, got: {read_result:?}"
    );

    // Write must fail: Stage-2 is read-only
    let write_result = smmu.translate(
        sid(0x41),
        pasid(0),
        iova(0x1000),
        AccessType::Write,
        SecurityState::NonSecure,
    );
    assert!(
        matches!(write_result, Err(TranslationError::PermissionViolation { .. })),
        "§3.3.1 / FINDING-NEW-29: write must be denied when Stage-2 is read-only, got: {write_result:?}"
    );
}

/// Stage-1 read-write, Stage-2 read-write → write must succeed.
#[test]
fn two_stage_both_readwrite_write_succeeds() {
    let smmu = setup_two_stage_stream(
        0x42,
        PagePermissions::read_write(),
        PagePermissions::read_write(),
    );

    let write_result = smmu.translate(
        sid(0x42),
        pasid(0),
        iova(0x1000),
        AccessType::Write,
        SecurityState::NonSecure,
    );
    assert!(
        write_result.is_ok(),
        "§3.3.1: write must succeed when both stages allow read-write, got: {write_result:?}"
    );
}

// ── FINDING-NEW-30: CMD_STALL_TERM must clear all stall records for a stream ─

/// CMD_STALL_TERM for a StreamID must remove ALL stall records for that stream,
/// not just one (the one matching the STAG in the command).
#[test]
fn stall_term_clears_all_stalls_for_stream() {
    let smmu = make_smmu();

    // --- Set up stream 0x50 with stall mode (two different PASIDs) ---
    let cfg_stall = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0x50), cfg_stall.clone()).unwrap();
    smmu.create_pasid(sid(0x50), pasid(0)).unwrap();
    smmu.create_pasid(sid(0x50), pasid(1)).unwrap();

    // --- Set up stream 0x51 with stall mode (one PASID) ---
    smmu.configure_stream(sid(0x51), cfg_stall).unwrap();
    smmu.create_pasid(sid(0x51), pasid(0)).unwrap();

    // Trigger two stall faults on stream 0x50 (unmapped addresses, different PASIDs)
    let r0 = smmu.translate(sid(0x50), pasid(0), iova(0xA000), AccessType::Read, SecurityState::NonSecure);
    assert!(matches!(r0, Err(TranslationError::Stalled { .. })), "expected stall on stream 0x50 pasid 0, got: {r0:?}");

    let r1 = smmu.translate(sid(0x50), pasid(1), iova(0xB000), AccessType::Read, SecurityState::NonSecure);
    assert!(matches!(r1, Err(TranslationError::Stalled { .. })), "expected stall on stream 0x50 pasid 1, got: {r1:?}");

    // Trigger one stall fault on stream 0x51
    let r2 = smmu.translate(sid(0x51), pasid(0), iova(0xC000), AccessType::Read, SecurityState::NonSecure);
    assert!(matches!(r2, Err(TranslationError::Stalled { .. })), "expected stall on stream 0x51 pasid 0, got: {r2:?}");

    // Verify we have at least 2 stall records for stream 0x50 and 1 for 0x51
    let stalls_before = smmu.get_stalled_transactions();
    let s50_before = stalls_before.iter().filter(|s| s.stream_id == 0x50).count();
    let s51_before = stalls_before.iter().filter(|s| s.stream_id == 0x51).count();
    assert!(s50_before >= 2, "expected >= 2 stall records for stream 0x50 before STALL_TERM, got {s50_before}");
    assert_eq!(s51_before, 1, "expected 1 stall record for stream 0x51 before STALL_TERM");

    // Issue CMD_STALL_TERM for stream 0x50 — must clear ALL stall records for it
    let mut cmd = CommandEntry::new(CommandType::StallTerm, 0x50, 0);
    cmd.stag = 0; // STAG=0: per §4.7.2 CMD_STALL_TERM uses StreamID, not STAG
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // After CMD_STALL_TERM(stream=0x50): 0 stall records for stream 0x50, 1 for 0x51
    let stalls_after = smmu.get_stalled_transactions();
    let s50_after = stalls_after.iter().filter(|s| s.stream_id == 0x50).count();
    let s51_after = stalls_after.iter().filter(|s| s.stream_id == 0x51).count();
    assert_eq!(
        s50_after, 0,
        "§4.7.2 / FINDING-NEW-30: CMD_STALL_TERM must clear ALL stall records for stream 0x50, found {s50_after}"
    );
    assert_eq!(
        s51_after, 1,
        "§4.7.2 / FINDING-NEW-30: stream 0x51 stall record must be unaffected, found {s51_after}"
    );
}
