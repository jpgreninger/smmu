#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]

//! Comprehensive tests for `types/queue_statistics.rs`
//!
//! Tests cover:
//! - `QueueStatistics` construction and initialization
//! - All getter methods (const functions)
//! - Utilization calculation for all three queue types
//! - Edge cases: empty queues, full queues, zero capacity, over-capacity
//! - Trait implementations (Clone, Debug, Default)
//! - Const context validation
//! - ARM SMMU v3 queue monitoring compliance

use smmu::types::QueueStatistics;

// ============================================================================
// Construction Tests
// ============================================================================

#[test]
fn test_queue_statistics_new() {
    let stats = QueueStatistics::new(10, 20, 30, 100, 200, 300);

    assert_eq!(stats.event_queue_size(), 10);
    assert_eq!(stats.command_queue_size(), 20);
    assert_eq!(stats.pri_queue_size(), 30);
}

#[test]
fn test_queue_statistics_new_zero_values() {
    let stats = QueueStatistics::new(0, 0, 0, 0, 0, 0);

    assert_eq!(stats.event_queue_size(), 0);
    assert_eq!(stats.command_queue_size(), 0);
    assert_eq!(stats.pri_queue_size(), 0);
}

#[test]
fn test_queue_statistics_new_maximum_values() {
    let stats = QueueStatistics::new(u64::MAX, u64::MAX, u64::MAX, usize::MAX, usize::MAX, usize::MAX);

    assert_eq!(stats.event_queue_size(), u64::MAX);
    assert_eq!(stats.command_queue_size(), u64::MAX);
    assert_eq!(stats.pri_queue_size(), u64::MAX);
}

#[test]
fn test_queue_statistics_new_mixed_values() {
    let stats = QueueStatistics::new(5, 0, 100, 10, 50, 200);

    assert_eq!(stats.event_queue_size(), 5);
    assert_eq!(stats.command_queue_size(), 0);
    assert_eq!(stats.pri_queue_size(), 100);
}

#[test]
fn test_queue_statistics_default() {
    let stats = QueueStatistics::default();

    assert_eq!(stats.event_queue_size(), 0);
    assert_eq!(stats.command_queue_size(), 0);
    assert_eq!(stats.pri_queue_size(), 0);
    assert_eq!(stats.event_queue_utilization(), 0.0);
    assert_eq!(stats.command_queue_utilization(), 0.0);
    assert_eq!(stats.pri_queue_utilization(), 0.0);
}

// ============================================================================
// Getter Tests
// ============================================================================

#[test]
fn test_event_queue_size_getter() {
    let stats = QueueStatistics::new(42, 0, 0, 100, 100, 100);
    assert_eq!(stats.event_queue_size(), 42);
}

#[test]
fn test_command_queue_size_getter() {
    let stats = QueueStatistics::new(0, 57, 0, 100, 100, 100);
    assert_eq!(stats.command_queue_size(), 57);
}

#[test]
fn test_pri_queue_size_getter() {
    let stats = QueueStatistics::new(0, 0, 88, 100, 100, 100);
    assert_eq!(stats.pri_queue_size(), 88);
}

#[test]
fn test_all_getters_together() {
    let stats = QueueStatistics::new(10, 20, 30, 100, 200, 300);

    assert_eq!(stats.event_queue_size(), 10);
    assert_eq!(stats.command_queue_size(), 20);
    assert_eq!(stats.pri_queue_size(), 30);
}

// ============================================================================
// Event Queue Utilization Tests
// ============================================================================

#[test]
fn test_event_queue_utilization_empty() {
    let stats = QueueStatistics::new(0, 0, 0, 100, 100, 100);
    assert_eq!(stats.event_queue_utilization(), 0.0);
}

#[test]
fn test_event_queue_utilization_half_full() {
    let stats = QueueStatistics::new(50, 0, 0, 100, 100, 100);
    assert_eq!(stats.event_queue_utilization(), 0.5);
}

#[test]
fn test_event_queue_utilization_full() {
    let stats = QueueStatistics::new(100, 0, 0, 100, 100, 100);
    assert_eq!(stats.event_queue_utilization(), 1.0);
}

#[test]
fn test_event_queue_utilization_over_capacity() {
    let stats = QueueStatistics::new(150, 0, 0, 100, 100, 100);
    assert_eq!(stats.event_queue_utilization(), 1.5);
}

#[test]
fn test_event_queue_utilization_zero_capacity() {
    let stats = QueueStatistics::new(50, 0, 0, 0, 100, 100);
    assert_eq!(stats.event_queue_utilization(), 0.0);
}

#[test]
fn test_event_queue_utilization_fractional() {
    let stats = QueueStatistics::new(33, 0, 0, 100, 100, 100);
    assert_eq!(stats.event_queue_utilization(), 0.33);
}

#[test]
fn test_event_queue_utilization_precision() {
    let stats = QueueStatistics::new(1, 0, 0, 3, 100, 100);
    let utilization = stats.event_queue_utilization();
    assert!((utilization - 0.333_333).abs() < 0.000_001);
}

// ============================================================================
// Command Queue Utilization Tests
// ============================================================================

#[test]
fn test_command_queue_utilization_empty() {
    let stats = QueueStatistics::new(0, 0, 0, 100, 100, 100);
    assert_eq!(stats.command_queue_utilization(), 0.0);
}

#[test]
fn test_command_queue_utilization_quarter_full() {
    let stats = QueueStatistics::new(0, 25, 0, 100, 100, 100);
    assert_eq!(stats.command_queue_utilization(), 0.25);
}

#[test]
fn test_command_queue_utilization_full() {
    let stats = QueueStatistics::new(0, 200, 0, 100, 200, 100);
    assert_eq!(stats.command_queue_utilization(), 1.0);
}

#[test]
fn test_command_queue_utilization_over_capacity() {
    let stats = QueueStatistics::new(0, 250, 0, 100, 200, 100);
    assert_eq!(stats.command_queue_utilization(), 1.25);
}

#[test]
fn test_command_queue_utilization_zero_capacity() {
    let stats = QueueStatistics::new(0, 50, 0, 100, 0, 100);
    assert_eq!(stats.command_queue_utilization(), 0.0);
}

#[test]
fn test_command_queue_utilization_fractional() {
    let stats = QueueStatistics::new(0, 75, 0, 100, 100, 100);
    assert_eq!(stats.command_queue_utilization(), 0.75);
}

#[test]
fn test_command_queue_utilization_precision() {
    let stats = QueueStatistics::new(0, 2, 0, 100, 7, 100);
    let utilization = stats.command_queue_utilization();
    assert!((utilization - 0.285_714).abs() < 0.000_001);
}

// ============================================================================
// PRI Queue Utilization Tests
// ============================================================================

#[test]
fn test_pri_queue_utilization_empty() {
    let stats = QueueStatistics::new(0, 0, 0, 100, 100, 100);
    assert_eq!(stats.pri_queue_utilization(), 0.0);
}

#[test]
fn test_pri_queue_utilization_three_quarters_full() {
    let stats = QueueStatistics::new(0, 0, 75, 100, 100, 100);
    assert_eq!(stats.pri_queue_utilization(), 0.75);
}

#[test]
fn test_pri_queue_utilization_full() {
    let stats = QueueStatistics::new(0, 0, 300, 100, 100, 300);
    assert_eq!(stats.pri_queue_utilization(), 1.0);
}

#[test]
fn test_pri_queue_utilization_over_capacity() {
    let stats = QueueStatistics::new(0, 0, 400, 100, 100, 300);
    let utilization = stats.pri_queue_utilization();
    assert!((utilization - 1.333_333).abs() < 0.000_001);
}

#[test]
fn test_pri_queue_utilization_zero_capacity() {
    let stats = QueueStatistics::new(0, 0, 50, 100, 100, 0);
    assert_eq!(stats.pri_queue_utilization(), 0.0);
}

#[test]
fn test_pri_queue_utilization_fractional() {
    let stats = QueueStatistics::new(0, 0, 90, 100, 100, 100);
    assert_eq!(stats.pri_queue_utilization(), 0.9);
}

#[test]
fn test_pri_queue_utilization_precision() {
    let stats = QueueStatistics::new(0, 0, 5, 100, 100, 13);
    let utilization = stats.pri_queue_utilization();
    assert!((utilization - 0.384_615).abs() < 0.000_001);
}

// ============================================================================
// All Queues Utilization Tests
// ============================================================================

#[test]
fn test_all_queues_utilization_empty() {
    let stats = QueueStatistics::new(0, 0, 0, 100, 200, 300);

    assert_eq!(stats.event_queue_utilization(), 0.0);
    assert_eq!(stats.command_queue_utilization(), 0.0);
    assert_eq!(stats.pri_queue_utilization(), 0.0);
}

#[test]
fn test_all_queues_utilization_half_full() {
    let stats = QueueStatistics::new(50, 100, 150, 100, 200, 300);

    assert_eq!(stats.event_queue_utilization(), 0.5);
    assert_eq!(stats.command_queue_utilization(), 0.5);
    assert_eq!(stats.pri_queue_utilization(), 0.5);
}

#[test]
fn test_all_queues_utilization_full() {
    let stats = QueueStatistics::new(100, 200, 300, 100, 200, 300);

    assert_eq!(stats.event_queue_utilization(), 1.0);
    assert_eq!(stats.command_queue_utilization(), 1.0);
    assert_eq!(stats.pri_queue_utilization(), 1.0);
}

#[test]
fn test_all_queues_utilization_mixed() {
    let stats = QueueStatistics::new(25, 100, 225, 100, 200, 300);

    assert_eq!(stats.event_queue_utilization(), 0.25);
    assert_eq!(stats.command_queue_utilization(), 0.5);
    assert_eq!(stats.pri_queue_utilization(), 0.75);
}

#[test]
fn test_all_queues_utilization_over_capacity() {
    let stats = QueueStatistics::new(150, 300, 450, 100, 200, 300);

    assert_eq!(stats.event_queue_utilization(), 1.5);
    assert_eq!(stats.command_queue_utilization(), 1.5);
    assert_eq!(stats.pri_queue_utilization(), 1.5);
}

// ============================================================================
// Trait Implementation Tests
// ============================================================================

#[test]
fn test_clone_trait() {
    let stats1 = QueueStatistics::new(10, 20, 30, 100, 200, 300);
    let stats2 = stats1;

    assert_eq!(stats2.event_queue_size(), 10);
    assert_eq!(stats2.command_queue_size(), 20);
    assert_eq!(stats2.pri_queue_size(), 30);
    assert_eq!(stats2.event_queue_utilization(), 0.1);
    assert_eq!(stats2.command_queue_utilization(), 0.1);
    assert_eq!(stats2.pri_queue_utilization(), 0.1);
}

#[test]
fn test_debug_trait() {
    let stats = QueueStatistics::new(10, 20, 30, 100, 200, 300);
    let debug_string = format!("{stats:?}");

    assert!(debug_string.contains("QueueStatistics"));
    assert!(debug_string.contains("event_queue_size"));
    assert!(debug_string.contains("command_queue_size"));
    assert!(debug_string.contains("pri_queue_size"));
}

#[test]
fn test_debug_trait_default() {
    let stats = QueueStatistics::default();
    let debug_string = format!("{stats:?}");

    assert!(debug_string.contains("QueueStatistics"));
}

// ============================================================================
// Const Context Tests
// ============================================================================

#[test]
fn test_const_constructor() {
    const STATS: QueueStatistics = QueueStatistics::new(10, 20, 30, 100, 200, 300);

    assert_eq!(STATS.event_queue_size(), 10);
    assert_eq!(STATS.command_queue_size(), 20);
    assert_eq!(STATS.pri_queue_size(), 30);
}

#[test]
fn test_const_getters() {
    const STATS: QueueStatistics = QueueStatistics::new(42, 57, 88, 100, 200, 300);

    const EVENT_SIZE: u64 = STATS.event_queue_size();
    const COMMAND_SIZE: u64 = STATS.command_queue_size();
    const PRI_SIZE: u64 = STATS.pri_queue_size();

    assert_eq!(EVENT_SIZE, 42);
    assert_eq!(COMMAND_SIZE, 57);
    assert_eq!(PRI_SIZE, 88);
}

// ============================================================================
// Edge Cases and Boundary Tests
// ============================================================================

#[test]
fn test_edge_case_single_entry_queue() {
    let stats = QueueStatistics::new(1, 1, 1, 1, 1, 1);

    assert_eq!(stats.event_queue_utilization(), 1.0);
    assert_eq!(stats.command_queue_utilization(), 1.0);
    assert_eq!(stats.pri_queue_utilization(), 1.0);
}

#[test]
fn test_edge_case_large_capacity_small_usage() {
    let stats = QueueStatistics::new(1, 1, 1, 10_000, 10_000, 10_000);

    assert_eq!(stats.event_queue_utilization(), 0.0001);
    assert_eq!(stats.command_queue_utilization(), 0.0001);
    assert_eq!(stats.pri_queue_utilization(), 0.0001);
}

#[test]
fn test_edge_case_small_capacity_large_usage() {
    let stats = QueueStatistics::new(10_000, 10_000, 10_000, 1, 1, 1);

    assert_eq!(stats.event_queue_utilization(), 10_000.0);
    assert_eq!(stats.command_queue_utilization(), 10_000.0);
    assert_eq!(stats.pri_queue_utilization(), 10_000.0);
}

#[test]
fn test_edge_case_mixed_zero_capacity() {
    let stats = QueueStatistics::new(50, 50, 50, 0, 100, 0);

    assert_eq!(stats.event_queue_utilization(), 0.0);
    assert_eq!(stats.command_queue_utilization(), 0.5);
    assert_eq!(stats.pri_queue_utilization(), 0.0);
}

#[test]
fn test_edge_case_asymmetric_queues() {
    let stats = QueueStatistics::new(10, 200, 3000, 100, 100, 100);

    assert_eq!(stats.event_queue_utilization(), 0.1);
    assert_eq!(stats.command_queue_utilization(), 2.0);
    assert_eq!(stats.pri_queue_utilization(), 30.0);
}

// ============================================================================
// Realistic Usage Scenarios
// ============================================================================

#[test]
fn test_realistic_low_load() {
    // Simulate low system load: ~10% utilization
    let stats = QueueStatistics::new(10, 5, 3, 100, 50, 30);

    assert_eq!(stats.event_queue_utilization(), 0.1);
    assert_eq!(stats.command_queue_utilization(), 0.1);
    assert_eq!(stats.pri_queue_utilization(), 0.1);
}

#[test]
fn test_realistic_medium_load() {
    // Simulate medium system load: ~50% utilization
    let stats = QueueStatistics::new(50, 100, 150, 100, 200, 300);

    assert_eq!(stats.event_queue_utilization(), 0.5);
    assert_eq!(stats.command_queue_utilization(), 0.5);
    assert_eq!(stats.pri_queue_utilization(), 0.5);
}

#[test]
fn test_realistic_high_load() {
    // Simulate high system load: ~90% utilization
    let stats = QueueStatistics::new(90, 180, 270, 100, 200, 300);

    assert_eq!(stats.event_queue_utilization(), 0.9);
    assert_eq!(stats.command_queue_utilization(), 0.9);
    assert_eq!(stats.pri_queue_utilization(), 0.9);
}

#[test]
fn test_realistic_critical_load() {
    // Simulate critical system load: near or at capacity
    let stats = QueueStatistics::new(99, 198, 297, 100, 200, 300);

    assert_eq!(stats.event_queue_utilization(), 0.99);
    assert_eq!(stats.command_queue_utilization(), 0.99);
    assert_eq!(stats.pri_queue_utilization(), 0.99);
}

#[test]
fn test_realistic_varying_queue_sizes() {
    // Different queue types may have different typical sizes
    let stats = QueueStatistics::new(
        32,  // Event queue: small, frequently processed
        128, // Command queue: medium, batch processing
        8,   // PRI queue: small, rarely used
        64,  // Event capacity
        256, // Command capacity
        32,  // PRI capacity
    );

    assert_eq!(stats.event_queue_utilization(), 0.5);
    assert_eq!(stats.command_queue_utilization(), 0.5);
    assert_eq!(stats.pri_queue_utilization(), 0.25);
}

// ============================================================================
// ARM SMMU v3 Specification Compliance Tests
// ============================================================================

#[test]
fn test_spec_compliance_queue_monitoring() {
    // ARM SMMU v3 specification requires queue monitoring capabilities
    // This ensures all three queue types can be monitored
    let stats = QueueStatistics::new(10, 20, 30, 100, 200, 300);

    // All queue sizes should be retrievable
    assert!(stats.event_queue_size() >= 0);
    assert!(stats.command_queue_size() >= 0);
    assert!(stats.pri_queue_size() >= 0);

    // All utilization metrics should be calculable
    assert!(stats.event_queue_utilization() >= 0.0);
    assert!(stats.command_queue_utilization() >= 0.0);
    assert!(stats.pri_queue_utilization() >= 0.0);
}

#[test]
fn test_spec_compliance_utilization_range() {
    // Utilization should be in valid range (0.0 to infinity for overflow)
    let stats_empty = QueueStatistics::new(0, 0, 0, 100, 100, 100);
    let stats_half = QueueStatistics::new(50, 50, 50, 100, 100, 100);
    let stats_full = QueueStatistics::new(100, 100, 100, 100, 100, 100);

    assert!(stats_empty.event_queue_utilization() >= 0.0);
    assert!(stats_half.event_queue_utilization() > 0.0 && stats_half.event_queue_utilization() < 1.0);
    assert!(stats_full.event_queue_utilization() >= 1.0);
}

#[test]
fn test_spec_compliance_zero_capacity_safety() {
    // Zero capacity should be handled gracefully (return 0.0, not panic)
    let stats = QueueStatistics::new(100, 100, 100, 0, 0, 0);

    assert_eq!(stats.event_queue_utilization(), 0.0);
    assert_eq!(stats.command_queue_utilization(), 0.0);
    assert_eq!(stats.pri_queue_utilization(), 0.0);
}
