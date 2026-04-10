#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

//! TDD tests for ARM SMMU v3 §8.1.1 "Recovery procedure" for PRI queue overflow.
//!
//! §8.1.1 specifies:
//! - Process entries from PRIQ_CONS.RD to PRIQ_PROD.WR.
//! - PRGs with a visible Last==1 are complete and must be processed.
//! - PRGs whose Last==1 was among the lost (auto-responded) entries are
//!   incomplete and must be **ignored**.
//! - Update PRIQ_CONS.RD including OVACKFLG to clear the overflow condition
//!   and mark the entire queue as empty or processed.

use smmu::smmu::PriqRecoveryResult;
use smmu::types::{AccessType, PRIEntry};
use smmu::SMMU;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Enable the SMMU with all queue bits set (SMMUEN + CMDQEN + EVENTQEN + PRIQEN).
fn smmu_with_priq_enabled(pri_queue_size: usize) -> SMMU {
    use smmu::types::{QueueConfig, SMMUConfigBuilder};
    let config = SMMUConfigBuilder::new()
        .queue_config(QueueConfig {
            event_queue_size: 64,
            command_queue_size: 64,
            pri_queue_size,
        })
        .build()
        .expect("SMMUConfigBuilder::build() must succeed");
    let smmu = SMMU::with_config(config);
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    smmu
}

/// Construct a PRIEntry with the given PRG index and Last flag.
const fn make_req(prg: u16, last: bool) -> PRIEntry {
    PRIEntry {
        stream_id: 1,
        pasid: 0,
        requested_address: 0x1000,
        access_type: AccessType::Read,
        is_last_request: last,
        timestamp: 0,
        prg_index: prg,
        security_state: smmu::types::SecurityState::NonSecure,
        span: 0,
    }
}

/// Check whether overflow is active: OVFLG (bit 31 of PRIQ_PROD) != OVACKFLG (bit 31 of PRIQ_CONS).
fn is_overflow_active(smmu: &SMMU) -> bool {
    let prod = smmu.priq_prod_index();
    let cons = smmu.priq_cons_index();
    (prod >> 31) != (cons >> 31)
}

// ---------------------------------------------------------------------------
// Test 1: happy-path overflow recovery clears overflow condition
// ---------------------------------------------------------------------------

/// §8.1.1 — After overflow recovery:
/// - OVFLG == OVACKFLG (overflow cleared).
/// - PRI queue is empty.
/// - RecoveryResult counts complete vs incomplete PRGs correctly.
///
/// Setup: capacity=4 (special test-only size).
/// Submit:
///   PRG-0 Last=0  — slot 0  (incomplete: no Last in queue)
///   PRG-1 Last=0  — slot 1
///   PRG-1 Last=1  — slot 2  (complete: Last visible)
///   PRG-2 Last=1  — slot 3
///   PRG-3 Last=1  — overflows, gets auto-response; PRG-3 never seen in queue
///
/// Queue at overflow: [PRG-0 Last=0, PRG-1 Last=0, PRG-1 Last=1, PRG-2 Last=1]
/// PRG-1: complete (Last=1 visible). PRG-2: complete (Last=1 visible).
/// PRG-0: incomplete (no Last in queue). PRG-3: auto-responded, never in queue.
#[test]
fn test_recover_priq_overflow_clears_overflow_condition() {
    // capacity = 4 (special size allowed by QueueConfig validator for overflow tests).
    let smmu = smmu_with_priq_enabled(4);

    smmu.submit_page_request(make_req(0, false)).expect("PRG-0 Last=0 must be accepted");
    smmu.submit_page_request(make_req(1, false)).expect("PRG-1 Last=0 must be accepted");
    smmu.submit_page_request(make_req(1, true)).expect("PRG-1 Last=1 must be accepted");
    smmu.submit_page_request(make_req(2, true)).expect("PRG-2 Last=1 must be accepted");
    // Queue is now full (4 of 4 slots used). Next entry triggers overflow.
    let r = smmu.submit_page_request(make_req(3, true));
    assert!(r.is_err(), "PRG-3 must be rejected (queue full → overflow)");

    // Overflow must now be active.
    assert!(
        is_overflow_active(&smmu),
        "OVFLG must differ from OVACKFLG after overflow"
    );

    // Run recovery.
    let result = smmu.recover_priq_overflow();

    // §8.1.1: overflow must be cleared after recovery.
    assert!(
        !is_overflow_active(&smmu),
        "OVFLG must equal OVACKFLG after recover_priq_overflow()"
    );

    // Queue must be empty.
    assert_eq!(smmu.get_pri_queue_size(), 0, "PRI queue must be empty after recovery");

    // PRG-1 and PRG-2 are complete (Last=1 visible). PRG-0 is incomplete.
    // PRG-3 was auto-responded and never entered the queue, so it does not
    // appear as either complete or incomplete in the recovery scan.
    assert_eq!(
        result,
        PriqRecoveryResult { complete_groups: 2, incomplete_groups: 1 },
        "§8.1.1: PRG-1 and PRG-2 complete, PRG-0 incomplete (no Last in queue)"
    );
}

// ---------------------------------------------------------------------------
// Test 2: incomplete groups (Last lost in overflow) are skipped
// ---------------------------------------------------------------------------

/// §8.1.1 — Groups whose Last was discarded (auto-responded on overflow) must
/// be ignored by software.
///
/// Setup: capacity=4 (special test-only size).
/// Submit:
///   PRG-0 Last=0  → slot 0 (no Last visible → incomplete)
///   PRG-1 Last=0  → slot 1
///   PRG-1 Last=1  → slot 2  (complete — Last visible)
///   PRG-2 Last=0  → slot 3
///   PRG-0 Last=1  → overflows, auto-responded (lost Last for PRG-0)
///
/// Queue at overflow: [PRG-0 Last=0, PRG-1 Last=0, PRG-1 Last=1, PRG-2 Last=0]
/// PRG-1 is complete (Last=1 in queue).
/// PRG-0 is incomplete (its Last was the one that overflowed).
/// PRG-2 is also incomplete (no Last in queue).
#[test]
fn test_recover_priq_overflow_skips_incomplete_groups() {
    let smmu = smmu_with_priq_enabled(4);

    smmu.submit_page_request(make_req(0, false)).expect("PRG-0 part1 must be accepted");
    smmu.submit_page_request(make_req(1, false)).expect("PRG-1 part1 must be accepted");
    smmu.submit_page_request(make_req(1, true)).expect("PRG-1 Last must be accepted");
    smmu.submit_page_request(make_req(2, false)).expect("PRG-2 part1 must be accepted");
    // Queue is now full. PRG-0's Last entry overflows — auto-responded, lost from queue.
    let r = smmu.submit_page_request(make_req(0, true));
    assert!(r.is_err(), "PRG-0 Last must be rejected (queue full)");

    assert!(is_overflow_active(&smmu), "overflow must be active");

    let result = smmu.recover_priq_overflow();

    // Overflow cleared.
    assert!(!is_overflow_active(&smmu), "overflow must be cleared after recovery");
    // Queue empty.
    assert_eq!(smmu.get_pri_queue_size(), 0, "queue must be empty after recovery");

    // PRG-1 is complete (has Last=1 in queue).
    // PRG-0 and PRG-2 are incomplete (no Last visible in queue).
    assert_eq!(
        result,
        PriqRecoveryResult { complete_groups: 1, incomplete_groups: 2 },
        "§8.1.1: PRG-1 complete, PRG-0 and PRG-2 incomplete (Last was lost or not yet arrived)"
    );
}

// ---------------------------------------------------------------------------
// Test 3: no-op when no overflow is active
// ---------------------------------------------------------------------------

/// §8.1.1 — When no overflow is active (OVFLG == OVACKFLG), recover_priq_overflow()
/// must return immediately with zero counts and leave state unchanged.
#[test]
fn test_recover_priq_overflow_noop_when_no_overflow() {
    let smmu = smmu_with_priq_enabled(16);

    // Submit one entry — no overflow.
    smmu.submit_page_request(make_req(0, true)).expect("entry must be accepted");

    assert!(!is_overflow_active(&smmu), "no overflow expected");

    let prod_before = smmu.priq_prod_index();
    let cons_before = smmu.priq_cons_index();

    let result = smmu.recover_priq_overflow();

    // Must return zeros.
    assert_eq!(
        result,
        PriqRecoveryResult { complete_groups: 0, incomplete_groups: 0 },
        "§8.1.1: no-op when overflow is not active"
    );

    // State must be unchanged.
    assert_eq!(
        smmu.priq_prod_index(),
        prod_before,
        "priq_prod must be unchanged when no overflow"
    );
    assert_eq!(
        smmu.priq_cons_index(),
        cons_before,
        "priq_cons must be unchanged when no overflow"
    );

    // Queue entry must still be there.
    assert_eq!(smmu.get_pri_queue_size(), 1, "queue entry must be preserved when no overflow");
}
