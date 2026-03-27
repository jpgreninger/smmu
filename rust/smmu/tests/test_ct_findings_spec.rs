#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]

//! ARM SMMU v3 CT-09, CT-13, CT-14, CT-19, CT-20, CT-23, CT-30, CT-33 spec compliance tests
//!
//! Tests for findings from QA review:
//! - CT-30: Missing Command Opcodes (§4.1.1)
//! - CT-20: `STE.STRW` (`StreamWorld`) field (§5.2)
//! - CT-19: STE Output-Attribute Override Fields (§5.2)
//! - CT-23: Stage-2 STE Translation Parameters (§5.2)
//! - CT-14: `CD.AA64` Field (§5.4)
//! - CT-13: `CD.T0SZ` / `CD.T1SZ` Fields + `C_BAD_CD` Validation (§5.4)
//! - CT-33: `CR0.CMDQEN` / `CR0.EVENTQEN` / `CR0.PRIQEN` Queue Enable Gates (§4.1.2, §7.2.1)
//! - CT-09: `STE.Config==0b000` must abort silently without event (§5.2)

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventType, SecurityState, StreamConfig, StreamID, IOVA,
    PASID,
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

fn pasid(n: u32) -> PASID {
    PASID::new(n).unwrap()
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

// ============================================================================
// CT-30: Missing Command Opcodes (§4.1.1)
// ============================================================================

/// §4.1.1 complete command opcode table verification
#[test]
fn test_all_spec_command_opcodes_present() {
    assert_eq!(CommandType::CfgiVmsPidm as u8, 0x07);
    assert_eq!(CommandType::TlbiEl3All as u8, 0x18);
    assert_eq!(CommandType::TlbiEl3Va as u8, 0x1A);
    assert_eq!(CommandType::TlbiSEl2All as u8, 0x50);
    assert_eq!(CommandType::TlbiSEl2Asid as u8, 0x51);
    assert_eq!(CommandType::TlbiSEl2Va as u8, 0x52);
    assert_eq!(CommandType::TlbiSEl2Vaa as u8, 0x53);
    assert_eq!(CommandType::TlbiSS12Vmall as u8, 0x58);
    assert_eq!(CommandType::TlbiSS2Ipa as u8, 0x5A);
    assert_eq!(CommandType::TlbiSnhAll as u8, 0x60);
    assert_eq!(CommandType::DptiAll as u8, 0x70);
    assert_eq!(CommandType::DptiPa as u8, 0x73);
}

/// New opcodes should be usable in `CommandEntry`
#[test]
fn test_new_opcodes_usable_in_command_entry() {
    let cmd = CommandEntry::new(CommandType::CfgiVmsPidm, 0, 0);
    assert_eq!(cmd.cmd_type, CommandType::CfgiVmsPidm);

    let cmd2 = CommandEntry::new(CommandType::TlbiEl3All, 0, 0);
    assert_eq!(cmd2.cmd_type, CommandType::TlbiEl3All);

    let cmd3 = CommandEntry::new(CommandType::DptiPa, 0, 0);
    assert_eq!(cmd3.cmd_type, CommandType::DptiPa);
}

/// BUG-NEW-28/29/30/32: Secure-queue-only opcodes and MPAM commands must raise
/// `CERROR_ILL` on the Non-secure Command queue.
///
/// Per ARM §4.4.2.11–14, §4.4.4.2, §4.4.3.3, §4.4.3.4, and §4.3.5, the
/// following commands are illegal on the NS queue and each must set
/// `CMDQ_CONS.ERR=CERROR_ILL`:
///   - `TlbiSEl2All`  (§4.4.2.11)
///   - `TlbiSEl2Va`   (§4.4.2.12)
///   - `TlbiSEl2Vaa`  (§4.4.2.13)
///   - `TlbiSEl2Asid` (§4.4.2.14)
///   - `TlbiSnhAll`   (§4.4.4.2)
///   - `TlbiSS2Ipa`   (§4.4.3.3)
///   - `TlbiSS12Vmall` (§4.4.3.4)
///   - `CfgiVmsPidm`  (§4.3.5: `IDR3.MPAM=0` → `CERROR_ILL`)
///
/// (Previously this test asserted they processed without error, which was
/// incorrect pre-BUG-NEW-28/29/30/32 fix.)
#[test]
fn test_new_opcodes_processable_in_command_queue() {
    // DPTI_ALL / DPTI_PA excluded: require IDR3.DPT=1; result in CERROR_ILL
    // when DPT is not supported (§4.6.1, CONF-GAP-4 fix).
    //
    // BUG-QA-9 fix (§4.4.2.5): TlbiEl3All is valid ONLY on the Secure Command
    // queue and is excluded from this list.
    //
    // BUG-NEW-28/29/30/32: all 8 remaining "new" opcodes also raise CERROR_ILL
    // on the NS queue — each is tested individually below.
    let secure_only_opcodes = [
        CommandType::TlbiSEl2All,
        CommandType::TlbiSEl2Asid,
        CommandType::TlbiSEl2Va,
        CommandType::TlbiSEl2Vaa,
        CommandType::TlbiSS12Vmall,
        CommandType::TlbiSS2Ipa,
        CommandType::TlbiSnhAll,
        CommandType::CfgiVmsPidm,
    ];

    for opcode in secure_only_opcodes {
        let smmu = make_smmu();
        let cmd = CommandEntry::new(opcode, 0, 0);
        smmu.submit_command(cmd).unwrap();
        let _ = smmu.process_command_queue();
        assert_eq!(
            smmu.get_cmdq_cons_err(),
            SMMU::CERROR_ILL,
            "{opcode:?} on NS queue must write CERROR_ILL per ARM spec"
        );
    }
}

// ============================================================================
// CT-20: STE.STRW (StreamWorld) field (§5.2)
// ============================================================================

use smmu::types::StreamWorld;

/// §5.2 STRW field present and settable via builder
#[test]
fn test_strw_field_present_in_stream_config() {
    let config = StreamConfig::builder()
        .strw(StreamWorld::El2)
        .build()
        .unwrap();
    assert_eq!(config.strw, StreamWorld::El2);

    // Default should be El1El0
    let default_config = StreamConfig::builder().build().unwrap();
    assert_eq!(default_config.strw, StreamWorld::El1El0);
}

/// All `StreamWorld` variants must have correct repr values
#[test]
fn test_stream_world_repr_values() {
    assert_eq!(StreamWorld::El1El0 as u8, 0x00);
    assert_eq!(StreamWorld::El2 as u8, 0x01);
    assert_eq!(StreamWorld::El2E2h as u8, 0x02);
    assert_eq!(StreamWorld::El3 as u8, 0x03);
}

/// `StreamWorld` can be stored and retrieved round-trip
#[test]
fn test_stream_world_round_trip_all_variants() {
    for variant in [
        StreamWorld::El1El0,
        StreamWorld::El2,
        StreamWorld::El2E2h,
        StreamWorld::El3,
    ] {
        let config = StreamConfig::builder().strw(variant).build().unwrap();
        assert_eq!(config.strw, variant);
    }
}

// ============================================================================
// CT-19: STE Output-Attribute Override Fields (§5.2)
// ============================================================================

/// §5.2 STE output attribute override fields present in `StreamConfig`
#[test]
fn test_ste_output_attribute_override_fields_present() {
    let config = StreamConfig::builder()
        .ns_cfg(0x2)
        .sh_cfg(0x1)
        .alloc_cfg(0xF)
        .mem_attr(0x4)
        .mt_cfg(true)
        .build()
        .unwrap();

    assert_eq!(config.ns_cfg, 0x2);
    assert_eq!(config.sh_cfg, 0x1);
    assert_eq!(config.alloc_cfg, 0xF);
    assert_eq!(config.mem_attr, 0x4);
    assert!(config.mt_cfg);
}

/// §5.2 `inst_cfg` and `priv_cfg` fields present
#[test]
fn test_ste_inst_cfg_priv_cfg_fields_present() {
    let config = StreamConfig::builder()
        .inst_cfg(0x2)
        .priv_cfg(0x1)
        .build()
        .unwrap();

    assert_eq!(config.inst_cfg, 0x2);
    assert_eq!(config.priv_cfg, 0x1);
}

/// §5.2 all override fields default to 0 / false
#[test]
fn test_ste_output_attribute_fields_default_to_zero() {
    let config = StreamConfig::builder().build().unwrap();

    assert_eq!(config.ns_cfg, 0);
    assert_eq!(config.sh_cfg, 0);
    assert_eq!(config.alloc_cfg, 0);
    assert_eq!(config.mem_attr, 0);
    assert_eq!(config.inst_cfg, 0);
    assert_eq!(config.priv_cfg, 0);
    assert!(!config.mt_cfg);
}

// ============================================================================
// CT-23: Stage-2 STE Translation Parameters (§5.2)
// ============================================================================

/// §5.2 Stage-2 STE translation parameters present in `StreamConfig`
#[test]
fn test_stage2_ste_parameters_present_in_stream_config() {
    let config = StreamConfig::builder()
        .s2_t0sz(16)
        .s2_tg(0)
        .s2_sl0(1)
        .s2_aa64(true)
        .s2_ps(5)
        .s2_ttb(0x100_000)
        .build()
        .unwrap();

    assert_eq!(config.s2_t0sz, 16);
    assert_eq!(config.s2_tg, 0);
    assert_eq!(config.s2_sl0, 1);
    assert!(config.s2_aa64);
    assert_eq!(config.s2_ps, 5);
    assert_eq!(config.s2_ttb, 0x100_000);
}

/// §5.2 Stage-2 fields default to spec defaults
#[test]
fn test_stage2_ste_parameters_default_values() {
    let config = StreamConfig::builder().build().unwrap();

    assert_eq!(config.s2_t0sz, 16);
    assert_eq!(config.s2_tg, 0);
    assert_eq!(config.s2_sl0, 1);
    assert!(config.s2_aa64);
    assert_eq!(config.s2_ps, 5);
    assert_eq!(config.s2_ttb, 0);
}

/// §5.2 Stage-2 parameters survive round-trip through builder
#[test]
fn test_stage2_ste_parameters_round_trip() {
    let config = StreamConfig::builder()
        .s2_t0sz(32)
        .s2_tg(1)
        .s2_sl0(2)
        .s2_aa64(false)
        .s2_ps(3)
        .s2_ttb(0xDEAD_0000)
        .build()
        .unwrap();

    assert_eq!(config.s2_t0sz, 32);
    assert_eq!(config.s2_tg, 1);
    assert_eq!(config.s2_sl0, 2);
    assert!(!config.s2_aa64);
    assert_eq!(config.s2_ps, 3);
    assert_eq!(config.s2_ttb, 0xDEAD_0000);
}

// ============================================================================
// CT-14: CD.AA64 Field (§5.4)
// ============================================================================

/// §5.4 `CD.AA64` field present in `StreamConfig`
#[test]
fn test_aa64_field_present_in_stream_config() {
    let config = StreamConfig::builder().aa64(true).build().unwrap();
    assert!(config.aa64);

    let config2 = StreamConfig::builder().aa64(false).build().unwrap();
    assert!(!config2.aa64);
}

/// §5.4 CD.AA64 defaults to true (VMSAv8-64)
#[test]
fn test_aa64_field_defaults_to_true() {
    let config = StreamConfig::builder().build().unwrap();
    assert!(config.aa64, "CD.AA64 must default to true (VMSAv8-64)");
}

// ============================================================================
// CT-13: CD.T0SZ / CD.T1SZ Fields + C_BAD_CD Validation (§5.4)
// ============================================================================

/// §5.4 `T0SZ` and `T1SZ` fields present in `StreamConfig`
#[test]
fn test_t0sz_t1sz_fields_present_in_stream_config() {
    let config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .t0sz(16)
        .t1sz(16)
        .build()
        .unwrap();

    assert_eq!(config.t0sz, 16);
    assert_eq!(config.t1sz, 16);
}

/// §5.4 T0SZ/T1SZ default to 16 (48-bit address space)
#[test]
fn test_t0sz_t1sz_default_values() {
    let config = StreamConfig::builder().build().unwrap();
    assert_eq!(config.t0sz, 16, "T0SZ must default to 16 (48-bit)");
    assert_eq!(config.t1sz, 16, "T1SZ must default to 16 (48-bit)");
}

/// §5.4 Out-of-range `T0SZ` (> 39) generates `C_BAD_CD` event
#[test]
fn test_out_of_range_t0sz_generates_c_bad_cd() {
    let smmu = make_smmu();
    let config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .t0sz(40) // invalid for SMMUv3.0: valid range is 0-39
        .build()
        .unwrap();

    smmu.configure_stream(sid(1), config).unwrap();

    let result = smmu.translate(sid(1), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "Out-of-range T0SZ must abort translation");

    let events = smmu.get_events();
    assert!(!events.is_empty(), "C_BAD_CD event must be recorded for invalid T0SZ");
    assert_eq!(
        events[0].event_type,
        EventType::CBadCd,
        "Event type must be CBadCd (C_BAD_CD)"
    );
}

/// §5.4 Out-of-range `T1SZ` (> 39) generates `C_BAD_CD` event
#[test]
fn test_out_of_range_t1sz_generates_c_bad_cd() {
    let smmu = make_smmu();
    let config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .t1sz(40) // invalid for SMMUv3.0
        .build()
        .unwrap();

    smmu.configure_stream(sid(2), config).unwrap();

    let result = smmu.translate(sid(2), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "Out-of-range T1SZ must abort translation");

    let events = smmu.get_events();
    assert!(!events.is_empty(), "C_BAD_CD event must be recorded for invalid T1SZ");
    assert!(
        events.iter().any(|e| e.event_type == EventType::CBadCd),
        "At least one CBadCd event must be present"
    );
}

/// §5.4 Valid `T0SZ=39` (boundary) must NOT generate `C_BAD_CD`
#[test]
fn test_valid_t0sz_boundary_no_event() {
    let smmu = make_smmu();
    let config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .t0sz(39) // valid boundary value
        .build()
        .unwrap();

    smmu.configure_stream(sid(3), config).unwrap();

    // Just verify no C_BAD_CD event for the valid boundary
    let events = smmu.get_events();
    assert!(
        !events.iter().any(|e| e.event_type == EventType::CBadCd),
        "Valid T0SZ=39 must not generate C_BAD_CD"
    );
}

// ============================================================================
// CT-33: CR0 Queue Enable Gates (§4.1.2, §7.2.1)
// ============================================================================

/// §4.1.2 Command queue disabled when CMDQEN=0
#[test]
fn test_command_queue_disabled_when_cmdqen_not_set() {
    let smmu = SMMU::new();
    smmu.set_cr0(0); // CR0.CMDQEN=0

    // Submit a CFGI_ALL command
    let cmd = CommandEntry::new(CommandType::CfgiAll, 0, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue(); // should be no-op

    // Verify command was not processed (queue should still have the entry)
    assert_eq!(
        smmu.get_command_queue_size(),
        1,
        "Command queue should be unprocessed when CMDQEN=0"
    );
}

/// §7.2.1 Event not recorded when EVENTQEN=0
#[test]
fn test_event_not_recorded_when_eventqen_not_set() {
    let smmu = SMMU::new();
    // Enable SMMU but NOT the event queue
    smmu.set_cr0(SMMU::CR0_SMMUEN);

    // Trigger an event by translating with unknown stream
    let _ = smmu.translate(sid(9999), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);

    // Event must NOT be recorded since EVENTQEN=0
    assert!(
        smmu.get_events().is_empty(),
        "Events must not be recorded when EVENTQEN=0"
    );
}

/// §4.1.2 Command queue processed when CMDQEN=1
#[test]
fn test_command_queue_processed_when_cmdqen_set() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_SMMUEN);

    let cmd = CommandEntry::new(CommandType::CfgiAll, 0, 0);
    smmu.submit_command(cmd).unwrap();

    let result = smmu.process_command_queue();
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 1, "Command must be processed when CMDQEN=1");
    assert_eq!(smmu.get_command_queue_size(), 0, "Queue should be empty after processing");
}

/// §7.2.1 Event recorded when EVENTQEN=1
#[test]
fn test_event_recorded_when_eventqen_set() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);
    // §6.3.12 / BUG-NEW-RUST-2: RECINVSID=1 required for C_BAD_STREAMID events
    // to be recorded for unknown-stream translate() calls.
    smmu.set_cr2(SMMU::CR2_RECINVSID);

    // Trigger an event by translating with unknown stream
    let _ = smmu.translate(sid(8888), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);

    // Event must be recorded since EVENTQEN=1 and RECINVSID=1
    assert!(
        !smmu.get_events().is_empty(),
        "Events must be recorded when EVENTQEN=1"
    );
}

/// §6.3.9 CR0.SMMUEN controls bypass behavior
#[test]
fn test_cr0_smmuen_controls_smmu_enable() {
    let smmu = SMMU::new();

    // SMMUEN=0 → bypass mode (same as default disabled state)
    smmu.set_cr0(0);
    assert!(!smmu.is_enabled(), "SMMUEN=0 must leave SMMU disabled");

    // SMMUEN=1 → SMMU active
    smmu.set_cr0(SMMU::CR0_SMMUEN);
    assert!(smmu.is_enabled(), "SMMUEN=1 must enable SMMU");
}

/// CR0 get/set round-trip
#[test]
fn test_cr0_get_set_round_trip() {
    let smmu = SMMU::new();

    let value = SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN;
    smmu.set_cr0(value);
    assert_eq!(smmu.get_cr0(), value, "CR0 get/set must round-trip");
}

/// §4.1.2 PRI queue disabled when PRIQEN=0
#[test]
fn test_pri_queue_disabled_when_priqen_not_set() {
    use smmu::types::PRIEntry;

    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN); // PRIQEN=0

    let req = PRIEntry::new(1, 0);
    let result = smmu.submit_page_request(req);
    assert!(
        result.is_err(),
        "PRI queue submissions must fail when PRIQEN=0: got {result:?}"
    );
}

/// §4.1.2 PRI queue active when PRIQEN=1
#[test]
fn test_pri_queue_active_when_priqen_set() {
    use smmu::types::PRIEntry;

    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_PRIQEN);

    let req = PRIEntry::new(1, 0);
    let result = smmu.submit_page_request(req);
    assert!(result.is_ok(), "PRI queue must accept requests when PRIQEN=1");
}

// ============================================================================
// CT-09: STE.Config==0b000 aborts silently (§5.2)
// ============================================================================

/// §5.2 STE.Config==0b000: abort without any event recorded
#[test]
fn test_ste_config_0b000_aborts_silently_no_event() {
    let smmu = make_smmu();
    let config = StreamConfig::builder()
        .translation_enabled(false)
        .stage1_enabled(false)
        .stage2_enabled(false)
        .build()
        .unwrap();
    smmu.configure_stream(sid(1), config).unwrap();

    let result = smmu.translate(sid(1), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "STE.Config==0b000 must abort the transaction");

    // CRITICAL: no event should be recorded
    assert!(
        smmu.get_events().is_empty(),
        "STE.Config==0b000 must NOT generate any event in the event queue"
    );
}

/// §5.2 STE.Config==0b000: fault queue (internal) must also NOT receive events
#[test]
fn test_ste_config_0b000_no_fault_record() {
    let smmu = make_smmu();
    // Also enable event queue
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    let config = StreamConfig::builder()
        .translation_enabled(false)
        .stage1_enabled(false)
        .stage2_enabled(false)
        .build()
        .unwrap();
    smmu.configure_stream(sid(5), config).unwrap();

    let _ = smmu.translate(sid(5), pasid(0), iova(0x2000), AccessType::Write, SecurityState::NonSecure);

    // Still no event even with EVENTQEN=1 — spec §5.2 disallows it
    assert!(
        smmu.get_events().is_empty(),
        "STE.Config==0b000 must NOT generate any event even when EVENTQEN=1"
    );
}

/// §5.2 Contrast: bypass stream (Config==0b100) does NOT abort
#[test]
fn test_ste_config_bypass_returns_identity_mapping() {
    let smmu = make_smmu();
    let config = StreamConfig::builder().build().unwrap(); // defaults to bypass
    smmu.configure_stream(sid(2), config).unwrap();

    let result = smmu.translate(sid(2), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    // In bypass mode (enabled SMMU + bypass config), translation should succeed with identity mapping
    assert!(result.is_ok(), "Bypass stream must return identity mapping, not abort");
    assert_eq!(
        result.unwrap().physical_address().as_u64(),
        0x1000,
        "Bypass stream must return PA == IOVA"
    );
}
