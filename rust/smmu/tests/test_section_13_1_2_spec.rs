#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

//! TDD conformance tests for ARM SMMU v3 §13.1.2 — INSTCFG write-as-Data rule
//!
//! ARM §13.1.2: "The SMMU considers all incoming writes to be Data, even if the
//! client device marks the incoming write as Instruction. This is done on input,
//! prior to any translation table permission checks."
//!
//! Additionally: "STE.INSTCFG and SMMU_(S_)GBPA.INSTCFG can only change the
//! instruction/data marking of reads."
//!
//! Bugs covered:
//! - BUG-13.1.2-A: `effective_access_type()` with INSTCFG=3 incorrectly converts
//!   Write → Execute and WritePrivileged → ExecutePrivileged.  This causes fault
//!   EventEntry fields rnw and ind to be mis-set for write faults on
//!   INSTCFG=3 streams (rnw=true/ind=true instead of rnw=false/ind=false).
//! - BUG-13.1.2-B: GBPA bypass path passes raw `gbpa.inst_cfg` without the
//!   write-as-Data filter, allowing a write translation to return inst_cfg=3.
//!
//! These tests MUST FAIL before the fixes are applied (red → green TDD).

use smmu::types::{AccessType, SecurityState, StreamConfig, StreamID};
use smmu::{GbpaConfig, SMMU};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

/// Build an enabled SMMU with a single stage-1-only stream whose INSTCFG=3
/// (Force Instruction) and EVENTQEN is active (smmu.enable() sets CR0.EVENTQEN).
/// The stream has no address-space mapping so any translate() call triggers a
/// translation fault → EventEntry is recorded via `effective_access_type()`.
fn make_smmu_instcfg3_stream(stream_n: u32) -> SMMU {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let mut cfg = StreamConfig::stage1_only();
    cfg.inst_cfg = 3; // INSTCFG = 0b11 (Force Instruction)
    smmu.configure_stream(sid(stream_n), cfg).unwrap();
    smmu.create_pasid(sid(stream_n), smmu::types::PASID::new(0).unwrap())
        .unwrap();
    smmu
}

// ---------------------------------------------------------------------------
// BUG-13.1.2-A: effective_access_type — INSTCFG=3 must NOT promote writes
// ---------------------------------------------------------------------------

/// §13.1.2: With INSTCFG=3 (Force Instruction), a Write fault must still record
/// `rnw=false` (ARM §7.3: RnW=0 means Write) and `ind=false` (Data, not Instruction).
///
/// The bug: `effective_access_type()` maps Write → Execute under INSTCFG=3.
/// The Execute result is passed into `record_translation_fault` at line 5549,
/// which then sets `rnw = !access.can_write()` (true for Execute → rnw=true) and
/// `ind = access.can_execute() && !access.can_write()` (true for Execute → ind=true).
///
/// After the fix Write falls through `_ => access_type` unchanged, so:
///   rnw = !Write.can_write() = false  (correct)
///   ind = Write.can_execute() && !Write.can_write() = false  (correct)
#[test]
fn test_instcfg3_write_fault_event_rnw_stays_false() {
    let smmu = make_smmu_instcfg3_stream(1001);

    // Translate to an unmapped address — guaranteed fault (F_TRANSLATION).
    let _ = smmu.translate(
        sid(1001),
        smmu::types::PASID::new(0).unwrap(),
        smmu::types::IOVA::new(0xDEAD_1000).unwrap(),
        AccessType::Write,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "BUG-13.1.2-A: expected a fault event to be recorded for unmapped Write"
    );
    let ev = &events[0];
    assert!(
        !ev.rnw,
        "BUG-13.1.2-A: Write fault with INSTCFG=3 must have rnw=false (ARM §7.3: RnW=0=Write). \
         Got rnw=true — effective_access_type() wrongly converted Write → Execute."
    );
    assert!(
        !ev.ind,
        "BUG-13.1.2-A: Write fault with INSTCFG=3 must have ind=false (Data, not Instruction). \
         §13.1.2 — INSTCFG can only change the instruction/data marking of reads. \
         Got ind=true — effective_access_type() wrongly converted Write → Execute."
    );
}

/// §13.1.2: With INSTCFG=3, a WritePrivileged fault must still record `rnw=false`
/// and `ind=false`. WritePrivileged must not be promoted to ExecutePrivileged.
#[test]
fn test_instcfg3_write_privileged_fault_event_rnw_stays_false() {
    let smmu = make_smmu_instcfg3_stream(1002);

    let _ = smmu.translate(
        sid(1002),
        smmu::types::PASID::new(0).unwrap(),
        smmu::types::IOVA::new(0xDEAD_2000).unwrap(),
        AccessType::WritePrivileged,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "BUG-13.1.2-A: expected a fault event for unmapped WritePrivileged"
    );
    let ev = &events[0];
    assert!(
        !ev.rnw,
        "BUG-13.1.2-A: WritePrivileged fault with INSTCFG=3 must have rnw=false. \
         Got rnw=true — effective_access_type() wrongly converted WritePrivileged → ExecutePrivileged."
    );
    assert!(
        !ev.ind,
        "BUG-13.1.2-A: WritePrivileged fault with INSTCFG=3 must have ind=false (Data). \
         Got ind=true — effective_access_type() wrongly converted WritePrivileged → ExecutePrivileged."
    );
}

/// §13.1.2 regression: With INSTCFG=3, a Read fault must still record `rnw=true`
/// (ARM §7.3: RnW=1=Read/Execute) and `ind=true` (Instruction fetch).
/// This confirms the fix does not break the read-→-execute promotion path.
#[test]
fn test_instcfg3_read_fault_event_ind_becomes_true_regression() {
    let smmu = make_smmu_instcfg3_stream(1003);

    let _ = smmu.translate(
        sid(1003),
        smmu::types::PASID::new(0).unwrap(),
        smmu::types::IOVA::new(0xDEAD_3000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "regression: expected a fault event for unmapped Read"
    );
    let ev = &events[0];
    assert!(
        ev.rnw,
        "regression: Read fault with INSTCFG=3 must have rnw=true (ARM §7.3: RnW=1=Read). \
         Got rnw=false — INSTCFG=3 Read→Execute promotion broken."
    );
    assert!(
        ev.ind,
        "regression: Read fault with INSTCFG=3 must have ind=true (Instruction). \
         INSTCFG=3 promotes Read → Execute. Got ind=false."
    );
}

/// §13.1.2 regression: With INSTCFG=3, a ReadPrivileged fault must record
/// `rnw=true` and `ind=true` (promoted to ExecutePrivileged).
#[test]
fn test_instcfg3_read_privileged_fault_event_ind_becomes_true_regression() {
    let smmu = make_smmu_instcfg3_stream(1004);

    let _ = smmu.translate(
        sid(1004),
        smmu::types::PASID::new(0).unwrap(),
        smmu::types::IOVA::new(0xDEAD_4000).unwrap(),
        AccessType::ReadPrivileged,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "regression: expected a fault event for unmapped ReadPrivileged"
    );
    let ev = &events[0];
    assert!(
        ev.rnw,
        "regression: ReadPrivileged fault with INSTCFG=3 must have rnw=true. \
         Got rnw=false — INSTCFG=3 ReadPrivileged→ExecutePrivileged promotion broken."
    );
    assert!(
        ev.ind,
        "regression: ReadPrivileged fault with INSTCFG=3 must have ind=true (Instruction). \
         Got ind=false."
    );
}

// ---------------------------------------------------------------------------
// BUG-13.1.2-B: GBPA bypass must clamp inst_cfg to 0 for write accesses
// ---------------------------------------------------------------------------

/// §13.1.2: When the SMMU is disabled (GBPA bypass path), GBPA.INSTCFG=3
/// (Force Instruction) must NOT be applied to write accesses. The output
/// `inst_cfg` for a write translation must be 0 (use-incoming / Data).
///
/// The bug: the bypass path at line 4724 of smmu/mod.rs returns `gbpa.inst_cfg`
/// raw without checking whether the access is a write.
#[test]
fn test_gbpa_bypass_write_inst_cfg_clamped_to_data() {
    // SMMU disabled (SMMUEN=0) — bypass path is taken.
    let smmu = SMMU::new();
    smmu.configure_stream(sid(2001), StreamConfig::bypass()).unwrap();

    // Set GBPA.INSTCFG=3 (Force Instruction).
    smmu.set_gbpa_config(GbpaConfig {
        inst_cfg: 3,
        ..GbpaConfig::default()
    });

    // Write access — GBPA.INSTCFG must NOT apply; inst_cfg in output must be 0.
    let result = smmu.translate(
        sid(2001),
        smmu::types::PASID::new(0).unwrap(),
        smmu::types::IOVA::new(0x1000).unwrap(),
        AccessType::Write,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_ok(),
        "GBPA bypass Write must succeed when GBPA.ABORT=false. Got: {result:?}"
    );
    let data = result.unwrap();
    assert_eq!(
        data.inst_cfg(),
        0u8,
        "BUG-13.1.2-B: GBPA bypass write must clamp inst_cfg to 0 (Data). \
         §13.1.2 — INSTCFG can only change the instruction/data marking of reads. \
         Got inst_cfg={}",
        data.inst_cfg()
    );
}

/// §13.1.2 regression: When the SMMU is disabled (GBPA bypass), a Read access
/// with GBPA.INSTCFG=3 should still return inst_cfg=3 in the output.
/// INSTCFG=3 applies to reads.
#[test]
fn test_gbpa_bypass_read_inst_cfg_preserved_regression() {
    let smmu = SMMU::new();
    smmu.configure_stream(sid(2002), StreamConfig::bypass()).unwrap();

    smmu.set_gbpa_config(GbpaConfig {
        inst_cfg: 3,
        ..GbpaConfig::default()
    });

    let result = smmu.translate(
        sid(2002),
        smmu::types::PASID::new(0).unwrap(),
        smmu::types::IOVA::new(0x1000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_ok(),
        "GBPA bypass Read must succeed when GBPA.ABORT=false. Got: {result:?}"
    );
    let data = result.unwrap();
    assert_eq!(
        data.inst_cfg(),
        3u8,
        "BUG-13.1.2-B regression: GBPA bypass read with inst_cfg=3 must preserve \
         inst_cfg=3 in output. Got inst_cfg={}",
        data.inst_cfg()
    );
}
