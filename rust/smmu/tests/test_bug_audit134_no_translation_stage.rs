//! TDD test for BUG-AUDIT-134.
//!
//! ARM SMMU v3 spec §3.3 line 1429:
//! "An SMMU must support at least one stage of translation."
//!
//! Setting both IDR0.S1P=false AND IDR0.S2P=false simultaneously means the
//! SMMU claims to support no translation stages, which is illegal per the spec.
//!
//! BEFORE FIX: set_s1p_supported(false) + set_s2p_supported(false) silently
//!             results in an SMMU that advertises no translation capability.
//!             get_idr0() would return bits 0 and 1 both clear.
//!
//! AFTER FIX:  set_s2p_supported(false) is rejected (returns Err or panics) when
//!             S1P is already false, and vice versa. Or: one of the setters is
//!             enforced such that at least one remains true.
//!             Alternatively: get_idr0() always ensures at least one of S1P/S2P=1.
//!
//! Implementation note: the simplest compliant fix is to reject the second setter
//! call that would clear the last supported stage via a new method that returns
//! Result, or to restore S1P=true when both would be false.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{SMMUConfig};
use smmu::SMMU;

fn make_smmu() -> SMMU {
    let smmu = SMMU::with_config(SMMUConfig::default());
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    smmu
}

// ============================================================================
// BUG-AUDIT-134 — s1p=false AND s2p=false violates §3.3 "≥1 stage required"
// ============================================================================

/// §3.3 line 1429: "An SMMU must support at least one stage of translation."
///
/// When both S1P and S2P are disabled, IDR0 advertises no translation stages.
/// After BUG-AUDIT-134 fix: IDR0 must always have at least one of S1P/S2P set.
///
/// BEFORE FIX: IDR0 bits 0 (S2P) and 1 (S1P) are both 0 when both disabled.
/// AFTER FIX:  IDR0 must have S1P=1 OR S2P=1 (at least one) in all configurations.
#[test]
fn audit134_idr0_always_has_at_least_one_translation_stage() {
    let smmu = make_smmu();

    // Default: both S1P and S2P are true — IDR0 should have both bits set.
    let idr0 = smmu.get_idr0();
    let s1p = (idr0 >> 1) & 1;
    let s2p = idr0 & 1;
    assert!(s1p == 1 || s2p == 1, "IDR0 must have at least one of S1P/S2P set by default");

    // Disable S2P, keep S1P enabled — still valid.
    smmu.set_s2p_supported(false);
    let idr0 = smmu.get_idr0();
    let s1p = (idr0 >> 1) & 1;
    let s2p = idr0 & 1;
    assert_eq!(s2p, 0, "S2P should be 0 after set_s2p_supported(false)");
    assert_eq!(s1p, 1, "S1P must remain 1 (at least one stage required)");

    // Attempting to also disable S1P when S2P is already false must be rejected.
    // The implementation must either: refuse the operation, or restore S2P=1.
    smmu.set_s1p_supported(false);
    let idr0 = smmu.get_idr0();
    let s1p_after = (idr0 >> 1) & 1;
    let s2p_after = idr0 & 1;
    assert!(
        s1p_after == 1 || s2p_after == 1,
        "IDR0 must still have at least one of S1P/S2P=1 after attempting to disable both. \
         Got IDR0={:#010x} (S1P={}, S2P={})",
        idr0, s1p_after, s2p_after
    );
}

/// §3.3 line 1429: Symmetric test — disable S1P first, then attempt to disable S2P.
///
/// BEFORE FIX: Both bits clear → IDR0 has no translation stages.
/// AFTER FIX:  Second disable is blocked; IDR0 retains at least one stage.
#[test]
fn audit134_idr0_at_least_one_stage_disable_s1p_first() {
    let smmu = make_smmu();

    // Disable S1P — S2P still enabled.
    smmu.set_s1p_supported(false);
    let idr0 = smmu.get_idr0();
    let s2p = idr0 & 1;
    assert_eq!(s2p, 1, "S2P must remain 1 when S1P disabled (one stage required)");

    // Now attempt to disable S2P as well.
    smmu.set_s2p_supported(false);
    let idr0 = smmu.get_idr0();
    let s1p_after = (idr0 >> 1) & 1;
    let s2p_after = idr0 & 1;
    assert!(
        s1p_after == 1 || s2p_after == 1,
        "IDR0 must retain at least one of S1P/S2P=1 after attempt to disable both. \
         Got IDR0={:#010x} (S1P={}, S2P={})",
        idr0, s1p_after, s2p_after
    );
}
