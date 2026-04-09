//! TDD tests for BUG-AUDIT-124: `set_cr0()` must enforce RO guards on
//! CMDQEN, EVENTQEN, and PRIQEN per ARM IHI0070G.b §6.3.9.3/§6.3.9.4.
//!
//! Spec: A queue-enable bit (CMDQEN, EVENTQEN, PRIQEN) is read-only (RO)
//! while it is 1 in either CR0 or CR0ACK.  Attempting to clear it via
//! `set_cr0()` must be silently ignored — the bit must remain 1.
//!
//! The bug: `set_cr0()` unconditionally overwrites CR0/CR0ACK with the
//! caller-supplied value, allowing queue-enable bits to be cleared while
//! the queue is active — a spec violation.
//!
//! Fix: after the PRI stripping step, read the current CR0 and CR0ACK
//! values and OR the currently-set queue-enable bits back into `effective`,
//! preventing them from being cleared (mirroring the guard in `set_cr1()`).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]

use smmu::SMMU;

// ============================================================================
// Test 1: CMDQEN is RO while set
// ============================================================================

/// BUG-AUDIT-124: CMDQEN must not be clearable via `set_cr0()` while it is 1.
///
/// ARM §6.3.9.3: CMDQEN is RO when CMDQEN=1 in CR0 or CR0ACK.
///
/// Steps:
///  1. `enable()` sets CMDQEN=1, EVENTQEN=1, PRIQEN=1, SMMUEN=1 in CR0/CR0ACK.
///  2. `set_cr0(CR0_SMMUEN)` attempts to clear CMDQEN (and EVENTQEN, PRIQEN).
///  3. CMDQEN must remain 1 (RO guard must preserve it).
#[test]
fn test_cmdqen_ro_while_set() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    // Precondition: CMDQEN=1 after enable().
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_CMDQEN,
        0,
        "precondition: CMDQEN must be 1 after enable()"
    );

    // Attempt to clear CMDQEN by writing only SMMUEN.
    smmu.set_cr0(SMMU::CR0_SMMUEN);

    // CMDQEN must still be 1 (RO while set per §6.3.9.3).
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_CMDQEN,
        0,
        "BUG-AUDIT-124/§6.3.9.3: CMDQEN must remain 1 — it is RO while set in CR0 or CR0ACK"
    );
}

// ============================================================================
// Test 2: EVENTQEN is RO while set
// ============================================================================

/// BUG-AUDIT-124: EVENTQEN must not be clearable via `set_cr0()` while it is 1.
///
/// ARM §6.3.9.4: EVENTQEN is RO when EVENTQEN=1 in CR0 or CR0ACK.
///
/// Steps:
///  1. `enable()` sets EVENTQEN=1.
///  2. `set_cr0(CR0_SMMUEN)` attempts to clear EVENTQEN.
///  3. EVENTQEN must remain 1.
#[test]
fn test_eventqen_ro_while_set() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    // Precondition: EVENTQEN=1 after enable().
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_EVENTQEN,
        0,
        "precondition: EVENTQEN must be 1 after enable()"
    );

    // Attempt to clear EVENTQEN by writing only SMMUEN.
    smmu.set_cr0(SMMU::CR0_SMMUEN);

    // EVENTQEN must still be 1 (RO while set per §6.3.9.4).
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_EVENTQEN,
        0,
        "BUG-AUDIT-124/§6.3.9.4: EVENTQEN must remain 1 — it is RO while set in CR0 or CR0ACK"
    );
}

// ============================================================================
// Test 3: PRIQEN is RO while set
// ============================================================================

/// BUG-AUDIT-124: PRIQEN must not be clearable via `set_cr0()` while it is 1.
///
/// Same RO-while-set rule applies to PRIQEN (§6.3.9.3 pattern).
///
/// Steps:
///  1. `enable()` sets PRIQEN=1 (PRI is supported by default).
///  2. `set_cr0(CR0_SMMUEN | CR0_CMDQEN | CR0_EVENTQEN)` attempts to clear
///     PRIQEN while leaving the other queue-enables set.
///  3. PRIQEN must remain 1.
#[test]
fn test_priqen_ro_while_set() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    // Precondition: PRIQEN=1 after enable() when PRI is supported.
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_PRIQEN,
        0,
        "precondition: PRIQEN must be 1 after enable() (PRI supported by default)"
    );

    // Attempt to clear PRIQEN while keeping other queue-enables set.
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // PRIQEN must still be 1 (RO while set).
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_PRIQEN,
        0,
        "BUG-AUDIT-124/§6.3.9.3: PRIQEN must remain 1 — it is RO while set in CR0 or CR0ACK"
    );
}

// ============================================================================
// Test 4: Queue bits are writable before any queue is enabled
// ============================================================================

/// BUG-AUDIT-124: Queue-enable bits must be settable when no queue is
/// currently enabled (CR0 = 0 = CR0ACK at reset).
///
/// The RO guard only applies while the bit is already 1.  Before `enable()`,
/// all queue-enable bits are 0, so `set_cr0(CR0_CMDQEN)` must succeed and
/// CMDQEN must read back as 1.
#[test]
fn test_queue_bits_writable_before_enable() {
    let smmu = SMMU::new();

    // Precondition: CR0=0 after reset.
    assert_eq!(smmu.get_cr0(), 0, "precondition: CR0 must be 0 after reset");

    // Set CMDQEN before any queue has been enabled.
    smmu.set_cr0(SMMU::CR0_CMDQEN);

    // CMDQEN must be 1 — it was 0 so it is not RO, and the write must succeed.
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_CMDQEN,
        0,
        "BUG-AUDIT-124: CMDQEN must be settable when no queue is currently enabled (bit was 0)"
    );
}

// ============================================================================
// Test 5: SMMUEN is clearable after disable() clears it along with queue bits
// ============================================================================

/// BUG-AUDIT-124: After `disable()` (which clears only SMMUEN, leaving
/// CMDQEN/EVENTQEN/PRIQEN set), calling `set_cr0(0)` must NOT be able to
/// clear SMMUEN — because the queue-enable bits are still 1 in CR0/CR0ACK
/// and the RO guard must preserve them.
///
/// Note: `disable()` only clears SMMUEN via `fetch_and(!CR0_SMMUEN)`, so
/// the queue-enable bits remain 1.  Therefore `set_cr0(0)` is still
/// constrained by the RO guard on those queue bits; SMMUEN can be left 0
/// (it was already 0 after `disable()`), but queue-enable bits must stay 1.
///
/// This verifies that SMMUEN=0 (already cleared by `disable()`) is preserved
/// correctly while queue bits remain locked.
#[test]
fn test_smmuen_clearable_when_queues_disabled() {
    let smmu = SMMU::new();

    // Set up: enable SMMU (sets SMMUEN=1, CMDQEN=1, EVENTQEN=1, PRIQEN=1).
    smmu.enable().unwrap();
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_SMMUEN,
        0,
        "precondition: SMMUEN must be 1 after enable()"
    );

    // disable() clears only SMMUEN via fetch_and, leaving queue bits set.
    smmu.disable().unwrap();
    assert_eq!(
        smmu.get_cr0() & SMMU::CR0_SMMUEN,
        0,
        "precondition: SMMUEN must be 0 after disable()"
    );
    // Queue bits must still be 1 after disable().
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_CMDQEN,
        0,
        "precondition: CMDQEN must remain 1 after disable()"
    );

    // Now call set_cr0(0) — SMMUEN is already 0, queue bits are RO (still 1).
    smmu.set_cr0(0);

    // SMMUEN must remain 0 (it was already 0; set_cr0(0) cannot set it).
    assert_eq!(
        smmu.get_cr0() & SMMU::CR0_SMMUEN,
        0,
        "BUG-AUDIT-124: SMMUEN must remain 0 after set_cr0(0) — it was already 0"
    );
    // Queue-enable bits must remain 1 (RO guard preserves them).
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_CMDQEN,
        0,
        "BUG-AUDIT-124: CMDQEN must remain 1 — RO guard preserves it while set in CR0ACK"
    );
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_EVENTQEN,
        0,
        "BUG-AUDIT-124: EVENTQEN must remain 1 — RO guard preserves it while set in CR0ACK"
    );
}
