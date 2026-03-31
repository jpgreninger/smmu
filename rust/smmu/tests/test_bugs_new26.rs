//! TDD failing tests for BUG-AUDIT-58, BUG-AUDIT-59, BUG-AUDIT-60, and BUG-AUDIT-61.
//!
//! **BUG-AUDIT-58** (§5.2 SteIllegal): S2TG not validated against IDR5 granule support.
//!   Spec §5.2 SteIllegal() line 8362: if !GranuleSupported(s2tg) then return TRUE.
//!   IDR5 advertises only GRAN4K=1 (GRAN16K=0, GRAN64K=0).
//!   s2_tg=0 → 4KB (valid); s2_tg=1 → 64KB (invalid); s2_tg=2 → 16KB (invalid);
//!   s2_tg=3 → Reserved (always invalid).
//!   BEFORE FIX: configure_stream() accepts s2_tg=1/2/3 silently → tests FAIL.
//!   AFTER FIX:  configure_stream() emits CBadSte for s2_tg != 0 → tests PASS.
//!
//! **BUG-AUDIT-59** (Rust-only §6.3.1/§6.3.6): Relaxed ordering in IDR register reads.
//!   `get_idr0()` and `get_idr5()` read `self.stall_model.load(Ordering::Relaxed)`.
//!   The store in `set_stall_model()` uses `Ordering::Release`.
//!   For correct pairing, reads must use `Ordering::Acquire`.
//!   NOTE: On x86 Relaxed is functionally equivalent; these tests verify FUNCTIONAL
//!   correctness (not the ordering per se). The fix is changing Relaxed to Acquire
//!   in the load() calls in get_idr0() and get_idr5().
//!   BEFORE FIX: Tests PASS (Relaxed is functionally correct on x86).
//!   AFTER FIX:  Tests continue to PASS.
//!
//! **BUG-AUDIT-60** (§6.3.12): CR2 write must be ignored when SMMUEN=1.
//!   Spec §6.3.12 lines 12578-12591: CR2 is effectively RO when SMMUEN=1.
//!   Current code applies the write even when SMMUEN=1.
//!   BEFORE FIX: get_cr2() returns non-zero after write with SMMUEN=1 → tests FAIL.
//!   AFTER FIX:  get_cr2() returns 0 (write silently ignored) → tests PASS.
//!
//! **BUG-AUDIT-61** (§6.3.11): CR1 write must be ignored when SMMUEN=1 or queues enabled.
//!   Spec §6.3.11: TABLE_* fields are RO when SMMUEN=1; QUEUE_* fields RO when queue enabled.
//!   Current code applies the write regardless.
//!   BEFORE FIX: get_cr1() returns non-zero after write with SMMUEN=1 → tests FAIL.
//!   AFTER FIX:  get_cr1() returns 0 (write silently ignored) → tests PASS.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{EventType, StreamConfig, StreamID, IOVA, PA, PASID};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

#[allow(dead_code)]
fn pasid0() -> PASID {
    PASID::new(0).unwrap()
}

#[allow(dead_code)]
fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

#[allow(dead_code)]
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

/// Build a minimal legal stage-2-only StreamConfig with the given s2_tg value.
fn stage2_config(s2_tg: u8) -> StreamConfig {
    let mut cfg = StreamConfig::stage2_only();
    cfg.s2_tg = s2_tg;
    cfg.s2_t0sz = 16; // legal minimum per BUG-AUDIT-45 fix
    cfg
}

// ============================================================================
// BUG-AUDIT-58: S2TG not validated against IDR5 granule support
// ============================================================================

/// BUG-AUDIT-58 test 1: s2_tg=1 (64KB) must emit CBadSte — not in IDR5.
///
/// ARM §5.2 SteIllegal() line 8362: GranuleSupported(s2tg) checks IDR5.
/// IDR5 advertises only GRAN4K (bit[4]=1). 64KB (s2_tg=1) is not supported.
///
/// BEFORE FIX: configure_stream() returns Ok, no CBadSte event → FAILS.
/// AFTER FIX:  CBadSte event in event queue → PASSES.
#[test]
fn bug_audit_58_s2tg_64k_emits_cbad_ste() {
    let smmu = make_smmu();

    let cfg = stage2_config(1); // s2_tg=1 → 64KB (not in IDR5)
    smmu.configure_stream(sid(600), cfg).ok(); // may or may not return Err; check event queue

    assert!(
        has_event(&smmu, EventType::CBadSte),
        "BUG-AUDIT-58: s2_tg=1 (64KB) must emit CBadSte because IDR5 only \
         advertises GRAN4K. ARM §5.2 SteIllegal() line 8362: \
         if !GranuleSupported(s2tg) then return TRUE. \
         Current code stores s2_tg without validation. \
         events={:?}",
        smmu.get_events()
    );
}

/// BUG-AUDIT-58 test 2: s2_tg=2 (16KB) must emit CBadSte — not in IDR5.
///
/// ARM §5.2 SteIllegal() line 8362: GranuleSupported(s2tg) checks IDR5.
/// IDR5.GRAN16K (bit[5]) is 0 in this model. 16KB (s2_tg=2) is not supported.
///
/// BEFORE FIX: configure_stream() returns Ok, no CBadSte event → FAILS.
/// AFTER FIX:  CBadSte event in event queue → PASSES.
#[test]
fn bug_audit_58_s2tg_16k_emits_cbad_ste() {
    let smmu = make_smmu();

    let cfg = stage2_config(2); // s2_tg=2 → 16KB (not in IDR5)
    smmu.configure_stream(sid(601), cfg).ok();

    assert!(
        has_event(&smmu, EventType::CBadSte),
        "BUG-AUDIT-58: s2_tg=2 (16KB) must emit CBadSte because IDR5 only \
         advertises GRAN4K. ARM §5.2 SteIllegal() line 8362. \
         Current code stores s2_tg without validation. \
         events={:?}",
        smmu.get_events()
    );
}

/// BUG-AUDIT-58 test 3: s2_tg=3 (Reserved) must emit CBadSte — always invalid.
///
/// ARM §5.2 SteIllegal() line 8362: s2tg=0b11 is Reserved and therefore never
/// supported regardless of IDR5 granule configuration.
///
/// BEFORE FIX: configure_stream() returns Ok, no CBadSte event → FAILS.
/// AFTER FIX:  CBadSte event in event queue → PASSES.
#[test]
fn bug_audit_58_s2tg_reserved_emits_cbad_ste() {
    let smmu = make_smmu();

    let cfg = stage2_config(3); // s2_tg=3 → Reserved (always illegal)
    smmu.configure_stream(sid(602), cfg).ok();

    assert!(
        has_event(&smmu, EventType::CBadSte),
        "BUG-AUDIT-58: s2_tg=3 (Reserved) must always emit CBadSte. \
         ARM §5.2 SteIllegal() line 8362: Reserved s2tg is always unsupported. \
         Current code stores s2_tg without validation. \
         events={:?}",
        smmu.get_events()
    );
}

/// BUG-AUDIT-58 test 4 (regression): s2_tg=0 (4KB) must NOT emit CBadSte.
///
/// IDR5.GRAN4K is set; s2_tg=0 (4KB) is the only supported granule.
/// This must pass both before and after the fix (regression guard).
///
/// BEFORE / AFTER FIX: s2_tg=0 is valid → no CBadSte → PASSES.
#[test]
fn bug_audit_58_s2tg_4k_valid_no_event() {
    let smmu = make_smmu();

    let cfg = stage2_config(0); // s2_tg=0 → 4KB (valid per IDR5)
    let result = smmu.configure_stream(sid(603), cfg);

    assert!(
        result.is_ok(),
        "BUG-AUDIT-58 regression: s2_tg=0 (4KB) must be accepted without error. \
         result={:?}",
        result
    );

    assert!(
        !has_event(&smmu, EventType::CBadSte),
        "BUG-AUDIT-58 regression: s2_tg=0 (4KB, supported in IDR5) must NOT emit \
         CBadSte. ARM §5.2 SteIllegal(): GranuleSupported(0) is TRUE. \
         events={:?}",
        smmu.get_events()
    );
}

// ============================================================================
// BUG-AUDIT-59: Relaxed ordering in IDR register reads (functional consistency)
// ============================================================================
//
// NOTE: These tests verify FUNCTIONAL correctness, not the memory ordering per se.
// On x86, Ordering::Relaxed is functionally equivalent to Ordering::Acquire for
// atomic loads, so these tests PASS even with the bug present. The actual fix is
// to change Ordering::Relaxed to Ordering::Acquire in the stall_model.load() calls
// in get_idr0() (src/smmu/mod.rs line ~1259) and get_idr5() (line ~1340).
//
// The ordering issue matters on weakly-ordered architectures (e.g., ARM): without
// Acquire, the compiler/CPU may observe the stall_model store out of order.

/// BUG-AUDIT-59 test 1: After set_stall_model(0x01), get_idr0() STALL_MODEL field
/// bits[25:24] must reflect 0x01.
///
/// ARM §6.3.1: STALL_MODEL[25:24] must match the runtime value.
/// The load in get_idr0() uses Ordering::Relaxed — must use Ordering::Acquire
/// to pair correctly with the Ordering::Release store in set_stall_model().
///
/// NOTE: PASSES on x86 today (Relaxed == Acquire on x86). Fix is required for
/// portability and correctness on weakly-ordered architectures.
#[test]
fn bug_audit_59_idr0_reflects_stall_model_after_set() {
    let smmu = SMMU::new();

    smmu.set_stall_model(0x01);
    let idr0 = smmu.get_idr0();
    let stall_model_bits = (idr0 >> 24) & 0x3;

    // This assertion validates functional correctness.
    // The fix (Relaxed → Acquire) ensures this is also memory-order correct.
    assert_eq!(
        stall_model_bits, 0x01,
        "BUG-AUDIT-59: After set_stall_model(0x01), get_idr0() bits[25:24] \
         (STALL_MODEL) must equal 0x01. \
         The load uses Ordering::Relaxed — should use Ordering::Acquire to pair \
         with the Ordering::Release store in set_stall_model() (ARM §6.3.1). \
         Fix: change stall_model.load(Ordering::Relaxed) to \
         stall_model.load(Ordering::Acquire) in get_idr0() at src/smmu/mod.rs ~1259. \
         idr0=0x{:08X}",
        idr0
    );
}

/// BUG-AUDIT-59 test 2: After set_stall_model(0x01), get_idr5() STALL_MAX
/// bits[31:16] must be 0 (no stall entries when terminate-only).
///
/// ARM §6.3.6: STALL_MAX=0 when STALL_MODEL==0x01 (terminate-only).
/// The load in get_idr5() also uses Ordering::Relaxed — must use Ordering::Acquire.
///
/// NOTE: PASSES on x86 today. The fix is required for correctness and portability.
#[test]
fn bug_audit_59_idr5_stall_max_zero_when_terminate_only() {
    let smmu = SMMU::new();

    smmu.set_stall_model(0x01); // terminate-only → STALL_MAX=0
    let idr5 = smmu.get_idr5();
    let stall_max = (idr5 >> 16) & 0xFFFF;

    assert_eq!(
        stall_max, 0,
        "BUG-AUDIT-59: When stall_model==0x01 (terminate-only), get_idr5() \
         STALL_MAX bits[31:16] must be 0. \
         The load uses Ordering::Relaxed — should use Ordering::Acquire \
         (ARM §6.3.6). \
         Fix: change stall_model.load(Ordering::Relaxed) to \
         stall_model.load(Ordering::Acquire) in get_idr5() at src/smmu/mod.rs ~1340. \
         idr5=0x{:08X}",
        idr5
    );
}

/// BUG-AUDIT-59 test 3: After set_stall_model(0x00) (or 0x02), get_idr5() STALL_MAX
/// bits[31:16] must be non-zero (64 stall entries supported).
///
/// ARM §6.3.6: STALL_MAX=64 when stall_model is 0x00 or 0x02.
/// Verifies the converse case is also consistent.
#[test]
fn bug_audit_59_idr5_stall_max_nonzero_when_stall_enabled() {
    let smmu = SMMU::new();

    smmu.set_stall_model(0x00); // stall supported → STALL_MAX=64
    let idr5 = smmu.get_idr5();
    let stall_max = (idr5 >> 16) & 0xFFFF;

    assert_ne!(
        stall_max, 0,
        "BUG-AUDIT-59: When stall_model==0x00, get_idr5() STALL_MAX bits[31:16] \
         must be non-zero (stall entries supported). \
         This verifies functional consistency of the IDR5 stall_model load \
         (ARM §6.3.6). idr5=0x{:08X}",
        idr5
    );
}

// ============================================================================
// BUG-AUDIT-60: CR2 write must be ignored when SMMUEN=1
// ============================================================================

/// BUG-AUDIT-60 test 1: set_cr2() must be silently ignored when SMMUEN=1.
///
/// ARM §6.3.12 lines 12578-12591: CR2 is effectively read-only when SMMUEN=1.
/// Writes must be silently discarded.
///
/// BEFORE FIX: get_cr2() returns CR2_RECINVSID (write applied) → FAILS.
/// AFTER FIX:  get_cr2() returns 0 (write ignored) → PASSES.
#[test]
fn bug_audit_60_set_cr2_ignored_when_smmuen1() {
    let smmu = make_smmu(); // SMMUEN=1

    // Attempt to write CR2_RECINVSID when SMMUEN=1. Must be ignored.
    smmu.set_cr2(SMMU::CR2_RECINVSID);

    let cr2 = smmu.get_cr2();
    assert_eq!(
        cr2 & SMMU::CR2_RECINVSID,
        0,
        "BUG-AUDIT-60: set_cr2(CR2_RECINVSID) must be silently ignored when \
         SMMUEN=1. ARM §6.3.12 lines 12578-12591: CR2 is RO when SMMUEN=1. \
         Current code applies the write. get_cr2()=0x{:08X}",
        cr2
    );
}

/// BUG-AUDIT-60 test 2: set_cr2() with CR2_PTM must be silently ignored when SMMUEN=1.
///
/// ARM §6.3.12: CR2.PTM (bit 2) write must be discarded when SMMUEN=1.
///
/// BEFORE FIX: get_cr2() returns CR2_PTM (write applied) → FAILS.
/// AFTER FIX:  get_cr2() returns 0 (write ignored) → PASSES.
#[test]
fn bug_audit_60_set_cr2_ptm_ignored_when_smmuen1() {
    let smmu = make_smmu(); // SMMUEN=1

    smmu.set_cr2(SMMU::CR2_PTM);

    let cr2 = smmu.get_cr2();
    assert_eq!(
        cr2 & SMMU::CR2_PTM,
        0,
        "BUG-AUDIT-60: set_cr2(CR2_PTM) must be silently ignored when \
         SMMUEN=1. ARM §6.3.12. Current code applies the write. \
         get_cr2()=0x{:08X}",
        cr2
    );
}

/// BUG-AUDIT-60 test 3 (regression): set_cr2() CAN be written when SMMUEN=0.
///
/// CR2 is writable during SMMU configuration (before enabling).
/// This must pass both before and after the fix.
///
/// BEFORE / AFTER FIX: SMMUEN=0 → write is applied → get_cr2() reflects write → PASSES.
#[test]
fn bug_audit_60_set_cr2_accepted_when_smmuen0() {
    let smmu = SMMU::new(); // SMMUEN=0 (reset state)

    smmu.set_cr2(SMMU::CR2_RECINVSID);
    let cr2 = smmu.get_cr2();

    assert_eq!(
        cr2 & SMMU::CR2_RECINVSID,
        SMMU::CR2_RECINVSID,
        "BUG-AUDIT-60 regression: set_cr2(CR2_RECINVSID) must succeed when SMMUEN=0. \
         CR2 is only read-only when SMMUEN=1 (ARM §6.3.12). \
         get_cr2()=0x{:08X}",
        cr2
    );
}

/// BUG-AUDIT-60 test 4: After enabling the SMMU, a subsequent CR2 write is ignored.
///
/// Flow: enable SMMU (SMMUEN=1) → write CR2 → write ignored.
///
/// BEFORE FIX: second write updates CR2 → FAILS.
/// AFTER FIX:  write ignored → CR2 remains 0 (reset default) → PASSES.
#[test]
fn bug_audit_60_set_cr2_after_enable_ignored() {
    let smmu = SMMU::new();

    // Step 1: Enable SMMU.
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);

    // Step 2: attempt to write CR2 with SMMUEN=1 (should be ignored).
    smmu.set_cr2(SMMU::CR2_PTM);

    let cr2 = smmu.get_cr2();
    assert_eq!(
        cr2 & SMMU::CR2_PTM,
        0,
        "BUG-AUDIT-60: After SMMUEN=1, set_cr2(CR2_PTM) must be silently ignored. \
         ARM §6.3.12. Current code applies the write. get_cr2()=0x{:08X}",
        cr2
    );
}

// ============================================================================
// BUG-AUDIT-61: CR1 write must be ignored when SMMUEN=1 or queues enabled
// ============================================================================

/// BUG-AUDIT-61 test 1: set_cr1() must be silently ignored when SMMUEN=1.
///
/// ARM §6.3.11: TABLE_* fields (bits[11:0]) are RO when SMMUEN=1.
/// QUEUE_* fields (bits[5:0]) are RO when any queue is enabled.
/// Writes must be silently discarded.
///
/// BEFORE FIX: get_cr1() returns the written value → FAILS.
/// AFTER FIX:  get_cr1() returns 0 (write ignored) → PASSES.
#[test]
fn bug_audit_61_set_cr1_ignored_when_smmuen1() {
    let smmu = make_smmu(); // SMMUEN=1, CMDQEN=1, EVENTQEN=1

    // Write all table shareability/cache bits — must be ignored.
    let cr1_all_table_bits = SMMU::CR1_TABLE_SH | SMMU::CR1_TABLE_OC | SMMU::CR1_TABLE_IC;
    smmu.set_cr1(cr1_all_table_bits);

    let cr1 = smmu.get_cr1();
    assert_eq!(
        cr1 & cr1_all_table_bits,
        0,
        "BUG-AUDIT-61: set_cr1() TABLE_* bits must be silently ignored when SMMUEN=1. \
         ARM §6.3.11: TABLE_* fields are RO when SMMUEN=1. \
         Current code applies the write. get_cr1()=0x{:08X}",
        cr1
    );
}

/// BUG-AUDIT-61 test 2: set_cr1() with value 0x3F must be silently ignored when SMMUEN=1.
///
/// 0x3F covers bits[5:0] (queue cache/shareability bits) which are RO when any
/// queue is enabled (CMDQEN=1 or EVENTQEN=1).
///
/// BEFORE FIX: get_cr1() returns 0x3F (or subset) → FAILS.
/// AFTER FIX:  get_cr1() returns 0 → PASSES.
#[test]
fn bug_audit_61_set_cr1_0x3f_ignored_when_smmuen1() {
    let smmu = make_smmu(); // SMMUEN=1

    smmu.set_cr1(0x3F);

    let cr1 = smmu.get_cr1();
    assert_eq!(
        cr1, 0,
        "BUG-AUDIT-61: set_cr1(0x3F) must be silently ignored when SMMUEN=1. \
         ARM §6.3.11. Current code applies the write. get_cr1()=0x{:08X}",
        cr1
    );
}

/// BUG-AUDIT-61 test 3 (regression): set_cr1() CAN be written when SMMUEN=0 and
/// queues are disabled.
///
/// CR1 is writable during SMMU configuration (before enabling).
/// This must pass both before and after the fix.
///
/// BEFORE / AFTER FIX: SMMUEN=0 + queues disabled → write applied → PASSES.
#[test]
fn bug_audit_61_set_cr1_accepted_when_smmuen0_queues_disabled() {
    let smmu = SMMU::new(); // SMMUEN=0, no queues enabled

    let cr1_val = SMMU::CR1_TABLE_SH | SMMU::CR1_TABLE_OC;
    smmu.set_cr1(cr1_val);

    let cr1 = smmu.get_cr1();
    assert_eq!(
        cr1 & cr1_val,
        cr1_val,
        "BUG-AUDIT-61 regression: set_cr1() must succeed when SMMUEN=0 and no queues \
         are enabled. ARM §6.3.11. get_cr1()=0x{:08X}",
        cr1
    );
}

/// BUG-AUDIT-61 test 4: Queue-only bits in CR1 must be ignored when queue is enabled
/// even if SMMUEN=0.
///
/// ARM §6.3.11: QUEUE_* fields (bits[5:0]) are RO when any queue is enabled (CMDQEN or
/// EVENTQEN). This tests the queue-enabled guard independent of SMMUEN.
///
/// BEFORE FIX: get_cr1() reflects the written queue bits → FAILS.
/// AFTER FIX:  get_cr1() returns 0 for queue bits when queue is enabled → PASSES.
#[test]
fn bug_audit_61_set_cr1_queue_bits_ignored_when_queue_enabled() {
    let smmu = SMMU::new();
    // Enable queues only — SMMUEN remains 0.
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // QUEUE_* bits: CR1_QUEUE_SH(bits[5:4]) | CR1_QUEUE_OC(bits[3:2]) | CR1_QUEUE_IC(bits[1:0])
    let cr1_queue_bits = SMMU::CR1_QUEUE_SH | SMMU::CR1_QUEUE_OC | SMMU::CR1_QUEUE_IC;
    smmu.set_cr1(cr1_queue_bits);

    let cr1 = smmu.get_cr1();
    assert_eq!(
        cr1 & cr1_queue_bits,
        0,
        "BUG-AUDIT-61: CR1 QUEUE_* bits must be silently ignored when any queue is \
         enabled (ARM §6.3.11), even if SMMUEN=0. Current code applies the write. \
         get_cr1()=0x{:08X}",
        cr1
    );
}
