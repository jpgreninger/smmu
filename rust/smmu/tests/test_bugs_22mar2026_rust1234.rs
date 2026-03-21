//! Failing tests (TDD step 1) for four confirmed bugs in the Rust SMMU
//! implementation.  Each test is designed to FAIL with the current buggy code
//! and PASS only after the corresponding fix is applied.
//!
//! # Bug Descriptions
//!
//! ## RUST-1: T0SZ inline F_TRANSLATION event uses pre-override access type for ind
//!
//! Location: `smmu/mod.rs` lines ~3580–3593 (T0SZ F_TRANSLATION inline path).
//!
//! The inline F_TRANSLATION event for T0SZ range violations sets `rnw` and `ind`
//! directly from the raw `access` parameter:
//! ```
//!   rnw: !access.can_write(),
//!   ind: access.can_execute() && !access.can_write(),
//! ```
//! This is incorrect when INSTCFG overrides the access type.  ARM §7.3.13
//! requires event fields to reflect the POST-STE-override effective access type.
//!
//! With INSTCFG=1 (Force Instruction) a Write access becomes Execute:
//!   raw Write:  ind = Write.can_execute() && !Write.can_write() = false
//!   effective Execute: ind = Execute.can_execute() && !Execute.can_write() = true
//!
//! ## RUST-2: C_BAD_CD inline event hardcodes ssv=false
//!
//! Location: `smmu/mod.rs` lines ~3503–3514 (C_BAD_CD inline event block).
//!
//! The `EventEntry` for C_BAD_CD uses `..EventEntry::zeroed()` with no explicit
//! `ssv` field, so `ssv` is always `false`.  ARM §7.3 requires
//! `ssv = (pasid != 0)` — the SubStream Valid bit must be set when the
//! transaction carries a non-zero PASID.
//!
//! Triggered when CD validation fails (AA64=false OR T0SZ/T1SZ out of range).
//!
//! ## RUST-3: C_* events set rnw/ind from access type (RES0 per ARM §7.3)
//!
//! Locations:
//!   - `record_stream_not_found_fault` (~line 4463): C_BAD_STREAMID sets rnw/ind.
//!   - `record_bad_ste_fault` (~line 4538): C_BAD_STE sets rnw/ind.
//!
//! ARM IHI0070G.b §7.3 wire formats for all C_* events show bits 100 (RnW) and
//! 99 (InD) as RES0.  Setting them from the access type is incorrect.
//!
//! ## RUST-4: F_STREAM_DISABLED inline event sets rnw/ind/ssv (all RES0)
//!
//! Location: `smmu/mod.rs` lines ~3818–3822 (S1DSS=0b00 abort path).
//!
//! The F_STREAM_DISABLED event sets rnw, ind, and ssv from the access type
//! and PASID.  ARM §7.3.7 shows all three fields as RES0 for this event.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

use smmu::types::{
    AccessType, EventType, SecurityState, StreamConfig, StreamID, PASID,
    IOVA,
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

fn sid(id: u32) -> StreamID {
    StreamID::new(id).unwrap()
}

fn pasid(id: u32) -> PASID {
    PASID::new(id).unwrap()
}

// ============================================================================
// RUST-1: T0SZ inline F_TRANSLATION event — ind wrong when INSTCFG overrides access
// ============================================================================
//
// Bug: the inline F_TRANSLATION event at the T0SZ range check uses the raw
// `access` parameter to compute `rnw` and `ind` (lines ~3586-3587):
//
//   rnw: !access.can_write(),
//   ind: access.can_execute() && !access.can_write(),
//
// When INSTCFG=1 (Force Instruction) is active, a Write access becomes Execute
// after STE override.  The event should reflect the POST-override type:
//   ind = true   (Execute is an instruction fetch)
// But the raw Write gives:
//   ind = false  (Write.can_execute() == false)
//
// Setup: stream with inst_cfg=1 (Force Instruction) and t0sz=16 (VA limit
// 2^(64-16) = 2^48).  Issue a Write to an address that exceeds 2^48.
// The T0SZ check fires before the address-space lookup.
//
// BEFORE FIX: event.ind == false (raw Write) → assertion fails.
// AFTER FIX:  event.ind == true  (effective Execute, INSTCFG=1)→ passes.

/// RUST-1: T0SZ F_TRANSLATION with INSTCFG=1 and Write — ind must be true.
///
/// INSTCFG=1 forces all Data writes to become instruction fetches.  The resulting
/// F_TRANSLATION event must carry ind=true (instruction access) not ind=false
/// (raw Write).
///
/// BEFORE FIX: ind=false  → test FAILS.
/// AFTER FIX:  ind=true   → test PASSES.
#[test]
fn rust1_t0sz_f_translation_instcfg1_write_ind_is_true() {
    let smmu = make_smmu();
    let stream_id = sid(0xB1_01);

    // INSTCFG=1 (Force Instruction): Write → Execute after STE override.
    // T0SZ=16 → VA limit = 2^(64-16) = 2^48 = 0x0001_0000_0000_0000.
    // Address 0x0001_0000_0000_0000 is exactly at the limit (violates T0SZ).
    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz     = 16;           // VA limit = 2^48
    cfg.inst_cfg = 1;            // Force Instruction: Write → Execute
    cfg.aa64     = true;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    // IOVA at the limit — triggers T0SZ range check.
    let violating_iova = iova(0x0001_0000_0000_0000u64);

    let result = smmu.translate(
        stream_id,
        pasid(0),
        violating_iova,
        AccessType::Write, // raw Write; INSTCFG=1 → effective Execute
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "RUST-1: T0SZ range violation must produce an error, got {result:?}"
    );

    // Find the F_TRANSLATION event for this stream.
    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::FTranslation)
        })
        .expect("RUST-1: Expected an F_TRANSLATION event for the T0SZ violation");

    // The effective access after INSTCFG=1 is Execute → ind must be true.
    //
    // ARM §7.3.13: event fields must reflect the POST-STE-override access type.
    //   ind = (effective access is instruction fetch) = true for Execute.
    assert!(
        ev.ind,
        "RUST-1: F_TRANSLATION event from T0SZ violation with INSTCFG=1 and Write access \
         must have ind=true (effective Execute after INSTCFG override). \
         Got ind=false — bug: raw Write.can_execute()=false used instead of \
         effective Execute.can_execute()=true."
    );
}

/// RUST-1 regression guard: T0SZ F_TRANSLATION with no INSTCFG override —
/// ind must be false for a plain Write (regression guard, passes before and after fix).
#[test]
fn rust1_t0sz_f_translation_no_instcfg_write_ind_is_false() {
    let smmu = make_smmu();
    let stream_id = sid(0xB1_02);

    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz     = 16;   // VA limit = 2^48
    cfg.inst_cfg = 0;    // No INSTCFG override
    cfg.aa64     = true;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    let violating_iova = iova(0x0001_0000_0000_0000u64);
    let _ = smmu.translate(
        stream_id,
        pasid(0),
        violating_iova,
        AccessType::Write,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::FTranslation)
        })
        .expect("RUST-1 guard: Expected an F_TRANSLATION event");

    // No override: raw Write → ind=false.
    assert!(
        !ev.ind,
        "RUST-1 regression guard: Write with no INSTCFG override → ind must be false. \
         got ind=true"
    );
}

/// RUST-1: T0SZ F_TRANSLATION with INSTCFG=1 and Write — rnw must be false.
///
/// With INSTCFG=1 and Write → effective Execute.  Execute.can_write()=false
/// so rnw = !can_write() = true.  But for raw Write: !Write.can_write() = false.
///
/// ARM §7.3.13: rnw must reflect the EFFECTIVE (post-override) access type.
///
/// BEFORE FIX: rnw=false (raw Write) → test FAILS.
/// AFTER FIX:  rnw=true  (effective Execute, !can_write() → true) → test PASSES.
#[test]
fn rust1_t0sz_f_translation_instcfg1_write_rnw_is_true() {
    let smmu = make_smmu();
    let stream_id = sid(0xB1_03);

    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz     = 16;
    cfg.inst_cfg = 1;  // Force Instruction: Write → Execute
    cfg.aa64     = true;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    let violating_iova = iova(0x0001_0000_0000_0000u64);
    let _ = smmu.translate(
        stream_id,
        pasid(0),
        violating_iova,
        AccessType::Write, // raw Write; INSTCFG=1 → effective Execute
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::FTranslation)
        })
        .expect("RUST-1: Expected an F_TRANSLATION event");

    // With INSTCFG=1: effective Execute → rnw = !Execute.can_write() = !false = true.
    // Bug: raw Write → rnw = !Write.can_write() = !true = false.
    assert!(
        ev.rnw,
        "RUST-1: F_TRANSLATION event from T0SZ violation with INSTCFG=1 and Write access \
         must have rnw=true (effective Execute: not a write). \
         Got rnw=false — bug: raw Write.can_write()=true used, yielding !true=false, \
         instead of effective Execute.can_write()=false → !false=true."
    );
}

// ============================================================================
// RUST-2: C_BAD_CD inline event hardcodes ssv=false (should be pasid != 0)
// ============================================================================
//
// The EventEntry for C_BAD_CD is built with `..EventEntry::zeroed()` and no
// explicit `ssv` field (line ~3513).  This forces ssv=false regardless of the
// PASID.  ARM §7.3 requires ssv = (pasid.as_u32() != 0).
//
// C_BAD_CD is triggered when CD validation fails at the top of the translate()
// slow path.  Conditions that trigger it (line ~3484):
//   - !aa64  (AArch32 LPAE unsupported)
//   - t0sz > 39
//   - t1sz > 39
//
// Setup: stream with stage1_enabled=true and t0sz=40 (out of range > 39).
// Translate with pasid != 0.  The resulting C_BAD_CD event must have ssv=true.
//
// BEFORE FIX: ssv=false (..EventEntry::zeroed()) → assertion fails.
// AFTER FIX:  ssv = pasid.as_u32() != 0 = true  → assertion passes.

/// RUST-2: C_BAD_CD event with non-zero PASID must have ssv=true.
///
/// BEFORE FIX: ssv=false → test FAILS.
/// AFTER FIX:  ssv=true  → test PASSES.
#[test]
fn rust2_c_bad_cd_nonzero_pasid_has_ssv_true() {
    let smmu = make_smmu();
    let stream_id = sid(0xB2_01);

    // t0sz=40 exceeds the valid range [0,39] → C_BAD_CD at line ~3484.
    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz  = 40;  // invalid: > 39 → C_BAD_CD
    cfg.aa64  = true;
    smmu.configure_stream(stream_id, cfg).unwrap();
    // Create PASID 1 (non-zero) so that ssv must be true.
    smmu.create_pasid(stream_id, pasid(1)).unwrap();

    // Translate with PASID=1.  T0SZ validation fails → C_BAD_CD before reaching
    // address-space lookup, so no page mapping is required.
    let result = smmu.translate(
        stream_id,
        pasid(1),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "RUST-2: t0sz=40 must cause a C_BAD_CD error, got {result:?}"
    );

    // Find the C_BAD_CD event.
    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::CBadCd)
        })
        .expect("RUST-2: Expected a C_BAD_CD event for t0sz=40 validation failure");

    // ssv must be true because PASID=1 != 0.
    assert!(
        ev.ssv,
        "RUST-2: C_BAD_CD event with non-zero PASID (pasid=1) must have ssv=true \
         (ARM §7.3: ssv = pasid != 0). \
         Got ssv=false — bug: ..EventEntry::zeroed() does not set ssv field."
    );
}

/// RUST-2: C_BAD_CD event with zero PASID must have ssv=false (regression guard).
///
/// Passes before and after the fix.
#[test]
fn rust2_c_bad_cd_zero_pasid_has_ssv_false() {
    let smmu = make_smmu();
    let stream_id = sid(0xB2_02);

    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz  = 40;   // invalid: > 39 → C_BAD_CD
    cfg.aa64  = true;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    let _ = smmu.translate(
        stream_id,
        pasid(0), // zero PASID → ssv must be false
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::CBadCd)
        })
        .expect("RUST-2 guard: Expected a C_BAD_CD event");

    // PASID=0 → ssv must be false (this passes before and after the fix).
    assert!(
        !ev.ssv,
        "RUST-2 regression guard: C_BAD_CD with PASID=0 must have ssv=false. \
         Got ssv=true."
    );
}

/// RUST-2: C_BAD_CD triggered by AA64=false must also have ssv=true for PASID!=0.
///
/// BEFORE FIX: ssv=false → test FAILS.
/// AFTER FIX:  ssv=true  → test PASSES.
#[test]
fn rust2_c_bad_cd_aa64_false_nonzero_pasid_has_ssv_true() {
    let smmu = make_smmu();
    let stream_id = sid(0xB2_03);

    // aa64=false triggers C_BAD_CD per ARM §5.4 / CT-14.
    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz = 16;    // valid t0sz; aa64=false is the trigger
    cfg.aa64 = false; // AArch32 LPAE unsupported → C_BAD_CD
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(5)).unwrap(); // PASID=5 (non-zero)

    let result = smmu.translate(
        stream_id,
        pasid(5),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "RUST-2: aa64=false must produce a C_BAD_CD error, got {result:?}"
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::CBadCd)
        })
        .expect("RUST-2: Expected a C_BAD_CD event for aa64=false");

    assert!(
        ev.ssv,
        "RUST-2: C_BAD_CD (aa64=false trigger) with PASID=5 must have ssv=true. \
         Got ssv=false — bug: ..EventEntry::zeroed() in the C_BAD_CD inline block."
    );
}

// ============================================================================
// RUST-3: C_BAD_STREAMID and C_BAD_STE set rnw/ind from access type (RES0)
// ============================================================================
//
// ARM IHI0070G.b §7.3 wire format tables show bits 100(RnW) and 99(InD) as
// RES0 for all C_* configuration events.  `record_stream_not_found_fault`
// (~line 4463) and `record_bad_ste_fault` (~line 4538) both set:
//   rnw: !access.can_write()
//   ind: access.can_execute() && !access.can_write()
// These fields must be zero (false) for all C_* events regardless of access type.
//
// C_BAD_STREAMID trigger: translate with a StreamID that was never configured
//   AND CR2.RECINVSID=1 (gates C_BAD_STREAMID recording, §7.3.3).
//
// C_BAD_STE trigger: translate with a StreamID that IS within the stream-table
//   bounds but has STE.V=0 (stream configured in streams map as invalid/absent).
//   The easiest way: use a reserved STE.Config combination.
//   Actually, C_BAD_STE fires when `record_bad_ste_fault` is called, which
//   happens in the "stream not found in DashMap" path that is separate from
//   "stream ID out of range".  Let's use a stream that was configured with
//   a reserved config so that it fires C_BAD_STE.
//
// BEFORE FIX: rnw=true for a Read access (since !Read.can_write()=true) → FAILS.
// AFTER FIX:  rnw=false (RES0 for C_* events) → PASSES.

/// RUST-3: C_BAD_STREAMID event from a Read access must have rnw=false (RES0).
///
/// Trigger: set strtab_log2size=4 (stream-table limit = 16 entries) so that
/// StreamID=20 is out-of-range.  `record_stream_not_found_fault` is called,
/// which emits C_BAD_STREAMID (gated on CR2.RECINVSID=1).
///
/// With a Read access the bug sets rnw=!Read.can_write()=true; should be false.
///
/// BEFORE FIX: rnw=true  → test FAILS.
/// AFTER FIX:  rnw=false → test PASSES.
#[test]
fn rust3_c_bad_streamid_read_access_rnw_is_false() {
    let smmu = make_smmu();

    // Restrict stream-table to 2^4=16 entries; StreamID=20 is out-of-range.
    smmu.set_strtab_log2size(4);
    // Enable CR2.RECINVSID so C_BAD_STREAMID events are recorded (§7.3.3).
    smmu.set_cr2(SMMU::CR2_RECINVSID);

    // StreamID 20 >= 16 = 2^4 → out-of-range → C_BAD_STREAMID.
    let out_of_range_sid = sid(20);

    // Read access — bug: rnw = !Read.can_write() = !false = true (should be 0).
    let result = smmu.translate(
        out_of_range_sid,
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "RUST-3: Out-of-range StreamID must produce an error, got {result:?}"
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == out_of_range_sid.as_u32()
                && matches!(e.event_type, EventType::CBadStreamid)
        })
        .expect(
            "RUST-3: Expected a C_BAD_STREAMID event \
             (strtab_log2size=4, StreamID=20 >= 16, CR2.RECINVSID=1)",
        );

    // ARM §7.3 RES0: rnw must be false for C_* events.
    assert!(
        !ev.rnw,
        "RUST-3: C_BAD_STREAMID event from Read access must have rnw=false (RES0 per \
         ARM §7.3). Got rnw=true — bug: rnw set from !access.can_write()."
    );
    // ind must also be false (RES0).
    assert!(
        !ev.ind,
        "RUST-3: C_BAD_STREAMID event must have ind=false (RES0 per ARM §7.3). \
         Got ind=true."
    );
}

/// RUST-3: C_BAD_STE event from a Read access must have rnw=false (RES0).
///
/// Trigger: translate with a StreamID that was never configured (strtab_log2size=32,
/// default, so no range limit).  The DashMap lookup misses, and `record_bad_ste_fault`
/// is called (line ~3645), emitting C_BAD_STE.  C_BAD_STE is NOT gated by
/// CR2.RECINVSID (§6.3.12).
///
/// With a Read access the bug sets rnw=!Read.can_write()=true; should be false.
///
/// BEFORE FIX: rnw=true  → test FAILS.
/// AFTER FIX:  rnw=false → test PASSES.
#[test]
fn rust3_c_bad_ste_read_access_rnw_is_false() {
    let smmu = make_smmu();

    // Do NOT set strtab_log2size — default is 32 (no range limit).
    // Do NOT configure this stream — it's absent from the DashMap.
    // DashMap miss with strtab_log2size=32 → record_bad_ste_fault → C_BAD_STE.
    // C_BAD_STE is NOT gated by CR2.RECINVSID per §6.3.12.
    let unconfigured_sid = sid(0xDE02);

    // Read access — bug: rnw = !Read.can_write() = !false = true (should be 0).
    let result = smmu.translate(
        unconfigured_sid,
        pasid(0),
        iova(0x2000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "RUST-3: Unconfigured stream must produce an error, got {result:?}"
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == unconfigured_sid.as_u32()
                && matches!(e.event_type, EventType::CBadSte)
        })
        .expect(
            "RUST-3: Expected a C_BAD_STE event for unconfigured stream \
             (DashMap miss → record_bad_ste_fault, not gated by RECINVSID)",
        );

    // ARM §7.3 RES0: rnw must be false for C_* events.
    assert!(
        !ev.rnw,
        "RUST-3: C_BAD_STE event from Read access must have rnw=false (RES0 per \
         ARM §7.3). Got rnw=true — bug: rnw set from !access.can_write()."
    );
    // ind must also be false (RES0).
    assert!(
        !ev.ind,
        "RUST-3: C_BAD_STE event must have ind=false (RES0 per ARM §7.3). \
         Got ind=true."
    );
}

/// RUST-3: C_BAD_STREAMID with Execute access — ind must be false (RES0).
///
/// Trigger: strtab_log2size=4 (limit=16), StreamID=21 (>= 16) → out-of-range
/// → record_stream_not_found_fault → C_BAD_STREAMID (CR2.RECINVSID=1).
///
/// Execute.can_execute()=true, Execute.can_write()=false →
///   ind = true && true = true (wrong — should be 0/false for C_* events, RES0).
///
/// BEFORE FIX: ind=true  → test FAILS.
/// AFTER FIX:  ind=false → test PASSES.
#[test]
fn rust3_c_bad_streamid_execute_access_ind_is_false() {
    let smmu = make_smmu();
    // Restrict stream-table to 2^4=16 entries; StreamID=21 is out-of-range.
    smmu.set_strtab_log2size(4);
    smmu.set_cr2(SMMU::CR2_RECINVSID);

    // StreamID 21 >= 16 = 2^4 → out-of-range → C_BAD_STREAMID.
    let out_of_range_sid = sid(21);

    // Execute access: bug ind = Execute.can_execute() && !Execute.can_write() = true.
    let result = smmu.translate(
        out_of_range_sid,
        pasid(0),
        iova(0x3000),
        AccessType::Execute, // ind = true if set from raw access type
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "RUST-3: Out-of-range StreamID must fail, got {result:?}"
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == out_of_range_sid.as_u32()
                && matches!(e.event_type, EventType::CBadStreamid)
        })
        .expect(
            "RUST-3: Expected a C_BAD_STREAMID event for Execute access \
             (strtab_log2size=4, StreamID=21 >= 16, CR2.RECINVSID=1)",
        );

    // ARM §7.3 RES0: ind must be false for C_* events.
    assert!(
        !ev.ind,
        "RUST-3: C_BAD_STREAMID event from Execute access must have ind=false \
         (RES0 per ARM §7.3). \
         Got ind=true — bug: ind set from access.can_execute() && !access.can_write() \
         = true for Execute."
    );
    assert!(
        !ev.rnw,
        "RUST-3: C_BAD_STREAMID event from Execute access must have rnw=false (RES0). \
         Got rnw=true."
    );
}

// ============================================================================
// RUST-4: F_STREAM_DISABLED inline event sets rnw/ind/ssv (all RES0)
// ============================================================================
//
// The F_STREAM_DISABLED event is emitted when S1DSS=0b00 (abort) fires for a
// non-substream (PASID=0) transaction on a substream-capable stream (s1cd_max>0).
//
// Lines ~3818-3822 set:
//   rnw: !access.can_write()   — must be 0 (RES0) per ARM §7.3.7
//   ind: access.can_execute() && !access.can_write()  — must be 0 (RES0)
//   ssv: pasid.as_u32() != 0   — must be 0 (RES0) per ARM §7.3.7
//
// Setup: configure a stream with s1cd_max=1 and s1dss=0 (abort).
// Issue a Read translation with PASID=0.  The S1DSS=0b00 arm fires and emits
// F_STREAM_DISABLED.
//
// For Read access:
//   rnw (bug) = !Read.can_write() = !false = true     (should be false)
//   ind (bug) = false                                  (happens to be correct for Read)
//   ssv (bug) = pasid.as_u32() != 0 = false            (happens to be correct for PASID=0)
//
// To detect rnw: use Read access → rnw should be 0 but is 1 with bug.
// To detect ssv: use PASID != 0 but that's not possible on this path because
//   the S1DSS block only fires for PASID=0 on substream-capable streams.
//   We can't demonstrate ssv with PASID=0 (ssv would be 0 either way).
//   Instead we verify rnw=false for Read and ind=false.
//
// BEFORE FIX: rnw=true (Read) → assertion fails.
// AFTER FIX:  rnw=false (RES0) → assertion passes.

/// RUST-4: F_STREAM_DISABLED event from Read access must have rnw=false (RES0).
///
/// ARM §7.3.7 wire format: bits 100(RnW) and 99(InD) are RES0 for F_STREAM_DISABLED.
///
/// BEFORE FIX: rnw=true  (set from !Read.can_write() = true) → test FAILS.
/// AFTER FIX:  rnw=false (RES0)                              → test PASSES.
#[test]
fn rust4_f_stream_disabled_read_access_rnw_is_false() {
    let smmu = make_smmu();
    let stream_id = sid(0xB4_01);

    // s1cd_max=1 makes the stream substream-capable.
    // s1dss=0 means: for PASID=0 on a substream-capable stream → abort with F_STREAM_DISABLED.
    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz     = 0;
    cfg.s1cd_max = 1;  // substream-capable (s1cd_max > 0 activates S1DSS routing)
    cfg.s1dss    = 0;  // S1DSS=0b00 → abort with F_STREAM_DISABLED for PASID=0
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    // Translate with PASID=0 (triggers S1DSS routing) and Read access.
    let result = smmu.translate(
        stream_id,
        pasid(0), // PASID=0 triggers S1DSS routing on s1cd_max>0 stream
        iova(0x1000),
        AccessType::Read, // rnw = !Read.can_write() = true if bug present
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "RUST-4: S1DSS=0b00 on substream-capable stream must abort, got {result:?}"
    );

    // Find the F_STREAM_DISABLED event.
    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::FStreamDisabled)
        })
        .expect("RUST-4: Expected an F_STREAM_DISABLED event for S1DSS=0b00 abort");

    // ARM §7.3.7: rnw is RES0 for F_STREAM_DISABLED — must be false.
    assert!(
        !ev.rnw,
        "RUST-4: F_STREAM_DISABLED event from Read access must have rnw=false \
         (RES0 per ARM §7.3.7). \
         Got rnw=true — bug: rnw set from !access.can_write() = !false = true for Read."
    );
    // ind must also be false (RES0).
    assert!(
        !ev.ind,
        "RUST-4: F_STREAM_DISABLED event must have ind=false (RES0 per ARM §7.3.7). \
         Got ind=true."
    );
}

/// RUST-4: F_STREAM_DISABLED with ssv must be false even for ssv-triggering case.
///
/// F_STREAM_DISABLED fires ONLY for PASID=0 (the non-substream path).
/// So pasid.as_u32() != 0 = false → ssv=false even with the bug.
/// This test documents the ssv behavior (regression guard — passes before and after fix).
///
/// The ssv bug (ssv: pasid.as_u32() != 0) is present in the code but produces
/// the correct value (false) because this path only fires for PASID=0.
/// We document this edge case: ssv must be false regardless of the bug.
#[test]
fn rust4_f_stream_disabled_ssv_is_false() {
    let smmu = make_smmu();
    let stream_id = sid(0xB4_02);

    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz     = 0;
    cfg.s1cd_max = 1;
    cfg.s1dss    = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    let _ = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x2000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::FStreamDisabled)
        })
        .expect("RUST-4: Expected F_STREAM_DISABLED event");

    // ssv must be false (PASID=0 → ssv=false; also RES0 per §7.3.7).
    assert!(
        !ev.ssv,
        "RUST-4: F_STREAM_DISABLED with PASID=0 must have ssv=false (RES0 per §7.3.7). \
         Got ssv=true."
    );
}

/// RUST-4: F_STREAM_DISABLED event from Execute access — ind must be false (RES0).
///
/// Execute: ind = Execute.can_execute() && !Execute.can_write() = true (with bug).
/// Must be false (RES0).
///
/// BEFORE FIX: ind=true  → test FAILS.
/// AFTER FIX:  ind=false → test PASSES.
#[test]
fn rust4_f_stream_disabled_execute_access_ind_is_false() {
    let smmu = make_smmu();
    let stream_id = sid(0xB4_03);

    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz     = 0;
    cfg.s1cd_max = 1;
    cfg.s1dss    = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    // Execute access: ind = Execute.can_execute() && !Execute.can_write() = true if bug present.
    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x3000),
        AccessType::Execute,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "RUST-4: S1DSS=0b00 must abort, got {result:?}"
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::FStreamDisabled)
        })
        .expect("RUST-4: Expected F_STREAM_DISABLED event for Execute access");

    // ind must be false (RES0) per ARM §7.3.7.
    assert!(
        !ev.ind,
        "RUST-4: F_STREAM_DISABLED event from Execute access must have ind=false \
         (RES0 per ARM §7.3.7). \
         Got ind=true — bug: ind set from access.can_execute() && !access.can_write() \
         = true && true = true for Execute."
    );
}
