#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]

//! Unit tests for `StreamContext` module
//!
//! Tests per-stream state management including PASID creation/removal,
//! translation, and state machine transitions per ARM SMMU v3 specification.

use smmu::stream_context::StreamContext;
use smmu::types::{
    AccessType, PagePermissions, SecurityState, StreamContextError, TranslationError, IOVA, PA, PAGE_SIZE, PASID,
};

#[allow(unused_imports)]
use std::sync::{Arc, RwLock};

// ============================================================================
// Construction Tests
// ============================================================================

#[test]
fn test_new_stream_context() {
    let stream_context = StreamContext::new();
    assert!(stream_context.is_stage1_enabled());
    assert!(!stream_context.is_stage2_enabled());
}

#[test]
fn test_default_stream_context() {
    let stream_context = StreamContext::default();
    assert!(stream_context.is_stage1_enabled());
}

// ============================================================================
// PASID Creation Tests
// ============================================================================

#[test]
fn test_create_pasid() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();

    let result = stream_context.create_pasid(pasid);
    assert!(result.is_ok());
    assert!(stream_context.has_pasid(pasid));
}

#[test]
fn test_create_pasid_zero() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(0).unwrap();

    let result = stream_context.create_pasid(pasid);
    assert!(result.is_ok());
    assert!(stream_context.has_pasid(pasid));
}

#[test]
fn test_create_multiple_pasids() {
    let stream_context = StreamContext::new();

    for i in 0..10 {
        let pasid = PASID::new(i).unwrap();
        stream_context.create_pasid(pasid).unwrap();
    }

    assert_eq!(stream_context.pasid_count(), 10);
}

#[test]
fn test_create_duplicate_pasid() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();

    stream_context.create_pasid(pasid).unwrap();
    let result = stream_context.create_pasid(pasid);

    assert!(matches!(result, Err(StreamContextError::PASIDAlreadyExists(1))));
}

// ============================================================================
// PASID Removal Tests
// ============================================================================

#[test]
fn test_remove_pasid() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();

    stream_context.create_pasid(pasid).unwrap();
    assert!(stream_context.has_pasid(pasid));

    stream_context.remove_pasid(pasid).unwrap();
    assert!(!stream_context.has_pasid(pasid));
}

#[test]
fn test_remove_nonexistent_pasid() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();

    let result = stream_context.remove_pasid(pasid);
    assert!(matches!(result, Err(StreamContextError::PASIDNotFound(1))));
}

#[test]
fn test_remove_pasid_zero() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(0).unwrap();

    stream_context.create_pasid(pasid).unwrap();
    stream_context.remove_pasid(pasid).unwrap();
    assert!(!stream_context.has_pasid(pasid));
}

// ============================================================================
// Translation Tests
// ============================================================================

#[test]
fn test_translate_with_pasid() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    stream_context.create_pasid(pasid).unwrap();
    stream_context
        .map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().physical_address().as_u64(), 0x2000);
}

#[test]
fn test_translate_with_pasid_zero() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    stream_context.create_pasid(pasid).unwrap();
    stream_context
        .map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok());
}

#[test]
fn test_translate_nonexistent_pasid() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    let result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(matches!(result, Err(TranslationError::PASIDNotFound)));
}

#[test]
fn test_translate_unmapped_page() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    stream_context.create_pasid(pasid).unwrap();

    let result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(matches!(result, Err(TranslationError::PageNotMapped)));
}

// ============================================================================
// Multiple PASID Tests
// ============================================================================

#[test]
fn test_multiple_pasids_independent() {
    let stream_context = StreamContext::new();
    let pasid1 = PASID::new(1).unwrap();
    let pasid2 = PASID::new(2).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa1 = PA::new(0x2000).unwrap();
    let pa2 = PA::new(0x3000).unwrap();

    stream_context.create_pasid(pasid1).unwrap();
    stream_context.create_pasid(pasid2).unwrap();

    stream_context
        .map_page(pasid1, iova, pa1, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();
    stream_context
        .map_page(pasid2, iova, pa2, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let result1 = stream_context
        .translate(pasid1, iova, AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    let result2 = stream_context
        .translate(pasid2, iova, AccessType::Read, SecurityState::NonSecure)
        .unwrap();

    assert_eq!(result1.physical_address().as_u64(), 0x2000);
    assert_eq!(result2.physical_address().as_u64(), 0x3000);
}

// ============================================================================
// State Machine Tests
// ============================================================================

#[test]
fn test_enable_disable_stream() {
    let mut stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();

    stream_context.create_pasid(pasid).unwrap();
    assert!(stream_context.is_enabled());

    stream_context.disable();
    assert!(!stream_context.is_enabled());

    // Operations should fail when disabled
    let result = stream_context.create_pasid(PASID::new(2).unwrap());
    assert!(result.is_err());

    stream_context.enable();
    assert!(stream_context.is_enabled());
}

#[test]
fn test_stage1_enable_disable() {
    let mut stream_context = StreamContext::new();

    assert!(stream_context.is_stage1_enabled());

    stream_context.set_stage1_enabled(false);
    assert!(!stream_context.is_stage1_enabled());

    stream_context.set_stage1_enabled(true);
    assert!(stream_context.is_stage1_enabled());
}

#[test]
fn test_stage2_enable_disable() {
    let mut stream_context = StreamContext::new();

    assert!(!stream_context.is_stage2_enabled());

    stream_context.set_stage2_enabled(true);
    assert!(stream_context.is_stage2_enabled());

    stream_context.set_stage2_enabled(false);
    assert!(!stream_context.is_stage2_enabled());
}

// ============================================================================
// Shared AddressSpace Tests
// ============================================================================

#[test]
fn test_shared_address_space() {
    let stream_context = StreamContext::new();
    let pasid1 = PASID::new(1).unwrap();
    let pasid2 = PASID::new(2).unwrap();

    stream_context.create_pasid(pasid1).unwrap();
    let addr_space = stream_context.get_pasid_address_space(pasid1).unwrap();

    stream_context.add_pasid(pasid2, addr_space).unwrap();

    assert!(stream_context.has_pasid(pasid1));
    assert!(stream_context.has_pasid(pasid2));
}

// ============================================================================
// Bulk Operations Tests
// ============================================================================

#[test]
fn test_bulk_pasid_creation() {
    let stream_context = StreamContext::new();

    // Create 100 PASIDs
    for i in 0..100 {
        let pasid = PASID::new(i).unwrap();
        stream_context.create_pasid(pasid).unwrap();
    }

    assert_eq!(stream_context.pasid_count(), 100);
}

#[test]
fn test_bulk_pasid_removal() {
    let stream_context = StreamContext::new();

    // Create and remove 50 PASIDs
    for i in 0..50 {
        let pasid = PASID::new(i).unwrap();
        stream_context.create_pasid(pasid).unwrap();
    }

    for i in 0..50 {
        let pasid = PASID::new(i).unwrap();
        stream_context.remove_pasid(pasid).unwrap();
    }

    assert_eq!(stream_context.pasid_count(), 0);
}

#[test]
fn test_bulk_translation() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();

    stream_context.create_pasid(pasid).unwrap();

    // Map 100 pages
    for i in 0..100 {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let pa = PA::new(0x2000 + i * PAGE_SIZE).unwrap();
        stream_context
            .map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();
    }

    // Translate all pages
    for i in 0..100 {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
        assert!(result.is_ok());
    }
}

// ============================================================================
// Page Mapping Tests
// ============================================================================

#[test]
fn test_map_page() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    stream_context.create_pasid(pasid).unwrap();
    let result = stream_context.map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure);

    assert!(result.is_ok());
}

#[test]
fn test_unmap_page() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    stream_context.create_pasid(pasid).unwrap();
    stream_context
        .map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let result = stream_context.unmap_page(pasid, iova);
    assert!(result.is_ok());
}

// ============================================================================
// Thread Safety Tests
// ============================================================================

#[test]
fn test_stream_context_send() {
    fn assert_send<T: Send>() {}
    assert_send::<StreamContext>();
}

#[test]
fn test_stream_context_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<StreamContext>();
}

// ============================================================================
// Configuration Tests
// ============================================================================

#[test]
fn test_pasid_count() {
    let stream_context = StreamContext::new();

    assert_eq!(stream_context.pasid_count(), 0);

    for i in 0..5 {
        let pasid = PASID::new(i).unwrap();
        stream_context.create_pasid(pasid).unwrap();
    }

    assert_eq!(stream_context.pasid_count(), 5);
}

#[test]
fn test_has_pasid() {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(1).unwrap();

    assert!(!stream_context.has_pasid(pasid));

    stream_context.create_pasid(pasid).unwrap();
    assert!(stream_context.has_pasid(pasid));
}

#[test]
fn test_clear_all_pasids() {
    let stream_context = StreamContext::new();

    for i in 0..10 {
        let pasid = PASID::new(i).unwrap();
        stream_context.create_pasid(pasid).unwrap();
    }

    assert_eq!(stream_context.pasid_count(), 10);

    stream_context.clear_all_pasids().unwrap();
    assert_eq!(stream_context.pasid_count(), 0);
}
