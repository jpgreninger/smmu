#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]

//! Concurrency tests using loom and standard threading
//!
//! Tests thread safety, race condition detection, and concurrent access
//! patterns using both loom for exhaustive checking and standard threads
//! for stress testing.

#![cfg(all(test, loom))]

use loom::sync::{Arc, Mutex};
use loom::thread;
use smmu::address_space::AddressSpace;
use smmu::stream_context::StreamContext;
use smmu::types::{AccessType, PagePermissions, SecurityState, IOVA, PA, PASID};
use smmu::SMMU;

// ============================================================================
// Loom-based Exhaustive Concurrency Tests
// ============================================================================

#[test]
#[cfg(loom)]
fn loom_concurrent_address_space_map() {
    loom::model(|| {
        let addr_space = Arc::new(Mutex::new(AddressSpace::new()));
        let iova1 = IOVA::new(0x1000).unwrap();
        let iova2 = IOVA::new(0x2000).unwrap();
        let pa = PA::new(0x3000).unwrap();

        let addr_space1 = Arc::clone(&addr_space);
        let addr_space2 = Arc::clone(&addr_space);

        let t1 = thread::spawn(move || {
            let mut as_guard = addr_space1.lock().unwrap();
            as_guard
                .map_page(iova1, pa, PagePermissions::read_only(), SecurityState::NonSecure)
                .unwrap();
        });

        let t2 = thread::spawn(move || {
            let mut as_guard = addr_space2.lock().unwrap();
            as_guard
                .map_page(iova2, pa, PagePermissions::read_write(), SecurityState::NonSecure)
                .unwrap();
        });

        t1.join().unwrap();
        t2.join().unwrap();

        let as_guard = addr_space.lock().unwrap();
        assert_eq!(as_guard.get_page_count().unwrap(), 2);
    });
}

#[test]
#[cfg(loom)]
fn loom_concurrent_stream_context_pasid_creation() {
    loom::model(|| {
        let stream_context = Arc::new(StreamContext::new());
        let pasid1 = PASID::new(1).unwrap();
        let pasid2 = PASID::new(2).unwrap();

        let sc1 = Arc::clone(&stream_context);
        let sc2 = Arc::clone(&stream_context);

        let t1 = thread::spawn(move || {
            sc1.create_pasid(pasid1).unwrap();
        });

        let t2 = thread::spawn(move || {
            sc2.create_pasid(pasid2).unwrap();
        });

        t1.join().unwrap();
        t2.join().unwrap();

        assert_eq!(stream_context.pasid_count(), 2);
    });
}

#[test]
#[cfg(loom)]
fn loom_concurrent_translation() {
    loom::model(|| {
        let stream_context = Arc::new(StreamContext::new());
        let pasid = PASID::new(1).unwrap();
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();

        stream_context.create_pasid(pasid).unwrap();
        stream_context
            .map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();

        let sc1 = Arc::clone(&stream_context);
        let sc2 = Arc::clone(&stream_context);

        let t1 = thread::spawn(move || {
            let result = sc1.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
            assert!(result.is_ok());
        });

        let t2 = thread::spawn(move || {
            let result = sc2.translate(pasid, iova, AccessType::Write, SecurityState::NonSecure);
            assert!(result.is_ok());
        });

        t1.join().unwrap();
        t2.join().unwrap();
    });
}

// ============================================================================
// Standard Thread-based Stress Tests (non-loom)
// ============================================================================

#[test]
#[cfg(not(loom))]
fn stress_concurrent_smmu_operations() {
    use std::sync::Arc;
    use std::thread;

    let smmu = Arc::new(SMMU::new());
    let mut handles = vec![];

    // Spawn 10 threads, each creating and using a stream
    for i in 0..10 {
        let smmu_clone = Arc::clone(&smmu);
        let handle = thread::spawn(move || {
            use smmu::types::StreamID;

            let stream_id = StreamID::new(i).unwrap();
            let pasid = PASID::new(1).unwrap();
            let iova = IOVA::new(0x1000).unwrap();
            let pa = PA::new(0x2000 + u64::from(i) * 0x1000).unwrap();

            smmu_clone.configure_stream(stream_id, Default::default()).unwrap();
            smmu_clone.create_pasid(stream_id, pasid).unwrap();
            smmu_clone
                .map_page(
                    stream_id,
                    pasid,
                    iova,
                    pa,
                    PagePermissions::read_write(),
                    SecurityState::NonSecure,
                )
                .unwrap();

            let result = smmu_clone.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
            assert!(result.is_ok());
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(smmu.get_stream_count(), 10);
}

#[test]
#[cfg(not(loom))]
fn stress_concurrent_address_space_operations() {
    use std::sync::{Arc, RwLock};
    use std::thread;

    let addr_space = Arc::new(AddressSpace::new());
    let mut handles = vec![];

    // Spawn 100 threads mapping different pages
    for i in 0..100 {
        let addr_space_clone = Arc::clone(&addr_space);
        let handle = thread::spawn(move || {
            let iova = IOVA::new(0x1000 + i * 0x1000).unwrap();
            let pa = PA::new(0x2000 + i * 0x1000).unwrap();

            let mut as_guard = addr_space_clone.write().unwrap();
            as_guard
                .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
                .unwrap();
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let as_guard = addr_space.read().unwrap();
    assert_eq!(as_guard.get_page_count().unwrap(), 100);
}

#[test]
#[cfg(not(loom))]
fn stress_concurrent_stream_context_operations() {
    use std::sync::Arc;
    use std::thread;

    let stream_context = Arc::new(StreamContext::new());
    let mut handles = vec![];

    // Spawn 50 threads creating different PASIDs
    for i in 0..50 {
        let sc_clone = Arc::clone(&stream_context);
        let handle = thread::spawn(move || {
            let pasid = PASID::new(i).unwrap();
            sc_clone.create_pasid(pasid).unwrap();
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(stream_context.pasid_count(), 50);
}

#[test]
#[cfg(not(loom))]
fn stress_concurrent_translations() {
    use std::sync::Arc;
    use std::thread;

    let stream_context = Arc::new(StreamContext::new());
    let pasid = PASID::new(1).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    stream_context.create_pasid(pasid).unwrap();
    stream_context
        .map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let mut handles = vec![];

    // Spawn 100 threads performing translations
    for _ in 0..100 {
        let sc_clone = Arc::clone(&stream_context);
        let handle = thread::spawn(move || {
            let result = sc_clone.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
            assert!(result.is_ok());
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
#[cfg(not(loom))]
fn stress_mixed_operations() {
    use std::sync::Arc;
    use std::thread;

    let smmu = Arc::new(SMMU::new());
    let mut handles = vec![];

    // Configure initial streams
    for i in 0..5 {
        let stream_id = smmu::types::StreamID::new(i).unwrap();
        smmu.configure_stream(stream_id, Default::default()).unwrap();
    }

    // Spawn threads performing mixed operations
    for i in 0..20 {
        let smmu_clone = Arc::clone(&smmu);
        let handle = thread::spawn(move || {
            use smmu::types::StreamID;

            let stream_id = StreamID::new(i % 5).unwrap();
            let pasid = PASID::new((i / 5) as u32).unwrap();
            let iova = IOVA::new(0x1000 + u64::from(i) * 0x1000).unwrap();
            let pa = PA::new(0x2000 + u64::from(i) * 0x1000).unwrap();

            // Create PASID if not exists
            let _ = smmu_clone.create_pasid(stream_id, pasid);

            // Map page
            let _ = smmu_clone.map_page(
                stream_id,
                pasid,
                iova,
                pa,
                PagePermissions::read_write(),
                SecurityState::NonSecure,
            );

            // Translate
            let _ = smmu_clone.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
#[cfg(not(loom))]
fn stress_high_contention() {
    use std::sync::{Arc, RwLock};
    use std::thread;

    let addr_space = Arc::new(AddressSpace::new());
    let iova = IOVA::new(0x1000).unwrap();
    let mut handles = vec![];

    // All threads map/unmap the same page (high contention)
    for i in 0..50 {
        let addr_space_clone = Arc::clone(&addr_space);
        let handle = thread::spawn(move || {
            let pa = PA::new(0x2000 + (i % 10) as u64 * 0x1000).unwrap();

            for _ in 0..10 {
                let mut as_guard = addr_space_clone.write().unwrap();
                as_guard
                    .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
                    .unwrap();
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let as_guard = addr_space.read().unwrap();
    assert_eq!(as_guard.get_page_count().unwrap(), 1);
}

#[test]
#[cfg(not(loom))]
fn stress_reader_writer_pattern() {
    use std::sync::{Arc, RwLock};
    use std::thread;

    let addr_space = Arc::new(AddressSpace::new());
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    // Initial mapping (no lock needed - AddressSpace is lock-free)
    {
        addr_space
            .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();
    }

    let mut handles = vec![];

    // Spawn many reader threads
    for _ in 0..100 {
        let addr_space_clone = Arc::clone(&addr_space);
        let handle = thread::spawn(move || {
            let as_guard = addr_space.read().unwrap();
            let _ = as_guard.translate_page(iova, AccessType::Read, SecurityState::NonSecure);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
#[cfg(not(loom))]
fn stress_concurrent_fault_generation() {
    use std::sync::Arc;
    use std::thread;

    let smmu = Arc::new(SMMU::new());
    let stream_id = smmu::types::StreamID::new(1).unwrap();
    let pasid = PASID::new(1).unwrap();

    smmu.configure_stream(stream_id, Default::default()).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    let mut handles = vec![];

    // Spawn threads generating faults
    for i in 0..100 {
        let smmu_clone = Arc::clone(&smmu);
        let handle = thread::spawn(move || {
            let iova = IOVA::new(0x1000 + i * 0x1000).unwrap();
            let _ = smmu_clone.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Should have recorded faults
    let faults = smmu.get_faults();
    assert!(!faults.is_empty());
}
