//! BUG-ANALYSIS-4: S1DSS=0 path double-counts failed_translations and double-records faults
//!
//! ARM IHI0070G.b §7.2 mandates "at most one event report" per transaction.
//!
//! Root cause (as described in bug analysis):
//!   In smmu/mod.rs, `stream_ref.value().translate()` is called at line ~2229 (inner
//!   translate).  The S1DSS=0 check happens AFTER this inner translate call.  When
//!   S1DSS=0, `failed_translations` is incremented and `record_fault()` is called in
//!   the S1DSS block — but the inner translate() may have already triggered fault
//!   recording at the StreamContext level, violating the "at most one event" rule.
//!
//! The CORRECT fix per spec: hoist the S1DSS=0 check BEFORE calling the inner
//! translate() so StreamContext::translate() is never invoked for S1DSS=0
//! non-substream transactions.
//!
//! These tests verify the invariant:
//!   - Exactly ONE fault record (F_STREAM_DISABLED) in smmu.get_faults()
//!   - failed_translations incremented exactly ONCE
//!   - Exactly ONE FStreamDisabled event in smmu.get_events()
//!
//! If double-recording occurs, any of these counts will be 2 instead of 1,
//! causing the test to fail.

#![allow(clippy::doc_markdown)]

use smmu::types::{
    AccessType, EventType, FaultType, PagePermissions, SecurityState, StreamConfig, StreamID,
    TranslationError, IOVA, PA, PASID,
};
use smmu::SMMU;

// ─── Helpers ──────────────────────────────────────────────────────────────────

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

fn iova(v: u64) -> IOVA {
    IOVA::new(v).unwrap()
}

fn pa(v: u64) -> PA {
    PA::new(v).unwrap()
}

/// Configure a substream-capable stream with S1DSS=0 (abort non-substream traffic).
///
/// ARM §5.2: s1cd_max > 0 makes the stream substream-capable.
/// s1dss=0b00 mandates that non-substream (PASID=0) transactions abort with
/// F_STREAM_DISABLED.
fn configure_s1dss0_stream(smmu: &SMMU, stream_id: StreamID) {
    let mut cfg = StreamConfig::stage1_only();
    cfg.s1dss = 0x00; // ARM §5.2: abort non-substream transactions
    cfg.s1cd_max = 4; // stream supports up to 2^4 substreams — makes s1dss active
    smmu.configure_stream(stream_id, cfg).unwrap();
}

// ─── Test 1: fault record count must be exactly one ──────────────────────────

/// ARM §7.2: "at most one event report" per transaction.
///
/// When S1DSS=0 and PASID=0 on a substream-capable stream:
///   - The SMMU-level S1DSS block records one F_STREAM_DISABLED fault
///   - The inner StreamContext::translate() must NOT record an additional fault
///   - get_faults() must return exactly ONE FaultRecord
///
/// This test fails if the inner translate() also records a fault (double-recording).
#[test]
fn s1dss0_pasid0_produces_exactly_one_fault_record() {
    let smmu = make_smmu();
    let stream_id = sid(0xA4_01);
    let pasid0 = pasid(0);
    let iova0 = iova(0x1000);

    // Configure stream with s1cd_max > 0 (substream-capable) and s1dss=0 (abort).
    configure_s1dss0_stream(&smmu, stream_id);

    // Create PASID 0 with a valid mapping — the inner translate() would succeed
    // if it were allowed to run.  If S1DSS=0 is checked AFTER the inner translate,
    // and the inner translate records any fault, this test will see count > 1.
    smmu.create_pasid(stream_id, pasid0).unwrap();
    smmu.map_page(
        stream_id,
        pasid0,
        iova0,
        pa(0xDEAD_0000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Clear any pre-existing faults from configuration.
    smmu.clear_faults();

    let result = smmu.translate(
        stream_id,
        pasid0,
        iova0,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        matches!(result, Err(TranslationError::StreamDisabled)),
        "S1DSS=0 with PASID=0 must return StreamDisabled; got: {result:?}"
    );

    let faults = smmu.get_faults();
    assert_eq!(
        faults.len(),
        1,
        "ARM §7.2: exactly ONE fault record must be generated for S1DSS=0 PASID=0; \
         got {} fault(s). Double-recording detected if count > 1.",
        faults.len()
    );

    assert_eq!(
        faults[0].fault_type(),
        FaultType::StreamDisabled,
        "The single fault record must have type FaultType::StreamDisabled (F_STREAM_DISABLED); \
         got: {:?}",
        faults[0].fault_type()
    );
}

// ─── Test 2: failed_translations counter must be incremented exactly once ────

/// ARM §7.2 / implementation invariant:
///
/// A single translate() call for S1DSS=0, PASID=0 must increment
/// failed_translations by exactly 1.
///
/// If the inner translate() increments the counter AND the S1DSS=0 block
/// increments it again, the delta will be 2 instead of 1.
#[test]
fn s1dss0_pasid0_increments_failed_translations_exactly_once() {
    let smmu = make_smmu();
    let stream_id = sid(0xA4_02);
    let pasid0 = pasid(0);
    let iova0 = iova(0x2000);

    configure_s1dss0_stream(&smmu, stream_id);
    smmu.create_pasid(stream_id, pasid0).unwrap();
    smmu.map_page(
        stream_id,
        pasid0,
        iova0,
        pa(0xBEEF_0000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    smmu.reset_translation_stats();
    let (_, _, before) = smmu.get_translation_stats();

    let result = smmu.translate(
        stream_id,
        pasid0,
        iova0,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let (_, _, after) = smmu.get_translation_stats();
    let delta = after - before;

    assert!(
        matches!(result, Err(TranslationError::StreamDisabled)),
        "S1DSS=0 with PASID=0 must return StreamDisabled; got: {result:?}"
    );
    assert_eq!(
        delta,
        1,
        "ARM §7.2: failed_translations must be incremented exactly ONCE for S1DSS=0 PASID=0; \
         got delta={delta}. Double-counting detected if delta > 1.",
    );
}

// ─── Test 3: exactly one FStreamDisabled event must be generated ──────────────

/// ARM §7.2 / §7.3.7: exactly one F_STREAM_DISABLED event per transaction.
///
/// If the inner translate() also generates an event, the event queue will
/// contain 2 FStreamDisabled entries instead of 1.
#[test]
fn s1dss0_pasid0_generates_exactly_one_f_stream_disabled_event() {
    let smmu = make_smmu();
    let stream_id = sid(0xA4_03);
    let pasid0 = pasid(0);
    let iova0 = iova(0x3000);

    configure_s1dss0_stream(&smmu, stream_id);
    smmu.create_pasid(stream_id, pasid0).unwrap();
    smmu.map_page(
        stream_id,
        pasid0,
        iova0,
        pa(0xCAFE_0000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    smmu.clear_event_queue();

    let result = smmu.translate(
        stream_id,
        pasid0,
        iova0,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        matches!(result, Err(TranslationError::StreamDisabled)),
        "S1DSS=0 with PASID=0 must return StreamDisabled; got: {result:?}"
    );

    let all_events = smmu.get_events();
    let disabled_events: Vec<_> = all_events
        .iter()
        .filter(|e| e.event_type == EventType::FStreamDisabled && e.stream_id == stream_id.as_u32())
        .collect();

    assert_eq!(
        disabled_events.len(),
        1,
        "ARM §7.2 / §7.3.7: exactly ONE F_STREAM_DISABLED event must be generated; \
         got {} event(s). Double-recording detected if count > 1.",
        disabled_events.len()
    );
}

// ─── Test 4: no mapping present — fault count still exactly one ───────────────

/// When no mapping exists for PASID=0, the inner translate() would fail with
/// PASIDNotFound or PageNotMapped if S1DSS=0 is not checked first.
///
/// If the S1DSS=0 check is hoisted correctly, StreamContext::translate() is
/// never called, so no fault is recorded there.  If not hoisted, the inner
/// translate() MAY record a fault before the S1DSS=0 block records another.
///
/// This test verifies exactly ONE fault regardless of whether PASID=0 has
/// a mapping or not.
#[test]
fn s1dss0_pasid0_no_mapping_exactly_one_fault() {
    let smmu = make_smmu();
    let stream_id = sid(0xA4_04);
    let pasid0 = pasid(0);
    let iova0 = iova(0x4000);

    configure_s1dss0_stream(&smmu, stream_id);
    // Intentionally do NOT create_pasid or map_page — PASID=0 has no entry.
    // If the inner translate() is called, it would return PASIDNotFound.
    // If S1DSS=0 is checked first, StreamContext is never invoked.

    smmu.clear_faults();

    let result = smmu.translate(
        stream_id,
        pasid0,
        iova0,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        matches!(result, Err(TranslationError::StreamDisabled)),
        "S1DSS=0 with PASID=0 (no mapping) must return StreamDisabled; got: {result:?}"
    );

    let faults = smmu.get_faults();
    assert_eq!(
        faults.len(),
        1,
        "ARM §7.2: exactly ONE fault must be recorded even when PASID=0 has no mapping; \
         got {} fault(s). Double-recording detected if count > 1.",
        faults.len()
    );
}

// ─── Test 5: failed_translations invariant without mapping ────────────────────

/// Complementary to Test 2: verify the counter invariant when no PASID=0
/// mapping is present (inner translate would fail with a non-StreamDisabled error).
///
/// The S1DSS=0 block must still increment failed_translations exactly ONCE,
/// regardless of whether the inner translate() encountered a different error.
#[test]
fn s1dss0_pasid0_no_mapping_counter_exactly_once() {
    let smmu = make_smmu();
    let stream_id = sid(0xA4_05);
    let pasid0 = pasid(0);
    let iova0 = iova(0x5000);

    configure_s1dss0_stream(&smmu, stream_id);
    // No create_pasid, no map_page.

    smmu.reset_translation_stats();
    let (_, _, before) = smmu.get_translation_stats();

    let result = smmu.translate(
        stream_id,
        pasid0,
        iova0,
        AccessType::Write,
        SecurityState::NonSecure,
    );

    let (_, _, after) = smmu.get_translation_stats();
    let delta = after - before;

    assert!(
        matches!(result, Err(TranslationError::StreamDisabled)),
        "S1DSS=0 with PASID=0 (no mapping) must return StreamDisabled; got: {result:?}"
    );
    assert_eq!(
        delta,
        1,
        "ARM §7.2: failed_translations must be incremented exactly ONCE; \
         got delta={delta}. Double-counting (delta=2) indicates the inner translate() \
         also incremented the counter before the S1DSS=0 block ran.",
    );
}

// ─── Test 6: total_translations == successful + failed invariant ──────────────

/// Verify the basic accounting invariant: total_translations must equal the
/// sum of successful_translations and failed_translations.
///
/// For an S1DSS=0 PASID=0 transaction, successful must be 0 and failed must be 1.
/// If double-counting occurs, failed will be 2 but total will be 1, breaking
/// this invariant.
#[test]
fn s1dss0_pasid0_translation_stats_invariant() {
    let smmu = make_smmu();
    let stream_id = sid(0xA4_06);
    let pasid0 = pasid(0);
    let iova0 = iova(0x6000);

    configure_s1dss0_stream(&smmu, stream_id);
    smmu.create_pasid(stream_id, pasid0).unwrap();
    smmu.map_page(
        stream_id,
        pasid0,
        iova0,
        pa(0x1234_0000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    smmu.reset_translation_stats();

    let result = smmu.translate(
        stream_id,
        pasid0,
        iova0,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let (total, successful, failed) = smmu.get_translation_stats();

    assert!(
        matches!(result, Err(TranslationError::StreamDisabled)),
        "S1DSS=0 with PASID=0 must return StreamDisabled; got: {result:?}"
    );
    assert_eq!(
        total,
        1,
        "Exactly one translate() call must produce total_translations=1; got {total}"
    );
    assert_eq!(
        successful,
        0,
        "S1DSS=0 must not produce a successful translation; got successful={successful}"
    );
    assert_eq!(
        failed,
        1,
        "ARM §7.2: failed_translations must be 1 for one S1DSS=0 PASID=0 call; \
         got failed={failed}. Double-counting produces failed=2 with total=1.",
    );
    assert_eq!(
        total,
        successful + failed,
        "Accounting invariant violated: total({total}) != successful({successful}) + failed({failed}). \
         Double-counting of failed_translations breaks this invariant.",
    );
}
