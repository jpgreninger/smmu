//! Tests for NEW-GAP-J (Access Flag Fault), NEW-GAP-K (WXN), NEW-GAP-L (S2PTW)
//!
//! - NEW-GAP-J (§3.13.2): `F_ACCESS` when AF=0, `ha=false`, `affd=false`
//! - NEW-GAP-K (§5.4): `WXN` blocks execute on writable page
//! - NEW-GAP-L (§5.2): `S2PTW` blocks translation through device-memory stage-2 page
#![allow(missing_docs)]
use smmu::types::{
    AccessType, IOVA, PagePermissions, PASID, SecurityState, StreamConfig, StreamID,
};
use smmu::SMMU;

fn make_smmu() -> SMMU {
    let s = SMMU::new();
    s.enable().unwrap();
    s
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn pa(addr: u64) -> smmu::types::PA {
    smmu::types::PA::new(addr).unwrap()
}

fn sid(id: u32) -> StreamID {
    StreamID::new(id).unwrap()
}

fn pasid(id: u32) -> PASID {
    PASID::new(id).unwrap()
}

// ─── NEW-GAP-J: Access Flag Fault ─────────────────────────────────────────────

// AF=true by default: no F_ACCESS fault.
#[test]
fn gap_j_no_fault_when_af_is_true() {
    let smmu = make_smmu();
    let stream_id = sid(1);
    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // map_page sets AF=true by default
    smmu.map_page(stream_id, pasid(0), iova(0x1000), pa(0x2000),
        PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
    let result = smmu.translate(stream_id, pasid(0), iova(0x1000),
        AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "Expected success, AF=true: {result:?}");
}

// AF=0, ha=false, affd=false → F_ACCESS (fault code 0x12).
#[test]
fn gap_j_fault_when_af_is_zero_ha_false_affd_false() {
    let smmu = make_smmu();
    let stream_id = sid(2);
    let mut cfg = StreamConfig::stage1_only();
    cfg.ha = false;
    cfg.affd = false;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // map_page_unaccessed sets AF=false
    smmu.map_page_unaccessed(stream_id, pasid(0), iova(0x1000), pa(0x2000),
        PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
    let result = smmu.translate(stream_id, pasid(0), iova(0x1000),
        AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "Expected AccessFlagFault: {result:?}");
    let events = smmu.get_events();
    assert!(!events.is_empty(), "Expected F_ACCESS event");
    // F_ACCESS = event type 0x12
    let last = events.last().unwrap();
    assert_eq!(last.event_type as u8, 0x12,
        "Expected F_ACCESS (0x12), got {:#x}", last.event_type as u8);
}

// AFFD=true suppresses F_ACCESS even when AF=0.
#[test]
fn gap_j_no_fault_when_affd_true() {
    let smmu = make_smmu();
    let stream_id = sid(3);
    let mut cfg = StreamConfig::stage1_only();
    cfg.ha = false;
    cfg.affd = true;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.map_page_unaccessed(stream_id, pasid(0), iova(0x1000), pa(0x2000),
        PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
    let result = smmu.translate(stream_id, pasid(0), iova(0x1000),
        AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "AFFD=true should suppress AF fault: {result:?}");
}

// HA=true: hardware sets AF, so F_ACCESS is never raised.
#[test]
fn gap_j_no_fault_when_ha_true() {
    let smmu = make_smmu();
    let stream_id = sid(4);
    let mut cfg = StreamConfig::stage1_only();
    cfg.ha = true;
    cfg.affd = false;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.map_page_unaccessed(stream_id, pasid(0), iova(0x1000), pa(0x2000),
        PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
    let result = smmu.translate(stream_id, pasid(0), iova(0x1000),
        AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "HA=true should suppress AF fault: {result:?}");
}

// ─── NEW-GAP-K: WXN ───────────────────────────────────────────────────────────

// WXN=true: execute on a writable page → F_PERMISSION.
#[test]
fn gap_k_wxn_blocks_execute_on_writable_page() {
    let smmu = make_smmu();
    let stream_id = sid(10);
    let mut cfg = StreamConfig::stage1_only();
    cfg.wxn = true;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.map_page(stream_id, pasid(0), iova(0x1000), pa(0x2000),
        PagePermissions::all(), SecurityState::NonSecure).unwrap();
    let result = smmu.translate(stream_id, pasid(0), iova(0x1000),
        AccessType::Execute, SecurityState::NonSecure);
    assert!(result.is_err(), "WXN should block execute on writable page: {result:?}");
    let events = smmu.get_events();
    assert!(!events.is_empty(), "Expected F_PERMISSION event");
    let last = events.last().unwrap();
    assert_eq!(last.event_type as u8, 0x13,
        "Expected F_PERMISSION (0x13), got {:#x}", last.event_type as u8);
}

// WXN=true: read on a writable page is NOT blocked.
#[test]
fn gap_k_wxn_allows_read_on_writable_page() {
    let smmu = make_smmu();
    let stream_id = sid(11);
    let mut cfg = StreamConfig::stage1_only();
    cfg.wxn = true;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.map_page(stream_id, pasid(0), iova(0x1000), pa(0x2000),
        PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    let result = smmu.translate(stream_id, pasid(0), iova(0x1000),
        AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "WXN should not block read: {result:?}");
}

// WXN=false: execute on a writable page is allowed.
#[test]
fn gap_k_no_wxn_allows_execute_on_writable_page() {
    let smmu = make_smmu();
    let stream_id = sid(12);
    let mut cfg = StreamConfig::stage1_only();
    cfg.wxn = false;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.map_page(stream_id, pasid(0), iova(0x1000), pa(0x2000),
        PagePermissions::all(), SecurityState::NonSecure).unwrap();
    let result = smmu.translate(stream_id, pasid(0), iova(0x1000),
        AccessType::Execute, SecurityState::NonSecure);
    assert!(result.is_ok(), "WXN=false should allow execute: {result:?}");
}

// ─── NEW-GAP-L: S2PTW ─────────────────────────────────────────────────────────

// S2PTW=true: two-stage translation where stage-2 page is device memory → F_PERMISSION.
#[test]
fn gap_l_s2ptw_blocks_translation_through_device_page() {
    let smmu = make_smmu();
    let stream_id = sid(20);
    let mut cfg = StreamConfig::two_stage();
    cfg.s2ptw = true;
    cfg.t0sz = 0;
    cfg.s2_t0sz = 16; // minimum legal S2T0SZ per §5.2 (BUG-AUDIT-45 fix: 0 is out of range [16,39])
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();
    // Stage-1: IOVA 0x1000 → IPA 0x5000
    smmu.map_page(stream_id, pasid(0), iova(0x1000), pa(0x5000),
        PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    // Stage-2: IPA 0x5000 → PA 0xA000 as device memory
    smmu.map_stage2_device_page(stream_id, iova(0x5000), pa(0xA000),
        PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    let result = smmu.translate(stream_id, pasid(0), iova(0x1000),
        AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "S2PTW should block device-memory page: {result:?}");
    let events = smmu.get_events();
    assert!(!events.is_empty(), "Expected F_PERMISSION event");
    let last = events.last().unwrap();
    assert_eq!(last.event_type as u8, 0x13,
        "Expected F_PERMISSION (0x13), got {:#x}", last.event_type as u8);
}

// S2PTW=true: stage-2 page is normal memory → translation succeeds.
#[test]
fn gap_l_s2ptw_allows_translation_through_normal_page() {
    let smmu = make_smmu();
    let stream_id = sid(21);
    let mut cfg = StreamConfig::two_stage();
    cfg.s2ptw = true;
    cfg.t0sz = 0;
    cfg.s2_t0sz = 16; // minimum legal S2T0SZ per §5.2 (BUG-AUDIT-45 fix: 0 is out of range [16,39])
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();
    smmu.map_page(stream_id, pasid(0), iova(0x1000), pa(0x5000),
        PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    // Stage-2: normal (non-device) page
    smmu.map_stage2_page(stream_id, iova(0x5000), pa(0xA000),
        PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    let result = smmu.translate(stream_id, pasid(0), iova(0x1000),
        AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "S2PTW=true with normal page should succeed: {result:?}");
}

// S2PTW=false: device-memory stage-2 page does NOT cause a fault.
#[test]
fn gap_l_no_fault_when_s2ptw_false() {
    let smmu = make_smmu();
    let stream_id = sid(22);
    let mut cfg = StreamConfig::two_stage();
    cfg.s2ptw = false;
    cfg.t0sz = 0;
    cfg.s2_t0sz = 16; // minimum legal S2T0SZ per §5.2 (BUG-AUDIT-45 fix: 0 is out of range [16,39])
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();
    smmu.map_page(stream_id, pasid(0), iova(0x1000), pa(0x5000),
        PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    smmu.map_stage2_device_page(stream_id, iova(0x5000), pa(0xA000),
        PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    let result = smmu.translate(stream_id, pasid(0), iova(0x1000),
        AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "S2PTW=false should not fault on device page: {result:?}");
}
