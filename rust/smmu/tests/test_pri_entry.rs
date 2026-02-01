//! Comprehensive tests for types/pri_entry.rs
//!
//! Tests cover:
//! - PRIEntry construction with all parameters
//! - All AccessType variants (Read, Write, Execute, ReadWrite, etc.)
//! - Field access and modification (6 fields)
//! - Trait implementations (Copy, Clone, Debug, PartialEq, Eq)
//! - Const context validation
//! - ARM SMMU v3 PRI scenarios per Section 7
//! - Realistic page request handling
//! - Request grouping (is_last_request flag)
//! - Timestamp ordering
//! - Edge cases and boundary conditions

use smmu::types::{AccessType, PRIEntry};

// ============================================================================
// PRIEntry Construction Tests
// ============================================================================

#[test]
fn test_pri_entry_new() {
    let entry = PRIEntry::new(10, 20, 0x1000, AccessType::Read);

    assert_eq!(entry.stream_id, 10);
    assert_eq!(entry.pasid, 20);
    assert_eq!(entry.requested_address, 0x1000);
    assert_eq!(entry.access_type, AccessType::Read);
    assert_eq!(entry.is_last_request, false);
    assert_eq!(entry.timestamp, 0);
}

#[test]
fn test_pri_entry_new_with_write() {
    let entry = PRIEntry::new(5, 10, 0x2000, AccessType::Write);

    assert_eq!(entry.stream_id, 5);
    assert_eq!(entry.pasid, 10);
    assert_eq!(entry.requested_address, 0x2000);
    assert_eq!(entry.access_type, AccessType::Write);
    assert_eq!(entry.is_last_request, false);
    assert_eq!(entry.timestamp, 0);
}

#[test]
fn test_pri_entry_new_with_execute() {
    let entry = PRIEntry::new(15, 25, 0x3000, AccessType::Execute);

    assert_eq!(entry.access_type, AccessType::Execute);
}

#[test]
fn test_pri_entry_new_with_read_write() {
    let entry = PRIEntry::new(1, 2, 0x4000, AccessType::ReadWrite);

    assert_eq!(entry.access_type, AccessType::ReadWrite);
}

#[test]
fn test_pri_entry_new_with_read_execute() {
    let entry = PRIEntry::new(1, 2, 0x5000, AccessType::ReadExecute);

    assert_eq!(entry.access_type, AccessType::ReadExecute);
}

#[test]
fn test_pri_entry_new_with_write_execute() {
    let entry = PRIEntry::new(1, 2, 0x6000, AccessType::WriteExecute);

    assert_eq!(entry.access_type, AccessType::WriteExecute);
}

#[test]
fn test_pri_entry_new_with_read_write_execute() {
    let entry = PRIEntry::new(1, 2, 0x7000, AccessType::ReadWriteExecute);

    assert_eq!(entry.access_type, AccessType::ReadWriteExecute);
}

#[test]
fn test_pri_entry_new_zero_values() {
    let entry = PRIEntry::new(0, 0, 0, AccessType::Read);

    assert_eq!(entry.stream_id, 0);
    assert_eq!(entry.pasid, 0);
    assert_eq!(entry.requested_address, 0);
}

#[test]
fn test_pri_entry_new_maximum_values() {
    let entry = PRIEntry::new(u32::MAX, u32::MAX, u64::MAX, AccessType::ReadWriteExecute);

    assert_eq!(entry.stream_id, u32::MAX);
    assert_eq!(entry.pasid, u32::MAX);
    assert_eq!(entry.requested_address, u64::MAX);
}

#[test]
fn test_pri_entry_default_fields() {
    // Verify constructor sets default values for optional fields
    let entry = PRIEntry::new(1, 2, 0x1000, AccessType::Read);

    assert_eq!(entry.is_last_request, false);
    assert_eq!(entry.timestamp, 0);
}

// ============================================================================
// PRIEntry Field Access Tests
// ============================================================================

#[test]
fn test_pri_entry_modify_stream_id() {
    let mut entry = PRIEntry::new(10, 20, 0x1000, AccessType::Read);
    entry.stream_id = 100;

    assert_eq!(entry.stream_id, 100);
}

#[test]
fn test_pri_entry_modify_pasid() {
    let mut entry = PRIEntry::new(10, 20, 0x1000, AccessType::Read);
    entry.pasid = 200;

    assert_eq!(entry.pasid, 200);
}

#[test]
fn test_pri_entry_modify_requested_address() {
    let mut entry = PRIEntry::new(10, 20, 0x1000, AccessType::Read);
    entry.requested_address = 0x5000;

    assert_eq!(entry.requested_address, 0x5000);
}

#[test]
fn test_pri_entry_modify_access_type() {
    let mut entry = PRIEntry::new(10, 20, 0x1000, AccessType::Read);
    entry.access_type = AccessType::Write;

    assert_eq!(entry.access_type, AccessType::Write);
}

#[test]
fn test_pri_entry_modify_is_last_request() {
    let mut entry = PRIEntry::new(10, 20, 0x1000, AccessType::Read);
    entry.is_last_request = true;

    assert_eq!(entry.is_last_request, true);
}

#[test]
fn test_pri_entry_modify_timestamp() {
    let mut entry = PRIEntry::new(10, 20, 0x1000, AccessType::Read);
    entry.timestamp = 12345;

    assert_eq!(entry.timestamp, 12345);
}

#[test]
fn test_pri_entry_modify_all_fields() {
    let mut entry = PRIEntry::new(1, 2, 0x1000, AccessType::Read);

    entry.stream_id = 42;
    entry.pasid = 57;
    entry.requested_address = 0xDEADBEEF;
    entry.access_type = AccessType::ReadWriteExecute;
    entry.is_last_request = true;
    entry.timestamp = 9999;

    assert_eq!(entry.stream_id, 42);
    assert_eq!(entry.pasid, 57);
    assert_eq!(entry.requested_address, 0xDEADBEEF);
    assert_eq!(entry.access_type, AccessType::ReadWriteExecute);
    assert_eq!(entry.is_last_request, true);
    assert_eq!(entry.timestamp, 9999);
}

// ============================================================================
// PRIEntry Copy/Clone Trait Tests
// ============================================================================

#[test]
fn test_pri_entry_copy() {
    let entry1 = PRIEntry::new(10, 20, 0x1000, AccessType::Read);
    let entry2 = entry1; // Copy

    assert_eq!(entry1.stream_id, entry2.stream_id);
    assert_eq!(entry1.pasid, entry2.pasid);
    assert_eq!(entry1.requested_address, entry2.requested_address);
    assert_eq!(entry1.access_type, entry2.access_type);
}

#[test]
fn test_pri_entry_clone() {
    let mut entry1 = PRIEntry::new(10, 20, 0x1000, AccessType::ReadWrite);
    entry1.is_last_request = true;
    entry1.timestamp = 5555;

    let entry2 = entry1.clone();

    assert_eq!(entry1.stream_id, entry2.stream_id);
    assert_eq!(entry1.pasid, entry2.pasid);
    assert_eq!(entry1.requested_address, entry2.requested_address);
    assert_eq!(entry1.access_type, entry2.access_type);
    assert_eq!(entry1.is_last_request, entry2.is_last_request);
    assert_eq!(entry1.timestamp, entry2.timestamp);
}

// ============================================================================
// PRIEntry Debug Trait Tests
// ============================================================================

#[test]
fn test_pri_entry_debug() {
    let entry = PRIEntry::new(42, 57, 0x1000, AccessType::Read);
    let debug_string = format!("{entry:?}");

    assert!(debug_string.contains("PRIEntry"));
    assert!(debug_string.contains("stream_id"));
    assert!(debug_string.contains("pasid"));
    assert!(debug_string.contains("requested_address"));
    assert!(debug_string.contains("access_type"));
}

#[test]
fn test_pri_entry_debug_with_all_fields() {
    let mut entry = PRIEntry::new(1, 2, 0x1000, AccessType::Write);
    entry.is_last_request = true;
    entry.timestamp = 12345;

    let debug_string = format!("{entry:?}");
    assert!(debug_string.contains("PRIEntry"));
}

// ============================================================================
// PRIEntry PartialEq/Eq Trait Tests
// ============================================================================

#[test]
fn test_pri_entry_equality() {
    let entry1 = PRIEntry::new(10, 20, 0x1000, AccessType::Read);
    let entry2 = PRIEntry::new(10, 20, 0x1000, AccessType::Read);
    let entry3 = PRIEntry::new(10, 20, 0x1000, AccessType::Write);

    assert_eq!(entry1, entry2);
    assert_ne!(entry1, entry3);
}

#[test]
fn test_pri_entry_equality_all_fields() {
    let mut entry1 = PRIEntry::new(100, 200, 0x1000, AccessType::ReadWrite);
    entry1.is_last_request = true;
    entry1.timestamp = 999;

    let mut entry2 = PRIEntry::new(100, 200, 0x1000, AccessType::ReadWrite);
    entry2.is_last_request = true;
    entry2.timestamp = 999;

    assert_eq!(entry1, entry2);

    // Change one field
    entry2.timestamp = 1000;
    assert_ne!(entry1, entry2);
}

#[test]
fn test_pri_entry_equality_different_stream_id() {
    let entry1 = PRIEntry::new(10, 20, 0x1000, AccessType::Read);
    let entry2 = PRIEntry::new(11, 20, 0x1000, AccessType::Read);

    assert_ne!(entry1, entry2);
}

#[test]
fn test_pri_entry_equality_different_pasid() {
    let entry1 = PRIEntry::new(10, 20, 0x1000, AccessType::Read);
    let entry2 = PRIEntry::new(10, 21, 0x1000, AccessType::Read);

    assert_ne!(entry1, entry2);
}

#[test]
fn test_pri_entry_equality_different_address() {
    let entry1 = PRIEntry::new(10, 20, 0x1000, AccessType::Read);
    let entry2 = PRIEntry::new(10, 20, 0x2000, AccessType::Read);

    assert_ne!(entry1, entry2);
}

// ============================================================================
// Const Context Tests
// ============================================================================

#[test]
fn test_pri_entry_const_constructor() {
    const ENTRY: PRIEntry = PRIEntry::new(42, 57, 0x1000, AccessType::Read);

    assert_eq!(ENTRY.stream_id, 42);
    assert_eq!(ENTRY.pasid, 57);
    assert_eq!(ENTRY.requested_address, 0x1000);
    assert_eq!(ENTRY.access_type, AccessType::Read);
    assert_eq!(ENTRY.is_last_request, false);
    assert_eq!(ENTRY.timestamp, 0);
}

// ============================================================================
// ARM SMMU v3 PRI Scenarios (Section 7)
// ============================================================================

#[test]
fn test_arm_spec_page_fault_read_request() {
    // ARM SMMU v3 Section 7: Page fault with read access
    let entry = PRIEntry::new(10, 5, 0x1000, AccessType::Read);

    assert_eq!(entry.stream_id, 10);
    assert_eq!(entry.pasid, 5);
    assert_eq!(entry.requested_address, 0x1000);
    assert_eq!(entry.access_type, AccessType::Read);
}

#[test]
fn test_arm_spec_page_fault_write_request() {
    // ARM SMMU v3 Section 7: Page fault with write access
    let entry = PRIEntry::new(15, 8, 0x2000, AccessType::Write);

    assert_eq!(entry.access_type, AccessType::Write);
}

#[test]
fn test_arm_spec_page_fault_execute_request() {
    // ARM SMMU v3 Section 7: Page fault with execute access
    let entry = PRIEntry::new(20, 12, 0x3000, AccessType::Execute);

    assert_eq!(entry.access_type, AccessType::Execute);
}

#[test]
fn test_arm_spec_last_request_in_group() {
    // ARM SMMU v3 Section 7: Last request in a group
    let mut entry = PRIEntry::new(10, 5, 0x1000, AccessType::Read);
    entry.is_last_request = true;

    assert_eq!(entry.is_last_request, true);
}

#[test]
fn test_arm_spec_request_with_timestamp() {
    // ARM SMMU v3 Section 7: Request ordering via timestamp
    let mut entry = PRIEntry::new(10, 5, 0x1000, AccessType::Read);
    entry.timestamp = 1234567890;

    assert_eq!(entry.timestamp, 1234567890);
}

// ============================================================================
// Realistic Page Request Scenarios
// ============================================================================

#[test]
fn test_realistic_single_page_request() {
    // Single page fault request
    let entry = PRIEntry::new(10, 20, 0x10000, AccessType::Read);

    assert_eq!(entry.stream_id, 10);
    assert_eq!(entry.pasid, 20);
    assert_eq!(entry.requested_address, 0x10000);
    assert_eq!(entry.access_type, AccessType::Read);
    assert_eq!(entry.is_last_request, false);
}

#[test]
fn test_realistic_multi_request_group() {
    // Multiple requests in a group
    let mut req1 = PRIEntry::new(10, 20, 0x1000, AccessType::Read);
    req1.is_last_request = false;
    req1.timestamp = 100;

    let mut req2 = PRIEntry::new(10, 20, 0x2000, AccessType::Read);
    req2.is_last_request = false;
    req2.timestamp = 101;

    let mut req3 = PRIEntry::new(10, 20, 0x3000, AccessType::Read);
    req3.is_last_request = true; // Last in group
    req3.timestamp = 102;

    assert_eq!(req1.is_last_request, false);
    assert_eq!(req2.is_last_request, false);
    assert_eq!(req3.is_last_request, true);
}

#[test]
fn test_realistic_write_fault_request() {
    // Write fault requiring page allocation
    let entry = PRIEntry::new(5, 10, 0x5000, AccessType::Write);

    assert_eq!(entry.access_type, AccessType::Write);
}

#[test]
fn test_realistic_code_page_fault() {
    // Code page fault (execute permission)
    let entry = PRIEntry::new(8, 15, 0x8000, AccessType::Execute);

    assert_eq!(entry.access_type, AccessType::Execute);
}

#[test]
fn test_realistic_data_and_code_page() {
    // Page requiring both read and execute (code page)
    let entry = PRIEntry::new(12, 25, 0xC000, AccessType::ReadExecute);

    assert_eq!(entry.access_type, AccessType::ReadExecute);
}

#[test]
fn test_realistic_stack_page_fault() {
    // Stack page requiring read/write access
    let entry = PRIEntry::new(3, 7, 0x7FFF_F000, AccessType::ReadWrite);

    assert_eq!(entry.access_type, AccessType::ReadWrite);
    assert_eq!(entry.requested_address, 0x7FFF_F000);
}

// ============================================================================
// PRI Queue Operations
// ============================================================================

#[test]
fn test_pri_queue_vec_operations() {
    let mut queue = Vec::new();

    queue.push(PRIEntry::new(10, 20, 0x1000, AccessType::Read));
    queue.push(PRIEntry::new(10, 20, 0x2000, AccessType::Write));
    queue.push(PRIEntry::new(10, 20, 0x3000, AccessType::Execute));

    assert_eq!(queue.len(), 3);
    assert_eq!(queue[0].requested_address, 0x1000);
    assert_eq!(queue[1].requested_address, 0x2000);
    assert_eq!(queue[2].requested_address, 0x3000);
}

#[test]
fn test_pri_queue_timestamp_ordering() {
    let mut req1 = PRIEntry::new(10, 20, 0x1000, AccessType::Read);
    req1.timestamp = 300;

    let mut req2 = PRIEntry::new(10, 20, 0x2000, AccessType::Read);
    req2.timestamp = 100;

    let mut req3 = PRIEntry::new(10, 20, 0x3000, AccessType::Read);
    req3.timestamp = 200;

    let mut queue = vec![req1, req2, req3];
    queue.sort_by_key(|e| e.timestamp);

    assert_eq!(queue[0].timestamp, 100);
    assert_eq!(queue[1].timestamp, 200);
    assert_eq!(queue[2].timestamp, 300);
}

#[test]
fn test_pri_queue_filtering_by_pasid() {
    let queue = vec![
        PRIEntry::new(10, 1, 0x1000, AccessType::Read),
        PRIEntry::new(10, 2, 0x2000, AccessType::Read),
        PRIEntry::new(10, 1, 0x3000, AccessType::Read),
        PRIEntry::new(10, 3, 0x4000, AccessType::Read),
    ];

    let pasid_1_requests: Vec<_> = queue.iter().filter(|e| e.pasid == 1).collect();

    assert_eq!(pasid_1_requests.len(), 2);
    assert_eq!(pasid_1_requests[0].requested_address, 0x1000);
    assert_eq!(pasid_1_requests[1].requested_address, 0x3000);
}

#[test]
fn test_pri_queue_filtering_by_access_type() {
    let queue = vec![
        PRIEntry::new(10, 20, 0x1000, AccessType::Read),
        PRIEntry::new(10, 20, 0x2000, AccessType::Write),
        PRIEntry::new(10, 20, 0x3000, AccessType::Read),
        PRIEntry::new(10, 20, 0x4000, AccessType::Execute),
    ];

    let read_requests: Vec<_> = queue.iter().filter(|e| e.access_type == AccessType::Read).collect();

    assert_eq!(read_requests.len(), 2);
}

// ============================================================================
// Request Grouping Tests
// ============================================================================

#[test]
fn test_request_group_identification() {
    let mut requests = vec![
        PRIEntry::new(10, 20, 0x1000, AccessType::Read),
        PRIEntry::new(10, 20, 0x2000, AccessType::Read),
        PRIEntry::new(10, 20, 0x3000, AccessType::Read),
    ];

    // Mark last request in group
    requests[2].is_last_request = true;

    let last_requests: Vec<_> = requests.iter().filter(|e| e.is_last_request).collect();

    assert_eq!(last_requests.len(), 1);
    assert_eq!(last_requests[0].requested_address, 0x3000);
}

#[test]
fn test_multiple_request_groups() {
    let mut requests = vec![
        PRIEntry::new(10, 20, 0x1000, AccessType::Read),
        PRIEntry::new(10, 20, 0x2000, AccessType::Read), // Last of group 1
        PRIEntry::new(10, 20, 0x3000, AccessType::Write),
        PRIEntry::new(10, 20, 0x4000, AccessType::Write), // Last of group 2
    ];

    requests[1].is_last_request = true;
    requests[3].is_last_request = true;

    let group_ends: Vec<_> = requests.iter().filter(|e| e.is_last_request).collect();

    assert_eq!(group_ends.len(), 2);
}

// ============================================================================
// Edge Cases and Boundary Tests
// ============================================================================

#[test]
fn test_edge_case_page_boundary() {
    // Request at page boundary (4KB aligned)
    let entry = PRIEntry::new(10, 20, 0x1000, AccessType::Read);

    assert_eq!(entry.requested_address % 0x1000, 0);
}

#[test]
fn test_edge_case_maximum_address() {
    // Maximum possible address
    let entry = PRIEntry::new(10, 20, u64::MAX, AccessType::Read);

    assert_eq!(entry.requested_address, u64::MAX);
}

#[test]
fn test_edge_case_zero_address() {
    // Null pointer dereference fault
    let entry = PRIEntry::new(10, 20, 0, AccessType::Read);

    assert_eq!(entry.requested_address, 0);
}

#[test]
fn test_edge_case_all_access_permissions() {
    // Page requiring all permissions
    let entry = PRIEntry::new(10, 20, 0x1000, AccessType::ReadWriteExecute);

    assert_eq!(entry.access_type, AccessType::ReadWriteExecute);
}

#[test]
fn test_edge_case_maximum_timestamp() {
    let mut entry = PRIEntry::new(10, 20, 0x1000, AccessType::Read);
    entry.timestamp = u64::MAX;

    assert_eq!(entry.timestamp, u64::MAX);
}

// ============================================================================
// Access Type Coverage Tests
// ============================================================================

#[test]
fn test_all_access_types_coverage() {
    let access_types = vec![
        AccessType::Read,
        AccessType::Write,
        AccessType::Execute,
        AccessType::ReadWrite,
        AccessType::ReadExecute,
        AccessType::WriteExecute,
        AccessType::ReadWriteExecute,
    ];

    for access_type in access_types {
        let entry = PRIEntry::new(10, 20, 0x1000, access_type);
        assert_eq!(entry.access_type, access_type);
    }
}

// ============================================================================
// ARM SMMU v3 Specification Compliance
// ============================================================================

#[test]
fn test_spec_compliance_pri_entry_structure() {
    // Verify PRIEntry has all required fields per ARM SMMU v3 Section 7
    let entry = PRIEntry::new(42, 57, 0x1000, AccessType::Read);

    // Required fields
    let _ = entry.stream_id;
    let _ = entry.pasid;
    let _ = entry.requested_address;
    let _ = entry.access_type;
    let _ = entry.is_last_request;
    let _ = entry.timestamp;
}

#[test]
fn test_spec_compliance_pasid_0() {
    // ARM SMMU v3: PASID 0 is valid for non-PASID contexts
    let entry = PRIEntry::new(10, 0, 0x1000, AccessType::Read);

    assert_eq!(entry.pasid, 0);
}

#[test]
fn test_spec_compliance_request_grouping() {
    // ARM SMMU v3 Section 7: Request grouping with is_last_request flag
    let mut entry = PRIEntry::new(10, 20, 0x1000, AccessType::Read);

    // First request in group
    assert_eq!(entry.is_last_request, false);

    // Mark as last request
    entry.is_last_request = true;
    assert_eq!(entry.is_last_request, true);
}
