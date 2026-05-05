//! TDD test for BUG-AUDIT-138.
//!
//! ARM SMMU v3 spec §3.4 / §3.4.1:
//! "Input range checks made for a stage 1 VMSAv8-64 translation table configured
//!  (with TxSZ) for an input range of N significant bits fail unless bits
//!  VA[AddrTop: N-1] are identical."
//!
//! AddrTop == 63 when TBI disabled; AddrTop == 55 when TBI enabled.
//!
//! BEFORE FIX: mod.rs:5102-5212 only checks `effective_iova_val >= va_limit`
//!             (a magnitude check). A non-canonical VA such as
//!             0x0000_8000_DEAD_BEEF (T0SZ=16 → N=48; VA[47]=1 → requires
//!             VA[63:48]=0xFFFF for sign-extension, but actual VA[63:48]=0x0000)
//!             passes because its magnitude 0x0000_8000_DEAD_BEEF < 2^48.
//!
//! AFTER FIX: The translation path also checks VA[AddrTop:N-1] are all identical.
//!            A non-canonical VA raises F_TRANSLATION.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{AccessType, EventType, IOVA, PASID, SMMUConfig, SecurityState, StreamConfig, StreamID};
use smmu::SMMU;

fn make_smmu() -> SMMU {
    let smmu = SMMU::with_config(SMMUConfig::default());
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    smmu
}

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

fn pasid0() -> PASID {
    PASID::new(0).unwrap()
}

// ============================================================================
// BUG-AUDIT-138 — non-canonical VA (TBI=0, T0SZ=16)
// ============================================================================

/// §3.4.1: T0SZ=16 → N=48 → AddrTop=63 (TBI=0).
/// VA[AddrTop:N-1] = VA[63:47] must all be identical (all 0 or all 1).
///
/// VA = 0x0000_8000_DEAD_BEEF:
///   - VA[47] = 1 (bit 47 is set)
///   - VA[63:48] = 0x0000 ≠ sign-extension of VA[47]=1 → non-canonical → F_TRANSLATION.
///   - Magnitude check alone: 0x0000_8000_DEAD_BEEF < 2^48 → passes old guard (bug).
///
/// BEFORE FIX: translate() returns Ok (magnitude check passes; canonical check absent).
/// AFTER FIX:  translate() returns Err and queues F_TRANSLATION.
#[test]
fn audit138_non_canonical_va_tbi_disabled_t0sz16_must_fault() {
    let smmu = make_smmu();

    // T0SZ=16, TBI=false, stage-1 only, IPS=5 (48-bit)
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .t0sz(16)
        .tbi(false)
        .ips(5)
        .build()
        .unwrap();
    smmu.configure_stream(sid(1), cfg).unwrap();

    // Non-canonical: VA[47]=1 but VA[63:48]=0x0000.
    // With T0SZ=16: N=48, AddrTop=63. VA[63:47] must all be equal.
    // VA[47]=1 requires VA[63:48]=0xFFFF; actual 0x0000 → non-canonical.
    let non_canonical_va = 0x0000_8000_DEAD_BEEFu64;
    let iova = IOVA::new(non_canonical_va).unwrap();

    let result = smmu.translate(sid(1), pasid0(), iova, AccessType::Read, SecurityState::NonSecure);

    assert!(
        result.is_err(),
        "Expected F_TRANSLATION for non-canonical VA 0x{:016x} (T0SZ=16, TBI=0, VA[47]=1 but VA[63:48]=0x0000)",
        non_canonical_va
    );

    let events = smmu.get_events();
    assert!(
        events.iter().any(|e| e.event_type == EventType::FTranslation),
        "Expected F_TRANSLATION event in queue, got: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

// ============================================================================
// BUG-AUDIT-138 — non-canonical VA (TBI=1, T0SZ=16)
// ============================================================================

/// §3.4.1: T0SZ=16, TBI=1 → N=48, AddrTop=55.
/// VA[AddrTop:N-1] = VA[55:47] must all be identical.
///
/// With TBI=1 the effective VA strips bits [63:56].
/// VA = 0x0080_0000_DEAD_BEEF:
///   effective_va = 0x0080_0000_DEAD_BEEF (bits [63:56] = 0x00 → already stripped)
///   VA[55] = 1 (0x0080... has bit 55 set).
///   VA[54:47] = 0 → not all identical to VA[55]=1 → non-canonical.
///
/// BEFORE FIX: passes (only magnitude checked after TBI masking).
/// AFTER FIX:  F_TRANSLATION raised.
#[test]
fn audit138_non_canonical_va_tbi_enabled_t0sz16_must_fault() {
    let smmu = make_smmu();

    // T0SZ=16, TBI=true, AddrTop=55
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .t0sz(16)
        .tbi(true)
        .ips(5)
        .build()
        .unwrap();
    smmu.configure_stream(sid(4), cfg).unwrap();

    // VA = 0x0080_0000_DEAD_BEEF:
    //   TBI mask → 0x0080_0000_DEAD_BEEF & 0x00FF_FFFF_FFFF_FFFF = 0x0080_0000_DEAD_BEEF
    //   Magnitude: 0x0080_0000_DEAD_BEEF < 2^48 = 0x0001_0000_0000_0000 → passes magnitude guard.
    //   Canonical check (AddrTop=55): VA[55]=1, VA[54:47]=0 → not identical → must fault.
    let non_canonical_va = 0x0080_0000_DEAD_BEEFu64;
    let iova = IOVA::new(non_canonical_va).unwrap();

    let result = smmu.translate(sid(4), pasid0(), iova, AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_err(),
        "Expected F_TRANSLATION for non-canonical VA 0x{:016x} (T0SZ=16, TBI=1, VA[55:47] not identical)",
        non_canonical_va
    );

    let events = smmu.get_events();
    assert!(
        events.iter().any(|e| e.event_type == EventType::FTranslation),
        "Expected F_TRANSLATION event for non-canonical TBI=1 VA"
    );
}
