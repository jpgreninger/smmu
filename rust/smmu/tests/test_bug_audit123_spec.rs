//! TDD test for BUG-AUDIT-123.
//!
//! ARM SMMU v3 spec §3.4.3:
//! "CD.TTB0/TTB1 and STE.S2TTB must be within the IPS/OAS-bounded range.
//!  At IPS=6 (52 bits), the valid range is [0, 2^52) = [0, 0x0010_0000_0000_0000).
//!  Any address >= 0x0010_0000_0000_0000 is out of range → C_BAD_CD / C_BAD_STE."
//!
//! BEFORE FIX: The guards use `ips_bits < 52` / `oas_bits < 52`, so the check is
//!             skipped entirely when IPS=6 or S2PS=6 (52-bit). Any TTB/S2TTB value
//!             is accepted even if it exceeds 2^52.
//! AFTER FIX:  The guards use `ips_bits <= 52` / `oas_bits <= 52`, so the check is
//!             applied at 52 bits as well. Addresses >= 2^52 are correctly rejected
//!             with C_BAD_CD / C_BAD_STE.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{EventType, SMMUConfig, StreamConfig, StreamID};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn make_smmu() -> SMMU {
    let smmu = SMMU::with_config(SMMUConfig::default());
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    smmu
}

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

// 2^52 = 0x0010_0000_0000_0000
const LIMIT_52BIT: u64 = 1u64 << 52;

// ============================================================================
// BUG-AUDIT-123 — IPS=52 and OAS=52 TTB/S2TTB range checks skipped
// ============================================================================

/// ARM §3.4.3 / CdIllegal(): TTB0 >= 2^52 with IPS=6 → C_BAD_CD.
///
/// Configure stage-1 stream with IPS=6 (52-bit limit = 0x0010_0000_0000_0000).
/// Set TTB0 = 0x0020_0000_0000_0000 (bit 53 set, >= limit), EPD0=false.
/// configure_stream() must return Err and queue exactly one C_BAD_CD event.
///
/// This test FAILS before the BUG-AUDIT-123 fix because the guard `ips_bits < 52`
/// skips the check when ips_bits == 52.
#[test]
fn audit_123_ttb0_exceeds_52bit_ips_must_emit_c_bad_cd() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .ips(6) // 52-bit IPS → limit = 0x0010_0000_0000_0000
        .ttb0(0x0020_0000_0000_0000_u64) // bit 53 set — exceeds 52-bit range
        .epd0(false) // TTBR0 walk enabled → check applies
        .build()
        .unwrap();

    let result = smmu.configure_stream(sid(1), cfg);

    assert!(
        result.is_err(),
        "BUG-AUDIT-123: §3.4.3 CdIllegal() requires C_BAD_CD when TTB0 \
         (0x{:016x}) >= 2^52 (0x{:016x}) at IPS=6. Expected Err, got Ok",
        0x0020_0000_0000_0000_u64,
        LIMIT_52BIT,
    );

    let events = smmu.get_events();
    let bad_cd_count = events
        .iter()
        .filter(|e| e.event_type == EventType::CBadCd)
        .count();
    assert_eq!(
        bad_cd_count, 1,
        "BUG-AUDIT-123: exactly one C_BAD_CD event must be queued when TTB0 \
         exceeds 52-bit IPS limit. Found {} C_BAD_CD events. All events: {:?}",
        bad_cd_count,
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// ARM §3.4.3 / CdIllegal(): TTB1 >= 2^52 with IPS=6 → C_BAD_CD.
///
/// Configure stage-1 stream with IPS=6 (52-bit limit = 0x0010_0000_0000_0000).
/// Set TTB1 = 0x0030_0000_0000_0000 (>= limit), EPD0=true, EPD1=false.
/// configure_stream() must return Err and queue exactly one C_BAD_CD event.
///
/// This test FAILS before the BUG-AUDIT-123 fix because the guard `ips_bits < 52`
/// skips the check when ips_bits == 52.
#[test]
fn audit_123_ttb1_exceeds_52bit_ips_must_emit_c_bad_cd() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .ips(6) // 52-bit IPS → limit = 0x0010_0000_0000_0000
        .epd0(true) // disable TTB0 walk → TTB0 not checked
        .ttb1(0x0030_0000_0000_0000_u64) // > 2^52 — out of range
        .epd1(false) // TTBR1 walk enabled → check applies
        .build()
        .unwrap();

    let result = smmu.configure_stream(sid(2), cfg);

    assert!(
        result.is_err(),
        "BUG-AUDIT-123: §3.4.3 CdIllegal() requires C_BAD_CD when TTB1 \
         (0x{:016x}) >= 2^52 (0x{:016x}) at IPS=6. Expected Err, got Ok",
        0x0030_0000_0000_0000_u64,
        LIMIT_52BIT,
    );

    let events = smmu.get_events();
    let bad_cd_count = events
        .iter()
        .filter(|e| e.event_type == EventType::CBadCd)
        .count();
    assert_eq!(
        bad_cd_count, 1,
        "BUG-AUDIT-123: exactly one C_BAD_CD event must be queued when TTB1 \
         exceeds 52-bit IPS limit. Found {} C_BAD_CD events. All events: {:?}",
        bad_cd_count,
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// ARM §3.4.3: S2TTB >= 2^52 with S2PS=6 → C_BAD_STE.
///
/// Configure stage-2 stream with S2PS=6 (52-bit OAS limit = 0x0010_0000_0000_0000).
/// Set S2TTB = 0x0020_0000_0000_0000 (>= 2^52).
/// configure_stream() must return Err and queue exactly one C_BAD_STE event.
///
/// This test FAILS before the BUG-AUDIT-123 fix because the guard `oas_bits < 52`
/// skips the check when oas_bits == 52.
#[test]
fn audit_123_s2ttb_exceeds_52bit_oas_must_emit_c_bad_ste() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage2_enabled(true)
        .translation_enabled(true)
        .s2_ps(6) // 52-bit OAS → limit = 0x0010_0000_0000_0000
        .s2_ttb(0x0020_0000_0000_0000_u64) // >= 2^52 — out of range
        .build()
        .unwrap();

    let result = smmu.configure_stream(sid(3), cfg);

    assert!(
        result.is_err(),
        "BUG-AUDIT-123: §3.4.3 requires C_BAD_STE when S2TTB \
         (0x{:016x}) >= 2^52 (0x{:016x}) at S2PS=6. Expected Err, got Ok",
        0x0020_0000_0000_0000_u64,
        LIMIT_52BIT,
    );

    let events = smmu.get_events();
    let bad_ste_count = events
        .iter()
        .filter(|e| e.event_type == EventType::CBadSte)
        .count();
    assert_eq!(
        bad_ste_count, 1,
        "BUG-AUDIT-123: exactly one C_BAD_STE event must be queued when S2TTB \
         exceeds 52-bit OAS limit. Found {} C_BAD_STE events. All events: {:?}",
        bad_ste_count,
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// ARM §3.4.3: TTB0 == 2^52 (exactly at the limit) with IPS=6 → C_BAD_CD.
///
/// The valid range is [0, 2^52), so the limit itself (2^52) is out of range.
/// configure_stream() must return Err and queue C_BAD_CD.
///
/// This test FAILS before the BUG-AUDIT-123 fix because the guard `ips_bits < 52`
/// skips the check at ips_bits == 52.
#[test]
fn audit_123_ttb0_at_52bit_limit_is_out_of_range() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .ips(6) // 52-bit IPS → limit = 2^52 = 0x0010_0000_0000_0000
        .ttb0(LIMIT_52BIT) // exactly at the limit — still out of range (>= limit)
        .epd0(false) // TTBR0 walk enabled → check applies
        .build()
        .unwrap();

    let result = smmu.configure_stream(sid(4), cfg);

    assert!(
        result.is_err(),
        "BUG-AUDIT-123: §3.4.3 CdIllegal() requires C_BAD_CD when TTB0 \
         equals 2^52 (0x{:016x}) at IPS=6 (>= limit means out of range). \
         Expected Err, got Ok",
        LIMIT_52BIT,
    );

    let events = smmu.get_events();
    let bad_cd_count = events
        .iter()
        .filter(|e| e.event_type == EventType::CBadCd)
        .count();
    assert_eq!(
        bad_cd_count, 1,
        "BUG-AUDIT-123: exactly one C_BAD_CD event must be queued when TTB0 \
         equals 52-bit IPS limit. Found {} C_BAD_CD events. All events: {:?}",
        bad_cd_count,
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// ARM §3.4.3: TTB0 just below 2^52 with IPS=6 → configure_stream() succeeds.
///
/// TTB0 = 0x000F_FFFF_FFFF_F000 is within [0, 2^52), so it is valid.
/// configure_stream() must return Ok and queue no C_BAD_CD event.
#[test]
fn audit_123_ttb0_just_below_52bit_limit_must_succeed() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .ips(6) // 52-bit IPS → limit = 0x0010_0000_0000_0000
        .ttb0(0x000F_FFFF_FFFF_F000_u64) // just below 2^52 — within range
        .epd0(false) // TTBR0 walk enabled
        .build()
        .unwrap();

    let result = smmu.configure_stream(sid(5), cfg);

    assert!(
        result.is_ok(),
        "BUG-AUDIT-123: TTB0 = 0x000F_FFFF_FFFF_F000 is within the 52-bit IPS \
         range and must succeed. Got: {:?}",
        result.err()
    );

    let events = smmu.get_events();
    let bad_cd_count = events
        .iter()
        .filter(|e| e.event_type == EventType::CBadCd)
        .count();
    assert_eq!(
        bad_cd_count, 0,
        "BUG-AUDIT-123: no C_BAD_CD event must be queued when TTB0 is within \
         52-bit IPS range. Found {} C_BAD_CD events.",
        bad_cd_count
    );
}
