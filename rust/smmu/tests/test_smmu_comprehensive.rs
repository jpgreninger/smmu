#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]

//! Comprehensive SMMU Controller Tests - Phase 2.4
//!
//! This test suite targets 100% coverage for smmu/mod.rs by testing:
//! 1. Stream configuration edge cases (max streams, conflicts)
//! 2. Command queue processing (Section 5.3.2 of spec)
//! 3. Event queue overflow handling
//! 4. PRI (Page Request Interface) queue operations
//! 5. Statistics collection and reporting
//! 6. Shutdown coordination and cleanup
//! 7. Stream limit enforcement
//! 8. Configuration update operations
//!
//! Current coverage: 69.63% → Target: 100%

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventEntry, EventType, FaultRecord, FaultType, PRIEntry, PagePermissions, SMMUConfig, SMMUError, SecurityState, StreamConfig, StreamID, IOVA, PA, PASID,
};
use smmu::SMMU;

// ============================================================================
// 1. Stream Configuration Edge Cases
// ============================================================================

#[test]
fn test_configure_stream_at_max_limit() {
    // Create SMMU with minimal config (1 stream max)
    let config = SMMUConfig::minimal();
    let smmu = SMMU::with_config(config);

    let stream_id1 = StreamID::new(1).unwrap();
    let stream_config = StreamConfig::bypass();

    // First stream should succeed
    assert!(smmu.configure_stream(stream_id1, stream_config.clone()).is_ok());
    assert_eq!(smmu.get_stream_count(), 1);

    // Second stream should fail due to limit
    let stream_id2 = StreamID::new(2).unwrap();
    let result = smmu.configure_stream(stream_id2, stream_config);

    assert!(result.is_err());
    match result.unwrap_err() {
        SMMUError::StreamLimitExceeded { current, limit } => {
            assert_eq!(current, 1);
            assert_eq!(limit, 1);
        },
        _ => panic!("Expected StreamLimitExceeded error"),
    }
}

#[test]
fn test_configure_stream_duplicate() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();
    let config = StreamConfig::stage1_only();

    // First configuration succeeds
    smmu.configure_stream(stream_id, config.clone()).unwrap();

    // Duplicate configuration fails
    let result = smmu.configure_stream(stream_id, config);
    assert!(result.is_err());
    match result.unwrap_err() {
        SMMUError::StreamAlreadyExists(_) => {},
        _ => panic!("Expected StreamAlreadyExists error"),
    }
}

#[test]
fn test_configure_stream_after_removal() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();
    let config1 = StreamConfig::stage1_only();

    // Configure, remove, then re-configure
    smmu.configure_stream(stream_id, config1).unwrap();
    smmu.remove_stream(stream_id).unwrap();

    // Re-configuration should succeed
    let config2 = StreamConfig::stage2_only();
    assert!(smmu.configure_stream(stream_id, config2).is_ok());
}

#[test]
fn test_configure_stream_invalid_config() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();

    // Create invalid config (translation disabled but stages enabled)
    let mut config = StreamConfig::default();
    config.stage1_enabled = true;
    config.stage2_enabled = true;
    config.translation_enabled = false; // Invalid: stages enabled without translation

    let result = smmu.configure_stream(stream_id, config);
    assert!(result.is_err());
}

#[test]
fn test_remove_stream_not_found() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(999).unwrap();

    let result = smmu.remove_stream(stream_id);
    assert!(result.is_err());
    match result.unwrap_err() {
        SMMUError::StreamNotFound(_) => {},
        _ => panic!("Expected StreamNotFound error"),
    }
}

#[test]
fn test_configure_stream_with_pasid_enabled() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();

    let mut config = StreamConfig::stage1_only();
    config.pasid_enabled = true;
    config.max_pasid = 16;

    assert!(smmu.configure_stream(stream_id, config).is_ok());
    assert!(smmu.has_stream(stream_id));
}

// ============================================================================
// 2. Command Queue Processing (Section 5.3.2)
// ============================================================================

#[test]
fn test_command_queue_submit_tlbi_nh_all() {
    let smmu = SMMU::new();

    let command = CommandEntry {
        cmd_type: CommandType::TlbiNhAll,
        stream_id: 0,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 0,
        prg_index: 0,
        asid: 0,
        vmid: 0,
        stag: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        security_state: SecurityState::NonSecure,
    };

    assert!(smmu.submit_command(command).is_ok());
    assert_eq!(smmu.get_command_queue_size(), 1);
}

#[test]
fn test_command_queue_submit_tlbi_el2_all() {
    let smmu = SMMU::new();

    let command = CommandEntry {
        cmd_type: CommandType::TlbiEl2All,
        stream_id: 0,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 0,
        prg_index: 0,
        asid: 0,
        vmid: 0,
        stag: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        security_state: SecurityState::NonSecure,
    };

    assert!(smmu.submit_command(command).is_ok());
    assert_eq!(smmu.get_command_queue_size(), 1);
}

#[test]
fn test_command_queue_submit_tlbi_s12_vmall() {
    let smmu = SMMU::new();

    let command = CommandEntry {
        cmd_type: CommandType::TlbiS12Vmall,
        stream_id: 1,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 0,
        prg_index: 0,
        asid: 0,
        vmid: 0,
        stag: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        security_state: SecurityState::NonSecure,
    };

    assert!(smmu.submit_command(command).is_ok());
    assert_eq!(smmu.get_command_queue_size(), 1);
}

#[test]
fn test_command_queue_submit_atc_inv() {
    let smmu = SMMU::new();

    let command = CommandEntry {
        cmd_type: CommandType::AtcInv,
        stream_id: 1,
        pasid: 0,
        start_address: 0x1000,
        end_address: 0x2000,
        flags: 0,
        timestamp: 0,
        prg_index: 0,
        asid: 0,
        vmid: 0,
        stag: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        security_state: SecurityState::NonSecure,
    };

    assert!(smmu.submit_command(command).is_ok());
    assert_eq!(smmu.get_command_queue_size(), 1);
}

#[test]
fn test_command_queue_submit_atc_inv_invalid_range() {
    let smmu = SMMU::new();

    // Invalid: end < start
    let command = CommandEntry {
        cmd_type: CommandType::AtcInv,
        stream_id: 1,
        pasid: 0,
        start_address: 0x2000,
        end_address: 0x1000,
        flags: 0,
        timestamp: 0,
        prg_index: 0,
        asid: 0,
        vmid: 0,
        stag: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        security_state: SecurityState::NonSecure,
    };

    let result = smmu.submit_command(command);
    assert!(result.is_err());
    match result.unwrap_err() {
        SMMUError::InvalidCommandParameters(_) => {},
        _ => panic!("Expected InvalidCommandParameters error"),
    }
}

#[test]
fn test_command_queue_submit_sync() {
    let smmu = SMMU::new();

    let command = CommandEntry {
        cmd_type: CommandType::Sync,
        stream_id: 0,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 0,
        prg_index: 0,
        asid: 0,
        vmid: 0,
        stag: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        security_state: SecurityState::NonSecure,
    };

    assert!(smmu.submit_command(command).is_ok());
    assert_eq!(smmu.get_command_queue_size(), 1);
}

#[test]
fn test_command_queue_submit_prefetch_config() {
    let smmu = SMMU::new();

    let command = CommandEntry {
        cmd_type: CommandType::PrefetchConfig,
        stream_id: 1,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 0,
        prg_index: 0,
        asid: 0,
        vmid: 0,
        stag: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        security_state: SecurityState::NonSecure,
    };

    assert!(smmu.submit_command(command).is_ok());
}

#[test]
fn test_command_queue_submit_prefetch_addr() {
    let smmu = SMMU::new();

    let command = CommandEntry {
        cmd_type: CommandType::PrefetchAddr,
        stream_id: 1,
        pasid: 0,
        start_address: 0x1000,
        end_address: 0x2000,
        flags: 0,
        timestamp: 0,
        prg_index: 0,
        asid: 0,
        vmid: 0,
        stag: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        security_state: SecurityState::NonSecure,
    };

    assert!(smmu.submit_command(command).is_ok());
}

#[test]
fn test_command_queue_submit_cfgi_ste() {
    let smmu = SMMU::new();

    let command = CommandEntry {
        cmd_type: CommandType::CfgiSte,
        stream_id: 1,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 0,
        prg_index: 0,
        asid: 0,
        vmid: 0,
        stag: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        security_state: SecurityState::NonSecure,
    };

    assert!(smmu.submit_command(command).is_ok());
}

#[test]
fn test_command_queue_submit_cfgi_all() {
    let smmu = SMMU::new();

    let command = CommandEntry {
        cmd_type: CommandType::CfgiAll,
        stream_id: 0,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 0,
        prg_index: 0,
        asid: 0,
        vmid: 0,
        stag: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        security_state: SecurityState::NonSecure,
    };

    assert!(smmu.submit_command(command).is_ok());
}

#[test]
fn test_command_queue_submit_pri_resp() {
    let smmu = SMMU::new();

    let command = CommandEntry {
        cmd_type: CommandType::PriResp,
        stream_id: 1,
        pasid: 5,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 0,
        prg_index: 0,
        asid: 0,
        vmid: 0,
        stag: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        security_state: SecurityState::NonSecure,
    };

    assert!(smmu.submit_command(command).is_ok());
}

#[test]
fn test_command_queue_submit_resume() {
    let smmu = SMMU::new();

    let command = CommandEntry {
        cmd_type: CommandType::Resume,
        stream_id: 1,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 0,
        prg_index: 0,
        asid: 0,
        vmid: 0,
        stag: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        security_state: SecurityState::NonSecure,
    };

    assert!(smmu.submit_command(command).is_ok());
}

#[test]
fn test_command_queue_process_single_tlbi() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN); // BUG-04 fix: CR0 resets to 0

    let command = CommandEntry {
        cmd_type: CommandType::TlbiNhAll,
        stream_id: 0,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 0,
        prg_index: 0,
        asid: 0,
        vmid: 0,
        stag: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        security_state: SecurityState::NonSecure,
    };

    smmu.submit_command(command).unwrap();
    assert_eq!(smmu.get_command_queue_size(), 1);

    let processed = smmu.process_command_queue().unwrap();
    assert_eq!(processed, 1);
    assert_eq!(smmu.get_command_queue_size(), 0);

    // Verify invalidation count increased
    let cache_stats = smmu.get_cache_statistics();
    assert_eq!(cache_stats.invalidation_count(), 1);
}

#[test]
fn test_command_queue_process_multiple_commands() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN); // BUG-04 fix: CR0 resets to 0

    // Submit multiple commands
    for i in 0..5 {
        let command = CommandEntry {
            cmd_type: CommandType::TlbiNhAll,
            stream_id: i,
            pasid: 0,
            start_address: 0,
            end_address: 0,
            flags: 0,
            timestamp: 0,
            prg_index: 0,
            asid: 0,
            vmid: 0,
        stag: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        security_state: SecurityState::NonSecure,
        };
        smmu.submit_command(command).unwrap();
    }

    assert_eq!(smmu.get_command_queue_size(), 5);

    let processed = smmu.process_command_queue().unwrap();
    assert_eq!(processed, 5);
    assert_eq!(smmu.get_command_queue_size(), 0);

    // Verify invalidation count
    let cache_stats = smmu.get_cache_statistics();
    assert_eq!(cache_stats.invalidation_count(), 5);
}

#[test]
fn test_command_queue_process_atc_inv_generates_event() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN); // BUG-04 fix: CR0 resets to 0

    let command = CommandEntry {
        cmd_type: CommandType::AtcInv,
        stream_id: 1,
        pasid: 0,
        start_address: 0x1000,
        end_address: 0x2000,
        flags: 0,
        timestamp: 0,
        prg_index: 0,
        asid: 0,
        vmid: 0,
        stag: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        security_state: SecurityState::NonSecure,
    };

    smmu.submit_command(command).unwrap();
    smmu.process_command_queue().unwrap();

    // Verify completion event was generated
    let events = smmu.get_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, EventType::AtcInvalidateCompletion);
    assert_eq!(events[0].stream_id, 1);
}

#[test]
fn test_command_queue_process_sync_generates_event() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN); // BUG-04 fix: CR0 resets to 0

    // §4.8 / FINDING-NEW-27: CS must be non-zero (SIG_IRQ=0x01) to trigger a
    // completion event; CS=0 (SIG_NONE) produces no event.
    let command = CommandEntry {
        cmd_type: CommandType::Sync,
        stream_id: 0,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 0,
        prg_index: 0,
        asid: 0,
        vmid: 0,
        stag: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 1, // SIG_IRQ — generate completion event
        security_state: SecurityState::NonSecure,
    };

    smmu.submit_command(command).unwrap();
    smmu.process_command_queue().unwrap();

    // Verify completion event was generated
    let events = smmu.get_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, EventType::CommandSyncCompletion);
}

#[test]
fn test_command_queue_process_empty_queue() {
    let smmu = SMMU::new();

    // Process empty queue
    let processed = smmu.process_command_queue().unwrap();
    assert_eq!(processed, 0);
}

#[test]
fn test_command_queue_clear() {
    let smmu = SMMU::new();

    // Submit commands
    for i in 0..3 {
        let command = CommandEntry {
            cmd_type: CommandType::TlbiNhAll,
            stream_id: i,
            pasid: 0,
            start_address: 0,
            end_address: 0,
            flags: 0,
            timestamp: 0,
            prg_index: 0,
            asid: 0,
            vmid: 0,
        stag: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        security_state: SecurityState::NonSecure,
        };
        smmu.submit_command(command).unwrap();
    }

    assert_eq!(smmu.get_command_queue_size(), 3);

    smmu.clear_command_queue();
    assert_eq!(smmu.get_command_queue_size(), 0);
}

#[test]
fn test_command_queue_is_full() {
    // Create SMMU with small command queue
    let mut config = SMMUConfig::default();
    config.queue_config.command_queue_size = 16;
    let smmu = SMMU::with_config(config);

    // Fill the queue
    for i in 0..16 {
        let command = CommandEntry {
            cmd_type: CommandType::TlbiNhAll,
            stream_id: i,
            pasid: 0,
            start_address: 0,
            end_address: 0,
            flags: 0,
            timestamp: 0,
            prg_index: 0,
            asid: 0,
            vmid: 0,
        stag: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        security_state: SecurityState::NonSecure,
        };
        smmu.submit_command(command).unwrap();
    }

    assert!(smmu.is_command_queue_full());
}

// ============================================================================
// 3. Event Queue Overflow Handling
// ============================================================================

#[test]
fn test_event_queue_submit_translation_fault() {
    let smmu = SMMU::new();

    let event = EventEntry {
        event_type: EventType::FTranslation,
        stream_id: 1,
        pasid: 0,
        address: 0x1000,
        security_state: SecurityState::NonSecure,
        error_code: 0,
        timestamp: 12_345,
        stall: false,
        stag: 0,
    };

    assert!(smmu.submit_event(event).is_ok());
    assert_eq!(smmu.get_event_queue_size(), 1);
}

#[test]
fn test_event_queue_submit_permission_fault() {
    let smmu = SMMU::new();

    let event = EventEntry {
        event_type: EventType::FPermission,
        stream_id: 2,
        pasid: 1,
        address: 0x2000,
        security_state: SecurityState::Secure,
        error_code: 0,
        timestamp: 12_346,
        stall: false,
        stag: 0,
    };

    assert!(smmu.submit_event(event).is_ok());
    assert!(smmu.has_events());
}

#[test]
fn test_event_queue_submit_access_fault() {
    let smmu = SMMU::new();

    let event = EventEntry {
        event_type: EventType::CBadSte,
        stream_id: 3,
        pasid: 2,
        address: 0x3000,
        security_state: SecurityState::NonSecure,
        error_code: 0,
        timestamp: 12_347,
        stall: false,
        stag: 0,
    };

    smmu.submit_event(event).unwrap();
    assert_eq!(smmu.get_event_queue_size(), 1);
}

#[test]
fn test_event_queue_overflow_with_small_queue() {
    // Create SMMU with small event queue (minimum is 16)
    let mut config = SMMUConfig::default();
    config.queue_config.event_queue_size = 16;
    let smmu = SMMU::with_config(config);

    // Fill the queue to capacity
    for i in 0..16 {
        let event = EventEntry {
            event_type: EventType::FTranslation,
            stream_id: i,
            pasid: 0,
            address: u64::from(i) * 0x1000,
            security_state: SecurityState::NonSecure,
            error_code: 0,
            timestamp: u64::from(i),
            stall: false,
            stag: 0,
        };
        smmu.submit_event(event).unwrap();
    }

    assert_eq!(smmu.get_event_queue_size(), 16);

    // Next submission should fail (overflow)
    let overflow_event = EventEntry {
        event_type: EventType::FTranslation,
        stream_id: 99,
        pasid: 0,
        address: 0x9_9000,
        security_state: SecurityState::NonSecure,
        error_code: 0,
        timestamp: 9999,
        stall: false,
        stag: 0,
    };

    let result = smmu.submit_event(overflow_event);
    assert!(result.is_err());
    match result.unwrap_err() {
        SMMUError::EventQueueFull => {},
        _ => panic!("Expected EventQueueFull error"),
    }
}

#[test]
fn test_event_queue_large_queue_no_overflow() {
    // Default config has large queue (512 entries)
    let smmu = SMMU::new();

    // Submit many events (less than capacity)
    for i in 0..100 {
        let event = EventEntry {
            event_type: EventType::FTranslation,
            stream_id: i,
            pasid: 0,
            address: u64::from(i) * 0x1000,
            security_state: SecurityState::NonSecure,
            error_code: 0,
            timestamp: u64::from(i),
            stall: false,
            stag: 0,
        };
        assert!(smmu.submit_event(event).is_ok());
    }

    assert_eq!(smmu.get_event_queue_size(), 100);
}

#[test]
fn test_event_queue_get_all_events() {
    let smmu = SMMU::new();

    // Submit multiple events
    for i in 0..5 {
        let event = EventEntry {
            event_type: EventType::FTranslation,
            stream_id: i,
            pasid: 0,
            address: u64::from(i) * 0x1000,
            security_state: SecurityState::NonSecure,
            error_code: 0,
            timestamp: u64::from(i),
            stall: false,
            stag: 0,
        };
        smmu.submit_event(event).unwrap();
    }

    let events = smmu.get_events();
    assert_eq!(events.len(), 5);
}

#[test]
fn test_event_queue_filter_by_type() {
    let smmu = SMMU::new();

    // Submit mixed event types
    smmu.submit_event(EventEntry {
        event_type: EventType::FTranslation,
        stream_id: 1,
        pasid: 0,
        address: 0x1000,
        security_state: SecurityState::NonSecure,
        error_code: 0,
        timestamp: 1,
        stall: false,
        stag: 0,
    })
    .unwrap();

    smmu.submit_event(EventEntry {
        event_type: EventType::FPermission,
        stream_id: 2,
        pasid: 0,
        address: 0x2000,
        security_state: SecurityState::NonSecure,
        error_code: 0,
        timestamp: 2,
        stall: false,
        stag: 0,
    })
    .unwrap();

    smmu.submit_event(EventEntry {
        event_type: EventType::FTranslation,
        stream_id: 3,
        pasid: 0,
        address: 0x3000,
        security_state: SecurityState::NonSecure,
        error_code: 0,
        timestamp: 3,
        stall: false,
        stag: 0,
    })
    .unwrap();

    let translation_faults = smmu.get_events_by_type(EventType::FTranslation);
    assert_eq!(translation_faults.len(), 2);

    let permission_faults = smmu.get_events_by_type(EventType::FPermission);
    assert_eq!(permission_faults.len(), 1);
}

#[test]
fn test_event_queue_filter_by_stream() {
    let smmu = SMMU::new();

    // Submit events for different streams
    for i in 0..3 {
        smmu.submit_event(EventEntry {
            event_type: EventType::FTranslation,
            stream_id: 1,
            pasid: 0,
            address: (i as u64) * 0x1000,
            security_state: SecurityState::NonSecure,
            error_code: 0,
            timestamp: i as u64,
            stall: false,
            stag: 0,
        })
        .unwrap();
    }

    smmu.submit_event(EventEntry {
        event_type: EventType::FTranslation,
        stream_id: 2,
        pasid: 0,
        address: 0x9000,
        security_state: SecurityState::NonSecure,
        error_code: 0,
        timestamp: 999,
        stall: false,
        stag: 0,
    })
    .unwrap();

    let stream1_events = smmu.get_events_by_stream(1);
    assert_eq!(stream1_events.len(), 3);

    let stream2_events = smmu.get_events_by_stream(2);
    assert_eq!(stream2_events.len(), 1);
}

#[test]
fn test_event_queue_clear() {
    let smmu = SMMU::new();

    // Submit events
    for i in 0..5 {
        smmu.submit_event(EventEntry {
            event_type: EventType::FTranslation,
            stream_id: i,
            pasid: 0,
            address: u64::from(i) * 0x1000,
            security_state: SecurityState::NonSecure,
            error_code: 0,
            timestamp: u64::from(i),
            stall: false,
            stag: 0,
        })
        .unwrap();
    }

    assert_eq!(smmu.get_event_queue_size(), 5);

    smmu.clear_event_queue();
    assert_eq!(smmu.get_event_queue_size(), 0);
    assert!(!smmu.has_events());
}

// ============================================================================
// 4. PRI (Page Request Interface) Queue Operations
// ============================================================================

#[test]
fn test_pri_queue_submit_page_request() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_PRIQEN | SMMU::CR0_EVENTQEN); // BUG-04 fix: CR0 resets to 0

    let pri_entry = PRIEntry {
        stream_id: 1,
        pasid: 0,
        requested_address: 0x1000,
        access_type: AccessType::Read,
        is_last_request: false,
        timestamp: 0,
        prg_index: 0,
        security_state: SecurityState::NonSecure,
    };

    assert!(smmu.submit_page_request(pri_entry).is_ok());
    assert_eq!(smmu.get_pri_queue_size(), 1);
}

#[test]
fn test_pri_queue_submit_multiple_requests() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_PRIQEN | SMMU::CR0_EVENTQEN); // BUG-04 fix: CR0 resets to 0

    // Submit multiple page requests
    for i in 0..5 {
        let pri_entry = PRIEntry {
            stream_id: 1,
            pasid: i,
            requested_address: u64::from(i) * 0x1000,
            access_type: AccessType::Read,
            is_last_request: false,
            timestamp: 0,
            prg_index: 0,
            security_state: SecurityState::NonSecure,
        };
        smmu.submit_page_request(pri_entry).unwrap();
    }

    assert_eq!(smmu.get_pri_queue_size(), 5);
}

#[test]
fn test_pri_queue_overflow_with_small_queue() {
    // Create SMMU with small PRI queue (minimum is 16)
    let mut config = SMMUConfig::default();
    config.queue_config.pri_queue_size = 16;
    let smmu = SMMU::with_config(config);
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_PRIQEN | SMMU::CR0_EVENTQEN); // BUG-04 fix: CR0 resets to 0

    // Fill the queue
    for i in 0..16 {
        let pri_entry = PRIEntry {
            stream_id: 1,
            pasid: 0,
            requested_address: (i as u64) * 0x1000,
            access_type: AccessType::Read,
            is_last_request: false,
            timestamp: 0,
            prg_index: 0,
            security_state: SecurityState::NonSecure,
        };
        smmu.submit_page_request(pri_entry).unwrap();
    }

    assert_eq!(smmu.get_pri_queue_size(), 16);

    // Next submission should fail (overflow)
    let overflow_entry = PRIEntry {
        stream_id: 99,
        pasid: 99,
        requested_address: 0x9_9000,
        access_type: AccessType::Read,
        is_last_request: false,
        timestamp: 0,
        prg_index: 0,
        security_state: SecurityState::NonSecure,
    };

    let result = smmu.submit_page_request(overflow_entry);
    assert!(result.is_err());
    match result.unwrap_err() {
        SMMUError::PriQueueFull => {},
        _ => panic!("Expected PriQueueFull error"),
    }
}

#[test]
fn test_pri_queue_get_all_requests() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_PRIQEN | SMMU::CR0_EVENTQEN); // BUG-04 fix: CR0 resets to 0

    // Submit requests
    for i in 0..3 {
        let pri_entry = PRIEntry {
            stream_id: 1,
            pasid: 0,
            requested_address: (i as u64) * 0x1000,
            access_type: AccessType::Read,
            is_last_request: false,
            timestamp: 0,
            prg_index: 0,
            security_state: SecurityState::NonSecure,
        };
        smmu.submit_page_request(pri_entry).unwrap();
    }

    let requests = smmu.get_pri_queue();
    assert_eq!(requests.len(), 3);
}

#[test]
fn test_pri_queue_process_generates_events() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_PRIQEN | SMMU::CR0_EVENTQEN); // BUG-04 fix: CR0 resets to 0

    // Submit page request
    let pri_entry = PRIEntry {
        stream_id: 1,
        pasid: 5,
        requested_address: 0x5000,
        access_type: AccessType::Read,
        is_last_request: false,
        timestamp: 0,
        prg_index: 0,
        security_state: SecurityState::NonSecure,
    };
    smmu.submit_page_request(pri_entry).unwrap();

    // Process PRI queue
    let processed = smmu.process_pri_queue().unwrap();
    assert_eq!(processed, 1);
    assert_eq!(smmu.get_pri_queue_size(), 0);

    // Verify PRI event was generated
    let events = smmu.get_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, EventType::EPageRequest);
    assert_eq!(events[0].stream_id, 1);
    assert_eq!(events[0].pasid, 5);
    assert_eq!(events[0].address, 0x5000);
}

#[test]
fn test_pri_queue_process_multiple_requests() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_PRIQEN | SMMU::CR0_EVENTQEN); // BUG-04 fix: CR0 resets to 0

    // Submit multiple requests
    for i in 0..5 {
        let pri_entry = PRIEntry {
            stream_id: 1,
            pasid: i,
            requested_address: u64::from(i) * 0x1000,
            access_type: AccessType::Read,
            is_last_request: false,
            timestamp: 0,
            prg_index: 0,
            security_state: SecurityState::NonSecure,
        };
        smmu.submit_page_request(pri_entry).unwrap();
    }

    let processed = smmu.process_pri_queue().unwrap();
    assert_eq!(processed, 5);
    assert_eq!(smmu.get_pri_queue_size(), 0);

    // Verify events were generated
    let events = smmu.get_events();
    assert_eq!(events.len(), 5);
}

#[test]
fn test_pri_queue_process_empty_queue() {
    let smmu = SMMU::new();

    let processed = smmu.process_pri_queue().unwrap();
    assert_eq!(processed, 0);
}

#[test]
fn test_pri_queue_clear() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_PRIQEN | SMMU::CR0_EVENTQEN); // BUG-04 fix: CR0 resets to 0

    // Submit requests
    for i in 0..3 {
        let pri_entry = PRIEntry {
            stream_id: 1,
            pasid: 0,
            requested_address: (i as u64) * 0x1000,
            access_type: AccessType::Read,
            is_last_request: false,
            timestamp: 0,
            prg_index: 0,
            security_state: SecurityState::NonSecure,
        };
        smmu.submit_page_request(pri_entry).unwrap();
    }

    assert_eq!(smmu.get_pri_queue_size(), 3);

    smmu.clear_pri_queue();
    assert_eq!(smmu.get_pri_queue_size(), 0);
}

// ============================================================================
// 5. Statistics Collection and Reporting
// ============================================================================

#[test]
fn test_translation_stats_initial_state() {
    let smmu = SMMU::new();

    let (total, successful, failed) = smmu.get_translation_stats();
    assert_eq!(total, 0);
    assert_eq!(successful, 0);
    assert_eq!(failed, 0);
}

#[test]
fn test_translation_stats_successful_translation() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();

    // Configure stream in bypass mode
    smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();

    // Perform translation
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let _result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);

    let (total, successful, failed) = smmu.get_translation_stats();
    assert_eq!(total, 1);
    assert_eq!(successful, 1);
    assert_eq!(failed, 0);
}

#[test]
fn test_translation_stats_failed_translation_no_stream() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required to reach stream-not-found path (§6.3.9)
    let stream_id = StreamID::new(999).unwrap();

    // Translate without configuring stream
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let _result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);

    let (total, successful, failed) = smmu.get_translation_stats();
    assert_eq!(total, 1);
    assert_eq!(successful, 0);
    assert_eq!(failed, 1);
}

#[test]
fn test_translation_stats_multiple_translations() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();

    let pasid = PASID::new(0).unwrap();

    // Perform multiple successful translations
    for i in 0..10 {
        let iova = IOVA::new(0x1000 + (i * 0x1000)).unwrap();
        let _result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    }

    let (total, successful, failed) = smmu.get_translation_stats();
    assert_eq!(total, 10);
    assert_eq!(successful, 10);
    assert_eq!(failed, 0);
}

#[test]
fn test_translation_stats_reset() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();

    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let _result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);

    let (total, _successful, _failed) = smmu.get_translation_stats();
    assert!(total > 0);

    smmu.reset_translation_stats();

    let (total, successful, failed) = smmu.get_translation_stats();
    assert_eq!(total, 0);
    assert_eq!(successful, 0);
    assert_eq!(failed, 0);
}

#[test]
fn test_queue_statistics() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN); // BUG-04 fix

    // Submit to all queues
    smmu.submit_event(EventEntry {
        event_type: EventType::FTranslation,
        stream_id: 1,
        pasid: 0,
        address: 0x1000,
        security_state: SecurityState::NonSecure,
        error_code: 0,
        timestamp: 1,
        stall: false,
        stag: 0,
    })
    .unwrap();

    smmu.submit_command(CommandEntry {
        cmd_type: CommandType::TlbiNhAll,
        stream_id: 0,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 0,
        prg_index: 0,
        asid: 0,
        vmid: 0,
        stag: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        security_state: SecurityState::NonSecure,
    })
    .unwrap();

    smmu.submit_page_request(PRIEntry {
        stream_id: 1,
        pasid: 0,
        requested_address: 0x1000,
        access_type: AccessType::Read,
        is_last_request: false,
        timestamp: 0,
        prg_index: 0,
        security_state: SecurityState::NonSecure,
    })
    .unwrap();

    let stats = smmu.get_queue_statistics();
    assert_eq!(stats.event_queue_size(), 1);
    assert_eq!(stats.command_queue_size(), 1);
    assert_eq!(stats.pri_queue_size(), 1);
}

#[test]
fn test_cache_statistics_invalidation_count() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN); // BUG-04 fix: CR0 resets to 0

    let initial_stats = smmu.get_cache_statistics();
    assert_eq!(initial_stats.invalidation_count(), 0);

    // Submit and process invalidation commands
    for _ in 0..3 {
        smmu.submit_command(CommandEntry {
            cmd_type: CommandType::TlbiNhAll,
            stream_id: 0,
            pasid: 0,
            start_address: 0,
            end_address: 0,
            flags: 0,
            timestamp: 0,
            prg_index: 0,
            asid: 0,
            vmid: 0,
        stag: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        security_state: SecurityState::NonSecure,
        })
        .unwrap();
    }

    smmu.process_command_queue().unwrap();

    let stats = smmu.get_cache_statistics();
    assert_eq!(stats.invalidation_count(), 3);
}

#[test]
fn test_reset_queues_atomically() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN); // BUG-04 fix

    // Populate all queues
    smmu.submit_event(EventEntry {
        event_type: EventType::FTranslation,
        stream_id: 1,
        pasid: 0,
        address: 0x1000,
        security_state: SecurityState::NonSecure,
        error_code: 0,
        timestamp: 1,
        stall: false,
        stag: 0,
    })
    .unwrap();

    smmu.submit_command(CommandEntry {
        cmd_type: CommandType::TlbiNhAll,
        stream_id: 0,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 0,
        prg_index: 0,
        asid: 0,
        vmid: 0,
        stag: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        security_state: SecurityState::NonSecure,
    })
    .unwrap();

    smmu.submit_page_request(PRIEntry {
        stream_id: 1,
        pasid: 0,
        requested_address: 0x1000,
        access_type: AccessType::Read,
        is_last_request: false,
        timestamp: 0,
        prg_index: 0,
        security_state: SecurityState::NonSecure,
    })
    .unwrap();

    // Reset all queues
    smmu.reset_queues();

    let stats = smmu.get_queue_statistics();
    assert_eq!(stats.event_queue_size(), 0);
    assert_eq!(stats.command_queue_size(), 0);
    assert_eq!(stats.pri_queue_size(), 0);
}

// ============================================================================
// 6. Shutdown Coordination and Cleanup
// ============================================================================

#[test]
fn test_shutdown_success() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();

    assert!(!smmu.is_shutdown());
    assert_eq!(smmu.get_stream_count(), 1);

    let result = smmu.shutdown();
    assert!(result.is_ok());
    assert!(smmu.is_shutdown());
    assert_eq!(smmu.get_stream_count(), 0);
}

#[test]
fn test_shutdown_idempotent() {
    let smmu = SMMU::new();

    // First shutdown succeeds
    assert!(smmu.shutdown().is_ok());
    assert!(smmu.is_shutdown());

    // Second shutdown returns error
    let result = smmu.shutdown();
    assert!(result.is_err());
    match result.unwrap_err() {
        SMMUError::ShutdownInProgress => {},
        _ => panic!("Expected ShutdownInProgress error"),
    }
}

#[test]
fn test_shutdown_clears_fault_queue() {
    let smmu = SMMU::new();

    // Add faults
    let fault = FaultRecord::builder()
        .stream_id(StreamID::new(1).unwrap())
        .pasid(PASID::new(0).unwrap())
        .address(IOVA::new(0x1000).unwrap())
        .fault_type(FaultType::TranslationFault)
        .access_type(AccessType::Read)
        .security_state(SecurityState::NonSecure)
        .timestamp(0)
        .build();

    smmu.record_fault(fault);
    assert_eq!(smmu.get_faults().len(), 1);

    smmu.shutdown().unwrap();

    // Fault queue should be cleared
    assert_eq!(smmu.get_faults().len(), 0);
}

#[test]
fn test_operations_after_shutdown_fail() {
    let smmu = SMMU::new();
    smmu.shutdown().unwrap();

    let stream_id = StreamID::new(1).unwrap();
    let config = StreamConfig::bypass();

    // All operations should fail
    let result = smmu.configure_stream(stream_id, config);
    assert!(result.is_err());
    match result.unwrap_err() {
        SMMUError::ShutdownInProgress => {},
        _ => panic!("Expected ShutdownInProgress error"),
    }
}

#[test]
fn test_initialize_after_shutdown_fails() {
    let smmu = SMMU::new();
    assert!(smmu.initialize().is_ok());

    smmu.shutdown().unwrap();

    let result = smmu.initialize();
    assert!(result.is_err());
    match result.unwrap_err() {
        SMMUError::ShutdownInProgress => {},
        _ => panic!("Expected ShutdownInProgress error"),
    }
}

#[test]
fn test_remove_stream_after_shutdown_fails() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();

    smmu.shutdown().unwrap();

    let result = smmu.remove_stream(stream_id);
    assert!(result.is_err());
}

#[test]
fn test_create_pasid_after_shutdown_fails() {
    let smmu = SMMU::new();
    smmu.shutdown().unwrap();

    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();

    let result = smmu.create_pasid(stream_id, pasid);
    assert!(result.is_err());
}

#[test]
fn test_map_page_after_shutdown_fails() {
    let smmu = SMMU::new();
    smmu.shutdown().unwrap();

    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    let result = smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    );
    assert!(result.is_err());
}

#[test]
fn test_update_config_after_shutdown_fails() {
    let smmu = SMMU::new();
    smmu.shutdown().unwrap();

    let result = smmu.update_config(|config| {
        config.cache_config.tlb_cache_size = 2048;
    });
    assert!(result.is_err());
}

// ============================================================================
// 7. Stream Limit Enforcement
// ============================================================================

#[test]
fn test_stream_limit_enforcement_at_boundary() {
    let mut config = SMMUConfig::default();
    config.address_config.max_stream_count = 5;
    let smmu = SMMU::with_config(config);

    // Configure 5 streams (should succeed)
    for i in 1..=5 {
        let stream_id = StreamID::new(i).unwrap();
        assert!(smmu.configure_stream(stream_id, StreamConfig::bypass()).is_ok());
    }

    assert_eq!(smmu.get_stream_count(), 5);

    // 6th stream should fail
    let stream_id = StreamID::new(6).unwrap();
    let result = smmu.configure_stream(stream_id, StreamConfig::bypass());
    assert!(result.is_err());
}

#[test]
fn test_stream_count_after_removal() {
    let smmu = SMMU::new();

    // Add streams
    for i in 1..=3 {
        let stream_id = StreamID::new(i).unwrap();
        smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();
    }

    assert_eq!(smmu.get_stream_count(), 3);

    // Remove one stream
    let stream_id = StreamID::new(2).unwrap();
    smmu.remove_stream(stream_id).unwrap();

    assert_eq!(smmu.get_stream_count(), 2);
}

#[test]
fn test_stream_limit_allows_reconfiguration_after_removal() {
    let mut config = SMMUConfig::default();
    config.address_config.max_stream_count = 2;
    let smmu = SMMU::with_config(config);

    // Fill to capacity
    let stream_id1 = StreamID::new(1).unwrap();
    let stream_id2 = StreamID::new(2).unwrap();
    smmu.configure_stream(stream_id1, StreamConfig::bypass()).unwrap();
    smmu.configure_stream(stream_id2, StreamConfig::bypass()).unwrap();

    // Remove one
    smmu.remove_stream(stream_id1).unwrap();

    // Should be able to add another
    let stream_id3 = StreamID::new(3).unwrap();
    assert!(smmu.configure_stream(stream_id3, StreamConfig::bypass()).is_ok());
}

// ============================================================================
// 8. Configuration Update Operations
// ============================================================================

#[test]
#[allow(clippy::no_effect_underscore_binding)]
fn test_update_config_transactional() {
    let smmu = SMMU::new();

    let original_config = smmu.get_config();
    let _original_cache_size = original_config.cache_config.tlb_cache_size;

    smmu.update_config(|config| {
        config.cache_config.tlb_cache_size = 2048;
    })
    .unwrap();

    let updated_config = smmu.get_config();
    assert_eq!(updated_config.cache_config.tlb_cache_size, 2048);
}

#[test]
fn test_update_config_validation_failure_rollback() {
    let smmu = SMMU::new();

    let original_config = smmu.get_config();

    // Try to set invalid queue size
    let result = smmu.update_config(|config| {
        config.queue_config.event_queue_size = 1; // Below minimum
    });

    assert!(result.is_err());

    // Configuration should be unchanged (rolled back)
    let current_config = smmu.get_config();
    assert_eq!(
        current_config.queue_config.event_queue_size,
        original_config.queue_config.event_queue_size
    );
}

#[test]
fn test_update_config_multiple_fields() {
    let smmu = SMMU::new();

    smmu.update_config(|config| {
        config.cache_config.tlb_cache_size = 4096;
        config.address_config.max_pasid_count = 64;
    })
    .unwrap();

    let config = smmu.get_config();
    assert_eq!(config.cache_config.tlb_cache_size, 4096);
    assert_eq!(config.address_config.max_pasid_count, 64);
}

#[test]
fn test_get_config_returns_copy() {
    let smmu = SMMU::new();

    let config1 = smmu.get_config();
    let config2 = smmu.get_config();

    // Should be equal but separate copies
    assert_eq!(config1, config2);
}

// ============================================================================
// Additional Edge Cases and Integration Tests
// ============================================================================

#[test]
fn test_translation_records_fault_on_stream_not_found() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required to reach stream-not-found fault path (§6.3.9)
    let stream_id = StreamID::new(999).unwrap();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    assert_eq!(smmu.get_faults().len(), 0);

    let _result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);

    // Fault should be recorded
    let faults = smmu.get_faults();
    assert_eq!(faults.len(), 1);
    assert_eq!(faults[0].fault_type(), FaultType::BadStreamID);
}

#[test]
fn test_translation_during_shutdown_fails() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();

    smmu.shutdown().unwrap();

    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);

    assert!(result.is_err());
}

#[test]
fn test_has_stream_returns_false_for_unconfigured() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(999).unwrap();

    assert!(!smmu.has_stream(stream_id));
}

#[test]
fn test_has_stream_returns_true_after_configuration() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();

    assert!(!smmu.has_stream(stream_id));

    smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();

    assert!(smmu.has_stream(stream_id));
}

#[test]
fn test_pasid_operations_on_configured_stream() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();

    let pasid = PASID::new(5).unwrap();

    // Create PASID
    assert!(smmu.create_pasid(stream_id, pasid).is_ok());

    // Remove PASID
    assert!(smmu.remove_pasid(stream_id, pasid).is_ok());
}

#[test]
fn test_stage2_address_space_operations() {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();

    // Configure for two-stage translation
    let config = StreamConfig::two_stage();
    smmu.configure_stream(stream_id, config).unwrap();

    // Create Stage-2 address space
    assert!(smmu.create_stage2_address_space(stream_id).is_ok());

    // Map Stage-2 page
    let ipa = IOVA::new(0x2000).unwrap();
    let pa = PA::new(0x3000).unwrap();
    assert!(smmu
        .map_stage2_page(stream_id, ipa, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .is_ok());
}

#[test]
fn test_clear_faults() {
    let smmu = SMMU::new();

    let fault = FaultRecord::builder()
        .stream_id(StreamID::new(1).unwrap())
        .pasid(PASID::new(0).unwrap())
        .address(IOVA::new(0x1000).unwrap())
        .fault_type(FaultType::TranslationFault)
        .access_type(AccessType::Read)
        .security_state(SecurityState::NonSecure)
        .timestamp(0)
        .build();

    smmu.record_fault(fault);
    assert_eq!(smmu.get_faults().len(), 1);

    smmu.clear_faults();
    assert_eq!(smmu.get_faults().len(), 0);
}

#[test]
fn test_multiple_event_types() {
    let smmu = SMMU::new();

    // Submit different event types
    smmu.submit_event(EventEntry {
        event_type: EventType::FTranslation,
        stream_id: 1,
        pasid: 0,
        address: 0x1000,
        security_state: SecurityState::NonSecure,
        error_code: 0,
        timestamp: 1,
        stall: false,
        stag: 0,
    })
    .unwrap();

    smmu.submit_event(EventEntry {
        event_type: EventType::FPermission,
        stream_id: 2,
        pasid: 0,
        address: 0x2000,
        security_state: SecurityState::NonSecure,
        error_code: 0,
        timestamp: 2,
        stall: false,
        stag: 0,
    })
    .unwrap();

    smmu.submit_event(EventEntry {
        event_type: EventType::CBadSte,
        stream_id: 3,
        pasid: 0,
        address: 0x3000,
        security_state: SecurityState::NonSecure,
        error_code: 0,
        timestamp: 3,
        stall: false,
        stag: 0,
    })
    .unwrap();

    smmu.submit_event(EventEntry {
        event_type: EventType::FTlbConflict,
        stream_id: 4,
        pasid: 0,
        address: 0x4000,
        security_state: SecurityState::NonSecure,
        error_code: 0,
        timestamp: 4,
        stall: false,
        stag: 0,
    })
    .unwrap();

    let events = smmu.get_events();
    assert_eq!(events.len(), 4);
}
