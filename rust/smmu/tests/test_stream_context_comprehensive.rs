#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]

//! Comprehensive tests for `StreamContext` module coverage
//!
//! This test suite targets uncovered code paths in `stream_context/mod.rs`
//! to achieve 100% coverage per `PLAN_100_PERCENT_COVERAGE.md` section 2.3

use smmu::address_space::AddressSpace;
use smmu::stream_context::{StreamConfigBuilder, StreamContext};
use smmu::types::{AccessType, FaultType, PagePermissions, SecurityState, StreamContextError, IOVA, PA, PASID};
use std::sync::Arc;

// ========================================================================
// PASID Management Tests
// ========================================================================

#[test]
fn test_pasid_limit_enforcement_at_limit() {
    let ctx = StreamContext::new();
    ctx.set_max_pasids_per_stream(2);

    // Fill to limit
    assert!(ctx.create_pasid(PASID::new(0).unwrap()).is_ok());
    assert!(ctx.create_pasid(PASID::new(1).unwrap()).is_ok());

    // Exceed limit
    let result = ctx.create_pasid(PASID::new(2).unwrap());
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), StreamContextError::PASIDLimitExceeded(2, 2)));
}

#[test]
fn test_pasid_limit_enforcement_add_pasid() {
    let ctx = StreamContext::new();
    ctx.set_max_pasids_per_stream(1);

    // Create first PASID
    ctx.create_pasid(PASID::new(0).unwrap()).unwrap();

    // Try to add second PASID with shared address space
    let addr_space = Arc::new(AddressSpace::new());
    let result = ctx.add_pasid(PASID::new(1).unwrap(), addr_space);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), StreamContextError::PASIDLimitExceeded(1, 1)));
}

#[test]
fn test_add_pasid_duplicate() {
    let ctx = StreamContext::new();
    ctx.create_pasid(PASID::new(1).unwrap()).unwrap();

    // Try to add duplicate
    let addr_space = Arc::new(AddressSpace::new());
    let result = ctx.add_pasid(PASID::new(1).unwrap(), addr_space);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), StreamContextError::PASIDAlreadyExists(1)));
}

#[test]
fn test_add_pasid_success() {
    let ctx = StreamContext::new();
    let addr_space = Arc::new(AddressSpace::new());

    assert!(ctx.add_pasid(PASID::new(1).unwrap(), addr_space).is_ok());
    assert!(ctx.has_pasid(PASID::new(1).unwrap()));
}

#[test]
fn test_remove_pasid_not_found() {
    let ctx = StreamContext::new();
    let result = ctx.remove_pasid(PASID::new(99).unwrap());
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), StreamContextError::PASIDNotFound(99)));
}

#[test]
fn test_clear_all_pasids() {
    let ctx = StreamContext::new();
    ctx.create_pasid(PASID::new(1).unwrap()).unwrap();
    ctx.create_pasid(PASID::new(2).unwrap()).unwrap();
    assert_eq!(ctx.pasid_count(), 2);

    assert!(ctx.clear_all_pasids().is_ok());
    assert_eq!(ctx.pasid_count(), 0);
}

#[test]
fn test_get_pasid_address_space_exists() {
    let ctx = StreamContext::new();
    ctx.create_pasid(PASID::new(1).unwrap()).unwrap();

    let addr_space = ctx.get_pasid_address_space(PASID::new(1).unwrap());
    assert!(addr_space.is_some());
}

#[test]
fn test_get_pasid_address_space_not_found() {
    let ctx = StreamContext::new();
    let addr_space = ctx.get_pasid_address_space(PASID::new(99).unwrap());
    assert!(addr_space.is_none());
}

// ========================================================================
// Two-Stage Translation Tests
// ========================================================================

#[test]
fn test_two_stage_translation_complete_path() {
    let ctx = StreamContext::new();
    ctx.set_stage1_enabled(true);
    ctx.set_stage2_enabled(true);

    // Create PASID and Stage-2 address space
    let pasid = PASID::new(1).unwrap();
    ctx.create_pasid(pasid).unwrap();
    ctx.create_stage2_address_space().unwrap();

    // Map Stage-1: IOVA 0x1000 → IPA 0x2000
    let iova = IOVA::new(0x1000).unwrap();
    let ipa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_write();
    ctx.map_page(pasid, iova, ipa, perms, SecurityState::NonSecure).unwrap();

    // Map Stage-2: IPA 0x2000 → PA 0x3000
    let ipa_iova = IOVA::new(0x2000).unwrap();
    let final_pa = PA::new(0x3000).unwrap();
    ctx.map_stage2_page(ipa_iova, final_pa, perms, SecurityState::NonSecure)
        .unwrap();

    // Translate through both stages
    let result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().physical_address(), final_pa);
}

#[test]
fn test_two_stage_translation_stage1_fault() {
    let ctx = StreamContext::new();
    ctx.set_stage1_enabled(true);
    ctx.set_stage2_enabled(true);

    let pasid = PASID::new(1).unwrap();
    ctx.create_pasid(pasid).unwrap();
    ctx.create_stage2_address_space().unwrap();

    // Don't map Stage-1, only Stage-2
    let final_pa = PA::new(0x3000).unwrap();
    let perms = PagePermissions::read_write();
    ctx.map_stage2_page(IOVA::new(0x2000).unwrap(), final_pa, perms, SecurityState::NonSecure)
        .unwrap();

    // Translation should fail at Stage-1
    let iova = IOVA::new(0x1000).unwrap();
    let result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err());

    // Should have recorded fault
    assert!(ctx.get_fault_count() > 0);
}

#[test]
fn test_two_stage_translation_stage2_fault() {
    let ctx = StreamContext::new();
    ctx.set_stage1_enabled(true);
    ctx.set_stage2_enabled(true);

    let pasid = PASID::new(1).unwrap();
    ctx.create_pasid(pasid).unwrap();
    ctx.create_stage2_address_space().unwrap();

    // Map Stage-1 but not Stage-2
    let iova = IOVA::new(0x1000).unwrap();
    let ipa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_write();
    ctx.map_page(pasid, iova, ipa, perms, SecurityState::NonSecure).unwrap();

    // Translation should fail at Stage-2
    let result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err());

    // Should have recorded fault
    assert!(ctx.get_fault_count() > 0);
}

#[test]
fn test_two_stage_pasid_not_found() {
    let ctx = StreamContext::new();
    ctx.set_stage1_enabled(true);
    ctx.set_stage2_enabled(true);

    let pasid = PASID::new(99).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err());
}

#[test]
fn test_two_stage_no_stage2_address_space() {
    let ctx = StreamContext::new();
    ctx.set_stage1_enabled(true);
    ctx.set_stage2_enabled(true);

    let pasid = PASID::new(1).unwrap();
    ctx.create_pasid(pasid).unwrap();

    // Map Stage-1
    let iova = IOVA::new(0x1000).unwrap();
    let ipa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_write();
    ctx.map_page(pasid, iova, ipa, perms, SecurityState::NonSecure).unwrap();

    // Don't create Stage-2 address space
    let result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err());
}

// ========================================================================
// Stage-2 Only Translation Tests
// ========================================================================

#[test]
fn test_stage2_only_translation_success() {
    let ctx = StreamContext::new();
    ctx.set_stage1_enabled(false);
    ctx.set_stage2_enabled(true);

    // §3.9: stage-2-only streams only accept PASID=0 (SubstreamID absent).
    let pasid = PASID::new(0).unwrap();
    ctx.create_stage2_address_space().unwrap();

    // Map Stage-2: IPA → PA
    let ipa = IOVA::new(0x2000).unwrap();
    let pa = PA::new(0x3000).unwrap();
    let perms = PagePermissions::read_write();
    ctx.map_stage2_page(ipa, pa, perms, SecurityState::NonSecure).unwrap();

    // Translate (IOVA treated as IPA)
    let result = ctx.translate(pasid, ipa, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().physical_address(), pa);
}

#[test]
fn test_stage2_only_translation_fault() {
    let ctx = StreamContext::new();
    ctx.set_stage1_enabled(false);
    ctx.set_stage2_enabled(true);

    // §3.9: stage-2-only streams only accept PASID=0 (SubstreamID absent).
    let pasid = PASID::new(0).unwrap();
    ctx.create_stage2_address_space().unwrap();

    // Don't map any pages
    let ipa = IOVA::new(0x2000).unwrap();
    let result = ctx.translate(pasid, ipa, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err());

    // Should have recorded fault
    assert!(ctx.get_fault_count() > 0);
}

#[test]
fn test_stage2_only_no_address_space() {
    let ctx = StreamContext::new();
    ctx.set_stage1_enabled(false);
    ctx.set_stage2_enabled(true);

    let pasid = PASID::new(1).unwrap();
    ctx.create_pasid(pasid).unwrap();

    // Don't create Stage-2 address space
    let ipa = IOVA::new(0x2000).unwrap();
    let result = ctx.translate(pasid, ipa, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err());
}

// ========================================================================
// Bypass Mode Translation Tests
// ========================================================================

#[test]
fn test_bypass_mode_translation() {
    let ctx = StreamContext::new();
    ctx.set_stage1_enabled(false);
    ctx.set_stage2_enabled(false);

    // §3.9: bypass streams only accept PASID=0 (SubstreamID absent).
    let pasid = PASID::new(0).unwrap();

    // No mapping needed in bypass mode
    let iova = IOVA::new(0x1000).unwrap();
    let result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok());

    // IOVA should equal PA
    assert_eq!(result.unwrap().physical_address().as_u64(), 0x1000);
}

#[test]
fn test_bypass_mode_full_permissions() {
    let ctx = StreamContext::new();
    ctx.set_stage1_enabled(false);
    ctx.set_stage2_enabled(false);

    // §3.9: bypass streams only accept PASID=0 (SubstreamID absent).
    let pasid = PASID::new(0).unwrap();

    let iova = IOVA::new(0x1000).unwrap();
    let result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok());

    let data = result.unwrap();
    assert!(data.permissions().allows(AccessType::Read));
    assert!(data.permissions().allows(AccessType::Write));
    assert!(data.permissions().allows(AccessType::Execute));
}

// ========================================================================
// Fault Recording and Statistics Tests
// ========================================================================

#[test]
fn test_fault_recording_on_translation_error() {
    let ctx = StreamContext::new();
    let pasid = PASID::new(1).unwrap();
    ctx.create_pasid(pasid).unwrap();

    // Trigger translation fault
    let iova = IOVA::new(0x1000).unwrap();
    let _ = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);

    assert_eq!(ctx.get_fault_count(), 1);
    let records = ctx.get_fault_records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].fault_type(), FaultType::TranslationFault);
}

#[test]
fn test_fault_rate_limiting() {
    let ctx = StreamContext::new();
    ctx.set_fault_rate_limit(2);

    let pasid = PASID::new(1).unwrap();
    ctx.create_pasid(pasid).unwrap();

    // Generate 5 faults, only 2 should be recorded
    for i in 0..5 {
        let iova = IOVA::new(0x1000 + i * 0x1000).unwrap();
        let _ = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    }

    assert_eq!(ctx.get_fault_count(), 2);
}

#[test]
fn test_fault_statistics_total_faults() {
    let ctx = StreamContext::new();
    let pasid = PASID::new(1).unwrap();
    ctx.create_pasid(pasid).unwrap();

    // Generate 3 faults
    for i in 0..3 {
        let iova = IOVA::new(0x1000 + i * 0x1000).unwrap();
        let _ = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    }

    let stats = ctx.get_fault_statistics();
    assert_eq!(stats.total_faults, 3);
}

#[test]
fn test_fault_statistics_by_type() {
    let ctx = StreamContext::new();
    let pasid = PASID::new(1).unwrap();
    ctx.create_pasid(pasid).unwrap();

    // Generate translation faults
    for i in 0..2 {
        let iova = IOVA::new(0x1000 + i * 0x1000).unwrap();
        let _ = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    }

    let stats = ctx.get_fault_statistics();
    assert_eq!(stats.page_not_mapped_count, 2);
    assert!(stats.faults_by_type.contains_key(&FaultType::TranslationFault));
}

#[test]
fn test_fault_statistics_by_pasid() {
    let ctx = StreamContext::new();
    let pasid1 = PASID::new(1).unwrap();
    let pasid2 = PASID::new(2).unwrap();
    ctx.create_pasid(pasid1).unwrap();
    ctx.create_pasid(pasid2).unwrap();

    // Generate faults for different PASIDs
    let iova = IOVA::new(0x1000).unwrap();
    let _ = ctx.translate(pasid1, iova, AccessType::Read, SecurityState::NonSecure);
    let _ = ctx.translate(pasid2, iova, AccessType::Read, SecurityState::NonSecure);
    let _ = ctx.translate(pasid2, iova, AccessType::Read, SecurityState::NonSecure);

    let stats = ctx.get_fault_statistics();
    assert_eq!(*stats.faults_by_pasid.get(&1).unwrap(), 1);
    assert_eq!(*stats.faults_by_pasid.get(&2).unwrap(), 2);
}

#[test]
fn test_fault_statistics_rate_limited_flag() {
    let ctx = StreamContext::new();
    ctx.set_fault_rate_limit(2);

    let pasid = PASID::new(1).unwrap();
    ctx.create_pasid(pasid).unwrap();

    // Generate faults to exceed limit
    for i in 0..3 {
        let iova = IOVA::new(0x1000 + i * 0x1000).unwrap();
        let _ = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    }

    let stats = ctx.get_fault_statistics();
    assert!(stats.rate_limited);
}

#[test]
fn test_clear_fault_records() {
    let ctx = StreamContext::new();
    let pasid = PASID::new(1).unwrap();
    ctx.create_pasid(pasid).unwrap();

    // Generate faults
    let iova = IOVA::new(0x1000).unwrap();
    let _ = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert_eq!(ctx.get_fault_count(), 1);

    // Clear
    ctx.clear_fault_records();
    assert_eq!(ctx.get_fault_count(), 0);
}

#[test]
fn test_reset_fault_statistics() {
    let ctx = StreamContext::new();
    let pasid = PASID::new(1).unwrap();
    ctx.create_pasid(pasid).unwrap();

    // Generate faults
    let iova = IOVA::new(0x1000).unwrap();
    let _ = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(ctx.get_fault_count() > 0);

    // Reset
    ctx.reset_fault_statistics();
    assert_eq!(ctx.get_fault_count(), 0);
}

#[test]
fn test_enable_fault_retry() {
    let ctx = StreamContext::new();
    ctx.enable_fault_retry(true);
    // Just verify it doesn't panic
    assert!(true);
}

#[test]
fn test_translate_with_retry() {
    let ctx = StreamContext::new();
    let pasid = PASID::new(1).unwrap();
    ctx.create_pasid(pasid).unwrap();

    let iova = IOVA::new(0x1000).unwrap();
    let result = ctx.translate_with_retry(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err());
}

// ========================================================================
// Stage-2 Address Space Tests
// ========================================================================

#[test]
fn test_create_stage2_address_space_success() {
    let ctx = StreamContext::new();
    assert!(ctx.create_stage2_address_space().is_ok());
}

#[test]
fn test_create_stage2_address_space_already_exists() {
    let ctx = StreamContext::new();
    ctx.create_stage2_address_space().unwrap();

    let result = ctx.create_stage2_address_space();
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), StreamContextError::InternalError(_)));
}

#[test]
fn test_map_stage2_page_success() {
    let ctx = StreamContext::new();
    ctx.create_stage2_address_space().unwrap();

    let ipa = IOVA::new(0x2000).unwrap();
    let pa = PA::new(0x3000).unwrap();
    let perms = PagePermissions::read_write();

    assert!(ctx.map_stage2_page(ipa, pa, perms, SecurityState::NonSecure).is_ok());
}

#[test]
fn test_map_stage2_page_no_address_space() {
    let ctx = StreamContext::new();

    let ipa = IOVA::new(0x2000).unwrap();
    let pa = PA::new(0x3000).unwrap();
    let perms = PagePermissions::read_write();

    let result = ctx.map_stage2_page(ipa, pa, perms, SecurityState::NonSecure);
    assert!(result.is_err());
}

#[test]
fn test_set_stage2_address_space() {
    let ctx = StreamContext::new();
    let stage2 = Arc::new(AddressSpace::new());
    ctx.set_stage2_address_space(Some(stage2));
    // Just verify it doesn't panic
    assert!(true);
}

// ========================================================================
// Configuration Update Tests
// ========================================================================

#[test]
fn test_update_config_builder_creation() {
    let ctx = StreamContext::new();
    let _builder = ctx.update_config_builder();
    // Just verify it doesn't panic
    assert!(true);
}

#[test]
fn test_apply_config_max_pasids() {
    let ctx = StreamContext::new();
    let config = StreamConfigBuilder::new().max_pasids_per_stream(512).build();

    assert!(ctx.apply_config(config).is_ok());
    assert_eq!(ctx.max_pasids_per_stream(), 512);
}

#[test]
fn test_apply_config_stage_flags() {
    let ctx = StreamContext::new();
    let stage2 = Arc::new(AddressSpace::new());
    let config = StreamConfigBuilder::new()
        .stage1_enabled(false)
        .stage2_enabled(true)
        .stage2_address_space(Some(stage2))
        .build();

    assert!(ctx.apply_config(config).is_ok());
    assert!(!ctx.is_stage1_enabled());
    assert!(ctx.is_stage2_enabled());
}

#[test]
fn test_apply_config_stage2_address_space() {
    let ctx = StreamContext::new();
    let stage2 = Arc::new(AddressSpace::new());
    let config = StreamConfigBuilder::new().stage2_address_space(Some(stage2)).build();

    assert!(ctx.apply_config(config).is_ok());
}

#[test]
fn test_validate_config_pasid_limit_exceeds_maximum() {
    let ctx = StreamContext::new();
    let config = StreamConfigBuilder::new()
        .max_pasids_per_stream(1 << 21) // Exceeds 2^20 - 1
        .build();

    let result = ctx.validate_config_update(&config);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), StreamContextError::ConfigurationError(_)));
}

#[test]
fn test_validate_config_reduce_limit_below_current_count() {
    let ctx = StreamContext::new();
    ctx.create_pasid(PASID::new(1).unwrap()).unwrap();
    ctx.create_pasid(PASID::new(2).unwrap()).unwrap();

    let config = StreamConfigBuilder::new()
        .max_pasids_per_stream(1) // Less than current count of 2
        .build();

    let result = ctx.validate_config_update(&config);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), StreamContextError::ConfigurationError(_)));
}

#[test]
fn test_validate_config_stage2_enabled_without_address_space() {
    let ctx = StreamContext::new();
    let config = StreamConfigBuilder::new().stage2_enabled(true).build();

    let result = ctx.validate_config_update(&config);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), StreamContextError::ConfigurationError(_)));
}

#[test]
fn test_validate_config_stage2_enabled_with_address_space() {
    let ctx = StreamContext::new();
    let stage2 = Arc::new(AddressSpace::new());
    let config = StreamConfigBuilder::new()
        .stage2_enabled(true)
        .stage2_address_space(Some(stage2))
        .build();

    let result = ctx.validate_config_update(&config);
    assert!(result.is_ok());
}

// ========================================================================
// State Machine Tests
// ========================================================================

#[test]
fn test_enable_stream() {
    let ctx = StreamContext::new();
    ctx.disable();
    assert!(!ctx.is_enabled());

    ctx.enable();
    assert!(ctx.is_enabled());
}

#[test]
fn test_disable_stream_clears_pasids() {
    let ctx = StreamContext::new();
    ctx.create_pasid(PASID::new(1).unwrap()).unwrap();
    ctx.create_pasid(PASID::new(2).unwrap()).unwrap();
    assert_eq!(ctx.pasid_count(), 2);

    ctx.disable();
    assert!(!ctx.is_enabled());
    assert_eq!(ctx.pasid_count(), 0);
}

#[test]
fn test_create_pasid_when_disabled() {
    // Bug 6 fix: ARM §3.21 commissioning sequence — CD/PASID setup is independent of
    // stream enable state.  create_pasid() must succeed on a disabled stream so that
    // drivers can pre-configure page tables before enabling the stream.  The enabled
    // check belongs only in translate() (transaction-path operations).
    let ctx = StreamContext::new();
    ctx.disable();

    let result = ctx.create_pasid(PASID::new(1).unwrap());
    assert!(result.is_ok(), "create_pasid must succeed on a disabled stream (ARM §3.21)");
}

#[test]
fn test_translate_when_disabled() {
    let ctx = StreamContext::new();
    let pasid = PASID::new(1).unwrap();
    ctx.create_pasid(pasid).unwrap();

    ctx.disable();

    let iova = IOVA::new(0x1000).unwrap();
    let result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err());
}

#[test]
fn test_map_page_when_disabled() {
    let ctx = StreamContext::new();
    let pasid = PASID::new(1).unwrap();
    ctx.create_pasid(pasid).unwrap();

    ctx.disable();

    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_write();
    let result = ctx.map_page(pasid, iova, pa, perms, SecurityState::NonSecure);
    assert!(result.is_err());
}

// ========================================================================
// Query Interface Tests
// ========================================================================

#[test]
fn test_query_pasid_count() {
    let ctx = StreamContext::new();
    ctx.create_pasid(PASID::new(1).unwrap()).unwrap();
    ctx.create_pasid(PASID::new(2).unwrap()).unwrap();

    let query = ctx.query();
    assert_eq!(query.pasid_count(), 2);
}

#[test]
fn test_query_has_pasid() {
    let ctx = StreamContext::new();
    ctx.create_pasid(PASID::new(1).unwrap()).unwrap();

    let query = ctx.query();
    assert!(query.has_pasid(PASID::new(1).unwrap()));
    assert!(!query.has_pasid(PASID::new(99).unwrap()));
}

#[test]
fn test_query_pasids_iterator() {
    let ctx = StreamContext::new();
    ctx.create_pasid(PASID::new(1).unwrap()).unwrap();
    ctx.create_pasid(PASID::new(2).unwrap()).unwrap();

    let query = ctx.query();
    assert_eq!(query.pasids().count(), 2);
}

#[test]
fn test_query_is_enabled() {
    let ctx = StreamContext::new();
    let query = ctx.query();
    assert!(query.is_enabled());

    ctx.disable();
    let query = ctx.query();
    assert!(!query.is_enabled());
}

#[test]
fn test_query_get_stats() {
    let ctx = StreamContext::new();
    let pasid = PASID::new(1).unwrap();
    ctx.create_pasid(pasid).unwrap();

    // Generate a fault
    let iova = IOVA::new(0x1000).unwrap();
    let _ = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);

    let query = ctx.query();
    let stats = query.get_stats();
    assert_eq!(stats.total_faults, 1);
}

#[test]
fn test_query_pasids_by_security_state() {
    let ctx = StreamContext::new();
    // Default security state is NonSecure; PASID 1 belongs to the stream's domain.
    ctx.create_pasid(PASID::new(1).unwrap()).unwrap();

    let query = ctx.query();
    // Stream is NonSecure → querying NonSecure returns all 1 PASID.
    assert_eq!(query.pasids_by_security_state(SecurityState::NonSecure).count(), 1);
    // Querying a different state returns nothing.
    assert_eq!(query.pasids_by_security_state(SecurityState::Secure).count(), 0);
}

// ========================================================================
// StreamConfigBuilder Tests
// ========================================================================

#[test]
fn test_stream_config_builder_new() {
    let builder = StreamConfigBuilder::new();
    let _config = builder.build();
    // Just verify it doesn't panic
    assert!(true);
}

#[test]
fn test_stream_config_builder_fluent_api() {
    let config = StreamConfigBuilder::new()
        .max_pasids_per_stream(256)
        .stage1_enabled(false)
        .stage2_enabled(true)
        .build();

    // Just verify fluent API works
    let ctx = StreamContext::new();
    let _result = ctx.apply_config(config);
    assert!(true);
}

// ========================================================================
// Page Operations Tests
// ========================================================================

#[test]
fn test_unmap_page_pasid_not_found() {
    let ctx = StreamContext::new();
    let iova = IOVA::new(0x1000).unwrap();
    let result = ctx.unmap_page(PASID::new(99).unwrap(), iova);
    assert!(result.is_err());
}

#[test]
fn test_unmap_page_success() {
    let ctx = StreamContext::new();
    let pasid = PASID::new(1).unwrap();
    ctx.create_pasid(pasid).unwrap();

    // Map a page
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_write();
    ctx.map_page(pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();

    // Unmap it
    assert!(ctx.unmap_page(pasid, iova).is_ok());
}

// ========================================================================
// Default Trait Tests
// ========================================================================

#[test]
fn test_stream_context_default() {
    let ctx = StreamContext::default();
    assert!(ctx.is_enabled());
    assert!(ctx.is_stage1_enabled());
    assert!(!ctx.is_stage2_enabled());
}

// ========================================================================
// Fault Type Coverage Tests
// ========================================================================

#[test]
fn test_fault_statistics_last_fault_time() {
    let ctx = StreamContext::new();
    let pasid = PASID::new(1).unwrap();
    ctx.create_pasid(pasid).unwrap();

    // Generate faults
    for i in 0..2 {
        let iova = IOVA::new(0x1000 + i * 0x1000).unwrap();
        let _ = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    let stats = ctx.get_fault_statistics();
    assert!(stats.last_fault_time.is_some());
}

#[test]
fn test_fault_statistics_empty() {
    let ctx = StreamContext::new();
    let stats = ctx.get_fault_statistics();
    assert_eq!(stats.total_faults, 0);
    assert_eq!(stats.page_not_mapped_count, 0);
    assert_eq!(stats.permission_violation_count, 0);
    assert!(stats.last_fault_time.is_none());
    assert!(!stats.rate_limited);
}

// ========================================================================
// Additional Coverage Tests
// ========================================================================

#[test]
fn test_validate_config_stage2_enabled_with_existing_address_space() {
    let ctx = StreamContext::new();
    // First create a Stage-2 address space
    ctx.create_stage2_address_space().unwrap();

    // Now try to enable Stage-2 without specifying a new address space
    // Should use the existing one
    let config = StreamConfigBuilder::new().stage2_enabled(true).build();

    let result = ctx.validate_config_update(&config);
    assert!(result.is_ok());
}

#[test]
fn test_permission_violation_fault_type() {
    let ctx = StreamContext::new();
    let pasid = PASID::new(1).unwrap();
    ctx.create_pasid(pasid).unwrap();

    // Map a page with read-only permissions
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_only();
    ctx.map_page(pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();

    // Try to write to a read-only page
    let _ = ctx.translate(pasid, iova, AccessType::Write, SecurityState::NonSecure);

    // Should have a permission fault
    let stats = ctx.get_fault_statistics();
    assert!(stats.permission_violation_count > 0);
}
