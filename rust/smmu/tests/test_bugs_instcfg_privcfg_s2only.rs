#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

//! TDD tests for three ARM SMMU v3 spec-compliance bugs:
//!
//! - BUG-1 (High): Stage-2-only faults have S2=false, IPA=0 in EventEntry.
//!   Spec §7.3.13: for a stage-2-only stream the IOVA is the IPA; any fault
//!   must emit s2=true and ipa=<iova value>.
//!
//! - BUG-2 (Moderate): INSTCFG=1 missing ReadPrivileged→ExecutePrivileged;
//!   INSTCFG=2 missing ExecutePrivileged→ReadPrivileged.
//!   Spec §13.1.2: INSTCFG affects ALL reads, privileged or not.
//!
//! - BUG-3 (Low): PRIVCFG not applied to AccessType in two-stage fast path
//!   (`translate_and_get_stage2_ipa`).  The `pnu` field in fault events for
//!   two-stage streams does not reflect PRIVCFG when PRIVCFG=3.
//!   Spec §3.3.4/§13.5.

use smmu::types::{
    AccessType, EventType, FaultMode, PagePermissions, SecurityState, StreamConfig, StreamID,
    IOVA, PA, PASID,
};
use smmu::SMMU;

// ── helpers ──────────────────────────────────────────────────────────────────

fn make_smmu() -> SMMU {
    let s = SMMU::new();
    s.enable().unwrap();
    s
}

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

// =============================================================================
// BUG-1: Stage-2-only streams — S2=false, IPA=0 in EventEntry
//
// ARM §7.3.13: "If translating an IPA for a transaction (whether by input to
// stage 2-only configuration...), CLASS == IN, and IPA is provided."
//
// BEFORE FIX: translate_and_get_stage2_ipa() lumps stage-2-only with
//             stage-1-only/bypass in the `else` branch, always returning
//             None for the IPA. The SMMU caller maps None→(false,0), so
//             EventEntry.s2=false and EventEntry.ipa=0 for stage-2-only faults.
// AFTER FIX:  Stage-2-only branch returns Some(iova.as_u64()) on error, so
//             EventEntry.s2=true and EventEntry.ipa == iova value.
// =============================================================================

/// BUG-1 Test 1: Stage-2-only stream, F_TRANSLATION on unmapped IPA.
///
/// A stage-2-only stream is configured with no entries in the stage-2 address
/// space.  Translating any address should fail with F_TRANSLATION, and the
/// resulting EventEntry MUST have s2=true and ipa equal to the IOVA (the IOVA
/// is the IPA for stage-2-only streams).
///
/// BEFORE FIX: s2=false, ipa=0 → test FAILS.
/// AFTER FIX:  s2=true, ipa != 0 (equals the input IOVA).
#[test]
fn bug1_stage2_only_ftranslation_s2_true_ipa_nonzero() {
    let smmu = make_smmu();
    let stream_id = sid(0xB1_00);

    let cfg = StreamConfig::builder()
        .stage1_enabled(false)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .s2_t0sz(25)
        .s2_ps(5)
        .build()
        .unwrap();
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();
    // Stage-2 address space intentionally left empty — any IPA faults.

    let input_iova: u64 = 0x4000;
    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(input_iova),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(result.is_err(), "Unmapped IPA must produce a fault; got Ok");

    let events = smmu.get_events();
    let ev = events
        .iter()
        .rev()
        .find(|e| e.stream_id == stream_id.as_u32())
        .expect("Expected an event for the stage-2-only stream");

    assert_eq!(
        ev.event_type,
        EventType::FTranslation,
        "BUG-1: expected F_TRANSLATION; got {:?}",
        ev.event_type
    );
    assert!(
        ev.s2,
        "BUG-1: stage-2-only fault must have s2=true; got s2=false. EventEntry={ev:?}"
    );
    assert_ne!(
        ev.ipa, 0,
        "BUG-1: stage-2-only fault must have ipa != 0 (ipa should equal IOVA {input_iova:#x}); got ipa=0"
    );
    assert_eq!(
        ev.ipa, input_iova,
        "BUG-1: stage-2-only fault ipa should equal input IOVA {input_iova:#x}; got ipa={:#x}",
        ev.ipa
    );
}

/// BUG-1 Test 2: Stage-2-only stream, S2T0SZ violation → F_TRANSLATION.
///
/// With S2T0SZ=32 the IPA range is 2^32 = 4 GiB.  An IPA at exactly 2^32 must
/// trigger F_TRANSLATION and the EventEntry must have s2=true, ipa == IOVA.
///
/// BEFORE FIX: s2=false, ipa=0 → test FAILS.
/// AFTER FIX:  s2=true, ipa == ipa_at_limit.
#[test]
fn bug1_stage2_only_s2t0sz_violation_s2_true_ipa_equals_iova() {
    let smmu = make_smmu();
    let stream_id = sid(0xB1_01);

    let cfg = StreamConfig::builder()
        .stage1_enabled(false)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .s2_t0sz(32) // IPA limit = 2^(64-32) = 2^32 = 4 GiB
        .s2_ps(5)
        .build()
        .unwrap();
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();

    let ipa_at_limit: u64 = 0x1_0000_0000; // exactly 2^32

    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(ipa_at_limit),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "BUG-1: IPA at S2T0SZ limit must produce a fault; got Ok"
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .rev()
        .find(|e| e.stream_id == stream_id.as_u32())
        .expect("Expected an event for the stage-2-only stream");

    assert!(
        ev.s2,
        "BUG-1: S2T0SZ violation on stage-2-only stream must have s2=true; got s2=false. EventEntry={ev:?}"
    );
    assert_eq!(
        ev.ipa, ipa_at_limit,
        "BUG-1: stage-2-only fault ipa should equal IOVA {ipa_at_limit:#x}; got ipa={:#x}",
        ev.ipa
    );
}

// =============================================================================
// BUG-2: INSTCFG missing privileged variants
//
// ARM §13.1.2: STE.INSTCFG can only change the instruction/data marking of
// reads — this applies to ALL reads, privileged or not.
//
// INSTCFG=1 (Force-Instruction): Read→Execute, ReadPrivileged→ExecutePrivileged
// INSTCFG=2 (Force-Data):        Execute→Read, ExecutePrivileged→ReadPrivileged
//
// BEFORE FIX:
//   - INSTCFG=1 only maps Read→Execute; ReadPrivileged passes through unchanged.
//     A ReadPrivileged on a data-only page is allowed (no execute check needed),
//     so the fault does NOT occur. The ind flag is false.
//   - INSTCFG=2 only maps Execute→Read; ExecutePrivileged passes through,
//     so the execute permission check fires and F_PERMISSION is raised with ind=true.
// AFTER FIX:
//   - INSTCFG=1: ReadPrivileged→ExecutePrivileged. On a data-only page (no execute
//     permission), the access is treated as an execute → F_PERMISSION with ind=true.
//   - INSTCFG=2: ExecutePrivileged→ReadPrivileged. The access is treated as a data
//     read; on a data-readable page it succeeds, or if faulting, ind=false.
// =============================================================================

/// BUG-2 Test 1: INSTCFG=1, ReadPrivileged on data-only page → F_PERMISSION, ind=true.
///
/// Setup: stage-1-only stream with INSTCFG=1 (Force-Instruction).
/// Page mapped with read-only (data) permissions.
/// Access: ReadPrivileged.
///
/// With INSTCFG=1, ReadPrivileged must become ExecutePrivileged.
/// The page has no execute permission → F_PERMISSION.
/// ind must be true (execute access in the event).
///
/// BEFORE FIX: ReadPrivileged passes through unchanged; data read on a
///             read-only page succeeds (returns Ok), ind never seen.
/// AFTER FIX:  Access treated as ExecutePrivileged → F_PERMISSION, ind=true.
#[test]
fn bug2_instcfg1_readprivileged_becomes_executeprivileged() {
    let smmu = make_smmu();
    let stream_id = sid(0xB2_00);

    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz = 0; // no VA range restriction
    cfg.inst_cfg = 1; // Force-Instruction: Read→Execute, ReadPrivileged→ExecutePrivileged
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    // Map page with read-only (data) permissions — NO execute permission.
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x1000),
        pa(0x9000),
        PagePermissions::read_only(), // no execute
        SecurityState::NonSecure,
    )
    .unwrap();

    // ReadPrivileged: with INSTCFG=1 this must become ExecutePrivileged.
    // The page has no execute permission → F_PERMISSION.
    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::ReadPrivileged,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "BUG-2: INSTCFG=1 ReadPrivileged on data-only page must fault (execute check); got Ok"
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .rev()
        .find(|e| e.stream_id == stream_id.as_u32())
        .expect("Expected an event for the stream");

    assert_eq!(
        ev.event_type,
        EventType::FPermission,
        "BUG-2: INSTCFG=1 ReadPrivileged→ExecutePrivileged on data-only page must be F_PERMISSION; got {:?}",
        ev.event_type
    );
    assert!(
        ev.ind,
        "BUG-2: INSTCFG=1 ReadPrivileged must be treated as execute (ind=true); got ind=false. EventEntry={ev:?}"
    );
}

/// BUG-2 Test 2: INSTCFG=2, ExecutePrivileged demoted to ReadPrivileged → ind=false if faults.
///
/// Setup: stage-1-only stream with INSTCFG=2 (Force-Data).
/// Page mapped with no permissions (unmapped) to force a fault.
/// Access: ExecutePrivileged.
///
/// With INSTCFG=2, ExecutePrivileged must become ReadPrivileged.
/// If the page is unmapped → F_TRANSLATION with ind=false (data access).
///
/// BEFORE FIX: ExecutePrivileged passes through unchanged → F_TRANSLATION but
///             ind=true (can_execute() on ExecutePrivileged is true).
/// AFTER FIX:  Access treated as ReadPrivileged → F_TRANSLATION, ind=false.
#[test]
fn bug2_instcfg2_executeprivileged_becomes_readprivileged() {
    let smmu = make_smmu();
    let stream_id = sid(0xB2_01);

    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz = 0; // no VA range restriction
    cfg.inst_cfg = 2; // Force-Data: Execute→Read, ExecutePrivileged→ReadPrivileged
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // Page intentionally unmapped — any access will produce F_TRANSLATION.

    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x2000),
        AccessType::ExecutePrivileged,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "BUG-2: unmapped page must fault; got Ok"
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .rev()
        .find(|e| e.stream_id == stream_id.as_u32())
        .expect("Expected an event for the stream");

    assert_eq!(
        ev.event_type,
        EventType::FTranslation,
        "BUG-2: expected F_TRANSLATION on unmapped page; got {:?}",
        ev.event_type
    );
    assert!(
        !ev.ind,
        "BUG-2: INSTCFG=2 ExecutePrivileged must be treated as data (ind=false); got ind=true. EventEntry={ev:?}"
    );
}

// =============================================================================
// BUG-3: PRIVCFG not applied in translate_and_get_stage2_ipa two-stage path
//
// ARM §3.3.4/§13.5: STE.PRIVCFG overrides the privilege level of the access
// before permission checks.  The `pnu` field in EventEntry must reflect the
// effective access type AFTER PRIVCFG is applied.
//
// PRIVCFG=3 (Force Privileged): all accesses are treated as privileged.
// A plain Read on a two-stage stream with PRIVCFG=3 must emit pnu=true.
//
// BEFORE FIX: translate_and_get_stage2_ipa() applies INSTCFG but not PRIVCFG
//             to effective_access; pnu for the event is derived from the
//             is_access_privileged() helper which does check PRIVCFG, but the
//             AccessType passed to the fault path hasn't been promoted, so the
//             pnu may be inconsistent with the actual effective access type
//             visible in the two-stage translation path.
//             For PRIVCFG=3: plain Read → access_type.is_privileged()=false but
//             is_access_effectively_privileged(Read) returns true (PRIVCFG=3
//             override). However the bug description says pnu is wrong because
//             effective_access isn't applied — we verify the pnu field equals
//             true for a plain Read with PRIVCFG=3 on a two-stage stream.
//
// AFTER FIX:  effective_access = ReadPrivileged after PRIVCFG=3 promotion;
//             the event's pnu field is true.
// =============================================================================

/// BUG-3 Test 1: Two-stage stream, PRIVCFG=3 (Force Privileged), plain Read → pnu=true.
///
/// A two-stage stream with PRIVCFG=3 is configured.  Stage-1 maps IOVA→IPA;
/// stage-2 is empty so any IPA faults.  A plain `Read` access (not privileged)
/// is issued.  With PRIVCFG=3 the access is force-promoted to privileged, so
/// the EventEntry.pnu must be true.
///
/// BEFORE FIX: effective_access stays Read in the two-stage path;
///             pnu=false (Read.is_privileged()==false) — test FAILS.
/// AFTER FIX:  effective_access becomes ReadPrivileged; pnu=true.
#[test]
fn bug3_twostage_privcfg3_plain_read_pnu_true() {
    let smmu = make_smmu();
    let stream_id = sid(0xB3_00);

    let mut cfg = StreamConfig::two_stage();
    cfg.t0sz = 0;
    cfg.s2_t0sz = 25; // valid S2T0SZ for 4KB/SL0=1 per §5.2 (BUG-AUDIT-88: min=22 for SL0=1)
    cfg.priv_cfg = 3; // Force Privileged
    cfg.security_state = SecurityState::NonSecure;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();

    // Stage-1 maps IOVA 0x1000 → IPA 0x5000.  Stage-2 is empty so the IPA faults.
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x1000),
        pa(0x5000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Plain Read (not privileged at the transaction level).
    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "BUG-3: two-stage with empty stage-2 must fault; got Ok"
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .rev()
        .find(|e| e.stream_id == stream_id.as_u32())
        .expect("Expected an event for the two-stage stream");

    assert!(
        ev.pnu,
        "BUG-3: two-stage stream with PRIVCFG=3 and plain Read must have pnu=true \
         (access promoted to privileged); got pnu=false. EventEntry={ev:?}"
    );
}
