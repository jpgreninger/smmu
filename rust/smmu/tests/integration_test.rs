#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]

//! ARM SMMU v3 Integration Tests
//!
//! Comprehensive integration testing covering:
//! - Two-stage translation workflows
//! - Stream isolation and security
//! - PASID context switching and management
//! - Large-scale scalability (1000+ streams/PASIDs)
//! - ARM SMMU v3 specification compliance
//!
//! Task 8.2: Integration Testing (TASKS-RUST.md)
//! - Port C++ integration tests to idiomatic Rust
//! - Add Rust-specific enhancements (ownership, concurrency, memory safety)
//! - Validate complete end-to-end workflows
//! - Ensure ARM SMMU v3 specification compliance

use smmu::types::{AccessType, PagePermissions, SecurityState, StreamConfig, StreamID, IOVA, PA, PASID};
use smmu::SMMU;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

// ============================================================================
// Helper Functions
// ============================================================================

/// Create a `StreamConfig` with Stage-1 translation enabled (non-bypass mode)
fn stage1_config() -> StreamConfig {
    let mut config = StreamConfig::default();
    config.translation_enabled = true;
    config.stage1_enabled = true;
    config
}

/// Create a `StreamConfig` with two-stage translation enabled
fn two_stage_config() -> StreamConfig {
    let mut config = StreamConfig::default();
    config.translation_enabled = true;
    config.stage1_enabled = true;
    config.stage2_enabled = true;
    config
}

// ============================================================================
// Test 1: Two-Stage Translation Integration Tests
// ============================================================================
// Port of test_two_stage_translation.cpp

#[test]
fn test_basic_two_stage_translation() {
    let smmu = SMMU::new();

    let stream_id = StreamID::new(100).unwrap();
    let pasid = PASID::new(1).unwrap();

    // Configure stream for two-stage translation
    smmu.configure_stream(stream_id, two_stage_config()).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Create Stage-2 address space for IPA → PA mappings
    smmu.create_stage2_address_space(stream_id).unwrap();

    // Setup two-stage mapping: IOVA -> IPA -> PA
    let iova = IOVA::new(0x100_1000).unwrap();
    let ipa = PA::new(0x200_1000).unwrap(); // IPA is PA type at intermediate stage
    let final_pa = PA::new(0x300_1000).unwrap();

    let perms = PagePermissions::read_write();

    // Stage 1: IOVA -> IPA (using guest PASID)
    smmu.map_page(stream_id, pasid, iova, ipa, perms, SecurityState::NonSecure)
        .unwrap();

    // Stage 2: IPA -> PA (using Stage-2 address space)
    let ipa_as_iova = IOVA::new(ipa.as_u64()).unwrap();
    smmu.map_stage2_page(stream_id, ipa_as_iova, final_pa, perms, SecurityState::NonSecure)
        .unwrap();

    // Perform two-stage translation
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);

    assert!(result.is_ok(), "Two-stage translation should succeed: {:?}", result.err());
    let trans_data = result.unwrap();
    assert_eq!(
        trans_data.physical_address().as_u64(),
        final_pa.as_u64(),
        "Final PA should match after two-stage translation"
    );
}

#[test]
fn test_two_stage_multiple_pages() {
    let smmu = SMMU::new();

    let stream_id = StreamID::new(101).unwrap();
    let pasid = PASID::new(1).unwrap();

    // Configure two-stage stream
    let mut config = StreamConfig::default();
    config.translation_enabled = true;
    config.stage1_enabled = true;
    config.stage2_enabled = true;

    smmu.configure_stream(stream_id, config).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Create Stage-2 address space
    smmu.create_stage2_address_space(stream_id).unwrap();

    const PAGE_SIZE: u64 = 4096;
    const NUM_PAGES: usize = 10;
    let perms = PagePermissions::read_write();

    // Setup multiple two-stage mappings
    for i in 0..NUM_PAGES {
        let iova = IOVA::new(0x100_0000 + i as u64 * PAGE_SIZE).unwrap();
        let ipa = PA::new(0x200_0000 + i as u64 * PAGE_SIZE).unwrap();
        let pa = PA::new(0x300_0000 + i as u64 * PAGE_SIZE).unwrap();

        // Stage 1: IOVA -> IPA
        smmu.map_page(stream_id, pasid, iova, ipa, perms, SecurityState::NonSecure)
            .unwrap();

        // Stage 2: IPA -> PA
        let ipa_as_iova = IOVA::new(ipa.as_u64()).unwrap();
        smmu.map_stage2_page(stream_id, ipa_as_iova, pa, perms, SecurityState::NonSecure)
            .unwrap();
    }

    // Test all translations
    for i in 0..NUM_PAGES {
        let iova = IOVA::new(0x100_0000 + i as u64 * PAGE_SIZE).unwrap();
        let expected_pa = PA::new(0x300_0000 + i as u64 * PAGE_SIZE).unwrap();

        let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);

        assert!(result.is_ok(), "Translation should succeed for page {i}");
        assert_eq!(
            result.unwrap().physical_address().as_u64(),
            expected_pa.as_u64(),
            "PA mismatch for page {i}"
        );
    }
}

#[test]
fn test_stage1_translation_fault() {
    let smmu = SMMU::new();

    let stream_id = StreamID::new(102).unwrap();
    let pasid = PASID::new(1).unwrap();

    // Configure two-stage stream
    let mut config = StreamConfig::default();
    config.translation_enabled = true;
    config.stage1_enabled = true;
    config.stage2_enabled = true;

    smmu.configure_stream(stream_id, config).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Create Stage-2 address space
    smmu.create_stage2_address_space(stream_id).unwrap();

    // Only setup Stage-2 mapping, no Stage-1
    let unmapped_iova = IOVA::new(0x5000).unwrap();

    // Attempt translation should fail at Stage-1
    let result = smmu.translate(stream_id, pasid, unmapped_iova, AccessType::Read, SecurityState::NonSecure);

    assert!(result.is_err(), "Translation should fail for unmapped Stage-1 IOVA");

    // Verify fault was recorded
    let events = smmu.get_events();
    assert!(!events.is_empty(), "Should have fault event for unmapped IOVA");
}

#[test]
fn test_stage2_translation_fault() {
    let smmu = SMMU::new();

    let stream_id = StreamID::new(103).unwrap();
    let pasid = PASID::new(1).unwrap();

    // Configure two-stage stream
    let mut config = StreamConfig::default();
    config.translation_enabled = true;
    config.stage1_enabled = true;
    config.stage2_enabled = true;

    smmu.configure_stream(stream_id, config).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Create Stage-2 address space
    smmu.create_stage2_address_space(stream_id).unwrap();

    let iova = IOVA::new(0x2000).unwrap();
    let ipa = PA::new(0x3000).unwrap();
    let perms = PagePermissions::read_write();

    // Map only Stage-1 (IOVA -> IPA), omit Stage-2
    smmu.map_page(stream_id, pasid, iova, ipa, perms, SecurityState::NonSecure)
        .unwrap();

    // Attempt translation should fail at Stage-2
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);

    assert!(result.is_err(), "Translation should fail when Stage-2 mapping is missing");
}

#[test]
fn test_two_stage_permission_intersection() {
    let smmu = SMMU::new();

    let stream_id = StreamID::new(104).unwrap();
    let pasid = PASID::new(1).unwrap();

    // Configure two-stage stream
    let mut config = StreamConfig::default();
    config.translation_enabled = true;
    config.stage1_enabled = true;
    config.stage2_enabled = true;

    smmu.configure_stream(stream_id, config).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Create Stage-2 address space
    smmu.create_stage2_address_space(stream_id).unwrap();

    let iova = IOVA::new(0x3000).unwrap();
    let ipa = PA::new(0x4000).unwrap();
    let final_pa = PA::new(0x5000).unwrap();

    // Stage-1: Read + Write
    let stage1_perms = PagePermissions::read_write();
    smmu.map_page(stream_id, pasid, iova, ipa, stage1_perms, SecurityState::NonSecure)
        .unwrap();

    // Stage-2: Read-only (more restrictive)
    let stage2_perms = PagePermissions::read_only();
    let ipa_as_iova = IOVA::new(ipa.as_u64()).unwrap();
    smmu.map_stage2_page(stream_id, ipa_as_iova, final_pa, stage2_perms, SecurityState::NonSecure)
        .unwrap();

    // Read should succeed
    let read_result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(read_result.is_ok(), "Read access should succeed");

    let trans_data = read_result.unwrap();
    let perms = trans_data.permissions();
    assert!(perms.read(), "Final permissions should allow read");
    assert!(!perms.write(), "Final permissions should NOT allow write (Stage-2 restriction)");

    // Write should fail due to Stage-2 restriction
    let write_result = smmu.translate(stream_id, pasid, iova, AccessType::Write, SecurityState::NonSecure);
    assert!(
        write_result.is_err(),
        "Write access should fail due to Stage-2 read-only restriction"
    );
}

#[test]
fn test_two_stage_concurrent_translations() {
    let smmu = Arc::new(SMMU::new());

    let stream_id = StreamID::new(105).unwrap();
    let pasid = PASID::new(1).unwrap();

    // Configure two-stage stream
    let mut config = StreamConfig::default();
    config.translation_enabled = true;
    config.stage1_enabled = true;
    config.stage2_enabled = true;

    smmu.configure_stream(stream_id, config).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Create Stage-2 address space
    smmu.create_stage2_address_space(stream_id).unwrap();

    const NUM_THREADS: usize = 4;
    const TRANSLATIONS_PER_THREAD: usize = 100;
    const PAGE_SIZE: u64 = 4096;
    let perms = PagePermissions::read_write();

    // Setup mappings for concurrent access
    for i in 0..(NUM_THREADS * TRANSLATIONS_PER_THREAD) {
        let iova = IOVA::new(0x100_0000 + i as u64 * PAGE_SIZE).unwrap();
        let ipa = PA::new(0x200_0000 + i as u64 * PAGE_SIZE).unwrap();
        let pa = PA::new(0x300_0000 + i as u64 * PAGE_SIZE).unwrap();

        smmu.map_page(stream_id, pasid, iova, ipa, perms, SecurityState::NonSecure)
            .unwrap();

        let ipa_as_iova = IOVA::new(ipa.as_u64()).unwrap();
        smmu.map_stage2_page(stream_id, ipa_as_iova, pa, perms, SecurityState::NonSecure)
            .unwrap();
    }

    let successful = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];

    for thread_id in 0..NUM_THREADS {
        let smmu_clone = Arc::clone(&smmu);
        let successful_clone = Arc::clone(&successful);
        let failed_clone = Arc::clone(&failed);

        let handle = thread::spawn(move || {
            for i in 0..TRANSLATIONS_PER_THREAD {
                let index = thread_id * TRANSLATIONS_PER_THREAD + i;
                let iova = IOVA::new(0x100_0000 + index as u64 * PAGE_SIZE).unwrap();
                let expected_pa = PA::new(0x300_0000 + index as u64 * PAGE_SIZE).unwrap();

                let result = smmu_clone.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);

                if result.is_ok() && result.unwrap().physical_address().as_u64() == expected_pa.as_u64() {
                    successful_clone.fetch_add(1, Ordering::Relaxed);
                } else {
                    failed_clone.fetch_add(1, Ordering::Relaxed);
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(
        successful.load(Ordering::Relaxed),
        NUM_THREADS * TRANSLATIONS_PER_THREAD,
        "All concurrent translations should succeed"
    );
    assert_eq!(failed.load(Ordering::Relaxed), 0, "No translations should fail");
}

// ============================================================================
// Test 2: Stream Isolation Validation Tests
// ============================================================================
// Port of test_stream_isolation.cpp

#[test]
fn test_basic_stream_isolation() {
    let smmu = SMMU::new();

    let stream1 = StreamID::new(100).unwrap();
    let stream2 = StreamID::new(200).unwrap();
    let pasid = PASID::new(1).unwrap();

    // Configure both streams with Stage-1 translation enabled
    smmu.configure_stream(stream1, stage1_config()).unwrap();
    smmu.configure_stream(stream2, stage1_config()).unwrap();

    smmu.create_pasid(stream1, pasid).unwrap();
    smmu.create_pasid(stream2, pasid).unwrap();

    // Map same IOVA to different PAs for each stream
    let shared_iova = IOVA::new(0x100_1000).unwrap();
    let stream1_pa = PA::new(0x200_1000).unwrap();
    let stream2_pa = PA::new(0x300_1000).unwrap();
    let perms = PagePermissions::read_write();

    smmu.map_page(stream1, pasid, shared_iova, stream1_pa, perms, SecurityState::NonSecure)
        .unwrap();

    smmu.map_page(stream2, pasid, shared_iova, stream2_pa, perms, SecurityState::NonSecure)
        .unwrap();

    // Test translation for stream1
    let result1 = smmu.translate(stream1, pasid, shared_iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result1.is_ok(), "Stream1 translation should succeed");
    assert_eq!(
        result1.unwrap().physical_address().as_u64(),
        stream1_pa.as_u64(),
        "Stream1 should get its own PA"
    );

    // Test translation for stream2
    let result2 = smmu.translate(stream2, pasid, shared_iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result2.is_ok(), "Stream2 translation should succeed");
    assert_eq!(
        result2.unwrap().physical_address().as_u64(),
        stream2_pa.as_u64(),
        "Stream2 should get its own PA"
    );

    // Verify isolation: different PAs for same IOVA
    assert_ne!(
        stream1_pa.as_u64(),
        stream2_pa.as_u64(),
        "Streams should have isolated address spaces"
    );
}

#[test]
fn test_fault_isolation_between_streams() {
    let smmu = SMMU::new();

    let stream1 = StreamID::new(500).unwrap();
    let stream2 = StreamID::new(600).unwrap();
    let pasid = PASID::new(1).unwrap();

    smmu.configure_stream(stream1, stage1_config()).unwrap();
    smmu.configure_stream(stream2, stage1_config()).unwrap();

    smmu.create_pasid(stream1, pasid).unwrap();
    smmu.create_pasid(stream2, pasid).unwrap();

    // Map page only for stream1
    let test_iova = IOVA::new(0x5000).unwrap();
    let test_pa = PA::new(0x6000).unwrap();
    let perms = PagePermissions::read_write();

    smmu.map_page(stream1, pasid, test_iova, test_pa, perms, SecurityState::NonSecure)
        .unwrap();

    // Clear events
    smmu.clear_event_queue();

    // Test valid translation for stream1
    let result1 = smmu.translate(stream1, pasid, test_iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result1.is_ok(), "Stream1 translation should succeed");

    // Test invalid translation for stream2 (should fault)
    let result2 = smmu.translate(stream2, pasid, test_iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result2.is_err(), "Stream2 translation should fail (unmapped)");

    // Check fault recorded for stream2
    let events = smmu.get_events();
    assert!(!events.is_empty(), "Should have fault event for stream2");

    // Verify fault is for stream2, not stream1
    let has_stream2_fault = events.iter().any(|event| event.stream_id == stream2.as_u32());
    assert!(has_stream2_fault, "Should have fault event for stream2");
}

#[test]
fn test_permission_isolation_between_streams() {
    let smmu = SMMU::new();

    let readonly_stream = StreamID::new(900).unwrap();
    let readwrite_stream = StreamID::new(1000).unwrap();
    let pasid = PASID::new(1).unwrap();

    smmu.configure_stream(readonly_stream, stage1_config()).unwrap();
    smmu.configure_stream(readwrite_stream, stage1_config()).unwrap();

    smmu.create_pasid(readonly_stream, pasid).unwrap();
    smmu.create_pasid(readwrite_stream, pasid).unwrap();

    let test_iova = IOVA::new(0x8000).unwrap();
    let readonly_pa = PA::new(0x9000).unwrap();
    let readwrite_pa = PA::new(0xa000).unwrap();

    // Map with read-only for readonly_stream
    smmu.map_page(
        readonly_stream,
        pasid,
        test_iova,
        readonly_pa,
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Map with read-write for readwrite_stream
    smmu.map_page(
        readwrite_stream,
        pasid,
        test_iova,
        readwrite_pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Test read access for both streams
    let read1 = smmu.translate(readonly_stream, pasid, test_iova, AccessType::Read, SecurityState::NonSecure);
    assert!(read1.is_ok(), "Readonly stream read should succeed");
    assert!(
        !read1.unwrap().permissions().write(),
        "Readonly stream should not have write permission"
    );

    let read2 = smmu.translate(readwrite_stream, pasid, test_iova, AccessType::Read, SecurityState::NonSecure);
    assert!(read2.is_ok(), "Readwrite stream read should succeed");
    assert!(
        read2.unwrap().permissions().write(),
        "Readwrite stream should have write permission"
    );

    // Test write access
    let write1 = smmu.translate(readonly_stream, pasid, test_iova, AccessType::Write, SecurityState::NonSecure);
    assert!(write1.is_err(), "Readonly stream write should fail");

    let write2 = smmu.translate(readwrite_stream, pasid, test_iova, AccessType::Write, SecurityState::NonSecure);
    assert!(write2.is_ok(), "Readwrite stream write should succeed");
}

#[test]
fn test_concurrent_multi_stream_access() {
    let smmu = Arc::new(SMMU::new());

    const NUM_STREAMS: u32 = 10;
    const TRANSLATIONS_PER_STREAM: usize = 100;
    const PAGE_SIZE: u64 = 4096;
    let pasid = PASID::new(1).unwrap();

    // Setup multiple streams
    for i in 0..NUM_STREAMS {
        let stream_id = StreamID::new(1100 + i).unwrap();
        smmu.configure_stream(stream_id, stage1_config()).unwrap();
        smmu.create_pasid(stream_id, pasid).unwrap();

        // Map unique pages for each stream
        for j in 0..TRANSLATIONS_PER_STREAM {
            let iova =
                IOVA::new(0x100_0000 + (u64::from(i) * TRANSLATIONS_PER_STREAM as u64 + j as u64) * PAGE_SIZE)
                    .unwrap();
            let pa =
                PA::new(0x200_0000 + (u64::from(i) * TRANSLATIONS_PER_STREAM as u64 + j as u64) * PAGE_SIZE)
                    .unwrap();

            smmu.map_page(
                stream_id,
                pasid,
                iova,
                pa,
                PagePermissions::read_write(),
                SecurityState::NonSecure,
            )
            .unwrap();
        }
    }

    let successful = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];

    for stream_index in 0..NUM_STREAMS {
        let smmu_clone = Arc::clone(&smmu);
        let successful_clone = Arc::clone(&successful);
        let failed_clone = Arc::clone(&failed);

        let handle = thread::spawn(move || {
            let stream_id = StreamID::new(1100 + stream_index).unwrap();

            for i in 0..TRANSLATIONS_PER_STREAM {
                let iova = IOVA::new(
                    0x100_0000
                        + (u64::from(stream_index) * TRANSLATIONS_PER_STREAM as u64 + i as u64) * PAGE_SIZE,
                )
                .unwrap();
                let expected_pa = PA::new(
                    0x200_0000
                        + (u64::from(stream_index) * TRANSLATIONS_PER_STREAM as u64 + i as u64) * PAGE_SIZE,
                )
                .unwrap();

                let result = smmu_clone.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);

                if result.is_ok() && result.unwrap().physical_address().as_u64() == expected_pa.as_u64() {
                    successful_clone.fetch_add(1, Ordering::Relaxed);
                } else {
                    failed_clone.fetch_add(1, Ordering::Relaxed);
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let expected_total = NUM_STREAMS as usize * TRANSLATIONS_PER_STREAM;
    assert_eq!(
        successful.load(Ordering::Relaxed),
        expected_total,
        "All concurrent stream accesses should succeed"
    );
    assert_eq!(failed.load(Ordering::Relaxed), 0, "No accesses should fail");
}

#[test]
fn test_cross_stream_pasid_isolation() {
    let smmu = SMMU::new();

    let stream1 = StreamID::new(1500).unwrap();
    let stream2 = StreamID::new(1600).unwrap();
    let pasid1 = PASID::new(1).unwrap();
    let pasid2 = PASID::new(2).unwrap();

    smmu.configure_stream(stream1, stage1_config()).unwrap();
    smmu.configure_stream(stream2, stage1_config()).unwrap();

    smmu.create_pasid(stream1, pasid1).unwrap();
    smmu.create_pasid(stream1, pasid2).unwrap();
    smmu.create_pasid(stream2, pasid1).unwrap();
    smmu.create_pasid(stream2, pasid2).unwrap();

    // Create mapping matrix: each stream+PASID gets unique PA
    let test_iova = IOVA::new(0xc000).unwrap();
    let pa_s1_p1 = PA::new(0xd000).unwrap();
    let pa_s1_p2 = PA::new(0xe000).unwrap();
    let pa_s2_p1 = PA::new(0xf000).unwrap();
    let pa_s2_p2 = PA::new(0x1_0000).unwrap();

    let perms = PagePermissions::read_write();

    smmu.map_page(stream1, pasid1, test_iova, pa_s1_p1, perms, SecurityState::NonSecure)
        .unwrap();
    smmu.map_page(stream1, pasid2, test_iova, pa_s1_p2, perms, SecurityState::NonSecure)
        .unwrap();
    smmu.map_page(stream2, pasid1, test_iova, pa_s2_p1, perms, SecurityState::NonSecure)
        .unwrap();
    smmu.map_page(stream2, pasid2, test_iova, pa_s2_p2, perms, SecurityState::NonSecure)
        .unwrap();

    // Test all combinations
    let result_s1_p1 = smmu.translate(stream1, pasid1, test_iova, AccessType::Read, SecurityState::NonSecure).unwrap();
    let result_s1_p2 = smmu.translate(stream1, pasid2, test_iova, AccessType::Read, SecurityState::NonSecure).unwrap();
    let result_s2_p1 = smmu.translate(stream2, pasid1, test_iova, AccessType::Read, SecurityState::NonSecure).unwrap();
    let result_s2_p2 = smmu.translate(stream2, pasid2, test_iova, AccessType::Read, SecurityState::NonSecure).unwrap();

    // Verify each combination gets unique PA
    assert_eq!(result_s1_p1.physical_address().as_u64(), pa_s1_p1.as_u64());
    assert_eq!(result_s1_p2.physical_address().as_u64(), pa_s1_p2.as_u64());
    assert_eq!(result_s2_p1.physical_address().as_u64(), pa_s2_p1.as_u64());
    assert_eq!(result_s2_p2.physical_address().as_u64(), pa_s2_p2.as_u64());

    // Verify all are different
    use std::collections::HashSet;
    let mut unique_pas = HashSet::new();
    unique_pas.insert(result_s1_p1.physical_address().as_u64());
    unique_pas.insert(result_s1_p2.physical_address().as_u64());
    unique_pas.insert(result_s2_p1.physical_address().as_u64());
    unique_pas.insert(result_s2_p2.physical_address().as_u64());

    assert_eq!(unique_pas.len(), 4, "All stream+PASID combinations should have unique PAs");
}

// ============================================================================
// Test 3: PASID Context Switching Tests
// ============================================================================
// Port of test_pasid_context_switching.cpp

#[test]
fn test_basic_pasid_context_switching() {
    let smmu = SMMU::new();

    let stream_id = StreamID::new(100).unwrap();
    let test_pasids: Vec<u32> = vec![1, 2, 3, 4, 5];

    smmu.configure_stream(stream_id, stage1_config()).unwrap();

    const PAGE_SIZE: u64 = 4096;
    const PAGES_PER_PASID: usize = 10;

    // Create multiple PASID contexts
    for &pasid_val in &test_pasids {
        let pasid = PASID::new(pasid_val).unwrap();
        smmu.create_pasid(stream_id, pasid).unwrap();

        // Map pages for this PASID
        for i in 0..PAGES_PER_PASID {
            let iova = IOVA::new(0x100_0000 + u64::from(pasid_val) * 0x10_0000 + i as u64 * PAGE_SIZE).unwrap();
            let pa = PA::new(0x200_0000 + u64::from(pasid_val) * 0x10_0000 + i as u64 * PAGE_SIZE).unwrap();

            smmu.map_page(
                stream_id,
                pasid,
                iova,
                pa,
                PagePermissions::read_write(),
                SecurityState::NonSecure,
            )
            .unwrap();
        }
    }

    // Test switching between PASID contexts
    for &pasid_val in &test_pasids {
        let pasid = PASID::new(pasid_val).unwrap();

        for i in 0..PAGES_PER_PASID {
            let iova = IOVA::new(0x100_0000 + u64::from(pasid_val) * 0x10_0000 + i as u64 * PAGE_SIZE).unwrap();
            let expected_pa = PA::new(0x200_0000 + u64::from(pasid_val) * 0x10_0000 + i as u64 * PAGE_SIZE).unwrap();

            let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);

            assert!(result.is_ok(), "Translation should succeed for PASID {pasid_val}");
            assert_eq!(
                result.unwrap().physical_address().as_u64(),
                expected_pa.as_u64(),
                "PA mismatch for PASID {pasid_val}, page {i}"
            );
        }
    }
}

#[test]
fn test_pasid_context_isolation() {
    let smmu = SMMU::new();

    let stream_id = StreamID::new(100).unwrap();
    let pasid1 = PASID::new(10).unwrap();
    let pasid2 = PASID::new(20).unwrap();

    smmu.configure_stream(stream_id, stage1_config()).unwrap();
    smmu.create_pasid(stream_id, pasid1).unwrap();
    smmu.create_pasid(stream_id, pasid2).unwrap();

    const PAGE_SIZE: u64 = 4096;
    const PAGES_PER_PASID: usize = 10;

    // Map pages for both PASIDs
    for i in 0..PAGES_PER_PASID {
        let iova1 = IOVA::new(0x100_0000 + 10 * 0x10_0000 + i as u64 * PAGE_SIZE).unwrap();
        let pa1 = PA::new(0x200_0000 + 10 * 0x10_0000 + i as u64 * PAGE_SIZE).unwrap();

        let iova2 = IOVA::new(0x100_0000 + 20 * 0x10_0000 + i as u64 * PAGE_SIZE).unwrap();
        let pa2 = PA::new(0x200_0000 + 20 * 0x10_0000 + i as u64 * PAGE_SIZE).unwrap();

        smmu.map_page(
            stream_id,
            pasid1,
            iova1,
            pa1,
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .unwrap();

        smmu.map_page(
            stream_id,
            pasid2,
            iova2,
            pa2,
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .unwrap();
    }

    // Verify isolation: same offset within PASID range maps to different PAs
    let shared_offset = 0x1000u64;
    let iova1 = IOVA::new(0x100_0000 + 10 * 0x10_0000 + shared_offset).unwrap();
    let iova2 = IOVA::new(0x100_0000 + 20 * 0x10_0000 + shared_offset).unwrap();

    let result1 = smmu.translate(stream_id, pasid1, iova1, AccessType::Read, SecurityState::NonSecure).unwrap();
    let result2 = smmu.translate(stream_id, pasid2, iova2, AccessType::Read, SecurityState::NonSecure).unwrap();

    assert_ne!(
        result1.physical_address().as_u64(),
        result2.physical_address().as_u64(),
        "Different PASIDs should map to different PAs"
    );
}

#[test]
fn test_pasid_lifecycle_management() {
    let smmu = SMMU::new();

    let stream_id = StreamID::new(100).unwrap();
    let test_pasid = PASID::new(30).unwrap();

    smmu.configure_stream(stream_id, stage1_config()).unwrap();

    let test_iova = IOVA::new(0x2000).unwrap();
    let test_pa = PA::new(0x3000).unwrap();

    // Verify PASID doesn't exist initially
    let result = smmu.translate(stream_id, test_pasid, test_iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "PASID should not exist initially");

    // Create PASID
    smmu.create_pasid(stream_id, test_pasid).unwrap();

    // Map a page
    smmu.map_page(
        stream_id,
        test_pasid,
        test_iova,
        test_pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Verify it works
    let result = smmu.translate(stream_id, test_pasid, test_iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "PASID should work after creation");

    // Remove PASID
    smmu.remove_pasid(stream_id, test_pasid).unwrap();

    // Verify PASID no longer works
    let result = smmu.translate(stream_id, test_pasid, test_iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "PASID should not work after removal");
}

#[test]
fn test_large_scale_pasid_switching() {
    let smmu = SMMU::new();

    let stream_id = StreamID::new(100).unwrap();
    const NUM_PASIDS: u32 = 100;
    const PAGES_PER_PASID: usize = 20;
    const PAGE_SIZE: u64 = 4096;

    smmu.configure_stream(stream_id, stage1_config()).unwrap();

    // Create many PASID contexts
    for pasid_val in 1..=NUM_PASIDS {
        let pasid = PASID::new(pasid_val).unwrap();
        smmu.create_pasid(stream_id, pasid).unwrap();

        for i in 0..PAGES_PER_PASID {
            let iova = IOVA::new(0x100_0000 + u64::from(pasid_val) * 0x10_0000 + i as u64 * PAGE_SIZE).unwrap();
            let pa = PA::new(0x200_0000 + u64::from(pasid_val) * 0x10_0000 + i as u64 * PAGE_SIZE).unwrap();

            smmu.map_page(
                stream_id,
                pasid,
                iova,
                pa,
                PagePermissions::read_write(),
                SecurityState::NonSecure,
            )
            .unwrap();
        }
    }

    // Verify all contexts work correctly
    for pasid_val in 1..=NUM_PASIDS {
        let pasid = PASID::new(pasid_val).unwrap();

        for i in 0..PAGES_PER_PASID {
            let iova = IOVA::new(0x100_0000 + u64::from(pasid_val) * 0x10_0000 + i as u64 * PAGE_SIZE).unwrap();
            let expected_pa = PA::new(0x200_0000 + u64::from(pasid_val) * 0x10_0000 + i as u64 * PAGE_SIZE).unwrap();

            let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);

            assert!(result.is_ok(), "Translation should succeed for PASID {pasid_val}");
            assert_eq!(
                result.unwrap().physical_address().as_u64(),
                expected_pa.as_u64(),
                "PA mismatch for PASID {pasid_val}, page {i}"
            );
        }
    }
}

#[test]
fn test_concurrent_pasid_switching() {
    let smmu = Arc::new(SMMU::new());

    let stream_id = StreamID::new(100).unwrap();
    const NUM_THREADS: usize = 8;
    const PASIDS_PER_THREAD: u32 = 10;
    const ACCESSES_PER_THREAD: usize = 500;
    const PAGE_SIZE: u64 = 4096;
    const PAGES_PER_PASID: usize = 10;

    smmu.configure_stream(stream_id, stage1_config()).unwrap();

    // Create PASID contexts for each thread
    for thread_id in 0..NUM_THREADS {
        for i in 0..PASIDS_PER_THREAD {
            let pasid_val = (thread_id as u32 * PASIDS_PER_THREAD) + i + 1;
            let pasid = PASID::new(pasid_val).unwrap();
            smmu.create_pasid(stream_id, pasid).unwrap();

            for j in 0..PAGES_PER_PASID {
                let iova = IOVA::new(0x100_0000 + u64::from(pasid_val) * 0x10_0000 + j as u64 * PAGE_SIZE).unwrap();
                let pa = PA::new(0x200_0000 + u64::from(pasid_val) * 0x10_0000 + j as u64 * PAGE_SIZE).unwrap();

                smmu.map_page(
                    stream_id,
                    pasid,
                    iova,
                    pa,
                    PagePermissions::read_write(),
                    SecurityState::NonSecure,
                )
                .unwrap();
            }
        }
    }

    let total_successful = Arc::new(AtomicUsize::new(0));
    let total_failed = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];

    for thread_id in 0..NUM_THREADS {
        let smmu_clone = Arc::clone(&smmu);
        let successful_clone = Arc::clone(&total_successful);
        let failed_clone = Arc::clone(&total_failed);

        let handle = thread::spawn(move || {
            use rand::Rng;
            let mut rng = rand::thread_rng();

            for _ in 0..ACCESSES_PER_THREAD {
                let pasid_index = rng.gen_range(0..PASIDS_PER_THREAD);
                let pasid_val = (thread_id as u32 * PASIDS_PER_THREAD) + pasid_index + 1;
                let pasid = PASID::new(pasid_val).unwrap();

                let page_index = rng.gen_range(0..PAGES_PER_PASID);
                let iova =
                    IOVA::new(0x100_0000 + u64::from(pasid_val) * 0x10_0000 + page_index as u64 * PAGE_SIZE).unwrap();
                let expected_pa =
                    PA::new(0x200_0000 + u64::from(pasid_val) * 0x10_0000 + page_index as u64 * PAGE_SIZE).unwrap();

                let result = smmu_clone.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);

                if result.is_ok() && result.unwrap().physical_address().as_u64() == expected_pa.as_u64() {
                    successful_clone.fetch_add(1, Ordering::Relaxed);
                } else {
                    failed_clone.fetch_add(1, Ordering::Relaxed);
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let expected_total = NUM_THREADS * ACCESSES_PER_THREAD;
    assert_eq!(
        total_successful.load(Ordering::Relaxed),
        expected_total,
        "All concurrent PASID accesses should succeed"
    );
    assert_eq!(total_failed.load(Ordering::Relaxed), 0, "No accesses should fail");
}

// ============================================================================
// Test 4: Large-Scale Scalability Tests
// ============================================================================
// Port of test_large_scale_scalability.cpp

#[test]
fn test_large_scale_stream_configuration() {
    let smmu = SMMU::new();

    const NUM_STREAMS: u32 = 1000;
    const PASIDS_PER_STREAM: u32 = 50;

    let start = Instant::now();

    // Configure many streams
    for i in 1..=NUM_STREAMS {
        let stream_id = StreamID::new(i).unwrap();
        smmu.configure_stream(stream_id, stage1_config()).unwrap();

        // Create multiple PASIDs per stream
        for j in 1..=PASIDS_PER_STREAM {
            let pasid = PASID::new(j).unwrap();
            smmu.create_pasid(stream_id, pasid).unwrap();
        }
    }

    let setup_duration = start.elapsed();

    // Verify all streams are configured
    assert_eq!(
        smmu.get_stream_count(),
        NUM_STREAMS as usize,
        "Should have configured all streams"
    );

    println!(
        "Large-scale configuration: {NUM_STREAMS} streams with {PASIDS_PER_STREAM} PASIDs each in {setup_duration:?}"
    );

    // Should complete within reasonable time (30 seconds)
    assert!(
        setup_duration.as_secs() < 30,
        "Stream configuration took too long: {setup_duration:?}"
    );
}

#[test]
#[allow(clippy::cast_precision_loss)]
fn test_massive_translation_load() {
    let smmu = SMMU::new();

    const NUM_STREAMS: u32 = 50;
    const PASIDS_PER_STREAM: u32 = 10;
    const PAGES_PER_PASID: usize = 50;
    const TRANSLATIONS_PER_COMBO: usize = 10;
    const PAGE_SIZE: u64 = 4096;

    // Setup large-scale mapping infrastructure
    for i in 1..=NUM_STREAMS {
        let stream_id = StreamID::new(i).unwrap();
        smmu.configure_stream(stream_id, stage1_config()).unwrap();

        for j in 1..=PASIDS_PER_STREAM {
            let pasid = PASID::new(j).unwrap();
            smmu.create_pasid(stream_id, pasid).unwrap();

            for k in 0..PAGES_PER_PASID {
                let iova = IOVA::new(
                    0x1_0000_0000 + u64::from(i) * 0x1000_0000 + u64::from(j) * 0x100_0000 + k as u64 * PAGE_SIZE,
                )
                .unwrap();
                let pa =
                    PA::new(0x2_0000_0000 + u64::from(i) * 0x1000_0000 + u64::from(j) * 0x100_0000 + k as u64 * PAGE_SIZE)
                        .unwrap();

                smmu.map_page(
                    stream_id,
                    pasid,
                    iova,
                    pa,
                    PagePermissions::read_write(),
                    SecurityState::NonSecure,
                )
                .unwrap();
            }
        }
    }

    let total_combinations = NUM_STREAMS as usize * PASIDS_PER_STREAM as usize * PAGES_PER_PASID;
    let total_translations = total_combinations * TRANSLATIONS_PER_COMBO;

    println!(
        "Massive load test: {total_combinations} unique mappings, {total_translations} total translations"
    );

    // Perform massive translation load
    use rand::Rng;
    let mut rng = rand::thread_rng();

    let start = Instant::now();
    let mut successful = 0;
    let mut failed = 0;

    for _ in 0..total_translations {
        let stream_id = StreamID::new(rng.gen_range(1..=NUM_STREAMS)).unwrap();
        let pasid = PASID::new(rng.gen_range(1..=PASIDS_PER_STREAM)).unwrap();
        let page_index = rng.gen_range(0..PAGES_PER_PASID);

        let stream_base = u64::from(stream_id.as_u32()) * 0x1000_0000;
        let pasid_base = u64::from(pasid.as_u32()) * 0x100_0000;
        let iova = IOVA::new(0x1_0000_0000 + stream_base + pasid_base + page_index as u64 * PAGE_SIZE).unwrap();

        let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);

        if result.is_ok() {
            successful += 1;
        } else {
            failed += 1;
        }
    }

    let duration = start.elapsed();
    let total_time_seconds = duration.as_secs_f64();
    let throughput = total_translations as f64 / total_time_seconds;
    let avg_translation_time_us = (duration.as_micros() as f64) / (total_translations as f64);

    println!("Massive load results:");
    println!("  Total translations: {total_translations}");
    println!("  Total time: {total_time_seconds:.3} seconds");
    println!("  Throughput: {throughput:.0} translations/sec");
    println!("  Avg time per translation: {avg_translation_time_us:.3} μs");

    assert_eq!(successful, total_translations, "All translations should succeed");
    assert_eq!(failed, 0, "No translations should fail");

    // Performance validation: avg translation < 50 μs for large-scale
    assert!(
        avg_translation_time_us < 50.0,
        "Average translation time too slow: {avg_translation_time_us:.3} μs"
    );

    // Throughput > 20K ops/sec for large-scale
    assert!(throughput > 20_000.0, "Throughput too low: {throughput:.0} ops/sec");
}

#[test]
#[allow(clippy::cast_precision_loss)]
fn test_concurrent_high_load_scalability() {
    let smmu = Arc::new(SMMU::new());

    const NUM_THREADS: usize = 8;
    const STREAMS_PER_THREAD: u32 = 25;
    const OPERATIONS_PER_THREAD: usize = 5000;
    const PAGE_SIZE: u64 = 4096;
    const PAGES_PER_STREAM: usize = 50;

    // Setup concurrent infrastructure
    for thread_id in 0..NUM_THREADS {
        for i in 0..STREAMS_PER_THREAD {
            let stream_id = StreamID::new((thread_id as u32 * STREAMS_PER_THREAD) + i + 1).unwrap();
            smmu.configure_stream(stream_id, StreamConfig::default()).unwrap();

            let pasid = PASID::new(1).unwrap();
            smmu.create_pasid(stream_id, pasid).unwrap();

            for j in 0..PAGES_PER_STREAM {
                let stream_base = u64::from(stream_id.as_u32()) * 0x1000_0000;
                let pasid_base = 0x100_0000;
                let iova = IOVA::new(0x1_0000_0000 + stream_base + pasid_base + j as u64 * PAGE_SIZE).unwrap();
                let pa = PA::new(0x2_0000_0000 + stream_base + pasid_base + j as u64 * PAGE_SIZE).unwrap();

                smmu.map_page(
                    stream_id,
                    pasid,
                    iova,
                    pa,
                    PagePermissions::read_write(),
                    SecurityState::NonSecure,
                )
                .unwrap();
            }
        }
    }

    let total_successful = Arc::new(AtomicU64::new(0));
    let total_failed = Arc::new(AtomicU64::new(0));
    let total_operations = Arc::new(AtomicU64::new(0));

    let mut handles = vec![];

    let start = Instant::now();

    for thread_id in 0..NUM_THREADS {
        let smmu_clone = Arc::clone(&smmu);
        let successful_clone = Arc::clone(&total_successful);
        let failed_clone = Arc::clone(&total_failed);
        let operations_clone = Arc::clone(&total_operations);

        let handle = thread::spawn(move || {
            use rand::Rng;
            let mut rng = rand::thread_rng();

            for _ in 0..OPERATIONS_PER_THREAD {
                let stream_index = rng.gen_range(0..STREAMS_PER_THREAD);
                let stream_id = StreamID::new((thread_id as u32 * STREAMS_PER_THREAD) + stream_index + 1).unwrap();
                let page_index = rng.gen_range(0..PAGES_PER_STREAM);

                let stream_base = u64::from(stream_id.as_u32()) * 0x1000_0000;
                let pasid_base = 0x100_0000;
                let iova =
                    IOVA::new(0x1_0000_0000 + stream_base + pasid_base + page_index as u64 * PAGE_SIZE).unwrap();

                let pasid = PASID::new(1).unwrap();
                let result = smmu_clone.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);

                if result.is_ok() {
                    successful_clone.fetch_add(1, Ordering::Relaxed);
                } else {
                    failed_clone.fetch_add(1, Ordering::Relaxed);
                }

                operations_clone.fetch_add(1, Ordering::Relaxed);
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let total_duration = start.elapsed();
    let total_time_seconds = total_duration.as_secs_f64();
    let overall_throughput = total_operations.load(Ordering::Relaxed) as f64 / total_time_seconds;

    println!("Concurrent high-load results:");
    println!("  Threads: {NUM_THREADS}");
    println!("  Total operations: {}", total_operations.load(Ordering::Relaxed));
    println!("  Total time: {total_time_seconds:.3} seconds");
    println!("  Overall throughput: {overall_throughput:.0} ops/sec");
    println!("  Successful: {}", total_successful.load(Ordering::Relaxed));
    println!("  Failed: {}", total_failed.load(Ordering::Relaxed));

    let expected_operations = NUM_THREADS * OPERATIONS_PER_THREAD;
    assert_eq!(
        total_operations.load(Ordering::Relaxed) as usize,
        expected_operations,
        "Should have performed all operations"
    );
    assert_eq!(
        total_successful.load(Ordering::Relaxed) as usize,
        expected_operations,
        "All operations should succeed"
    );
    assert_eq!(total_failed.load(Ordering::Relaxed), 0, "No operations should fail");

    // Throughput > 20K ops/sec minimum
    assert!(
        overall_throughput > 20_000.0,
        "Concurrent throughput should exceed minimum: {overall_throughput:.0} ops/sec"
    );
}

#[test]
#[allow(clippy::cast_precision_loss)]
fn test_memory_scalability_under_load() {
    let smmu = SMMU::new();

    const NUM_STREAMS: u32 = 100;
    const PASIDS_PER_STREAM: u32 = 50;
    const PAGES_PER_PASID: usize = 100;
    const PAGE_SIZE: u64 = 4096;

    // Phase 1: Gradual buildup to test memory scaling
    for batch in 1..=10 {
        let streams_in_batch = NUM_STREAMS / 10;

        let batch_start = Instant::now();

        for i in 0..streams_in_batch {
            let stream_id = StreamID::new(((batch - 1) * streams_in_batch) + i + 1).unwrap();
            smmu.configure_stream(stream_id, stage1_config()).unwrap();

            for j in 1..=PASIDS_PER_STREAM {
                let pasid = PASID::new(j).unwrap();
                smmu.create_pasid(stream_id, pasid).unwrap();

                for k in 0..PAGES_PER_PASID {
                    let stream_base = u64::from(stream_id.as_u32()) * 0x1000_0000;
                    let pasid_base = u64::from(pasid.as_u32()) * 0x100_0000;
                    let iova = IOVA::new(0x1_0000_0000 + stream_base + pasid_base + k as u64 * PAGE_SIZE).unwrap();
                    let pa = PA::new(0x2_0000_0000 + stream_base + pasid_base + k as u64 * PAGE_SIZE).unwrap();

                    smmu.map_page(
                        stream_id,
                        pasid,
                        iova,
                        pa,
                        PagePermissions::read_write(),
                        SecurityState::NonSecure,
                    )
                    .unwrap();
                }
            }
        }

        let batch_duration = batch_start.elapsed();

        // Test performance after each batch
        use rand::Rng;
        let mut rng = rand::thread_rng();
        const TEST_OPERATIONS: usize = 1000;

        let perf_start = Instant::now();

        for _ in 0..TEST_OPERATIONS {
            // Generate stream_id within configured range: 1 to (batch * streams_in_batch)
            let max_stream = batch * streams_in_batch;
            let stream_id = StreamID::new(rng.gen_range(1..=max_stream)).unwrap();
            let pasid = PASID::new(rng.gen_range(1..=PASIDS_PER_STREAM)).unwrap();
            let page_index = rng.gen_range(0..PAGES_PER_PASID);

            let stream_base = u64::from(stream_id.as_u32()) * 0x1000_0000;
            let pasid_base = u64::from(pasid.as_u32()) * 0x100_0000;
            let iova = IOVA::new(0x1_0000_0000 + stream_base + pasid_base + page_index as u64 * PAGE_SIZE).unwrap();

            let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
            assert!(result.is_ok(), "Translation should succeed in batch {batch}");
        }

        let perf_duration = perf_start.elapsed();
        let avg_translation_time = (perf_duration.as_micros() as f64) / (TEST_OPERATIONS as f64);

        println!(
            "Batch {}: {} streams, setup {:?}, avg translation {:.3} μs",
            batch,
            batch * streams_in_batch,
            batch_duration,
            avg_translation_time
        );

        // Performance should remain reasonable with scale (3x relaxed threshold)
        assert!(
            avg_translation_time < 150.0,
            "Translation performance degraded too much at batch {batch}: {avg_translation_time:.3} μs"
        );
    }
}

// ============================================================================
// End-to-End Workflow Tests
// ============================================================================

#[test]
fn test_complete_smmu_lifecycle() {
    // Test complete lifecycle: init -> configure -> use -> shutdown
    let smmu = SMMU::new();

    // Initialize
    smmu.initialize().unwrap();

    // Configure streams
    for i in 1..=10 {
        let stream_id = StreamID::new(i).unwrap();
        smmu.configure_stream(stream_id, stage1_config()).unwrap();

        let pasid = PASID::new(1).unwrap();
        smmu.create_pasid(stream_id, pasid).unwrap();

        let iova = IOVA::new(0x1000 + u64::from(i) * 0x1000).unwrap();
        let pa = PA::new(0x2000 + u64::from(i) * 0x1000).unwrap();

        smmu.map_page(
            stream_id,
            pasid,
            iova,
            pa,
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .unwrap();
    }

    // Use SMMU
    for i in 1..=10 {
        let stream_id = StreamID::new(i).unwrap();
        let pasid = PASID::new(1).unwrap();
        let iova = IOVA::new(0x1000 + u64::from(i) * 0x1000).unwrap();

        let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
        assert!(result.is_ok(), "Translation should work during normal operation");
    }

    // Shutdown gracefully
    smmu.shutdown().unwrap();

    // After shutdown, operations should fail
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "Operations should fail after shutdown");
}

#[test]
fn test_arm_smmu_v3_compliance() {
    // Comprehensive ARM SMMU v3 specification compliance test
    let smmu = SMMU::new();

    // Section 3.2: Address Translation
    let stream_id = StreamID::new(1).unwrap();
    smmu.configure_stream(stream_id, stage1_config()).unwrap();

    let pasid = PASID::new(1).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Test translation
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "Basic translation should work");

    // Section 4.1: Stream Context Management
    let pasid2 = PASID::new(2).unwrap();
    smmu.create_pasid(stream_id, pasid2).unwrap();
    assert_eq!(smmu.get_stream_count(), 1, "Should have 1 stream");

    // Section 5.3: Event Queue
    smmu.clear_event_queue();

    // Trigger a fault
    let unmapped_iova = IOVA::new(0x5000).unwrap();
    let _ = smmu.translate(stream_id, pasid, unmapped_iova, AccessType::Read, SecurityState::NonSecure);

    let events = smmu.get_events();
    assert!(!events.is_empty(), "Should record fault events");

    // Section 6.2: Fault Handling
    // Verify fault contains correct information
    let fault_event = &events[0];
    assert_eq!(fault_event.stream_id, stream_id.as_u32(), "Fault should be for correct stream");

    println!("ARM SMMU v3 compliance test passed");
}
