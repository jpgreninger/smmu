//! Tests for BUG-RUST-DBGR-8 — OVFLG toggle without CAS (§7.4)
#![allow(clippy::doc_markdown)]
//!
//! OVFLG (bit 31 of eventq_prod) should only be toggled once when transitioning
//! from non-overflow to overflow. Subsequent discarded events must not toggle it again.

use smmu::SMMU;
use smmu::types::{AccessType, SecurityState, StreamConfig, StreamID, IOVA, PASID, SMMUConfig};

/// Helper: create an SMMU with a very small event queue (4 entries) so overflow is easy.
fn smmu_with_small_queue() -> SMMU {
    let mut config = SMMUConfig::default();
    config.queue_config.event_queue_size = 4;
    let smmu = SMMU::with_config(config);
    smmu.enable().unwrap();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);
    smmu
}

/// OVFLG is toggled exactly once when queue overflows, and not again on subsequent overflow.
#[test]
fn dbgr8_ovflg_toggled_once_on_overflow() {
    let smmu = smmu_with_small_queue();

    let stream_id = StreamID::new(1).unwrap();
    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(stream_id, cfg).unwrap();
    let pasid = PASID::new(0).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();
    // No pages mapped → every translate() → PageNotMapped fault → event

    // Verify OVFLG starts clear
    let initial_prod = smmu.get_eventq_prod();
    assert_eq!(initial_prod >> 31, 0, "OVFLG must start at 0");

    // Fill the queue (queue_size=4) and then generate extra events to trigger overflow.
    for i in 0u64..8 {
        let iova = IOVA::new(0x1000 + i * 0x1000).unwrap();
        let _ = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    }

    let prod_after_overflow = smmu.get_eventq_prod();
    let ovflg_after_first = (prod_after_overflow >> 31) & 1;
    assert_eq!(ovflg_after_first, 1, "OVFLG must be toggled once on first overflow");

    // Generate even more events — OVFLG must NOT toggle back to 0.
    for i in 8u64..16 {
        let iova = IOVA::new(0x1000 + i * 0x1000).unwrap();
        let _ = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    }

    let prod_after_more = smmu.get_eventq_prod();
    let ovflg_after_second = (prod_after_more >> 31) & 1;
    assert_eq!(
        ovflg_after_second, 1,
        "OVFLG must NOT be toggled again when overflow already active (got {ovflg_after_second})"
    );
}

/// OVFLG idempotency: the bit stays stable regardless of how many events are discarded.
#[test]
fn dbgr8_ovflg_stable_during_sustained_overflow() {
    let smmu = smmu_with_small_queue();

    let stream_id = StreamID::new(2).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream_id, PASID::new(0).unwrap()).unwrap();

    // Overflow
    for i in 0u64..20 {
        let iova = IOVA::new(0x2000 + i * 0x1000).unwrap();
        let _ = smmu.translate(stream_id, PASID::new(0).unwrap(), iova, AccessType::Read, SecurityState::NonSecure);
    }

    // Record OVFLG value after first overflow.
    let ovflg_set = (smmu.get_eventq_prod() >> 31) & 1;

    // Generate 20 more discarded events.
    for i in 20u64..40 {
        let iova = IOVA::new(0x2000 + i * 0x1000).unwrap();
        let _ = smmu.translate(stream_id, PASID::new(0).unwrap(), iova, AccessType::Read, SecurityState::NonSecure);
    }

    let ovflg_final = (smmu.get_eventq_prod() >> 31) & 1;
    assert_eq!(
        ovflg_set, ovflg_final,
        "OVFLG must remain stable during sustained overflow; was {ovflg_set}, now {ovflg_final}"
    );
}
