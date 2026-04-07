//! TDD test for BUG-AUDIT-114.
//!
//! ARM SMMU v3 spec §3.4 line 1635:
//! "If bypassing stage 2 (because STE.Config == 0b10x or if unimplemented), the IPA is
//!  presented directly as the PA output address. If the IPA is outside the range of the OAS,
//!  the address is silently truncated to fit the OAS. If the IPA is smaller than the OAS, it
//!  is zero-extended."
//!
//! For stage-1-only (STE.Config=0b101, stage 2 bypassed), when the stage-1 output PA exceeds
//! the OAS the address MUST be silently truncated, NOT faulted with F_ADDR_SIZE.
//!
//! BEFORE FIX: translate() returns Err(AddressSizeError) and queues F_ADDR_SIZE.
//! AFTER FIX:  translate() returns Ok with the truncated (masked) PA, no fault queued.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{
    AccessType, AddressConfig, EventType, PagePermissions, SMMUConfig, SecurityState,
    StreamConfig, StreamID, IOVA, PA, PASID,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

fn pasid(n: u32) -> PASID {
    PASID::new(n).unwrap()
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

// ============================================================================
// BUG-AUDIT-114 — Stage-1-only OAS overflow must silently truncate, not fault
// ============================================================================

/// ARM §3.4 line 1635: when stage 2 is bypassed (STE.Config=0b10x), an IPA
/// (the stage-1 output PA) that exceeds OAS must be **silently truncated**.
///
/// Configure SMMU with 32-bit OAS.  Map a page to PA = 0x1_0000_0000, which
/// exceeds the 32-bit limit (0xFFFF_FFFF).  Translate should return Ok with
/// the lower 32 bits of that PA (= 0x0000_0000), and no F_ADDR_SIZE event
/// should be queued.
///
/// This test FAILS before the BUG-AUDIT-114 fix because the implementation
/// raises F_ADDR_SIZE instead of silently truncating.
#[test]
fn audit_114_stage1_only_pa_exceeds_oas_must_silently_truncate() {
    // Configure SMMU with a 32-bit OAS.
    let cfg = SMMUConfig {
        address_config: AddressConfig {
            max_iova_bits: 48,
            max_pa_bits: 32,
            max_stream_count: 65_536,
            max_pasid_count: 1_048_576,
        },
        ..SMMUConfig::default()
    };
    let smmu = SMMU::with_config(cfg);
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    let stream = sid(1);
    let p = pasid(0);
    let v = iova(0x0000_1000);
    // PA = 0x1_0000_0000 — exceeds 32-bit OAS (limit = 0xFFFF_FFFF).
    let over_oas_pa = pa(0x1_0000_0000);

    let cfg_stream = StreamConfig::stage1_only();
    smmu.configure_stream(stream, cfg_stream).unwrap();
    smmu.create_pasid(stream, p).unwrap();
    smmu.map_page(
        stream,
        p,
        v,
        over_oas_pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(stream, p, v, AccessType::Read, SecurityState::NonSecure);

    // §3.4 line 1635: must return Ok with the PA silently truncated to OAS width.
    // For 32-bit OAS: mask = (1u64 << 32) - 1 = 0xFFFF_FFFF.
    // 0x1_0000_0000 & 0xFFFF_FFFF = 0x0000_0000.
    let oas_mask: u64 = (1u64 << 32) - 1;
    let expected_truncated_pa = over_oas_pa.as_u64() & oas_mask;

    assert!(
        result.is_ok(),
        "BUG-AUDIT-114: §3.4 line 1635 requires silent truncation of PA exceeding OAS, \
         not F_ADDR_SIZE. Got: {:?}",
        result.err()
    );

    let data = result.unwrap();
    assert_eq!(
        data.physical_address().as_u64(),
        expected_truncated_pa,
        "BUG-AUDIT-114: translated PA must be silently truncated to OAS width. \
         Expected 0x{:X}, got 0x{:X}",
        expected_truncated_pa,
        data.physical_address().as_u64()
    );

    // No F_ADDR_SIZE event must be queued — the truncation is silent.
    let events = smmu.get_events();
    let has_addr_size = events.iter().any(|e| e.event_type == EventType::FAddrSize);
    assert!(
        !has_addr_size,
        "BUG-AUDIT-114: §3.4 line 1635 requires SILENT truncation — no F_ADDR_SIZE \
         event must be queued. Events queued: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}
