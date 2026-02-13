#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::use_self)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::option_if_let_else)]

//! `QuickCheck`-Based Property Testing (Task 5.1)
//!
//! This module provides `QuickCheck` integration for property-based testing
//! as an alternative to `PropTest`. `QuickCheck` has different shrinking strategies
//! and can find different edge cases.
//!
//! Key Differences from `PropTest`:
//! - Different random generation strategies
//! - Alternative shrinking algorithms
//! - Simpler test syntax for some cases
//! - Better integration with some testing workflows

use quickcheck::{Arbitrary, Gen, TestResult};
use quickcheck_macros::quickcheck;
use smmu::address_space::AddressSpace;
use smmu::stream_context::StreamContext;
use smmu::types::{
    AccessType, PagePermissions, SecurityState, StreamConfig, StreamID, IOVA, PA, PAGE_SIZE, PASID,
};
use smmu::SMMU;

// ============================================================================
// Custom Arbitrary Implementations
// ============================================================================

/// Wrapper for page-aligned IOVA
#[derive(Debug, Clone, Copy)]
struct PageAlignedIOVA(IOVA);

impl Arbitrary for PageAlignedIOVA {
    fn arbitrary(g: &mut Gen) -> Self {
        let addr = u64::arbitrary(g) % (1u64 << 48);
        let aligned = addr & !(PAGE_SIZE - 1);
        PageAlignedIOVA(IOVA::new(aligned).unwrap_or_else(|_| IOVA::new(PAGE_SIZE).unwrap()))
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        let current = self.0.as_u64();
        let mut shrinks = Vec::new();
        let mut addr = current;
        while addr >= PAGE_SIZE {
            if let Ok(iova) = IOVA::new(addr) {
                shrinks.push(PageAlignedIOVA(iova));
            }
            if addr < PAGE_SIZE {
                break;
            }
            addr = addr.saturating_sub(PAGE_SIZE);
        }
        shrinks.reverse();
        Box::new(shrinks.into_iter())
    }
}

/// Wrapper for valid PASID
#[derive(Debug, Clone, Copy)]
struct ValidPASID(PASID);

impl Arbitrary for ValidPASID {
    fn arbitrary(g: &mut Gen) -> Self {
        let val = u32::arbitrary(g) % 1_048_576; // 2^20
        ValidPASID(PASID::new(val).unwrap())
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        let current = self.0.as_u32();
        let mut shrinks = Vec::new();
        for val in (0..current).rev() {
            if let Ok(pasid) = PASID::new(val) {
                shrinks.push(ValidPASID(pasid));
            }
        }
        Box::new(shrinks.into_iter())
    }
}

/// Wrapper for valid StreamID
#[derive(Debug, Clone, Copy)]
struct ValidStreamID(StreamID);

impl Arbitrary for ValidStreamID {
    fn arbitrary(g: &mut Gen) -> Self {
        let val = u32::arbitrary(g) % 65536; // 2^16
        ValidStreamID(StreamID::new(val).unwrap())
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        let current = self.0.as_u32();
        let mut shrinks = Vec::new();
        for val in (0..current).rev() {
            if let Ok(stream_id) = StreamID::new(val) {
                shrinks.push(ValidStreamID(stream_id));
            }
        }
        Box::new(shrinks.into_iter())
    }
}

/// Wrapper for valid physical address
#[derive(Debug, Clone, Copy)]
struct ValidPA(PA);

impl Arbitrary for ValidPA {
    fn arbitrary(g: &mut Gen) -> Self {
        let addr = u64::arbitrary(g) % (1u64 << 52);
        let aligned = addr & !(PAGE_SIZE - 1);
        ValidPA(PA::new(aligned).unwrap_or_else(|_| PA::new(PAGE_SIZE).unwrap()))
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        let current = self.0.as_u64();
        let mut shrinks = Vec::new();
        let mut addr = current;
        while addr >= PAGE_SIZE {
            if let Ok(pa) = PA::new(addr) {
                shrinks.push(ValidPA(pa));
            }
            if addr < PAGE_SIZE {
                break;
            }
            addr = addr.saturating_sub(PAGE_SIZE);
        }
        shrinks.reverse();
        Box::new(shrinks.into_iter())
    }
}

// ============================================================================
// AddressSpace Properties
// ============================================================================

#[quickcheck]
fn qc_map_unmap_restores_state(iova: PageAlignedIOVA, pa: ValidPA) -> bool {
    let addr_space = AddressSpace::new();

    // Map page
    addr_space
        .map_page(
            iova.0,
            pa.0,
            PagePermissions::read_only(),
            SecurityState::NonSecure,
        )
        .ok();

    let count_after_map = addr_space.get_page_count().unwrap_or(0);

    // Unmap page
    addr_space.unmap_page(iova.0).ok();

    let count_after_unmap = addr_space.get_page_count().unwrap_or(0);

    count_after_map == 1 && count_after_unmap == 0
}

#[quickcheck]
fn qc_translation_deterministic(iova: PageAlignedIOVA, pa: ValidPA) -> bool {
    let addr_space = AddressSpace::new();

    addr_space
        .map_page(
            iova.0,
            pa.0,
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .ok();

    // Translate multiple times
    let r1 = addr_space.translate_page(iova.0, AccessType::Read, SecurityState::NonSecure);
    let r2 = addr_space.translate_page(iova.0, AccessType::Read, SecurityState::NonSecure);
    let r3 = addr_space.translate_page(iova.0, AccessType::Read, SecurityState::NonSecure);

    // All should succeed or all should fail
    match (r1, r2, r3) {
        (Ok(t1), Ok(t2), Ok(t3)) => {
            let pa1 = t1.physical_address().as_u64() & !0xFFF;
            let pa2 = t2.physical_address().as_u64() & !0xFFF;
            let pa3 = t3.physical_address().as_u64() & !0xFFF;
            pa1 == pa2 && pa2 == pa3
        }
        (Err(_), Err(_), Err(_)) => true,
        _ => false, // Inconsistent results
    }
}

#[quickcheck]
fn qc_page_offset_preserved(iova: PageAlignedIOVA, pa: ValidPA, offset: u64) -> TestResult {
    if offset >= PAGE_SIZE {
        return TestResult::discard();
    }

    let addr_space = AddressSpace::new();
    let iova_base = iova.0;

    let iova_with_offset = match IOVA::new(iova_base.as_u64() + offset) {
        Ok(v) => v,
        Err(_) => return TestResult::discard(),
    };

    addr_space
        .map_page(
            iova_base,
            pa.0,
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .ok();

    match addr_space.translate_page(iova_with_offset, AccessType::Read, SecurityState::NonSecure)
    {
        Ok(result) => {
            let translated_offset = result.physical_address().as_u64() & 0xFFF;
            TestResult::from_bool(translated_offset == offset)
        }
        Err(_) => TestResult::failed(),
    }
}

#[quickcheck]
fn qc_overwrite_updates_mapping(
    iova: PageAlignedIOVA,
    pa1: ValidPA,
    pa2: ValidPA,
) -> TestResult {
    if pa1.0.as_u64() == pa2.0.as_u64() {
        return TestResult::discard();
    }

    let addr_space = AddressSpace::new();

    // Map first PA
    addr_space
        .map_page(
            iova.0,
            pa1.0,
            PagePermissions::read_only(),
            SecurityState::NonSecure,
        )
        .ok();

    // Overwrite with second PA
    addr_space
        .map_page(
            iova.0,
            pa2.0,
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .ok();

    // Should still have only one page
    let count = addr_space.get_page_count().unwrap_or(0);
    if count != 1 {
        return TestResult::failed();
    }

    // Translation should return second PA
    match addr_space.translate_page(iova.0, AccessType::Read, SecurityState::NonSecure) {
        Ok(result) => {
            let pa = result.physical_address().as_u64() & !0xFFF;
            TestResult::from_bool(pa == pa2.0.as_u64())
        }
        Err(_) => TestResult::failed(),
    }
}

#[quickcheck]
fn qc_read_only_denies_write(iova: PageAlignedIOVA, pa: ValidPA) -> bool {
    let addr_space = AddressSpace::new();

    addr_space
        .map_page(
            iova.0,
            pa.0,
            PagePermissions::read_only(),
            SecurityState::NonSecure,
        )
        .ok();

    let read_ok = addr_space
        .translate_page(iova.0, AccessType::Read, SecurityState::NonSecure)
        .is_ok();
    let write_ok = addr_space
        .translate_page(iova.0, AccessType::Write, SecurityState::NonSecure)
        .is_ok();

    read_ok && !write_ok
}

#[quickcheck]
fn qc_read_write_allows_both(iova: PageAlignedIOVA, pa: ValidPA) -> bool {
    let addr_space = AddressSpace::new();

    addr_space
        .map_page(
            iova.0,
            pa.0,
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .ok();

    let read_ok = addr_space
        .translate_page(iova.0, AccessType::Read, SecurityState::NonSecure)
        .is_ok();
    let write_ok = addr_space
        .translate_page(iova.0, AccessType::Write, SecurityState::NonSecure)
        .is_ok();

    read_ok && write_ok
}

// ============================================================================
// StreamContext Properties
// ============================================================================

#[quickcheck]
fn qc_create_remove_pasid_restores_state(pasid: ValidPASID) -> bool {
    let stream_context = StreamContext::new();

    stream_context.create_pasid(pasid.0).ok();
    let has_after_create = stream_context.has_pasid(pasid.0);

    stream_context.remove_pasid(pasid.0).ok();
    let has_after_remove = stream_context.has_pasid(pasid.0);

    has_after_create && !has_after_remove
}

#[quickcheck]
fn qc_multiple_pasids_independent(count: u8) -> TestResult {
    if count == 0 || count > 50 {
        return TestResult::discard();
    }

    let stream_context = StreamContext::new();

    // Create multiple PASIDs
    for i in 0..count {
        if let Ok(pasid) = PASID::new(u32::from(i)) {
            stream_context.create_pasid(pasid).ok();
        }
    }

    let final_count = stream_context.pasid_count();

    TestResult::from_bool(final_count == count as usize)
}

#[quickcheck]
fn qc_pasid_translation_isolation(
    pasid1: ValidPASID,
    pasid2: ValidPASID,
    iova: PageAlignedIOVA,
    pa1: ValidPA,
    pa2: ValidPA,
) -> TestResult {
    if pasid1.0.as_u32() == pasid2.0.as_u32() {
        return TestResult::discard();
    }
    if pa1.0.as_u64() == pa2.0.as_u64() {
        return TestResult::discard();
    }

    let stream_context = StreamContext::new();

    stream_context.create_pasid(pasid1.0).ok();
    stream_context.create_pasid(pasid2.0).ok();

    // Map same IOVA to different PAs
    stream_context
        .map_page(
            pasid1.0,
            iova.0,
            pa1.0,
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .ok();
    stream_context
        .map_page(
            pasid2.0,
            iova.0,
            pa2.0,
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .ok();

    // Translations should be different
    let r1 = stream_context.translate(pasid1.0, iova.0, AccessType::Read, SecurityState::NonSecure);
    let r2 = stream_context.translate(pasid2.0, iova.0, AccessType::Read, SecurityState::NonSecure);

    match (r1, r2) {
        (Ok(t1), Ok(t2)) => {
            let translated_pa1 = t1.physical_address().as_u64() & !0xFFF;
            let translated_pa2 = t2.physical_address().as_u64() & !0xFFF;
            TestResult::from_bool(
                translated_pa1 == pa1.0.as_u64() && translated_pa2 == pa2.0.as_u64(),
            )
        }
        _ => TestResult::failed(),
    }
}

#[quickcheck]
fn qc_clear_all_pasids_empties(count: u8) -> TestResult {
    if count == 0 || count > 50 {
        return TestResult::discard();
    }

    let stream_context = StreamContext::new();

    // Create multiple PASIDs
    for i in 0..count {
        if let Ok(pasid) = PASID::new(u32::from(i)) {
            stream_context.create_pasid(pasid).ok();
        }
    }

    stream_context.clear_all_pasids().ok();
    let final_count = stream_context.pasid_count();

    TestResult::from_bool(final_count == 0)
}

// ============================================================================
// Type Validation Properties
// ============================================================================

#[quickcheck]
fn qc_pasid_validation(val: u32) -> bool {
    let result = PASID::new(val);

    // Valid range: 0..=1_048_575
    if val <= 1_048_575 {
        result.is_ok()
    } else {
        result.is_err()
    }
}

#[quickcheck]
fn qc_stream_id_validation(val: u32) -> bool {
    let result = StreamID::new(val);

    // Valid range: 0..=65_535
    if val <= 65_535 {
        result.is_ok()
    } else {
        result.is_err()
    }
}

#[quickcheck]
fn qc_iova_validation(addr: u64) -> bool {
    let result = IOVA::new(addr);

    // IOVA::new() currently always succeeds for API consistency
    // It doesn't validate address ranges
    result.is_ok()
}

#[quickcheck]
fn qc_pa_validation(addr: u64) -> bool {
    let result = PA::new(addr);

    // PA::new() currently always succeeds for API consistency
    // It doesn't validate address ranges
    result.is_ok()
}

// ============================================================================
// SMMU Integration Properties
// ============================================================================

#[quickcheck]
fn qc_smmu_stream_configuration(stream: ValidStreamID) -> bool {
    let smmu = SMMU::new();

    let config = StreamConfig::stage1_only();
    let result = smmu.configure_stream(stream.0, config);
    result.is_ok()
}

#[quickcheck]
fn qc_smmu_end_to_end_translation(
    stream: ValidStreamID,
    pasid: ValidPASID,
    iova: PageAlignedIOVA,
    pa: ValidPA,
) -> TestResult {
    let smmu = SMMU::new();

    let config = StreamConfig::stage1_only();
    if smmu.configure_stream(stream.0, config).is_err() {
        return TestResult::discard();
    }

    if smmu.create_pasid(stream.0, pasid.0).is_err() {
        return TestResult::discard();
    }

    if smmu
        .map_page(
            stream.0,
            pasid.0,
            iova.0,
            pa.0,
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .is_err()
    {
        return TestResult::discard();
    }

    // Translation should succeed and be consistent
    let r1 = smmu.translate(
        stream.0,
        pasid.0,
        iova.0,
        AccessType::Read,
    );
    let r2 = smmu.translate(
        stream.0,
        pasid.0,
        iova.0,
        AccessType::Read,
    );

    match (r1, r2) {
        (Ok(t1), Ok(t2)) => {
            let pa1 = t1.physical_address().as_u64() & !0xFFF;
            let pa2 = t2.physical_address().as_u64() & !0xFFF;
            TestResult::from_bool(pa1 == pa2 && pa1 == pa.0.as_u64())
        }
        _ => TestResult::failed(),
    }
}

#[quickcheck]
fn qc_smmu_stream_isolation(
    stream1: ValidStreamID,
    stream2: ValidStreamID,
    pasid: ValidPASID,
    iova: PageAlignedIOVA,
    pa1: ValidPA,
    pa2: ValidPA,
) -> TestResult {
    if stream1.0.as_u32() == stream2.0.as_u32() {
        return TestResult::discard();
    }
    if pa1.0.as_u64() == pa2.0.as_u64() {
        return TestResult::discard();
    }

    let smmu = SMMU::new();

    // Configure both streams
    let config = StreamConfig::stage1_only();
    if smmu.configure_stream(stream1.0, config.clone()).is_err()
        || smmu.configure_stream(stream2.0, config).is_err() {
        return TestResult::discard();
    }

    if smmu.create_pasid(stream1.0, pasid.0).is_err()
        || smmu.create_pasid(stream2.0, pasid.0).is_err()
    {
        return TestResult::discard();
    }

    // Map same IOVA to different PAs
    smmu.map_page(
        stream1.0,
        pasid.0,
        iova.0,
        pa1.0,
        PagePermissions::all(),
        SecurityState::NonSecure,
    )
    .ok();
    smmu.map_page(
        stream2.0,
        pasid.0,
        iova.0,
        pa2.0,
        PagePermissions::all(),
        SecurityState::NonSecure,
    )
    .ok();

    // Translations should be isolated
    let r1 = smmu.translate(
        stream1.0,
        pasid.0,
        iova.0,
        AccessType::Read,
    );
    let r2 = smmu.translate(
        stream2.0,
        pasid.0,
        iova.0,
        AccessType::Read,
    );

    match (r1, r2) {
        (Ok(t1), Ok(t2)) => {
            let translated_pa1 = t1.physical_address().as_u64() & !0xFFF;
            let translated_pa2 = t2.physical_address().as_u64() & !0xFFF;
            TestResult::from_bool(translated_pa1 == pa1.0.as_u64() && translated_pa2 == pa2.0.as_u64())
        }
        _ => TestResult::failed(),
    }
}

// ============================================================================
// Invariant Properties
// ============================================================================

#[quickcheck]
fn qc_page_count_consistent(count: u8) -> TestResult {
    if count == 0 || count > 100 {
        return TestResult::discard();
    }

    let addr_space = AddressSpace::new();

    // Map multiple pages
    for i in 0..count {
        let iova = match IOVA::new(0x1000 + (u64::from(i)) * PAGE_SIZE) {
            Ok(v) => v,
            Err(_) => return TestResult::discard(),
        };
        let pa = PA::new(0x2000).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
            .ok();
    }

    let page_count = addr_space.get_page_count().unwrap_or(0);
    TestResult::from_bool(page_count == count as usize)
}

#[quickcheck]
fn qc_is_mapped_consistent_with_translation(iova: PageAlignedIOVA, pa: ValidPA) -> bool {
    let addr_space = AddressSpace::new();

    // Before mapping
    let not_mapped_before = !addr_space.is_page_mapped(iova.0).unwrap_or(true);
    let translation_fails_before = addr_space
        .translate_page(iova.0, AccessType::Read, SecurityState::NonSecure)
        .is_err();

    // After mapping
    addr_space
        .map_page(
            iova.0,
            pa.0,
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .ok();

    let is_mapped_after = addr_space.is_page_mapped(iova.0).unwrap_or(false);
    let translation_succeeds_after = addr_space
        .translate_page(iova.0, AccessType::Read, SecurityState::NonSecure)
        .is_ok();

    not_mapped_before && translation_fails_before && is_mapped_after && translation_succeeds_after
}

// ============================================================================
// Security Properties
// ============================================================================

#[quickcheck]
fn qc_security_state_enforcement(iova: PageAlignedIOVA, pa: ValidPA) -> bool {
    let addr_space = AddressSpace::new();

    // Map in NonSecure state
    addr_space
        .map_page(
            iova.0,
            pa.0,
            PagePermissions::all(),
            SecurityState::NonSecure,
        )
        .ok();

    // Translation in same security state should succeed
    let same_ok = addr_space
        .translate_page(iova.0, AccessType::Read, SecurityState::NonSecure)
        .is_ok();

    // Translation in different security state should fail
    let diff_fails = addr_space
        .translate_page(iova.0, AccessType::Read, SecurityState::Secure)
        .is_err();

    same_ok && diff_fails
}
