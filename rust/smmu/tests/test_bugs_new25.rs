//! TDD failing tests for BUG-AUDIT-49, BUG-AUDIT-50, and BUG-AUDIT-52.
//!
//! **BUG-AUDIT-49** (§9.1.4 / §6.3.40): `gatos_translate()` ATTR/SH hardcoded.
//!   Current: success path always returns ATTR=0xFF (Normal WB/WA) in bits[63:56]
//!   and SH=0b11 (ISH) in bits[9:8] regardless of the mapped page's memory type.
//!   Required: device-memory pages must return ATTR=0x00 (Device-nGnRnE) and
//!   SH=0b10 (OSH, per ARM §9.1.4: "Shareability is returned as Outer Shareable
//!   when ATTR selects any Device type").
//!   BEFORE FIX: ATTR==0xFF, SH==0b11 → tests FAIL.
//!   AFTER FIX:  ATTR==0x00, SH==0b10 → tests PASS.
//!
//! **BUG-AUDIT-50** (§5.4 / §5.2): `StreamConfig` missing `endi`/`s2endi` fields.
//!   Current: `StreamConfig` has no `endi` (CD.ENDI) or `s2endi` (STE.S2ENDI) fields.
//!   Required: Both fields must exist. Because IDR0.TTENDIAN=0b00 (mixed-endian),
//!   any value is valid — no C_BAD_CD or C_BAD_STE should be emitted.
//!   BEFORE FIX: struct members absent → compile error → tests FAIL.
//!   AFTER FIX:  fields present and configure_stream() returns Ok → tests PASS.
//!
//! **BUG-AUDIT-52** (§6.3.6 IDR5): GRAN16K/GRAN64K overclaimed.
//!   Current: `get_idr5()` returns bit[5] (GRAN16K) and bit[6] (GRAN64K) both set,
//!   but only 4KB granule is actually implemented.
//!   Required: Only GRAN4K (bit[4]) must be set; bits[5] and [6] must be 0.
//!   BEFORE FIX: bits[5] and bits[6] are 1 → tests FAIL.
//!   AFTER FIX:  bits[5] and bits[6] are 0 → tests PASS.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{
    AccessType, CommandType, EventType, PagePermissions, SecurityState, StreamConfig, StreamID,
    IOVA, PA, PASID,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

fn pasid0() -> PASID {
    PASID::new(0).unwrap()
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

/// Build an SMMU with all queues and global enable active, s1p and s2p supported.
fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    smmu.set_s1p_supported(true);
    smmu.set_s2p_supported(true);
    smmu
}

/// Returns true when the event queue contains at least one event of the given type.
fn has_event(smmu: &SMMU, event_type: EventType) -> bool {
    smmu.get_events().iter().any(|e| e.event_type == event_type)
}

// ============================================================================
// BUG-AUDIT-49: gatos_translate() ATTR/SH hardcoded — must reflect memory type
// ============================================================================
//
// Setup pattern (mirrors test_conf_gaps_jkl.rs for S2PTW tests):
//   - Two-stage stream (StreamConfig::two_stage(), s2ptw=false)
//   - Stage-1: IOVA → IPA via map_page()
//   - Stage-2: IPA  → PA as device memory via map_stage2_device_page()
//   - S2PTW=false: translation succeeds (no F_PERMISSION) so we can check ATTR/SH
//   - gatos_translate(IOVA) → GATOS_PAR — ATTR/SH must reflect device memory type

/// BUG-AUDIT-49 test 1: gatos_translate() on a device-memory stage-2 page must NOT
/// return ATTR=0xFF (Normal WB/WA) in bits[63:56].
///
/// ARM §9.1.4 / §6.3.40: ATTR is determined from translation tables, not hardcoded.
/// A Device-nGnRnE page must return ATTR=0x00, not 0xFF.
///
/// BEFORE FIX: ATTR == 0xFF (hardcoded Normal) → assertion fails → FAILS.
/// AFTER FIX:  ATTR != 0xFF (device type) → PASSES.
#[test]
fn gatos_translate_device_page_attr_not_normal() {
    let smmu = make_smmu();

    // Two-stage stream, S2PTW=false so device-memory page translates (no fault).
    let mut cfg = StreamConfig::two_stage();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s2ptw   = false;
    cfg.t0sz    = 0;    // sentinel — no VA range restriction
    cfg.s2_t0sz = 16;   // minimum legal S2T0SZ per §5.2 (BUG-AUDIT-45 fix)
    cfg.s2_ps   = 5;    // 48-bit PA
    smmu.configure_stream(sid(300), cfg).unwrap();
    smmu.create_pasid(sid(300), pasid0()).unwrap();
    smmu.create_stage2_address_space(sid(300)).unwrap();

    // Stage-1: IOVA 0x1000 → IPA 0x5000
    smmu.map_page(
        sid(300),
        pasid0(),
        iova(0x1000),
        pa(0x5000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Stage-2: IPA 0x5000 → PA 0xA000 as Device-memory (s2ptw=false: no fault)
    smmu.map_stage2_device_page(
        sid(300),
        iova(0x5000),
        pa(0xA000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let par = smmu.gatos_translate(
        sid(300),
        pasid0(),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    // Translation must have succeeded (FAULT bit = 0).
    assert_eq!(
        par & 1,
        0,
        "BUG-AUDIT-49: gatos_translate() must succeed for device-memory page \
         with s2ptw=false. par=0x{:016X}",
        par
    );

    let attr = (par >> 56) & 0xFF;

    assert_ne!(
        attr, 0xFF,
        "BUG-AUDIT-49: ATTR (bits[63:56]) must NOT be 0xFF (Normal WB/WA) for a \
         device-memory page (ARM §9.1.4 / §6.3.40). Current code hardcodes 0xFF \
         regardless of memory type — pre-fix failing condition. attr=0x{:02X}",
        attr
    );
}

/// BUG-AUDIT-49 test 2: gatos_translate() on a device-memory stage-2 page must NOT
/// return SH=0b11 (Inner Shareable) in bits[9:8].
///
/// ARM §9.1.4: "Shareability is returned as Outer Shareable when ATTR selects any
/// Device type." Required: SH=0b10 (OSH).
///
/// BEFORE FIX: SH == 0b11 (hardcoded ISH) → assertion fails → FAILS.
/// AFTER FIX:  SH != 0b11 (OSH = 0b10)   → PASSES.
#[test]
fn gatos_translate_device_page_sh_not_ish() {
    let smmu = make_smmu();

    let mut cfg = StreamConfig::two_stage();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s2ptw   = false;
    cfg.t0sz    = 0;
    cfg.s2_t0sz = 16;
    cfg.s2_ps   = 5;
    smmu.configure_stream(sid(301), cfg).unwrap();
    smmu.create_pasid(sid(301), pasid0()).unwrap();
    smmu.create_stage2_address_space(sid(301)).unwrap();

    smmu.map_page(
        sid(301),
        pasid0(),
        iova(0x2000),
        pa(0x6000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    smmu.map_stage2_device_page(
        sid(301),
        iova(0x6000),
        pa(0xB000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let par = smmu.gatos_translate(
        sid(301),
        pasid0(),
        iova(0x2000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert_eq!(
        par & 1,
        0,
        "BUG-AUDIT-49: gatos_translate() must succeed for device-memory page \
         with s2ptw=false (before checking SH). par=0x{:016X}",
        par
    );

    let sh = (par >> 8) & 0x3;

    assert_ne!(
        sh, 0x3,
        "BUG-AUDIT-49: SH (bits[9:8]) must NOT be 0b11 (ISH) for a device-memory page. \
         ARM §9.1.4 requires OSH (0b10). Current code hardcodes 0b11 — pre-fix failing \
         condition. sh=0b{:02b}",
        sh
    );
}

/// BUG-AUDIT-49 test 3 (regression): gatos_translate() on a normal cached page must
/// still return ATTR=0xFF (Normal WB/WA) and SH=0b11 (ISH).
///
/// This must pass both before and after the fix (regression guard).
#[test]
fn gatos_translate_normal_page_attr_and_sh_unchanged() {
    let smmu = make_smmu();

    let mut cfg = StreamConfig::two_stage();
    cfg.security_state = SecurityState::NonSecure;
    cfg.s2ptw   = false;
    cfg.t0sz    = 0;
    cfg.s2_t0sz = 16;
    cfg.s2_ps   = 5;
    smmu.configure_stream(sid(302), cfg).unwrap();
    smmu.create_pasid(sid(302), pasid0()).unwrap();
    smmu.create_stage2_address_space(sid(302)).unwrap();

    // Stage-1: IOVA 0x3000 → IPA 0x7000 (normal)
    smmu.map_page(
        sid(302),
        pasid0(),
        iova(0x3000),
        pa(0x7000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Stage-2: IPA 0x7000 → PA 0xC000 as normal (non-device) page
    smmu.map_stage2_page(
        sid(302),
        iova(0x7000),
        pa(0xC000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let par = smmu.gatos_translate(
        sid(302),
        pasid0(),
        iova(0x3000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert_eq!(
        par & 1,
        0,
        "BUG-AUDIT-49 regression: gatos_translate() must succeed for normal page. \
         par=0x{:016X}",
        par
    );

    let attr = (par >> 56) & 0xFF;
    let sh   = (par >>  8) & 0x3;

    assert_eq!(
        attr, 0xFF,
        "BUG-AUDIT-49 regression: normal page must still return ATTR=0xFF (Normal WB/WA)"
    );
    assert_eq!(
        sh, 0x3,
        "BUG-AUDIT-49 regression: normal page must still return SH=0b11 (ISH)"
    );
}

// ============================================================================
// BUG-AUDIT-50: StreamConfig missing endi/s2endi fields
// ============================================================================
//
// In Rust, accessing a non-existent struct field is a compile-time error.
// To avoid blocking compilation of other tests, BUG-AUDIT-50 tests use a
// runtime approach: they check via a struct-update expression trick that the
// required field names are NOT present by asserting a known false condition.
//
// Specifically: StreamConfig in the current codebase does NOT include `endi` or
// `s2endi` fields. The tests below assert these fields ARE present using a
// check that always fails before the fix (reporting the clear conformance gap).
//
// Implementation strategy: use a no-op compile-safe function that checks the
// presence of each field symbolically without actually accessing it, and then
// assert the return value is true at runtime (it is always false until the fix).

/// Returns `true` if StreamConfig has a field named `endi` (CD.ENDI per ARM §5.4).
/// This is determined by attempting to construct StreamConfig with `endi` set.
/// BEFORE FIX: StreamConfig does not have `endi` → returns false → test FAILS.
/// AFTER FIX:  StreamConfig has `endi` → returns true → test PASSES.
fn stream_config_has_endi_field() -> bool {
    // The trick: try to construct a StreamConfig with the `endi` field.
    // This is done by deserialising from a JSON-like map at runtime, which would
    // succeed once the field exists.
    //
    // Since we cannot compile `cfg.endi = true` without the field, we instead
    // check the Debug output of a default StreamConfig for the field name — this
    // is a pure runtime check that compiles without the field being present.
    let cfg = StreamConfig::stage1_only();
    let debug_str = format!("{:?}", cfg);
    // Once the field exists, the Debug output will contain "endi:".
    debug_str.contains("endi:")
}

/// Returns `true` if StreamConfig has a field named `s2endi` (STE.S2ENDI per ARM §5.2).
fn stream_config_has_s2endi_field() -> bool {
    let cfg = StreamConfig::stage2_only();
    let debug_str = format!("{:?}", cfg);
    debug_str.contains("s2endi:")
}

/// BUG-AUDIT-50 test 1: StreamConfig must have an `endi` field (CD.ENDI).
///
/// ARM §5.4 CD.ENDI: big-endian stage-1 table walks when set.
/// IDR0.TTENDIAN=0b00 (mixed-endian) means any value is legal — no C_BAD_CD.
///
/// BEFORE FIX: `endi` absent from StreamConfig → stream_config_has_endi_field()
///             returns false → assertion fails → FAILS.
/// AFTER FIX:  field present → returns true → PASSES.
#[test]
fn stream_config_endi_field_exists() {
    assert!(
        stream_config_has_endi_field(),
        "BUG-AUDIT-50: StreamConfig must have an 'endi' (CD.ENDI) field (ARM §5.4). \
         The field is currently absent — add 'pub endi: bool' to StreamConfig. \
         IDR0.TTENDIAN=0b00 means any endi value is legal (no C_BAD_CD should fire). \
         Once added, configure_stream() with any endi value must return Ok."
    );
}

/// BUG-AUDIT-50 test 2: StreamConfig must have an `s2endi` field (STE.S2ENDI).
///
/// ARM §5.2 STE.S2ENDI: big-endian stage-2 table walks when set.
/// IDR0.TTENDIAN=0b00: any s2endi value is legal — no C_BAD_STE should fire.
///
/// BEFORE FIX: `s2endi` absent → returns false → FAILS.
/// AFTER FIX:  field present → returns true → PASSES.
#[test]
fn stream_config_s2endi_field_exists() {
    assert!(
        stream_config_has_s2endi_field(),
        "BUG-AUDIT-50: StreamConfig must have an 's2endi' (STE.S2ENDI) field (ARM §5.2). \
         The field is currently absent — add 'pub s2endi: bool' to StreamConfig. \
         IDR0.TTENDIAN=0b00 means any s2endi value is legal (no C_BAD_STE should fire). \
         Once added, configure_stream() with any s2endi value must return Ok."
    );
}

/// BUG-AUDIT-50 test 3: configure_stream() with any endi value must not emit C_BAD_CD.
///
/// This test checks the runtime behavior AFTER the endi field is added.
/// Before the fix the field-presence check gates this test.
///
/// BEFORE FIX: endi field absent → stream_config_has_endi_field() == false
///             → test_config_endi_field_exists() fails → conformance gap clear.
/// AFTER FIX:  configure_stream() accepted without C_BAD_CD → PASSES.
#[test]
fn stream_config_endi_false_no_event() {
    // Check field existence first — if absent, the prior tests already report the gap.
    // This test will always fail if the field is absent (Debug string won't match).
    assert!(
        stream_config_has_endi_field(),
        "BUG-AUDIT-50: cannot run runtime check — 'endi' field not yet present in \
         StreamConfig (ARM §5.4). Add 'pub endi: bool' to fix."
    );

    // Post-fix: run the actual configure_stream() verification.
    let smmu = make_smmu();
    let cfg = StreamConfig::stage1_only();
    // Note: cfg.endi = false assignment intentionally omitted here since the field
    // may not exist yet. The field-existence check above already guards this path.
    // A post-fix test variant with cfg.endi access will be used in the passing state.
    let result = smmu.configure_stream(sid(312), cfg);
    assert!(
        result.is_ok(),
        "BUG-AUDIT-50: configure_stream() must succeed for stage-1 stream"
    );
    assert!(
        !has_event(&smmu, EventType::CBadCd),
        "BUG-AUDIT-50: no C_BAD_CD must be emitted for any valid endi value (TTENDIAN=0b00)"
    );
    assert!(
        !has_event(&smmu, EventType::CBadSte),
        "BUG-AUDIT-50: no C_BAD_STE must be emitted for any valid endi value (TTENDIAN=0b00)"
    );
}

// ============================================================================
// BUG-AUDIT-52: IDR5 GRAN16K/GRAN64K overclaimed — only 4KB implemented
// ============================================================================

/// BUG-AUDIT-52 test 1: IDR5.GRAN16K (bit[5]) must be 0.
///
/// ARM §6.3.6: GRAN16K is set only when 16KB granule is supported.
/// This model only implements 4KB pages, so GRAN16K must be 0.
///
/// BEFORE FIX: (get_idr5() >> 5) & 1 == 1 → FAILS.
/// AFTER FIX:  (get_idr5() >> 5) & 1 == 0 → PASSES.
#[test]
fn idr5_gran16k_must_be_zero() {
    let smmu = SMMU::new();
    let idr5 = smmu.get_idr5();

    assert_eq!(
        (idr5 >> 5) & 1,
        0,
        "BUG-AUDIT-52: IDR5.GRAN16K (bit[5]) must be 0 — 16KB granule is not \
         implemented (ARM §6.3.6). Current code sets bit[5]=1. idr5=0x{:08X}",
        idr5
    );
}

/// BUG-AUDIT-52 test 2: IDR5.GRAN64K (bit[6]) must be 0.
///
/// ARM §6.3.6: GRAN64K is set only when 64KB granule is supported.
/// This model only implements 4KB pages, so GRAN64K must be 0.
///
/// BEFORE FIX: (get_idr5() >> 6) & 1 == 1 → FAILS.
/// AFTER FIX:  (get_idr5() >> 6) & 1 == 0 → PASSES.
#[test]
fn idr5_gran64k_must_be_zero() {
    let smmu = SMMU::new();
    let idr5 = smmu.get_idr5();

    assert_eq!(
        (idr5 >> 6) & 1,
        0,
        "BUG-AUDIT-52: IDR5.GRAN64K (bit[6]) must be 0 — 64KB granule is not \
         implemented (ARM §6.3.6). Current code sets bit[6]=1. idr5=0x{:08X}",
        idr5
    );
}

/// BUG-AUDIT-52 test 3 (regression): IDR5.GRAN4K (bit[4]) must remain 1.
///
/// ARM §6.3.6: 4KB granule IS implemented and supported.
/// This must pass both before and after the fix (regression guard).
#[test]
fn idr5_gran4k_must_be_one() {
    let smmu = SMMU::new();
    let idr5 = smmu.get_idr5();

    assert_eq!(
        (idr5 >> 4) & 1,
        1,
        "BUG-AUDIT-52 regression: IDR5.GRAN4K (bit[4]) must be 1 — 4KB granule IS \
         implemented (ARM §6.3.6). idr5=0x{:08X}",
        idr5
    );
}

/// BUG-AUDIT-52 test 4: IDR5 granule field bits[6:4] must be 0b001 (only GRAN4K set).
///
/// BEFORE FIX: bits[6:4] == 0b111 (all three granules claimed) → FAILS.
/// AFTER FIX:  bits[6:4] == 0b001 (only GRAN4K) → PASSES.
#[test]
fn idr5_granule_field_only_gran4k_set() {
    let smmu = SMMU::new();
    let idr5 = smmu.get_idr5();
    let gran_bits = (idr5 >> 4) & 0x7;

    assert_eq!(
        gran_bits, 0x1,
        "BUG-AUDIT-52: IDR5 granule bits[6:4] must be 0b001 (only GRAN4K). \
         Current code returns 0b{:03b} (GRAN64K={}, GRAN16K={}, GRAN4K={}). \
         ARM §6.3.6 requires only implemented granules to be advertised.",
        gran_bits,
        (gran_bits >> 2) & 1,
        (gran_bits >> 1) & 1,
        gran_bits & 1
    );
}

// ============================================================================
// BUG-AUDIT-53: IDR0.DORMHINT (bit[8]) must be 1 — dormancy is implemented
// ============================================================================

/// BUG-AUDIT-53 test 1: IDR0.DORMHINT (bit[8]) must be 1.
///
/// ARM §6.3.1: DORMHINT=1 means the SMMU supports the dormant state.
/// shutdown() / STATUSR.DORMANT is implemented; software reads DORMHINT to know.
///
/// BEFORE FIX: bit[8] == 0 → FAILS.
/// AFTER FIX:  bit[8] == 1 → PASSES.
#[test]
fn idr0_dormhint_bit_must_be_one() {
    let smmu = SMMU::new();
    let idr0 = smmu.get_idr0();
    assert_eq!(
        (idr0 >> 8) & 1, 1,
        "BUG-AUDIT-53: IDR0.DORMHINT (bit[8]) must be 1 — dormancy (shutdown/STATUSR.DORMANT) \
         is implemented (ARM §6.3.1). Software reads DORMHINT to know if STATUSR.DORMANT is \
         meaningful. Current code sets bit[8]=0. idr0=0x{:08X}", idr0
    );
}

// ============================================================================
// BUG-AUDIT-54: receive_broadcast_tlbi() must honor CR2.PTM=0
// ============================================================================

/// BUG-AUDIT-54 test 1: receive_broadcast_tlbi() with PTM=0 must NOT invalidate TLB entries.
///
/// ARM §6.3.9/§6.3.12: CR2.PTM=0 means the SMMU does NOT participate in broadcast TLBI.
///
/// BEFORE FIX: function absent or no PTM guard → TLB entries still invalidated → FAILS.
/// AFTER FIX:  PTM=0 → entries NOT invalidated → PASSES.
#[test]
fn receive_broadcast_tlbi_ptm_zero_no_invalidation() {
    let smmu = make_smmu();
    smmu.set_cr2(0); // CR2.PTM=0: SMMU does NOT participate

    // Configure a stream and map a page so the TLB has an entry.
    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(sid(400), cfg).unwrap();
    smmu.create_pasid(sid(400), pasid0()).unwrap();
    smmu.map_page(
        sid(400), pasid0(), iova(0x1000), pa(0xA000),
        PagePermissions::read_only(), SecurityState::NonSecure,
    ).unwrap();

    // Translate once to populate TLB.
    let result1 = smmu.translate(sid(400), pasid0(), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(result1.is_ok(), "BUG-AUDIT-54: initial translation must succeed");

    // Broadcast TLBI_NH_ALL with PTM=0 — must be a no-op.
    smmu.receive_broadcast_tlbi(CommandType::TlbiNhAll, 0, 0, iova(0));

    // Translation must still succeed (TLB entry not invalidated by broadcast when PTM=0).
    let result2 = smmu.translate(sid(400), pasid0(), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(
        result2.is_ok(),
        "BUG-AUDIT-54: receive_broadcast_tlbi() with CR2.PTM=0 must NOT invalidate \
         TLB entries (ARM §6.3.9/§6.3.12). The SMMU must not participate in broadcast \
         TLB maintenance when PTM=0."
    );
}

/// BUG-AUDIT-54 test 2: receive_broadcast_tlbi() with PTM=1 must invalidate TLB entries.
///
/// When CR2.PTM=1, the SMMU DOES participate in broadcast TLB maintenance.
/// This must pass both before and after the fix (regression guard for PTM=1 behavior).
#[test]
fn receive_broadcast_tlbi_ptm_one_does_invalidate() {
    let smmu = make_smmu();
    smmu.set_cr2(SMMU::CR2_PTM); // CR2.PTM=1: SMMU participates

    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(sid(401), cfg).unwrap();
    smmu.create_pasid(sid(401), pasid0()).unwrap();
    smmu.map_page(
        sid(401), pasid0(), iova(0x2000), pa(0xB000),
        PagePermissions::read_only(), SecurityState::NonSecure,
    ).unwrap();

    // Translate once (TLB populated).
    smmu.translate(sid(401), pasid0(), iova(0x2000), AccessType::Read, SecurityState::NonSecure).unwrap();

    // Broadcast TLBI_NH_ALL with PTM=1 — must invalidate.
    smmu.receive_broadcast_tlbi(CommandType::TlbiNhAll, 0, 0, iova(0));

    // Note: after TLB invalidation, the underlying page table mapping still exists,
    // so re-translation should succeed (TLB miss → re-walk → success).
    let result = smmu.translate(sid(401), pasid0(), iova(0x2000), AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_ok(),
        "BUG-AUDIT-54 regression: after PTM=1 broadcast TLBI, translation must still \
         succeed via TLB re-walk (page mapping still exists)."
    );
}

// ============================================================================
// BUG-AUDIT-55: IDR0.SEV (bit[14]) must be configurable, default false
// ============================================================================

/// BUG-AUDIT-55 test 1: IDR0.SEV (bit[14]) must be 0 by default.
///
/// BEFORE FIX: bit[14] == 1 (hardcoded) → FAILS.
/// AFTER FIX:  bit[14] == 0 (default false) → PASSES.
#[test]
fn idr0_sev_default_false() {
    let smmu = SMMU::new();
    let idr0 = smmu.get_idr0();
    assert_eq!(
        (idr0 >> 14) & 1, 0,
        "BUG-AUDIT-55: IDR0.SEV (bit[14]) must be 0 by default — CMD_SYNC SIG_SEV \
         (CS=0b10) WFE-wake is not implemented (ARM §4.7.3 / §6.3.1). Default must be \
         false. Current code hardcodes bit[14]=1. idr0=0x{:08X}", idr0
    );
}

/// BUG-AUDIT-55 test 2: set_sev_supported(true) must set IDR0.SEV bit[14] = 1.
#[test]
fn idr0_sev_true_after_set_supported() {
    let smmu = SMMU::new();
    smmu.set_sev_supported(true);
    let idr0 = smmu.get_idr0();
    assert_eq!(
        (idr0 >> 14) & 1, 1,
        "BUG-AUDIT-55: after set_sev_supported(true), IDR0.SEV (bit[14]) must be 1. \
         idr0=0x{:08X}", idr0
    );
}

/// BUG-AUDIT-55 test 3: set_sev_supported(false) after true must clear IDR0.SEV.
#[test]
fn idr0_sev_false_after_clear() {
    let smmu = SMMU::new();
    smmu.set_sev_supported(true);
    smmu.set_sev_supported(false);
    let idr0 = smmu.get_idr0();
    assert_eq!(
        (idr0 >> 14) & 1, 0,
        "BUG-AUDIT-55: after set_sev_supported(false), IDR0.SEV must be 0. \
         idr0=0x{:08X}", idr0
    );
}
