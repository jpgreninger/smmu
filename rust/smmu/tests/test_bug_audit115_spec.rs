//! TDD test for BUG-AUDIT-115.
//!
//! ARM SMMU v3 spec §3.4.3 / CdIllegal() pseudocode lines 9819–9828:
//! "CD.TTB0 and CD.TTB1 base addresses must be within the range described by
//!  the CD's effective IPS (IPA size). If TTB0 or TTB1 is out of this range,
//!  the CD is ILLEGAL → emit C_BAD_CD and reject the configuration."
//!
//! BEFORE FIX: `configure_stream()` accepts any TTB0/TTB1 value regardless of IPS.
//! AFTER FIX:  `configure_stream()` returns `Err` and queues `C_BAD_CD` when
//!              TTB0 or TTB1 (with the corresponding EPD disabled) exceeds the
//!              IPS-bounded range.
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

// ============================================================================
// BUG-AUDIT-115 — CD.TTB0/TTB1 must be within IPS-bounded range
// ============================================================================

/// ARM §3.4.3 / CdIllegal(): TTB0 >= 2^IPS_bits → C_BAD_CD.
///
/// Configure stage-1 stream with IPS=0 (32-bit limit = 0x1_0000_0000).
/// Set TTB0 = 0x1_0000_0000 (≥ limit) with EPD0=false (walk enabled).
/// configure_stream() must return Err and queue exactly one C_BAD_CD event.
///
/// This test FAILS before the BUG-AUDIT-115 fix because the implementation
/// accepts any TTB0 value without checking against the IPS range.
#[test]
fn audit_115_ttb0_exceeds_ips_limit_must_emit_c_bad_cd() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .ips(0) // 32-bit IPS → limit = 0x1_0000_0000
        .ttb0(0x1_0000_0000_u64) // exactly at limit — out of range
        .epd0(false) // TTBR0 walk enabled → check applies
        .build()
        .unwrap();

    let result = smmu.configure_stream(sid(1), cfg);

    assert!(
        result.is_err(),
        "BUG-AUDIT-115: §3.4.3 CdIllegal() requires C_BAD_CD when TTB0 \
         exceeds IPS range. Expected Err, got Ok"
    );

    let events = smmu.get_events();
    let bad_cd_count = events
        .iter()
        .filter(|e| e.event_type == EventType::CBadCd)
        .count();
    assert_eq!(
        bad_cd_count, 1,
        "BUG-AUDIT-115: exactly one C_BAD_CD event must be queued when TTB0 \
         exceeds IPS limit. Found {} C_BAD_CD events. All events: {:?}",
        bad_cd_count,
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// ARM §3.4.3 / CdIllegal(): TTB1 >= 2^IPS_bits → C_BAD_CD.
///
/// Configure stage-1 stream with IPS=0 (32-bit limit = 0x1_0000_0000).
/// Set TTB1 = 0x2_0000_0000 (> limit) with EPD1=false (walk enabled).
/// configure_stream() must return Err and queue exactly one C_BAD_CD event.
///
/// This test FAILS before the BUG-AUDIT-115 fix because the implementation
/// accepts any TTB1 value without checking against the IPS range.
#[test]
fn audit_115_ttb1_exceeds_ips_limit_must_emit_c_bad_cd() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .ips(0) // 32-bit IPS → limit = 0x1_0000_0000
        .epd0(true) // disable TTB0 walk → TTB0 not checked
        .ttb1(0x2_0000_0000_u64) // > limit — out of range
        .epd1(false) // TTBR1 walk enabled → check applies
        .build()
        .unwrap();

    let result = smmu.configure_stream(sid(2), cfg);

    assert!(
        result.is_err(),
        "BUG-AUDIT-115: §3.4.3 CdIllegal() requires C_BAD_CD when TTB1 \
         exceeds IPS range. Expected Err, got Ok"
    );

    let events = smmu.get_events();
    let bad_cd_count = events
        .iter()
        .filter(|e| e.event_type == EventType::CBadCd)
        .count();
    assert_eq!(
        bad_cd_count, 1,
        "BUG-AUDIT-115: exactly one C_BAD_CD event must be queued when TTB1 \
         exceeds IPS limit. Found {} C_BAD_CD events. All events: {:?}",
        bad_cd_count,
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// ARM §3.4.3: TTB0 and TTB1 within IPS range → configure_stream() succeeds.
///
/// Configure stage-1 stream with IPS=0 (32-bit limit = 0x1_0000_0000).
/// Set TTB0 = 0x0000_1000 and TTB1 = 0xFFFF_0000 (both < limit).
/// configure_stream() must return Ok and queue no C_BAD_CD event.
#[test]
fn audit_115_ttb0_and_ttb1_within_ips_limit_must_succeed() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .ips(0) // 32-bit IPS → limit = 0x1_0000_0000
        .ttb0(0x0000_1000_u64) // well within 32-bit range
        .epd0(false) // TTBR0 walk enabled
        .ttb1(0xFFFF_0000_u64) // within 32-bit range (< 0x1_0000_0000)
        .epd1(false) // TTBR1 walk enabled
        .build()
        .unwrap();

    let result = smmu.configure_stream(sid(3), cfg);

    assert!(
        result.is_ok(),
        "BUG-AUDIT-115: TTB0 and TTB1 within IPS range must succeed. \
         Got: {:?}",
        result.err()
    );

    let events = smmu.get_events();
    let bad_cd_count = events
        .iter()
        .filter(|e| e.event_type == EventType::CBadCd)
        .count();
    assert_eq!(
        bad_cd_count, 0,
        "BUG-AUDIT-115: no C_BAD_CD event must be queued when TTB0/TTB1 \
         are within IPS range. Found {} C_BAD_CD events.",
        bad_cd_count
    );
}

/// ARM §3.4.3 guard: EPD0=true disables TTBR0 walk → TTB0 range check skipped.
///
/// Configure stage-1 stream with IPS=0 (32-bit limit).
/// Set TTB0 = 0x1_FFFF_0000 (> limit) but EPD0=true (walk disabled).
/// configure_stream() must return Ok because EPD0 suppresses the TTB0 check.
#[test]
fn audit_115_epd0_true_skips_ttb0_check() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .ips(0) // 32-bit IPS → limit = 0x1_0000_0000
        .ttb0(0x1_FFFF_0000_u64) // > limit, but EPD0=true means walk is disabled
        .epd0(true) // TTBR0 walk DISABLED → TTB0 not checked
        .epd1(true) // also disable TTB1 walk to avoid any TTB1 check
        .build()
        .unwrap();

    let result = smmu.configure_stream(sid(4), cfg);

    assert!(
        result.is_ok(),
        "BUG-AUDIT-115: EPD0=true must suppress TTB0 range check even if \
         TTB0 would exceed IPS limit. Got: {:?}",
        result.err()
    );

    let events = smmu.get_events();
    let bad_cd_count = events
        .iter()
        .filter(|e| e.event_type == EventType::CBadCd)
        .count();
    assert_eq!(
        bad_cd_count, 0,
        "BUG-AUDIT-115: no C_BAD_CD event must be queued when EPD0=true \
         suppresses TTB0 check. Found {} C_BAD_CD events.",
        bad_cd_count
    );
}
