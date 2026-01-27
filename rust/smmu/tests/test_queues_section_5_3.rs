//! ARM SMMU v3 Event and Command Processing Tests - Section 5.3
//!
//! Comprehensive TDD test suite for queue management and command processing.
//! These tests are written BEFORE implementation to drive the design and ensure
//! complete functionality coverage per Section 5.3 of TASKS-RUST.md.
//!
//! # Test Organization
//!
//! 1. **Event Queue Tests** (5.3.1) - Event queue operations
//!    - Event queue initialization with proper size limits
//!    - Event submission (translation faults, permission faults, etc.)
//!    - Event retrieval (FIFO ordering)
//!    - Event queue overflow handling
//!    - Event queue clearing
//!    - Concurrent event submission (thread-safe)
//!    - Event filtering by type/StreamID/PASID
//!    - Event queue size limits enforcement
//!
//! 2. **Command Queue Tests** (5.3.2) - Command queue operations
//!    - Command queue initialization
//!    - Command submission (all CommandType variants)
//!    - Command processing (FIFO order)
//!    - Command queue overflow handling
//!    - Command validation (invalid commands rejected)
//!    - Command queue clearing
//!    - Concurrent command submission
//!    - Command completion events
//!
//! 3. **PRI Queue Tests** (5.3.3) - Page Request Interface queue
//!    - PRI queue initialization
//!    - Page request submission
//!    - Page request processing (priority ordering)
//!    - PRI queue overflow handling
//!    - PRI request lifecycle management
//!    - Concurrent PRI submission
//!    - Last request flag handling
//!
//! 4. **Cache Invalidation Command Tests** (5.3.4)
//!    - TLBI_NH_ALL command processing
//!    - TLBI_EL2_ALL command processing
//!    - TLBI_S12_VMALL command processing
//!    - ATC_INV command processing
//!    - Selective invalidation (StreamID/PASID)
//!    - Address range invalidation
//!    - Batch invalidation operations
//!    - Invalidation completion events
//!
//! 5. **Queue Integration Tests** (5.3.5)
//!    - Multiple queues operating concurrently
//!    - Event generation from command processing
//!    - Queue overflow cascading behavior
//!    - Queue statistics and monitoring
//!    - Queue reset operations
//!    - Memory efficiency under load
//!
//! # ARM SMMU v3 Compliance
//!
//! - Event queue management per ARM specification Section 6.3
//! - Command queue processing per ARM specification Section 6.4
//! - PRI queue handling per ARM specification Section 7
//! - Cache invalidation commands per ARM specification Section 9
//!
//! Copyright (c) 2024 John Greninger

#![cfg(test)]

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventEntry, EventType, PRIEntry, SecurityState,
    StreamID, QueueConfig, IOVA, PASID,
};
use smmu::SMMU;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

// ============================================================================
// Section 5.3.1: Event Queue Tests
// ============================================================================

/// Test: Event queue initialization with default configuration
///
/// ARM SMMU v3 spec: Event queue must support at least 512 entries
#[test]
fn test_section_5_3_1_event_queue_initialization() {
    let smmu = SMMU::new();

    // Verify event queue is empty on initialization
    assert_eq!(smmu.get_event_queue_size(), 0, "Event queue should be empty on init");
    assert!(!smmu.has_events(), "Should have no events initially");

    // Verify default queue capacity
    let config = smmu.get_config();
    assert!(
        config.queue_config().event_queue_size() >= 512,
        "Event queue must support at least 512 entries per ARM spec"
    );
}

/// Test: Event queue submission - translation fault event
///
/// ARM SMMU v3 spec: Translation fault events must include full context
#[test]
fn test_section_5_3_1_submit_translation_fault_event() {
    let smmu = SMMU::new();

    let event = EventEntry {
        event_type: EventType::TranslationFault,
        stream_id: 100,
        pasid: 5,
        address: 0x1000,
        security_state: SecurityState::NonSecure,
        error_code: 0,
        timestamp: 12345,
    };

    // Submit event
    let result = smmu.submit_event(event);
    assert!(result.is_ok(), "Event submission should succeed");

    // Verify event was queued
    assert!(smmu.has_events(), "Should have events after submission");
    assert_eq!(smmu.get_event_queue_size(), 1, "Should have 1 event");
}

/// Test: Event queue submission - permission fault event
///
/// ARM SMMU v3 spec: Permission fault events must capture access type
#[test]
fn test_section_5_3_1_submit_permission_fault_event() {
    let smmu = SMMU::new();

    let event = EventEntry {
        event_type: EventType::PermissionFault,
        stream_id: 200,
        pasid: 0,
        address: 0x2000,
        security_state: SecurityState::Secure,
        error_code: 1,
        timestamp: 12346,
    };

    let result = smmu.submit_event(event);
    assert!(result.is_ok(), "Permission fault event submission should succeed");
    assert_eq!(smmu.get_event_queue_size(), 1);
}

/// Test: Event queue retrieval - FIFO ordering
///
/// ARM SMMU v3 spec: Events must be retrieved in submission order (FIFO)
#[test]
fn test_section_5_3_1_event_queue_fifo_ordering() {
    let smmu = SMMU::new();

    // Submit multiple events
    let events = vec![
        EventEntry {
            event_type: EventType::TranslationFault,
            stream_id: 1,
            pasid: 0,
            address: 0x1000,
            security_state: SecurityState::NonSecure,
            error_code: 0,
            timestamp: 100,
        },
        EventEntry {
            event_type: EventType::PermissionFault,
            stream_id: 2,
            pasid: 0,
            address: 0x2000,
            security_state: SecurityState::NonSecure,
            error_code: 0,
            timestamp: 101,
        },
        EventEntry {
            event_type: EventType::ConfigurationError,
            stream_id: 3,
            pasid: 0,
            address: 0x3000,
            security_state: SecurityState::NonSecure,
            error_code: 0,
            timestamp: 102,
        },
    ];

    for event in &events {
        smmu.submit_event(*event).unwrap();
    }

    // Retrieve events
    let retrieved = smmu.get_events();
    assert_eq!(retrieved.len(), 3, "Should retrieve all 3 events");

    // Verify FIFO ordering
    assert_eq!(retrieved[0].stream_id, 1, "First event should have StreamID 1");
    assert_eq!(retrieved[1].stream_id, 2, "Second event should have StreamID 2");
    assert_eq!(retrieved[2].stream_id, 3, "Third event should have StreamID 3");
}

/// Test: Event queue overflow handling
///
/// ARM SMMU v3 spec: Queue overflow must be handled gracefully
#[test]
fn test_section_5_3_1_event_queue_overflow() {
    // Create SMMU with small event queue for testing overflow
    let config = QueueConfig::default().with_event_queue_size(4);
    let smmu = SMMU::with_config(config.into());

    let event_template = EventEntry {
        event_type: EventType::TranslationFault,
        stream_id: 1,
        pasid: 0,
        address: 0x1000,
        security_state: SecurityState::NonSecure,
        error_code: 0,
        timestamp: 100,
    };

    // Fill the queue to capacity
    for i in 0..4 {
        let mut event = event_template;
        event.stream_id = i;
        let result = smmu.submit_event(event);
        assert!(result.is_ok(), "Should accept events up to capacity");
    }

    // Attempt to exceed capacity
    let mut overflow_event = event_template;
    overflow_event.stream_id = 99;
    let result = smmu.submit_event(overflow_event);

    // ARM spec: Overflow behavior - either reject or overwrite oldest
    // We test for graceful handling (no panic, returns error)
    assert!(
        result.is_err() || smmu.get_event_queue_size() <= 4,
        "Queue overflow should be handled gracefully"
    );
}

/// Test: Event queue clearing
///
/// ARM SMMU v3 spec: Queue clearing must remove all events atomically
#[test]
fn test_section_5_3_1_event_queue_clear() {
    let smmu = SMMU::new();

    // Submit multiple events
    for i in 0..10 {
        let event = EventEntry {
            event_type: EventType::TranslationFault,
            stream_id: i,
            pasid: 0,
            address: 0x1000,
            security_state: SecurityState::NonSecure,
            error_code: 0,
            timestamp: 100 + i as u64,
        };
        smmu.submit_event(event).unwrap();
    }

    assert_eq!(smmu.get_event_queue_size(), 10, "Should have 10 events");

    // Clear the queue
    smmu.clear_event_queue();

    // Verify queue is empty
    assert_eq!(smmu.get_event_queue_size(), 0, "Queue should be empty after clear");
    assert!(!smmu.has_events(), "has_events() should return false");
    assert_eq!(smmu.get_events().len(), 0, "get_events() should return empty vector");
}

/// Test: Concurrent event submission (thread safety)
///
/// ARM SMMU v3 spec: Event queue must support concurrent producer access
#[test]
fn test_section_5_3_1_concurrent_event_submission() {
    let smmu = Arc::new(SMMU::new());
    let num_threads = 10;
    let events_per_thread = 50;
    let barrier = Arc::new(Barrier::new(num_threads));

    let mut handles = vec![];

    for thread_id in 0..num_threads {
        let smmu_clone = Arc::clone(&smmu);
        let barrier_clone = Arc::clone(&barrier);

        let handle = thread::spawn(move || {
            // Wait for all threads to be ready
            barrier_clone.wait();

            // Submit events from this thread
            for i in 0..events_per_thread {
                let event = EventEntry {
                    event_type: EventType::TranslationFault,
                    stream_id: (thread_id * 1000 + i) as u32,
                    pasid: thread_id as u32,
                    address: 0x1000 * (i + 1) as u64,
                    security_state: SecurityState::NonSecure,
                    error_code: 0,
                    timestamp: (thread_id * 1000 + i) as u64,
                };

                smmu_clone.submit_event(event).unwrap();
            }
        });

        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify total event count
    let total_events = num_threads * events_per_thread;
    assert_eq!(
        smmu.get_event_queue_size(),
        total_events as u64,
        "Should have all events from all threads"
    );
}

/// Test: Event filtering by type
///
/// ARM SMMU v3 spec: Software should be able to filter events by type
#[test]
fn test_section_5_3_1_event_filtering_by_type() {
    let smmu = SMMU::new();

    // Submit mixed event types
    let event_types = vec![
        EventType::TranslationFault,
        EventType::PermissionFault,
        EventType::TranslationFault,
        EventType::ConfigurationError,
        EventType::TranslationFault,
    ];

    for (i, event_type) in event_types.iter().enumerate() {
        let event = EventEntry {
            event_type: *event_type,
            stream_id: i as u32,
            pasid: 0,
            address: 0x1000,
            security_state: SecurityState::NonSecure,
            error_code: 0,
            timestamp: i as u64,
        };
        smmu.submit_event(event).unwrap();
    }

    // Filter translation faults
    let translation_faults = smmu.get_events_by_type(EventType::TranslationFault);
    assert_eq!(
        translation_faults.len(),
        3,
        "Should have 3 translation fault events"
    );

    // Filter permission faults
    let permission_faults = smmu.get_events_by_type(EventType::PermissionFault);
    assert_eq!(
        permission_faults.len(),
        1,
        "Should have 1 permission fault event"
    );
}

/// Test: Event filtering by StreamID
///
/// ARM SMMU v3 spec: Software should be able to query events for specific stream
#[test]
fn test_section_5_3_1_event_filtering_by_stream() {
    let smmu = SMMU::new();

    // Submit events for different streams
    for i in 0..10 {
        let event = EventEntry {
            event_type: EventType::TranslationFault,
            stream_id: if i < 5 { 100 } else { 200 },
            pasid: 0,
            address: 0x1000,
            security_state: SecurityState::NonSecure,
            error_code: 0,
            timestamp: i,
        };
        smmu.submit_event(event).unwrap();
    }

    // Filter by StreamID
    let stream_100_events = smmu.get_events_by_stream(100);
    assert_eq!(
        stream_100_events.len(),
        5,
        "Should have 5 events for StreamID 100"
    );

    let stream_200_events = smmu.get_events_by_stream(200);
    assert_eq!(
        stream_200_events.len(),
        5,
        "Should have 5 events for StreamID 200"
    );
}

// ============================================================================
// Section 5.3.2: Command Queue Tests
// ============================================================================

/// Test: Command queue initialization
///
/// ARM SMMU v3 spec: Command queue must support at least 256 entries
#[test]
fn test_section_5_3_2_command_queue_initialization() {
    let smmu = SMMU::new();

    // Verify command queue is empty
    assert_eq!(smmu.get_command_queue_size(), 0, "Command queue should be empty");
    assert!(!smmu.is_command_queue_full(), "Command queue should not be full");

    // Verify default capacity
    let config = smmu.get_config();
    assert!(
        config.queue_config().command_queue_size() >= 256,
        "Command queue must support at least 256 entries per ARM spec"
    );
}

/// Test: Command submission - SYNC command
///
/// ARM SMMU v3 spec: SYNC command creates synchronization barrier
#[test]
fn test_section_5_3_2_submit_sync_command() {
    let smmu = SMMU::new();

    let command = CommandEntry {
        cmd_type: CommandType::Sync,
        stream_id: 0,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 100,
    };

    let result = smmu.submit_command(command);
    assert!(result.is_ok(), "SYNC command submission should succeed");
    assert_eq!(smmu.get_command_queue_size(), 1, "Should have 1 command queued");
}

/// Test: Command submission - all command types
///
/// ARM SMMU v3 spec: All command types must be supported
#[test]
fn test_section_5_3_2_submit_all_command_types() {
    let smmu = SMMU::new();

    let command_types = vec![
        CommandType::PrefetchConfig,
        CommandType::PrefetchAddr,
        CommandType::CfgiSte,
        CommandType::CfgiAll,
        CommandType::TlbiNhAll,
        CommandType::TlbiEl2All,
        CommandType::TlbiS12Vmall,
        CommandType::AtcInv,
        CommandType::PriResp,
        CommandType::Resume,
        CommandType::Sync,
    ];

    for (i, cmd_type) in command_types.iter().enumerate() {
        let command = CommandEntry {
            cmd_type: *cmd_type,
            stream_id: i as u32,
            pasid: 0,
            start_address: 0x1000,
            end_address: 0x2000,
            flags: 0,
            timestamp: i as u64,
        };

        let result = smmu.submit_command(command);
        assert!(
            result.is_ok(),
            "Command type {:?} submission should succeed",
            cmd_type
        );
    }

    assert_eq!(
        smmu.get_command_queue_size(),
        command_types.len() as u64,
        "All commands should be queued"
    );
}

/// Test: Command processing - FIFO order
///
/// ARM SMMU v3 spec: Commands must be processed in submission order
#[test]
fn test_section_5_3_2_command_processing_fifo() {
    let smmu = SMMU::new();

    // Submit commands with different StreamIDs
    for i in 0..5 {
        let command = CommandEntry {
            cmd_type: CommandType::Sync,
            stream_id: i,
            pasid: 0,
            start_address: 0,
            end_address: 0,
            flags: 0,
            timestamp: i as u64,
        };
        smmu.submit_command(command).unwrap();
    }

    // Process commands
    smmu.process_command_queue();

    // Verify processing order via events (SYNC commands generate completion events)
    let events = smmu.get_events_by_type(EventType::CommandSyncCompletion);
    assert_eq!(events.len(), 5, "Should have 5 completion events");

    // Verify FIFO order
    for (i, event) in events.iter().enumerate() {
        assert_eq!(
            event.stream_id, i as u32,
            "Event {} should have StreamID {}",
            i, i
        );
    }
}

/// Test: Command queue overflow handling
///
/// ARM SMMU v3 spec: Command queue full condition must be detectable
#[test]
fn test_section_5_3_2_command_queue_overflow() {
    // Create SMMU with small command queue
    let config = QueueConfig::default().with_command_queue_size(4);
    let smmu = SMMU::with_config(config.into());

    let command_template = CommandEntry {
        cmd_type: CommandType::Sync,
        stream_id: 0,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 100,
    };

    // Fill queue to capacity
    for _ in 0..4 {
        let result = smmu.submit_command(command_template);
        assert!(result.is_ok(), "Should accept commands up to capacity");
    }

    // Check queue full status
    assert!(smmu.is_command_queue_full(), "Queue should be full");

    // Attempt to exceed capacity
    let result = smmu.submit_command(command_template);
    assert!(result.is_err(), "Should reject command when queue is full");
}

/// Test: Command validation - invalid command parameters
///
/// ARM SMMU v3 spec: Invalid commands must be rejected
#[test]
fn test_section_5_3_2_command_validation() {
    let smmu = SMMU::new();

    // Invalid: Address range with start > end
    let invalid_command = CommandEntry {
        cmd_type: CommandType::AtcInv,
        stream_id: 100,
        pasid: 0,
        start_address: 0x2000,
        end_address: 0x1000, // Invalid: end < start
        flags: 0,
        timestamp: 100,
    };

    let result = smmu.submit_command(invalid_command);
    assert!(
        result.is_err(),
        "Should reject command with invalid address range"
    );
}

/// Test: Command queue clearing
///
/// ARM SMMU v3 spec: Command queue can be cleared (e.g., during reset)
#[test]
fn test_section_5_3_2_command_queue_clear() {
    let smmu = SMMU::new();

    // Submit multiple commands
    for i in 0..10 {
        let command = CommandEntry {
            cmd_type: CommandType::Sync,
            stream_id: i,
            pasid: 0,
            start_address: 0,
            end_address: 0,
            flags: 0,
            timestamp: i as u64,
        };
        smmu.submit_command(command).unwrap();
    }

    assert_eq!(smmu.get_command_queue_size(), 10, "Should have 10 commands");

    // Clear queue
    smmu.clear_command_queue();

    // Verify queue is empty
    assert_eq!(smmu.get_command_queue_size(), 0, "Queue should be empty");
    assert!(!smmu.is_command_queue_full(), "Queue should not be full");
}

/// Test: Concurrent command submission
///
/// ARM SMMU v3 spec: Command queue must support concurrent submission
#[test]
fn test_section_5_3_2_concurrent_command_submission() {
    let smmu = Arc::new(SMMU::new());
    let num_threads = 8;
    let commands_per_thread = 25;
    let barrier = Arc::new(Barrier::new(num_threads));

    let mut handles = vec![];

    for thread_id in 0..num_threads {
        let smmu_clone = Arc::clone(&smmu);
        let barrier_clone = Arc::clone(&barrier);

        let handle = thread::spawn(move || {
            barrier_clone.wait();

            for i in 0..commands_per_thread {
                let command = CommandEntry {
                    cmd_type: CommandType::Sync,
                    stream_id: (thread_id * 100 + i) as u32,
                    pasid: thread_id as u32,
                    start_address: 0x1000 * (i + 1) as u64,
                    end_address: 0x2000 * (i + 1) as u64,
                    flags: 0,
                    timestamp: (thread_id * 1000 + i) as u64,
                };

                smmu_clone.submit_command(command).unwrap();
            }
        });

        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify total command count
    let total_commands = num_threads * commands_per_thread;
    assert_eq!(
        smmu.get_command_queue_size(),
        total_commands as u64,
        "Should have all commands from all threads"
    );
}

// ============================================================================
// Section 5.3.3: PRI Queue Tests
// ============================================================================

/// Test: PRI queue initialization
///
/// ARM SMMU v3 spec: PRI queue must support at least 128 entries
#[test]
fn test_section_5_3_3_pri_queue_initialization() {
    let smmu = SMMU::new();

    // Verify PRI queue is empty
    assert_eq!(smmu.get_pri_queue_size(), 0, "PRI queue should be empty");

    // Verify default capacity
    let config = smmu.get_config();
    assert!(
        config.queue_config().pri_queue_size() >= 128,
        "PRI queue must support at least 128 entries per ARM spec"
    );
}

/// Test: PRI request submission
///
/// ARM SMMU v3 spec: Page request must include address and access type
#[test]
fn test_section_5_3_3_submit_page_request() {
    let smmu = SMMU::new();

    let page_request = PRIEntry {
        stream_id: 100,
        pasid: 5,
        requested_address: 0x1000,
        access_type: AccessType::Read,
        is_last_request: false,
        timestamp: 1000,
    };

    let result = smmu.submit_page_request(page_request);
    assert!(result.is_ok(), "Page request submission should succeed");
    assert_eq!(smmu.get_pri_queue_size(), 1, "Should have 1 page request");
}

/// Test: PRI request processing
///
/// ARM SMMU v3 spec: Page requests should be processed in priority order
#[test]
fn test_section_5_3_3_pri_processing() {
    let smmu = SMMU::new();

    // Submit multiple page requests
    for i in 0..5 {
        let request = PRIEntry {
            stream_id: i,
            pasid: 0,
            requested_address: 0x1000 * (i as u64 + 1),
            access_type: AccessType::Read,
            is_last_request: false,
            timestamp: i as u64,
        };
        smmu.submit_page_request(request).unwrap();
    }

    // Process PRI queue
    smmu.process_pri_queue();

    // Verify requests were processed (queue should be empty or processed)
    let remaining = smmu.get_pri_queue_size();
    assert!(
        remaining <= 5,
        "PRI queue should be processed or in progress"
    );

    // Verify PRI response events were generated
    let pri_events = smmu.get_events_by_type(EventType::PriPageRequest);
    assert!(
        !pri_events.is_empty(),
        "Should generate PRI page request events"
    );
}

/// Test: PRI queue overflow handling
///
/// ARM SMMU v3 spec: PRI queue overflow must be handled gracefully
#[test]
fn test_section_5_3_3_pri_queue_overflow() {
    // Create SMMU with small PRI queue
    let config = QueueConfig::default().with_pri_queue_size(4);
    let smmu = SMMU::with_config(config.into());

    let request_template = PRIEntry {
        stream_id: 100,
        pasid: 0,
        requested_address: 0x1000,
        access_type: AccessType::Read,
        is_last_request: false,
        timestamp: 100,
    };

    // Fill queue to capacity
    for _ in 0..4 {
        let result = smmu.submit_page_request(request_template);
        assert!(result.is_ok(), "Should accept requests up to capacity");
    }

    // Attempt overflow
    let result = smmu.submit_page_request(request_template);
    assert!(
        result.is_err() || smmu.get_pri_queue_size() <= 4,
        "PRI queue overflow should be handled"
    );
}

/// Test: Last request flag handling
///
/// ARM SMMU v3 spec: Last request flag indicates completion of group
#[test]
fn test_section_5_3_3_last_request_flag() {
    let smmu = SMMU::new();

    // Submit sequence of requests with last request flag
    for i in 0..3 {
        let request = PRIEntry {
            stream_id: 100,
            pasid: 5,
            requested_address: 0x1000 * (i as u64 + 1),
            access_type: AccessType::Write,
            is_last_request: i == 2, // Last request in group
            timestamp: i as u64,
        };
        smmu.submit_page_request(request).unwrap();
    }

    // Retrieve requests
    let requests = smmu.get_pri_queue();
    assert_eq!(requests.len(), 3, "Should have 3 requests");
    assert!(
        requests[2].is_last_request,
        "Third request should have last_request flag set"
    );
}

/// Test: Concurrent PRI submission
///
/// ARM SMMU v3 spec: PRI queue must support concurrent access
#[test]
fn test_section_5_3_3_concurrent_pri_submission() {
    let smmu = Arc::new(SMMU::new());
    let num_threads = 5;
    let requests_per_thread = 20;
    let barrier = Arc::new(Barrier::new(num_threads));

    let mut handles = vec![];

    for thread_id in 0..num_threads {
        let smmu_clone = Arc::clone(&smmu);
        let barrier_clone = Arc::clone(&barrier);

        let handle = thread::spawn(move || {
            barrier_clone.wait();

            for i in 0..requests_per_thread {
                let request = PRIEntry {
                    stream_id: (thread_id * 100 + i) as u32,
                    pasid: thread_id as u32,
                    requested_address: 0x1000 * (i + 1) as u64,
                    access_type: AccessType::Read,
                    is_last_request: i == requests_per_thread - 1,
                    timestamp: (thread_id * 1000 + i) as u64,
                };

                smmu_clone.submit_page_request(request).unwrap();
            }
        });

        handles.push(handle);
    }

    // Wait for completion
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify total request count
    let total_requests = num_threads * requests_per_thread;
    assert_eq!(
        smmu.get_pri_queue_size(),
        total_requests as u64,
        "Should have all requests from all threads"
    );
}

/// Test: PRI queue clearing
///
/// ARM SMMU v3 spec: PRI queue can be cleared
#[test]
fn test_section_5_3_3_pri_queue_clear() {
    let smmu = SMMU::new();

    // Submit multiple requests
    for i in 0..10 {
        let request = PRIEntry {
            stream_id: i,
            pasid: 0,
            requested_address: 0x1000,
            access_type: AccessType::Read,
            is_last_request: false,
            timestamp: i as u64,
        };
        smmu.submit_page_request(request).unwrap();
    }

    assert_eq!(smmu.get_pri_queue_size(), 10, "Should have 10 requests");

    // Clear queue
    smmu.clear_pri_queue();

    // Verify empty
    assert_eq!(smmu.get_pri_queue_size(), 0, "Queue should be empty");
    assert_eq!(smmu.get_pri_queue().len(), 0, "get_pri_queue() should return empty");
}

// ============================================================================
// Section 5.3.4: Cache Invalidation Command Tests
// ============================================================================

/// Test: TLBI_NH_ALL command processing
///
/// ARM SMMU v3 spec: Non-secure hyp TLB invalidation
#[test]
fn test_section_5_3_4_tlbi_nh_all_command() {
    let smmu = SMMU::new();

    let command = CommandEntry {
        cmd_type: CommandType::TlbiNhAll,
        stream_id: 0,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 100,
    };

    let result = smmu.submit_command(command);
    assert!(result.is_ok(), "TLBI_NH_ALL command should succeed");

    // Process command
    smmu.process_command_queue();

    // Verify TLB was invalidated (check via cache statistics)
    let stats = smmu.get_cache_statistics();
    assert!(
        stats.invalidation_count() > 0,
        "TLB invalidation should increment invalidation count"
    );
}

/// Test: TLBI_EL2_ALL command processing
///
/// ARM SMMU v3 spec: EL2 TLB invalidation
#[test]
fn test_section_5_3_4_tlbi_el2_all_command() {
    let smmu = SMMU::new();

    let command = CommandEntry {
        cmd_type: CommandType::TlbiEl2All,
        stream_id: 0,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 100,
    };

    let result = smmu.submit_command(command);
    assert!(result.is_ok(), "TLBI_EL2_ALL command should succeed");

    smmu.process_command_queue();

    // Verify invalidation occurred
    let stats = smmu.get_cache_statistics();
    assert!(stats.invalidation_count() > 0);
}

/// Test: TLBI_S12_VMALL command processing
///
/// ARM SMMU v3 spec: Stage 1&2 VM TLB invalidation
#[test]
fn test_section_5_3_4_tlbi_s12_vmall_command() {
    let smmu = SMMU::new();

    let command = CommandEntry {
        cmd_type: CommandType::TlbiS12Vmall,
        stream_id: 100,
        pasid: 5,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 100,
    };

    let result = smmu.submit_command(command);
    assert!(result.is_ok(), "TLBI_S12_VMALL command should succeed");

    smmu.process_command_queue();

    // Verify selective invalidation for stream/PASID
    let stats = smmu.get_cache_statistics();
    assert!(stats.invalidation_count() > 0);
}

/// Test: ATC_INV command processing
///
/// ARM SMMU v3 spec: Address Translation Cache invalidation
#[test]
fn test_section_5_3_4_atc_inv_command() {
    let smmu = SMMU::new();

    let command = CommandEntry {
        cmd_type: CommandType::AtcInv,
        stream_id: 100,
        pasid: 5,
        start_address: 0x10000,
        end_address: 0x20000,
        flags: 0,
        timestamp: 100,
    };

    let result = smmu.submit_command(command);
    assert!(result.is_ok(), "ATC_INV command should succeed");

    smmu.process_command_queue();

    // Verify address range invalidation
    let stats = smmu.get_cache_statistics();
    assert!(stats.invalidation_count() > 0);

    // Verify completion event
    let events = smmu.get_events_by_type(EventType::AtcInvalidateCompletion);
    assert!(
        !events.is_empty(),
        "Should generate ATC invalidation completion event"
    );
}

/// Test: Selective invalidation by StreamID/PASID
///
/// ARM SMMU v3 spec: Invalidation can target specific stream/PASID
#[test]
fn test_section_5_3_4_selective_invalidation() {
    let smmu = SMMU::new();

    // Configure multiple streams with caching
    // (Requires stream configuration implementation)
    // This test will be expanded when stream management is implemented

    let command = CommandEntry {
        cmd_type: CommandType::TlbiS12Vmall,
        stream_id: 100, // Target specific stream
        pasid: 5,       // Target specific PASID
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 100,
    };

    let result = smmu.submit_command(command);
    assert!(result.is_ok(), "Selective invalidation should succeed");

    smmu.process_command_queue();

    // Verify only target stream/PASID was invalidated
    // (Full verification requires cache inspection APIs)
}

/// Test: Address range invalidation
///
/// ARM SMMU v3 spec: Invalidation can target address ranges
#[test]
fn test_section_5_3_4_address_range_invalidation() {
    let smmu = SMMU::new();

    let command = CommandEntry {
        cmd_type: CommandType::AtcInv,
        stream_id: 100,
        pasid: 5,
        start_address: 0x10000,
        end_address: 0x1FFFF, // 64KB range
        flags: 0,
        timestamp: 100,
    };

    let result = smmu.submit_command(command);
    assert!(result.is_ok(), "Range invalidation should succeed");

    smmu.process_command_queue();

    // Verify range was invalidated
    let stats = smmu.get_cache_statistics();
    assert!(stats.invalidation_count() > 0);
}

/// Test: Batch invalidation operations
///
/// ARM SMMU v3 spec: Multiple invalidations can be batched for efficiency
#[test]
fn test_section_5_3_4_batch_invalidation() {
    let smmu = SMMU::new();

    // Submit multiple invalidation commands
    let invalidation_commands = vec![
        CommandEntry {
            cmd_type: CommandType::TlbiNhAll,
            stream_id: 0,
            pasid: 0,
            start_address: 0,
            end_address: 0,
            flags: 0,
            timestamp: 100,
        },
        CommandEntry {
            cmd_type: CommandType::TlbiS12Vmall,
            stream_id: 100,
            pasid: 5,
            start_address: 0,
            end_address: 0,
            flags: 0,
            timestamp: 101,
        },
        CommandEntry {
            cmd_type: CommandType::AtcInv,
            stream_id: 200,
            pasid: 10,
            start_address: 0x10000,
            end_address: 0x20000,
            flags: 0,
            timestamp: 102,
        },
    ];

    for command in invalidation_commands {
        smmu.submit_command(command).unwrap();
    }

    // Process all commands in batch
    smmu.process_command_queue();

    // Verify all invalidations occurred
    let stats = smmu.get_cache_statistics();
    assert!(
        stats.invalidation_count() >= 3,
        "Should have at least 3 invalidations"
    );
}

/// Test: Invalidation completion events
///
/// ARM SMMU v3 spec: Invalidation commands generate completion events
#[test]
fn test_section_5_3_4_invalidation_completion_events() {
    let smmu = SMMU::new();

    // Submit invalidation with SYNC
    let atc_inv = CommandEntry {
        cmd_type: CommandType::AtcInv,
        stream_id: 100,
        pasid: 5,
        start_address: 0x10000,
        end_address: 0x20000,
        flags: 0,
        timestamp: 100,
    };

    let sync_cmd = CommandEntry {
        cmd_type: CommandType::Sync,
        stream_id: 100,
        pasid: 5,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 101,
    };

    smmu.submit_command(atc_inv).unwrap();
    smmu.submit_command(sync_cmd).unwrap();

    // Process commands
    smmu.process_command_queue();

    // Verify completion events
    let atc_events = smmu.get_events_by_type(EventType::AtcInvalidateCompletion);
    let sync_events = smmu.get_events_by_type(EventType::CommandSyncCompletion);

    assert!(!atc_events.is_empty(), "Should have ATC completion event");
    assert!(!sync_events.is_empty(), "Should have SYNC completion event");
}

// ============================================================================
// Section 5.3.5: Queue Integration Tests
// ============================================================================

/// Test: Multiple queues operating concurrently
///
/// ARM SMMU v3 spec: All queues must operate independently
#[test]
fn test_section_5_3_5_concurrent_queue_operations() {
    let smmu = Arc::new(SMMU::new());
    let barrier = Arc::new(Barrier::new(3));

    // Thread 1: Submit events
    let smmu1 = Arc::clone(&smmu);
    let barrier1 = Arc::clone(&barrier);
    let handle1 = thread::spawn(move || {
        barrier1.wait();
        for i in 0..50 {
            let event = EventEntry {
                event_type: EventType::TranslationFault,
                stream_id: i,
                pasid: 0,
                address: 0x1000,
                security_state: SecurityState::NonSecure,
                error_code: 0,
                timestamp: i as u64,
            };
            smmu1.submit_event(event).unwrap();
        }
    });

    // Thread 2: Submit commands
    let smmu2 = Arc::clone(&smmu);
    let barrier2 = Arc::clone(&barrier);
    let handle2 = thread::spawn(move || {
        barrier2.wait();
        for i in 0..50 {
            let command = CommandEntry {
                cmd_type: CommandType::Sync,
                stream_id: i,
                pasid: 0,
                start_address: 0,
                end_address: 0,
                flags: 0,
                timestamp: i as u64,
            };
            smmu2.submit_command(command).unwrap();
        }
    });

    // Thread 3: Submit PRI requests
    let smmu3 = Arc::clone(&smmu);
    let barrier3 = Arc::clone(&barrier);
    let handle3 = thread::spawn(move || {
        barrier3.wait();
        for i in 0..50 {
            let request = PRIEntry {
                stream_id: i,
                pasid: 0,
                requested_address: 0x1000,
                access_type: AccessType::Read,
                is_last_request: false,
                timestamp: i as u64,
            };
            smmu3.submit_page_request(request).unwrap();
        }
    });

    // Wait for all threads
    handle1.join().unwrap();
    handle2.join().unwrap();
    handle3.join().unwrap();

    // Verify all queues have entries
    assert_eq!(smmu.get_event_queue_size(), 50, "Should have 50 events");
    assert_eq!(smmu.get_command_queue_size(), 50, "Should have 50 commands");
    assert_eq!(smmu.get_pri_queue_size(), 50, "Should have 50 PRI requests");
}

/// Test: Event generation from command processing
///
/// ARM SMMU v3 spec: Commands generate events upon completion
#[test]
fn test_section_5_3_5_command_generates_events() {
    let smmu = SMMU::new();

    // Submit SYNC command
    let sync_command = CommandEntry {
        cmd_type: CommandType::Sync,
        stream_id: 100,
        pasid: 5,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 100,
    };

    smmu.submit_command(sync_command).unwrap();

    // Initial event queue should be empty
    assert_eq!(smmu.get_event_queue_size(), 0, "No events before processing");

    // Process command
    smmu.process_command_queue();

    // Verify event was generated
    assert!(
        smmu.get_event_queue_size() > 0,
        "Command processing should generate events"
    );

    let events = smmu.get_events_by_type(EventType::CommandSyncCompletion);
    assert_eq!(events.len(), 1, "Should have 1 SYNC completion event");
    assert_eq!(events[0].stream_id, 100, "Event should match command StreamID");
}

/// Test: Queue overflow cascading behavior
///
/// ARM SMMU v3 spec: Overflow in one queue should not affect others
#[test]
fn test_section_5_3_5_queue_overflow_isolation() {
    // Create SMMU with small queues
    let config = QueueConfig::default()
        .with_event_queue_size(4)
        .with_command_queue_size(4)
        .with_pri_queue_size(4);
    let smmu = SMMU::with_config(config.into());

    // Fill event queue to overflow
    for i in 0..5 {
        let event = EventEntry {
            event_type: EventType::TranslationFault,
            stream_id: i,
            pasid: 0,
            address: 0x1000,
            security_state: SecurityState::NonSecure,
            error_code: 0,
            timestamp: i as u64,
        };
        let _ = smmu.submit_event(event); // Ignore overflow error
    }

    // Command queue should still accept commands
    let command = CommandEntry {
        cmd_type: CommandType::Sync,
        stream_id: 0,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 100,
    };

    let result = smmu.submit_command(command);
    assert!(
        result.is_ok(),
        "Command queue should work despite event queue overflow"
    );

    // PRI queue should still accept requests
    let request = PRIEntry {
        stream_id: 0,
        pasid: 0,
        requested_address: 0x1000,
        access_type: AccessType::Read,
        is_last_request: false,
        timestamp: 100,
    };

    let result = smmu.submit_page_request(request);
    assert!(
        result.is_ok(),
        "PRI queue should work despite event queue overflow"
    );
}

/// Test: Queue statistics and monitoring
///
/// ARM SMMU v3 spec: Queue usage statistics should be available
#[test]
fn test_section_5_3_5_queue_statistics() {
    let smmu = SMMU::new();

    // Submit various queue entries
    for i in 0..10 {
        let event = EventEntry {
            event_type: EventType::TranslationFault,
            stream_id: i,
            pasid: 0,
            address: 0x1000,
            security_state: SecurityState::NonSecure,
            error_code: 0,
            timestamp: i as u64,
        };
        smmu.submit_event(event).unwrap();
    }

    for i in 0..5 {
        let command = CommandEntry {
            cmd_type: CommandType::Sync,
            stream_id: i,
            pasid: 0,
            start_address: 0,
            end_address: 0,
            flags: 0,
            timestamp: i as u64,
        };
        smmu.submit_command(command).unwrap();
    }

    // Verify statistics
    let stats = smmu.get_queue_statistics();
    assert_eq!(stats.event_queue_size(), 10, "Event queue size should be 10");
    assert_eq!(stats.command_queue_size(), 5, "Command queue size should be 5");
    assert!(
        stats.event_queue_utilization() > 0.0,
        "Event queue utilization should be > 0"
    );
}

/// Test: Queue reset operations
///
/// ARM SMMU v3 spec: All queues can be reset atomically
#[test]
fn test_section_5_3_5_queue_reset() {
    let smmu = SMMU::new();

    // Fill all queues
    smmu.submit_event(EventEntry {
        event_type: EventType::TranslationFault,
        stream_id: 1,
        pasid: 0,
        address: 0x1000,
        security_state: SecurityState::NonSecure,
        error_code: 0,
        timestamp: 100,
    })
    .unwrap();

    smmu.submit_command(CommandEntry {
        cmd_type: CommandType::Sync,
        stream_id: 1,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 100,
    })
    .unwrap();

    smmu.submit_page_request(PRIEntry {
        stream_id: 1,
        pasid: 0,
        requested_address: 0x1000,
        access_type: AccessType::Read,
        is_last_request: false,
        timestamp: 100,
    })
    .unwrap();

    // Verify queues have entries
    assert!(smmu.get_event_queue_size() > 0);
    assert!(smmu.get_command_queue_size() > 0);
    assert!(smmu.get_pri_queue_size() > 0);

    // Reset all queues
    smmu.reset_queues();

    // Verify all queues are empty
    assert_eq!(smmu.get_event_queue_size(), 0, "Event queue should be empty");
    assert_eq!(smmu.get_command_queue_size(), 0, "Command queue should be empty");
    assert_eq!(smmu.get_pri_queue_size(), 0, "PRI queue should be empty");
}

/// Test: Memory efficiency under load
///
/// ARM SMMU v3 spec: Queues should use memory efficiently
#[test]
fn test_section_5_3_5_memory_efficiency() {
    let smmu = SMMU::new();

    // Submit and process many entries to verify no memory leaks
    for iteration in 0..10 {
        // Fill queues
        for i in 0..100 {
            let event = EventEntry {
                event_type: EventType::TranslationFault,
                stream_id: i,
                pasid: 0,
                address: 0x1000,
                security_state: SecurityState::NonSecure,
                error_code: 0,
                timestamp: (iteration * 100 + i) as u64,
            };
            smmu.submit_event(event).unwrap();
        }

        // Process and clear
        let _ = smmu.get_events(); // Retrieve events
        smmu.clear_event_queue();

        // Verify queue is actually empty (no memory leaks)
        assert_eq!(
            smmu.get_event_queue_size(),
            0,
            "Queue should be empty after clear"
        );
    }

    // Final verification - queue should still be functional
    let event = EventEntry {
        event_type: EventType::TranslationFault,
        stream_id: 999,
        pasid: 0,
        address: 0x1000,
        security_state: SecurityState::NonSecure,
        error_code: 0,
        timestamp: 9999,
    };
    let result = smmu.submit_event(event);
    assert!(result.is_ok(), "Queue should still accept events after stress test");
}

// ============================================================================
// Section 5.3.6: Queue Performance Tests
// ============================================================================

/// Test: Event queue performance under load
///
/// Performance target: >100k events/second submission rate
#[test]
fn test_section_5_3_6_event_queue_performance() {
    let smmu = SMMU::new();
    let num_events = 10000;

    let start = std::time::Instant::now();

    for i in 0..num_events {
        let event = EventEntry {
            event_type: EventType::TranslationFault,
            stream_id: i % 1000,
            pasid: i % 100,
            address: 0x1000 * (i as u64 + 1),
            security_state: SecurityState::NonSecure,
            error_code: 0,
            timestamp: i as u64,
        };
        smmu.submit_event(event).unwrap();
    }

    let duration = start.elapsed();
    let events_per_sec = (num_events as f64) / duration.as_secs_f64();

    println!("Event submission rate: {:.0} events/sec", events_per_sec);
    assert!(
        events_per_sec > 100_000.0,
        "Event submission should exceed 100k events/sec (got {:.0})",
        events_per_sec
    );
}

/// Test: Command queue performance under load
///
/// Performance target: >50k commands/second submission rate
#[test]
fn test_section_5_3_6_command_queue_performance() {
    let smmu = SMMU::new();
    let num_commands = 5000;

    let start = std::time::Instant::now();

    for i in 0..num_commands {
        let command = CommandEntry {
            cmd_type: CommandType::Sync,
            stream_id: i % 1000,
            pasid: i % 100,
            start_address: 0x1000 * (i as u64),
            end_address: 0x2000 * (i as u64),
            flags: 0,
            timestamp: i as u64,
        };
        smmu.submit_command(command).unwrap();
    }

    let duration = start.elapsed();
    let commands_per_sec = (num_commands as f64) / duration.as_secs_f64();

    println!("Command submission rate: {:.0} commands/sec", commands_per_sec);
    assert!(
        commands_per_sec > 50_000.0,
        "Command submission should exceed 50k commands/sec (got {:.0})",
        commands_per_sec
    );
}

/// Test: Queue latency under light load
///
/// Performance target: <1μs submission latency
#[test]
fn test_section_5_3_6_queue_latency() {
    let smmu = SMMU::new();
    let num_samples = 1000;

    let mut total_duration = Duration::ZERO;

    for i in 0..num_samples {
        let start = std::time::Instant::now();

        let event = EventEntry {
            event_type: EventType::TranslationFault,
            stream_id: i,
            pasid: 0,
            address: 0x1000,
            security_state: SecurityState::NonSecure,
            error_code: 0,
            timestamp: i as u64,
        };
        smmu.submit_event(event).unwrap();

        total_duration += start.elapsed();
    }

    let avg_latency = total_duration / num_samples;
    let avg_latency_us = avg_latency.as_micros();

    println!("Average event submission latency: {}μs", avg_latency_us);
    assert!(
        avg_latency_us < 1,
        "Event submission latency should be <1μs (got {}μs)",
        avg_latency_us
    );
}
