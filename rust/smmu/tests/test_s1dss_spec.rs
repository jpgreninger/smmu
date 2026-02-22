//! ARM SMMU v3 FINDING-NEW-17 and FINDING-NEW-18 spec tests
//!
//! Tests for:
//! - `CommandEntry.leaf` field (ARM §4.3.1 / §4.3.3)
//! - `StreamConfig.s1dss` and `s1cd_max` fields (ARM §3.9 / §5.2)
//! - S1DSS routing for non-substream (PASID==0) transactions on substream-capable streams

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventType, PagePermissions, SecurityState, StreamConfig,
    StreamID, IOVA, PA, PASID,
};
use smmu::SMMU;

fn make_stream_id(v: u32) -> StreamID {
    StreamID::new(v).unwrap()
}

fn make_pasid(v: u32) -> PASID {
    PASID::new(v).unwrap()
}

fn make_iova(v: u64) -> IOVA {
    IOVA::new(v).unwrap()
}

// ── FINDING-NEW-17: CommandEntry has a `leaf` field ───────────────────────────

/// ARM §4.3.1 / §4.3.3: `CommandEntry` must have a `leaf` boolean field.
/// The default value must be `false` (Leaf=0).
#[test]
fn command_entry_has_leaf_field_default_false() {
    let cmd = CommandEntry::new(CommandType::CfgiSte, 1, 0);
    assert!(!cmd.leaf, "CommandEntry.leaf must default to false");
}

/// The `leaf` field must be writable (not read-only / derived).
#[test]
fn command_entry_leaf_field_settable() {
    let mut cmd = CommandEntry::new(CommandType::CfgiSte, 1, 0);
    cmd.leaf = true;
    assert!(cmd.leaf, "CommandEntry.leaf must be settable to true");
}

/// Leaf=false survives a round-trip through `new()`.
#[test]
fn command_entry_leaf_false_round_trip() {
    let cmd = CommandEntry::new(CommandType::CfgiCd, 5, 3);
    assert!(!cmd.leaf, "CommandEntry.leaf must remain false after new()");
}

/// `CfgiCd` command also defaults `leaf` to false.
#[test]
fn command_entry_cfgi_cd_leaf_default_false() {
    let cmd = CommandEntry::new(CommandType::CfgiCd, 10, 7);
    assert!(!cmd.leaf, "CfgiCd CommandEntry.leaf must default to false");
}

// ── FINDING-NEW-18: StreamConfig has `s1dss` and `s1cd_max` fields ────────────

/// ARM §5.2: `StreamConfig::bypass()` must initialise `s1dss` to 2 (use CD[0]).
#[test]
fn stream_config_has_s1dss_default_2() {
    let cfg = StreamConfig::bypass();
    assert_eq!(cfg.s1dss, 2, "s1dss must default to 0b10 (use CD[0])");
}

/// ARM §5.2: `StreamConfig::bypass()` must initialise `s1cd_max` to 0 (not substream-capable).
#[test]
fn stream_config_has_s1cd_max_default_0() {
    let cfg = StreamConfig::bypass();
    assert_eq!(cfg.s1cd_max, 0, "s1cd_max must default to 0 (not substream-capable)");
}

/// `StreamConfig::stage1_only()` must also carry the correct defaults.
#[test]
fn stream_config_stage1_only_defaults() {
    let cfg = StreamConfig::stage1_only();
    assert_eq!(cfg.s1dss, 2);
    assert_eq!(cfg.s1cd_max, 0);
}

/// `StreamConfig::two_stage()` must carry the correct defaults.
#[test]
fn stream_config_two_stage_defaults() {
    let cfg = StreamConfig::two_stage();
    assert_eq!(cfg.s1dss, 2);
    assert_eq!(cfg.s1cd_max, 0);
}

/// `StreamConfig` fields `s1dss` and `s1cd_max` must be independently settable.
#[test]
fn stream_config_fields_settable() {
    let mut cfg = StreamConfig::stage1_only();
    cfg.s1dss = 0x01;
    cfg.s1cd_max = 4;
    assert_eq!(cfg.s1dss, 0x01);
    assert_eq!(cfg.s1cd_max, 4);
}

// ── NEW-18: s1cd_max==0 preserves existing behaviour — s1dss ignored ──────────

/// When `s1cd_max == 0` the stream is not substream-capable.
/// `s1dss` must be ignored and PASID=0 uses CD[0] normally.
#[test]
fn s1cd_max_zero_s1dss_ignored_pasid0_uses_cd0() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = make_stream_id(0x50);
    let pasid0 = make_pasid(0);
    let iova = make_iova(0x2000);

    let mut cfg = StreamConfig::stage1_only();
    cfg.s1dss = 0x00; // would abort if s1cd_max > 0, but s1cd_max==0 so ignored
    cfg.s1cd_max = 0;
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, pasid0).unwrap();

    let pa = PA::new(0x8000_0000).unwrap();
    smmu.map_page(sid, pasid0, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let r = smmu.translate(sid, pasid0, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(r.is_ok(), "s1cd_max==0 must not apply S1DSS routing: {r:?}");
    assert_eq!(r.unwrap().physical_address().as_u64(), 0x8000_0000u64);
}

// ── NEW-18 S1DSS==0b00: abort with F_STREAM_DISABLED ─────────────────────────

/// ARM §7.3.7 / §5.2: S1DSS==0b00 — non-substream (PASID=0) transaction on a
/// substream-capable stream must be aborted and generate an `F_STREAM_DISABLED` event.
#[test]
fn s1dss_00_pasid0_aborts_with_f_stream_disabled() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = make_stream_id(0x51);
    let pasid0 = make_pasid(0);
    let iova = make_iova(0x2000);

    let mut cfg = StreamConfig::stage1_only();
    cfg.s1dss = 0x00; // non-substream transactions abort
    cfg.s1cd_max = 4; // stream supports up to 2^4 substreams
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, pasid0).unwrap();

    let pa = PA::new(0x8000_0000).unwrap();
    smmu.map_page(sid, pasid0, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let r = smmu.translate(sid, pasid0, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(r.is_err(), "S1DSS==0b00: PASID=0 must abort");

    let events = smmu.get_events();
    assert!(!events.is_empty(), "S1DSS==0b00 must generate F_STREAM_DISABLED event");
    assert_eq!(
        events[0].event_type,
        EventType::FStreamDisabled,
        "Event must be F_STREAM_DISABLED (0x06)"
    );
}

/// S1DSS==0b00 abort must not succeed even with a valid mapping present.
#[test]
fn s1dss_00_abort_independent_of_mapping() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = make_stream_id(0x54);
    let pasid0 = make_pasid(0);
    let iova = make_iova(0x4000);

    let mut cfg = StreamConfig::stage1_only();
    cfg.s1dss = 0x00;
    cfg.s1cd_max = 8;
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, pasid0).unwrap();

    // Valid mapping exists — S1DSS overrides the result
    let pa = PA::new(0xA000_0000).unwrap();
    smmu.map_page(sid, pasid0, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let r = smmu.translate(sid, pasid0, iova, AccessType::Write, SecurityState::NonSecure);
    assert!(r.is_err(), "S1DSS==0b00 must abort even when mapping exists");
}

// ── NEW-18 S1DSS==0b01: bypass stage-1 (identity PA) ─────────────────────────

/// ARM §3.9 S1DSS==0b01: non-substream (PASID=0) transactions bypass stage-1
/// and receive an identity mapping (PA == IOVA).
#[test]
fn s1dss_01_pasid0_bypasses_stage1() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = make_stream_id(0x52);
    let pasid0 = make_pasid(0);
    let iova_val = 0x2000u64;
    let iova = make_iova(iova_val);

    let mut cfg = StreamConfig::stage1_only();
    cfg.s1dss = 0x01; // bypass stage-1 for non-substream
    cfg.s1cd_max = 4;
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, pasid0).unwrap();
    // No page mapped — bypass means identity PA regardless

    let r = smmu.translate(sid, pasid0, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(r.is_ok(), "S1DSS==0b01: PASID=0 must succeed with bypass: {r:?}");
    let data = r.unwrap();
    assert_eq!(
        data.physical_address().as_u64(),
        iova_val,
        "S1DSS==0b01 bypass: PA must equal IOVA (identity mapping)"
    );
}

/// S1DSS==0b01: bypass must not depend on having a mapping.
#[test]
fn s1dss_01_bypass_works_without_mapping() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = make_stream_id(0x55);
    let pasid0 = make_pasid(0);
    let iova_val = 0xCAFE_0000u64;
    let iova = make_iova(iova_val);

    let mut cfg = StreamConfig::stage1_only();
    cfg.s1dss = 0x01;
    cfg.s1cd_max = 2;
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, pasid0).unwrap();
    // Intentionally no map_page call

    let r = smmu.translate(sid, pasid0, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(r.is_ok(), "S1DSS==0b01 must bypass even without mapping: {r:?}");
    assert_eq!(r.unwrap().physical_address().as_u64(), iova_val);
}

// ── NEW-18 S1DSS==0b10: use CD[0] (normal translation) ───────────────────────

/// ARM §5.2 S1DSS==0b10: non-substream (PASID=0) uses CD[0] — the default path.
#[test]
fn s1dss_10_pasid0_uses_cd0() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = make_stream_id(0x53);
    let pasid0 = make_pasid(0);
    let iova = make_iova(0x2000);

    let mut cfg = StreamConfig::stage1_only();
    cfg.s1dss = 0x02; // use CD[0] (default)
    cfg.s1cd_max = 4;
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, pasid0).unwrap();

    let pa = PA::new(0x8000_0000).unwrap();
    smmu.map_page(sid, pasid0, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let r = smmu.translate(sid, pasid0, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(r.is_ok(), "S1DSS==0b10: PASID=0 uses CD[0] and must succeed: {r:?}");
    let data = r.unwrap();
    assert_eq!(
        data.physical_address().as_u64(),
        0x8000_0000u64,
        "S1DSS==0b10 CD[0] lookup must return mapped PA"
    );
}

/// S1DSS==0b10 with no mapping must fail with a translation fault (not a stream-disabled event).
#[test]
fn s1dss_10_pasid0_no_mapping_translation_fault() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = make_stream_id(0x56);
    let pasid0 = make_pasid(0);
    let iova = make_iova(0x9000);

    let mut cfg = StreamConfig::stage1_only();
    cfg.s1dss = 0x02;
    cfg.s1cd_max = 4;
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, pasid0).unwrap();
    // No mapping

    let r = smmu.translate(sid, pasid0, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(r.is_err(), "S1DSS==0b10 with no mapping must fail");

    let events = smmu.get_events();
    // Must NOT be FStreamDisabled — it should be a translation fault
    for ev in &events {
        assert_ne!(
            ev.event_type,
            EventType::FStreamDisabled,
            "S1DSS==0b10 must not emit FStreamDisabled"
        );
    }
}

// ── Interaction: non-zero PASID is never affected by S1DSS ───────────────────

/// S1DSS routing only applies to PASID==0.  Non-zero PASIDs must use their
/// own CD entry regardless of S1DSS.
#[test]
fn s1dss_does_not_affect_nonzero_pasid() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let sid = make_stream_id(0x57);
    let pasid0 = make_pasid(0);
    let pasid1 = make_pasid(1);
    let iova = make_iova(0x3000);

    let mut cfg = StreamConfig::stage1_only();
    cfg.s1dss = 0x00; // would abort PASID=0, must not affect PASID=1
    cfg.s1cd_max = 4;
    smmu.configure_stream(sid, cfg).unwrap();
    smmu.create_pasid(sid, pasid0).unwrap();
    smmu.create_pasid(sid, pasid1).unwrap();

    let pa = PA::new(0xDEAD_0000).unwrap();
    smmu.map_page(sid, pasid1, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    // PASID=1 should translate normally
    let r = smmu.translate(sid, pasid1, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(r.is_ok(), "Non-zero PASID must not be affected by S1DSS: {r:?}");
    assert_eq!(r.unwrap().physical_address().as_u64(), 0xDEAD_0000u64);
}
