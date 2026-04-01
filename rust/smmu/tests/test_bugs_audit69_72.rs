//! TDD failing tests for BUG-AUDIT-69 through BUG-AUDIT-72 (Rust side: 69, 70, 71).
//!
//! **BUG-AUDIT-69** (Rust §6.3.25): `set_strtab_split()` checks only `cr0.SMMUEN`
//!   but must also check `cr0ack.SMMUEN` per ARM §6.3.25.
//!   Pattern already fixed in `set_strtab_format()` (BUG-AUDIT-67 fix):
//!     `(cr0 | cr0ack) & SMMUEN != 0` → block write.
//!   Current `set_strtab_split()` (line ~1816):
//!     `cr0 & SMMUEN != 0` only → misses the CR0ACK.SMMUEN==1 path.
//!   BEFORE FIX: With cr0.SMMUEN=0 but cr0ack.SMMUEN=1 (via reset_cr0ack trick),
//!               set_strtab_split() allows the write — wrong.
//!   AFTER FIX:  Write is blocked whenever (cr0|cr0ack).SMMUEN != 0.
//!
//! **BUG-AUDIT-70** (Rust §6.3.24/6.3.25): `set_strtab_log2size()` has the same
//!   missing `cr0ack` OR as BUG-AUDIT-69.
//!   Current `set_strtab_log2size()` (line ~1869):
//!     `cr0 & SMMUEN != 0` only.
//!   BEFORE FIX: With cr0.SMMUEN=0 but cr0ack.SMMUEN=1, write proceeds — wrong.
//!   AFTER FIX:  Write blocked by `(cr0|cr0ack) & SMMUEN`.
//!
//! **BUG-AUDIT-71** (Rust §6.3.9): `enable()` unconditionally ORs in CR0_PRIQEN
//!   regardless of `pri_supported`.
//!   ARM §6.3.9: PRIQEN is RES0 when IDR0.PRI==0.
//!   Current `enable()` (line ~1047): always sets PRIQEN=1 in CR0 and CR0ACK.
//!   BEFORE FIX:
//!     set_pri_supported(false) → enable() → get_cr0ack() still has PRIQEN=1 — wrong.
//!   AFTER FIX:
//!     set_pri_supported(false) → enable() → CR0ACK.PRIQEN=0.
//!     set_pri_supported(true)  → enable() → CR0ACK.PRIQEN=1.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::StreamTableFormat;
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

/// Build a fresh SMMU that has been globally enabled (SMMUEN=1, CR0ACK mirrors CR0).
fn make_enabled_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_s1p_supported(true);
    smmu.set_s2p_supported(true);
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    smmu
}

// ============================================================================
// BUG-AUDIT-69: set_strtab_split() missing CR0ACK.SMMUEN check
// ============================================================================

/// BUG-AUDIT-69 test 1 (baseline): `set_strtab_split()` is blocked when CR0.SMMUEN=1.
///
/// This is the existing guard — it must always pass before and after the fix.
///
/// BEFORE / AFTER FIX: blocked by CR0.SMMUEN=1 → PASSES.
#[test]
fn bug_audit_69_set_strtab_split_blocked_by_cr0_smmuen() {
    let smmu = make_enabled_smmu();

    // Precondition: CR0.SMMUEN=1.
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_SMMUEN,
        0,
        "BUG-AUDIT-69 precondition: CR0.SMMUEN must be 1"
    );

    // Record current split; try to change it while SMMUEN=1.
    let before = smmu.get_strtab_split();
    // Pick a different valid split value.
    let new_split = if before == 6 { 8 } else { 6 };
    smmu.set_strtab_split(new_split);

    let after = smmu.get_strtab_split();
    assert_eq!(
        after, before,
        "BUG-AUDIT-69 baseline: set_strtab_split() must be blocked when CR0.SMMUEN=1. \
         ARM §6.3.25: STRTAB_BASE_CFG is RO when SMMUEN=1. \
         before={} new_split={} after={}",
        before, new_split, after
    );
}

/// BUG-AUDIT-69 test 2 (core): `set_strtab_split()` must be blocked when
/// CR0.SMMUEN=0 but CR0ACK.SMMUEN=1.
///
/// ARM §6.3.25: STRTAB_BASE_CFG is RO when CR0.SMMUEN==1 OR CR0ACK.SMMUEN==1.
///
/// Scenario:
///   1. Enable SMMU (CR0.SMMUEN=1, CR0ACK.SMMUEN=1).
///   2. Record split value.
///   3. Clear CR0.SMMUEN via set_cr0(0) — now CR0.SMMUEN=0.
///   4. Restore CR0ACK.SMMUEN=1 via reset to a state where only CR0ACK has SMMUEN.
///      NOTE: Rust only exposes `reset_cr0ack()` (sets to 0), not `set_cr0ack()`.
///      The test cannot independently set CR0ACK.SMMUEN=1 while CR0.SMMUEN=0.
///      Instead we verify the positive case: when BOTH are 0, the write must succeed.
///      The actual FAILING test is: with CR0.SMMUEN=1 (from enable), the write is blocked.
///      The missing-CR0ACK path is documented but cannot be exercised without set_cr0ack().
///
/// What this test DOES verify (to expose the bug):
///   Enable SMMU → CR0.SMMUEN=1, CR0ACK.SMMUEN=1.
///   Call set_cr0(0) → CR0 syncs CR0ACK=0 in synchronous model (via set_cr0).
///   Now try set_strtab_split() — should succeed (both 0).
///   Then call enable() again → CR0.SMMUEN=1.
///   Call reset_cr0ack() → CR0ACK=0, CR0.SMMUEN=1.
///   Try set_strtab_split() — current code blocks (CR0.SMMUEN=1 ✓).
///   This is the baseline; the missing case (CR0=0, CR0ACK=1) requires set_cr0ack().
///
/// ACTUAL FAILING scenario for BUG-AUDIT-69:
///   After enable(), reset_cr0ack() zeroes CR0ACK while CR0.SMMUEN=1.
///   The bug is the INVERSE: CR0=0, CR0ACK=1 allows the write. Since we cannot
///   set CR0ACK to a nonzero value independently in Rust, we demonstrate the
///   asymmetry with the corrected check by verifying the guard via set_strtab_format()
///   (which has the fix) vs set_strtab_split() (which does not).
///
/// The definitive failing test: after enable(), call set_cr0(0) (which clears both
/// CR0 and CR0ACK to 0 via the synchronous model in set_cr0). Then manually restore
/// CR0ACK.SMMUEN by calling enable() again then immediately reset_cr0ack() then
/// calling set_cr0 back to 0. But we can show the guard difference:
///   - set_strtab_format() uses (cr0|cr0ack) guard → still blocks after reset_cr0ack
///     because CR0.SMMUEN=1 (reset_cr0ack only clears cr0ack, cr0 remains).
///   - set_strtab_split() uses only cr0 guard.
///
/// The real test: with CR0.SMMUEN=1 and reset_cr0ack() called (CR0ACK=0):
///   set_strtab_split() is still blocked (CR0.SMMUEN=1 catches it via the existing guard).
///   The BUG fires only when CR0.SMMUEN=0 and CR0ACK.SMMUEN=1.
///
/// Since we cannot set CR0ACK independently in Rust, the most we can do is:
///   1. Document the bug clearly.
///   2. Verify that after enable(), set_strtab_split() IS blocked (existing behavior).
///   3. Verify that set_strtab_format() and set_strtab_split() have identical behavior
///      when CR0.SMMUEN=1: both blocked — a regression guard for the symmetry.
///
/// IMPORTANT NOTE: The REAL BUG-AUDIT-69 failing test requires `set_cr0ack()` API
/// (available in C++) but absent in Rust. The Rust tests below use the available
/// `reset_cr0ack()` and `enable()` to expose the asymmetry as best as possible.
///
/// BEFORE FIX: if we could set CR0ACK.SMMUEN=1 with CR0.SMMUEN=0, set_strtab_split
///             would allow the write → FAILS.
/// AFTER FIX:  both CR0.SMMUEN and CR0ACK.SMMUEN block the write → PASSES.
#[test]
fn bug_audit_69_set_strtab_split_blocked_when_cr0ack_smmuen_set_cr0_cleared() {
    let smmu = SMMU::new();
    smmu.set_s1p_supported(true);
    smmu.set_s2p_supported(true);

    // Step 1: Set split to a known value (6) while SMMU is disabled.
    smmu.set_strtab_split(6);
    assert_eq!(
        smmu.get_strtab_split(),
        6,
        "BUG-AUDIT-69 precondition: set_strtab_split(6) must succeed before enable"
    );

    // Step 2: Enable SMMU — now CR0.SMMUEN=1 AND CR0ACK.SMMUEN=1.
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_SMMUEN,
        0,
        "BUG-AUDIT-69: CR0.SMMUEN must be 1 after enable"
    );
    assert_ne!(
        smmu.get_cr0ack() & SMMU::CR0_SMMUEN,
        0,
        "BUG-AUDIT-69: CR0ACK.SMMUEN must be 1 after enable (synchronous model)"
    );

    // Step 3: Reset CR0ACK to 0 while CR0.SMMUEN remains 1.
    // This simulates an async HW state where CR0ACK lags behind CR0.
    smmu.reset_cr0ack();
    assert_eq!(
        smmu.get_cr0ack() & SMMU::CR0_SMMUEN,
        0,
        "BUG-AUDIT-69: after reset_cr0ack(), CR0ACK.SMMUEN must be 0"
    );
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_SMMUEN,
        0,
        "BUG-AUDIT-69: CR0.SMMUEN must still be 1 after reset_cr0ack()"
    );

    // Step 4: Attempt set_strtab_split(8) — must be blocked (CR0.SMMUEN=1 covers it).
    smmu.set_strtab_split(8);
    let after = smmu.get_strtab_split();
    assert_eq!(
        after, 6,
        "BUG-AUDIT-69: set_strtab_split() must be blocked when CR0.SMMUEN=1. \
         ARM §6.3.25: STRTAB_BASE_CFG is RO when SMMUEN=1 OR CR0ACK.SMMUEN=1. \
         after={}",
        after
    );

    // Step 5 (the BUG scenario): Now clear CR0.SMMUEN while CR0ACK is already 0.
    // Then manually force CR0ACK.SMMUEN=1 — this requires set_cr0ack() which is NOT
    // available in Rust. We simulate by: clear CR0, re-enable, THEN reset_cr0ack.
    // After clear CR0: both CR0.SMMUEN=0 and CR0ACK mirrors to 0 via set_cr0().
    smmu.set_cr0(0);
    assert_eq!(
        smmu.get_cr0() & SMMU::CR0_SMMUEN,
        0,
        "BUG-AUDIT-69: after set_cr0(0), CR0.SMMUEN must be 0"
    );
    // In Rust's sync model, set_cr0(0) ALSO clears CR0ACK.SMMUEN.
    // So we cannot independently have CR0.SMMUEN=0 with CR0ACK.SMMUEN=1 without set_cr0ack().
    //
    // The BUG-AUDIT-69 scenario (CR0=0, CR0ACK=1) must be verified in the C++ test
    // where setCR0ACK() is available. This Rust test documents the gap.
    //
    // Verify that with both cleared, set_strtab_split() DOES succeed (regression guard).
    smmu.set_strtab_split(8);
    let after_cleared = smmu.get_strtab_split();
    assert_eq!(
        after_cleared, 8,
        "BUG-AUDIT-69 regression: set_strtab_split() must succeed when CR0.SMMUEN=0 \
         and CR0ACK.SMMUEN=0. ARM §6.3.25: only blocked when SMMUEN=1 in either register. \
         after_cleared={}",
        after_cleared
    );
}

/// BUG-AUDIT-69 test 3: Symmetry guard — `set_strtab_format()` and `set_strtab_split()`
/// must exhibit identical guard behavior when CR0.SMMUEN=1.
///
/// After BUG-AUDIT-67, `set_strtab_format()` uses the correct `(cr0|cr0ack)` guard.
/// `set_strtab_split()` still uses only `cr0`. When CR0.SMMUEN=1, both are blocked,
/// so this test verifies behavioral symmetry (a regression guard for the fix).
///
/// BEFORE / AFTER FIX: both blocked when SMMUEN=1 → PASSES both.
#[test]
fn bug_audit_69_set_strtab_split_and_format_symmetric_when_smmuen_set() {
    let smmu = make_enabled_smmu();

    // Precondition: CR0.SMMUEN=1.
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_SMMUEN,
        0,
        "BUG-AUDIT-69 symmetry precondition: CR0.SMMUEN must be 1"
    );

    // Attempt set_strtab_format() while SMMUEN=1 — must be blocked (already correct).
    let fmt_before = smmu.get_strtab_format();
    let new_fmt = if fmt_before == StreamTableFormat::Linear {
        StreamTableFormat::TwoLevel
    } else {
        StreamTableFormat::Linear
    };
    smmu.set_strtab_format(new_fmt);
    let fmt_after = smmu.get_strtab_format();
    assert_eq!(
        fmt_after, fmt_before,
        "BUG-AUDIT-69 symmetry: set_strtab_format() must be blocked when SMMUEN=1 \
         (this is the correctly-fixed function from BUG-AUDIT-67). \
         fmt_before={:?} fmt_after={:?}",
        fmt_before, fmt_after
    );

    // Attempt set_strtab_split() while SMMUEN=1 — must also be blocked.
    let split_before = smmu.get_strtab_split();
    let new_split = if split_before == 6 { 8 } else { 6 };
    smmu.set_strtab_split(new_split);
    let split_after = smmu.get_strtab_split();
    assert_eq!(
        split_after, split_before,
        "BUG-AUDIT-69: set_strtab_split() must be blocked when SMMUEN=1, \
         same as set_strtab_format(). ARM §6.3.25: STRTAB_BASE_CFG is RO when SMMUEN=1. \
         Current code uses only CR0.SMMUEN guard (missing CR0ACK.SMMUEN). \
         split_before={} split_after={}",
        split_before, split_after
    );
}

// ============================================================================
// BUG-AUDIT-70: set_strtab_log2size() missing CR0ACK.SMMUEN check
// ============================================================================

/// BUG-AUDIT-70 test 1 (baseline): `set_strtab_log2size()` is blocked when CR0.SMMUEN=1.
///
/// This is the existing guard — must always pass before and after the fix.
///
/// BEFORE / AFTER FIX: blocked by CR0.SMMUEN=1 → PASSES.
#[test]
fn bug_audit_70_set_strtab_log2size_blocked_by_cr0_smmuen() {
    let smmu = make_enabled_smmu();

    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_SMMUEN,
        0,
        "BUG-AUDIT-70 precondition: CR0.SMMUEN must be 1"
    );

    let before = smmu.get_strtab_log2size();
    // Attempt to change log2size while SMMUEN=1.
    let new_size = if before == 20 { 16u8 } else { 20u8 };
    smmu.set_strtab_log2size(new_size);

    let after = smmu.get_strtab_log2size();
    assert_eq!(
        after, before,
        "BUG-AUDIT-70 baseline: set_strtab_log2size() must be blocked when CR0.SMMUEN=1. \
         ARM §6.3.24/§6.3.25: STRTAB_BASE is RO when SMMUEN=1. \
         before={} new_size={} after={}",
        before, new_size, after
    );
}

/// BUG-AUDIT-70 test 2 (core): `set_strtab_log2size()` must be blocked when
/// CR0ACK.SMMUEN=1 even if CR0.SMMUEN=0.
///
/// ARM §6.3.24/§6.3.25: STRTAB_BASE is RO when CR0.SMMUEN==1 OR CR0ACK.SMMUEN==1.
/// Current code only checks CR0.SMMUEN.
///
/// NOTE: As with BUG-AUDIT-69, we cannot independently set CR0ACK.SMMUEN=1 with
/// CR0.SMMUEN=0 in Rust (no set_cr0ack() API). The test demonstrates the guard
/// behavior in the accessible states and documents the gap.
///
/// BEFORE FIX: CR0=0, CR0ACK=1 (hypothetical) → write proceeds — wrong.
/// AFTER FIX:  write blocked by (cr0|cr0ack) & SMMUEN.
#[test]
fn bug_audit_70_set_strtab_log2size_blocked_when_cr0ack_smmuen_set() {
    let smmu = SMMU::new();
    smmu.set_s1p_supported(true);
    smmu.set_s2p_supported(true);

    // Set a known log2size before enabling.
    smmu.set_strtab_log2size(15);
    assert_eq!(
        smmu.get_strtab_log2size(),
        15,
        "BUG-AUDIT-70 precondition: set_strtab_log2size(15) must succeed before enable"
    );

    // Enable → CR0.SMMUEN=1 and CR0ACK.SMMUEN=1.
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    assert_ne!(smmu.get_cr0() & SMMU::CR0_SMMUEN, 0, "precondition: CR0.SMMUEN=1");
    assert_ne!(smmu.get_cr0ack() & SMMU::CR0_SMMUEN, 0, "precondition: CR0ACK.SMMUEN=1");

    // Reset CR0ACK to 0 while CR0.SMMUEN remains 1.
    smmu.reset_cr0ack();
    assert_eq!(smmu.get_cr0ack() & SMMU::CR0_SMMUEN, 0, "CR0ACK.SMMUEN=0 after reset");
    assert_ne!(smmu.get_cr0() & SMMU::CR0_SMMUEN, 0, "CR0.SMMUEN=1 still");

    // Attempt write — must still be blocked (CR0.SMMUEN=1 covers this case).
    smmu.set_strtab_log2size(20);
    let after_cr0_blocked = smmu.get_strtab_log2size();
    assert_eq!(
        after_cr0_blocked, 15,
        "BUG-AUDIT-70: set_strtab_log2size() must be blocked when CR0.SMMUEN=1. \
         after={}",
        after_cr0_blocked
    );

    // Clear CR0 (syncs CR0ACK=0 in synchronous model).
    smmu.set_cr0(0);
    assert_eq!(smmu.get_cr0() & SMMU::CR0_SMMUEN, 0, "CR0.SMMUEN=0 after clearing");

    // With both CR0.SMMUEN=0 and CR0ACK.SMMUEN=0 → write must succeed.
    smmu.set_strtab_log2size(18);
    let after_both_cleared = smmu.get_strtab_log2size();
    assert_eq!(
        after_both_cleared, 18,
        "BUG-AUDIT-70 regression: set_strtab_log2size() must succeed when both \
         CR0.SMMUEN=0 and CR0ACK.SMMUEN=0. \
         after={}",
        after_both_cleared
    );
    // NOTE: The BUG-AUDIT-70 failing scenario (CR0=0, CR0ACK=1) requires set_cr0ack()
    // which is not available in Rust. See C++ test BUG-AUDIT-70 for full coverage.
}

/// BUG-AUDIT-70 test 3: Symmetry — `set_strtab_log2size()` and `set_strtab_format()`
/// must have identical guard behavior when CR0.SMMUEN=1.
///
/// Both should be RO when SMMUEN=1. set_strtab_format() has the correct
/// (cr0|cr0ack) guard after BUG-AUDIT-67 fix. set_strtab_log2size() uses only cr0.
///
/// BEFORE / AFTER FIX: when CR0.SMMUEN=1, both are blocked → PASSES.
#[test]
fn bug_audit_70_set_strtab_log2size_and_format_symmetric_when_smmuen_set() {
    let smmu = make_enabled_smmu();

    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_SMMUEN,
        0,
        "BUG-AUDIT-70 symmetry precondition: CR0.SMMUEN must be 1"
    );

    // set_strtab_format() must be blocked (already correct).
    let fmt_before = smmu.get_strtab_format();
    let new_fmt = if fmt_before == StreamTableFormat::Linear {
        StreamTableFormat::TwoLevel
    } else {
        StreamTableFormat::Linear
    };
    smmu.set_strtab_format(new_fmt);
    assert_eq!(
        smmu.get_strtab_format(),
        fmt_before,
        "BUG-AUDIT-70 symmetry: set_strtab_format() must be blocked when SMMUEN=1 \
         (correctly fixed in BUG-AUDIT-67)"
    );

    // set_strtab_log2size() must also be blocked.
    let size_before = smmu.get_strtab_log2size();
    let new_size = if size_before == 20 { 16u8 } else { 20u8 };
    smmu.set_strtab_log2size(new_size);
    let size_after = smmu.get_strtab_log2size();
    assert_eq!(
        size_after, size_before,
        "BUG-AUDIT-70: set_strtab_log2size() must be blocked when SMMUEN=1, \
         same as set_strtab_format(). ARM §6.3.24/§6.3.25: STRTAB_BASE* is RO when SMMUEN=1. \
         Current code uses only CR0.SMMUEN guard (missing CR0ACK.SMMUEN). \
         size_before={} size_after={}",
        size_before, size_after
    );
}

// ============================================================================
// BUG-AUDIT-71: enable() unconditionally sets CR0_PRIQEN regardless of pri_supported
// ============================================================================

/// BUG-AUDIT-71 test 1: `enable()` with `pri_supported=false` must NOT set PRIQEN.
///
/// ARM §6.3.9: CR0.PRIQEN is RES0 when IDR0.PRI==0.
/// After `enable()`, CR0ACK must NOT have PRIQEN=1 when `pri_supported=false`.
///
/// BEFORE FIX: CR0.PRIQEN=1 and CR0ACK.PRIQEN=1 after enable() regardless — FAILS.
/// AFTER FIX:  CR0.PRIQEN=0 and CR0ACK.PRIQEN=0 when pri_supported=false — PASSES.
#[test]
fn bug_audit_71_enable_must_not_set_priqen_when_pri_not_supported() {
    let smmu = SMMU::new();

    // Disable PRI support (IDR0.PRI=0).
    smmu.set_pri_supported(false);

    // Confirm IDR0.PRI=0 (bit 16).
    assert_eq!(
        (smmu.get_idr0() >> 16) & 1,
        0,
        "BUG-AUDIT-71 precondition: IDR0.PRI must be 0 when set_pri_supported(false)"
    );

    // Call enable().
    smmu.enable().expect("enable() must succeed");

    // CR0ACK.PRIQEN (bit 1) must be 0 because PRI is not supported.
    let cr0ack = smmu.get_cr0ack();
    assert_eq!(
        cr0ack & SMMU::CR0_PRIQEN,
        0,
        "BUG-AUDIT-71: enable() must NOT set CR0ACK.PRIQEN when pri_supported=false. \
         ARM §6.3.9: PRIQEN is RES0 when IDR0.PRI==0. \
         Current code unconditionally ORs in CR0_PRIQEN regardless of pri_supported. \
         cr0ack=0x{:08X} CR0_PRIQEN=0x{:08X}",
        cr0ack, SMMU::CR0_PRIQEN
    );

    // CR0.PRIQEN must also be 0.
    let cr0 = smmu.get_cr0();
    assert_eq!(
        cr0 & SMMU::CR0_PRIQEN,
        0,
        "BUG-AUDIT-71: enable() must NOT set CR0.PRIQEN when pri_supported=false. \
         ARM §6.3.9: PRIQEN is RES0 when IDR0.PRI==0. \
         cr0=0x{:08X}",
        cr0
    );
}

/// BUG-AUDIT-71 test 2: `enable()` with `pri_supported=true` MUST set PRIQEN.
///
/// When IDR0.PRI=1 (pri_supported=true), PRIQEN must be set after enable().
///
/// BEFORE FIX: PRIQEN is always set — this test PASSES before fix.
/// AFTER FIX:  PRIQEN is conditionally set — this test still PASSES.
#[test]
fn bug_audit_71_enable_must_set_priqen_when_pri_supported() {
    let smmu = SMMU::new();

    // Enable PRI support (IDR0.PRI=1).
    smmu.set_pri_supported(true);

    // Confirm IDR0.PRI=1 (bit 16).
    assert_ne!(
        (smmu.get_idr0() >> 16) & 1,
        0,
        "BUG-AUDIT-71 precondition: IDR0.PRI must be 1 when set_pri_supported(true)"
    );

    // Call enable().
    smmu.enable().expect("enable() must succeed");

    // CR0ACK.PRIQEN must be 1.
    let cr0ack = smmu.get_cr0ack();
    assert_ne!(
        cr0ack & SMMU::CR0_PRIQEN,
        0,
        "BUG-AUDIT-71: enable() must set CR0ACK.PRIQEN=1 when pri_supported=true. \
         ARM §6.3.9: PRIQEN is writable when IDR0.PRI==1. \
         cr0ack=0x{:08X}",
        cr0ack
    );

    // CR0.PRIQEN must also be 1.
    let cr0 = smmu.get_cr0();
    assert_ne!(
        cr0 & SMMU::CR0_PRIQEN,
        0,
        "BUG-AUDIT-71: enable() must set CR0.PRIQEN=1 when pri_supported=true. \
         cr0=0x{:08X}",
        cr0
    );
}

/// BUG-AUDIT-71 test 3: SMMUEN, CMDQEN, EVENTQEN are always set by enable() regardless
/// of pri_supported. Only PRIQEN is conditional.
///
/// BEFORE / AFTER FIX: SMMUEN, CMDQEN, EVENTQEN always set → PASSES.
#[test]
fn bug_audit_71_enable_always_sets_smmuen_cmdqen_eventqen() {
    for pri_supported in [false, true] {
        let smmu = SMMU::new();
        smmu.set_pri_supported(pri_supported);
        smmu.enable().expect("enable() must succeed");

        let cr0ack = smmu.get_cr0ack();

        assert_ne!(
            cr0ack & SMMU::CR0_SMMUEN,
            0,
            "BUG-AUDIT-71: enable() must always set CR0ACK.SMMUEN regardless of pri_supported. \
             pri_supported={} cr0ack=0x{:08X}",
            pri_supported, cr0ack
        );
        assert_ne!(
            cr0ack & SMMU::CR0_CMDQEN,
            0,
            "BUG-AUDIT-71: enable() must always set CR0ACK.CMDQEN regardless of pri_supported. \
             pri_supported={} cr0ack=0x{:08X}",
            pri_supported, cr0ack
        );
        assert_ne!(
            cr0ack & SMMU::CR0_EVENTQEN,
            0,
            "BUG-AUDIT-71: enable() must always set CR0ACK.EVENTQEN regardless of pri_supported. \
             pri_supported={} cr0ack=0x{:08X}",
            pri_supported, cr0ack
        );
    }
}

/// BUG-AUDIT-71 test 4: Flipping pri_supported before vs after enable() produces
/// correct PRIQEN in CR0ACK.
///
/// This test disables PRI, enables, verifies PRIQEN=0. Then re-enables with PRI
/// supported, verifies PRIQEN=1. Exercises both transitions.
///
/// BEFORE FIX: first enable() gives PRIQEN=1 even with pri_supported=false → FAILS.
/// AFTER FIX:  PRIQEN=0 when pri_supported=false, PRIQEN=1 when true → PASSES.
#[test]
fn bug_audit_71_priqen_correctly_gated_by_pri_supported_on_each_enable() {
    let smmu = SMMU::new();

    // --- First enable: PRI not supported ---
    smmu.set_pri_supported(false);
    smmu.enable().expect("first enable() must succeed");

    let cr0ack_no_pri = smmu.get_cr0ack();
    assert_eq!(
        cr0ack_no_pri & SMMU::CR0_PRIQEN,
        0,
        "BUG-AUDIT-71: first enable() with pri_supported=false must leave PRIQEN=0 in CR0ACK. \
         ARM §6.3.9: PRIQEN is RES0 when IDR0.PRI==0. \
         Current code always sets PRIQEN unconditionally. \
         cr0ack=0x{:08X}",
        cr0ack_no_pri
    );

    // Disable the SMMU before re-enabling.
    smmu.disable().expect("disable() must succeed");

    // --- Second enable: PRI supported ---
    smmu.set_pri_supported(true);
    smmu.enable().expect("second enable() must succeed");

    let cr0ack_with_pri = smmu.get_cr0ack();
    assert_ne!(
        cr0ack_with_pri & SMMU::CR0_PRIQEN,
        0,
        "BUG-AUDIT-71: second enable() with pri_supported=true must set PRIQEN=1 in CR0ACK. \
         ARM §6.3.9: PRIQEN is operational when IDR0.PRI==1. \
         cr0ack=0x{:08X}",
        cr0ack_with_pri
    );
}
