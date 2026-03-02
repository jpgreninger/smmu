#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]

//! Unit tests for SMMU controller
//!
//! Tests stream configuration, translation orchestration, fault handling,
//! and multi-stream independence per ARM SMMU v3 specification.

use smmu::types::{
    AccessType, PagePermissions, SMMUConfig, SecurityState, StreamConfig, StreamID, IOVA, PA, PASID,
};
use smmu::SMMU;

// ============================================================================
// Construction Tests
// ============================================================================

#[test]
fn test_new_smmu() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    assert!(!smmu.is_shutdown());
    assert_eq!(smmu.get_stream_count(), 0);
}

#[test]
fn test_smmu_with_config() {
    let config = SMMUConfig::high_performance();
    let smmu = SMMU::with_config(config);
    assert_eq!(smmu.get_stream_count(), 0);
}

#[test]
fn test_smmu_default() {
    let smmu = SMMU::default();
    assert_eq!(smmu.get_stream_count(), 0);
}

// ============================================================================
// Stream Configuration Tests
// ============================================================================

#[test]
fn test_configure_stream() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream_id = StreamID::new(1).unwrap();
    let config = StreamConfig::stage1_only();

    let result = smmu.configure_stream(stream_id, config);
    assert!(result.is_ok());
    assert!(smmu.has_stream(stream_id));
    assert_eq!(smmu.get_stream_count(), 1);
}

#[test]
fn test_configure_multiple_streams() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)

    for i in 1..=10 {
        let stream_id = StreamID::new(i).unwrap();
        smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    }

    assert_eq!(smmu.get_stream_count(), 10);
}

#[test]
fn test_configure_stream_zero() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream_id = StreamID::new(0).unwrap();

    let result = smmu.configure_stream(stream_id, StreamConfig::stage1_only());
    assert!(result.is_ok());
}

// ============================================================================
// Stream Enable/Disable Tests
// ============================================================================

#[test]
fn test_enable_disable_stream() {
    // §5.2 / §7.3.7: disable_stream() sets STE.Config==0b000 equivalent;
    // translate() must return Err(StreamDisabled) and no event is recorded.
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();

    smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Before disable: translation reaches the normal path (may fault on unmapped
    // address, but must NOT return StreamDisabled).
    let before = smmu.translate(
        stream_id, pasid,
        IOVA::new(0x1000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        !matches!(before, Err(smmu::types::TranslationError::StreamDisabled)),
        "before disable: must not return StreamDisabled; got {before:?}"
    );

    // Disable the stream; drain any events from the pre-disable translation.
    smmu.disable_stream(stream_id).unwrap();
    smmu.clear_event_queue();

    // After disable: must abort silently (StreamDisabled), no event recorded.
    let after = smmu.translate(
        stream_id, pasid,
        IOVA::new(0x1000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        matches!(after, Err(smmu::types::TranslationError::StreamDisabled)),
        "after disable: expected StreamDisabled; got {after:?}"
    );
    assert!(
        smmu.get_events().is_empty(),
        "§7.3.7: no event must be recorded for a disabled stream"
    );

    // Re-enable: translation must reach the normal path again.
    smmu.enable_stream(stream_id).unwrap();
    let re_enabled = smmu.translate(
        stream_id, pasid,
        IOVA::new(0x1000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        !matches!(re_enabled, Err(smmu::types::TranslationError::StreamDisabled)),
        "after re-enable: must not return StreamDisabled; got {re_enabled:?}"
    );
}

#[test]
fn test_disable_nonexistent_stream() {
    // Calling disable_stream() on a stream that was never configured must
    // return an error — there is no stream context to disable.
    let smmu = SMMU::new();
    let stream_id = StreamID::new(99).unwrap();

    let result = smmu.disable_stream(stream_id);
    assert!(
        result.is_err(),
        "disable_stream on unconfigured stream must return Err; got Ok"
    );
}

// ============================================================================
// Translation Tests with PASID 0
// ============================================================================

#[test]
fn test_translate_with_pasid_zero() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();
    smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().physical_address().as_u64(), 0x2000);
}

#[test]
fn test_translate_basic() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();
    smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok());
}

#[test]
fn test_translate_nonexistent_stream() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err());
}

// ============================================================================
// Multiple Stream Independence Tests
// ============================================================================

#[test]
fn test_multiple_streams_independent() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream1 = StreamID::new(1).unwrap();
    let stream2 = StreamID::new(2).unwrap();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa1 = PA::new(0x2000).unwrap();
    let pa2 = PA::new(0x3000).unwrap();

    // Configure both streams
    smmu.configure_stream(stream1, StreamConfig::stage1_only()).unwrap();
    smmu.configure_stream(stream2, StreamConfig::stage1_only()).unwrap();

    // Create PASIDs
    smmu.create_pasid(stream1, pasid).unwrap();
    smmu.create_pasid(stream2, pasid).unwrap();

    // Map different PAs to same IOVA in different streams
    smmu.map_page(
        stream1,
        pasid,
        iova,
        pa1,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();
    smmu.map_page(
        stream2,
        pasid,
        iova,
        pa2,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Verify independence
    let result1 = smmu.translate(stream1, pasid, iova, AccessType::Read, SecurityState::NonSecure).unwrap();
    let result2 = smmu.translate(stream2, pasid, iova, AccessType::Read, SecurityState::NonSecure).unwrap();

    assert_eq!(result1.physical_address().as_u64(), 0x2000);
    assert_eq!(result2.physical_address().as_u64(), 0x3000);
}

// ============================================================================
// Fault Recording Tests
// ============================================================================

#[test]
fn test_record_fault() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Attempt translation of unmapped address (should generate fault)
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err());

    // Check fault was recorded
    let faults = smmu.get_faults();
    assert!(!faults.is_empty());
}

#[test]
fn test_clear_faults() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Generate fault
    let _ = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);

    // Clear faults
    smmu.clear_faults();
    let faults = smmu.get_faults();
    assert!(faults.is_empty());
}

// ============================================================================
// Event Handling Tests
// ============================================================================

#[test]
fn test_get_events() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let events = smmu.get_events();
    assert!(events.is_empty());
}

// ============================================================================
// Shutdown Tests
// ============================================================================

#[test]
fn test_shutdown() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream_id = StreamID::new(1).unwrap();

    smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();

    assert!(!smmu.is_shutdown());
    smmu.shutdown().unwrap();
    assert!(smmu.is_shutdown());
}

#[test]
fn test_operations_after_shutdown() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    smmu.shutdown().unwrap();

    let stream_id = StreamID::new(1).unwrap();
    let result = smmu.configure_stream(stream_id, StreamConfig::stage1_only());
    assert!(result.is_err());
}

// ============================================================================
// Statistics Tests
// ============================================================================

#[test]
fn test_translation_statistics() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();
    smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Perform translation
    let _ = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);

    // Check translation statistics
    let (total, _successful, _failed) = smmu.get_translation_stats();
    assert!(total > 0);
}

// ============================================================================
// Thread Safety Tests
// ============================================================================

#[test]
fn test_smmu_send() {
    fn assert_send<T: Send>() {}
    assert_send::<SMMU>();
}

#[test]
fn test_smmu_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<SMMU>();
}
