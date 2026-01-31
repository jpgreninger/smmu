//! Comprehensive tests for types/command_entry.rs
//!
//! Tests cover:
//! - All 11 CommandType variants per ARM SMMU v3 Section 6.4
//! - CommandType trait implementations (Copy, Clone, Debug, PartialEq, Eq, Hash, Default)
//! - CommandEntry construction and field access
//! - CommandEntry trait implementations (Copy, Clone, Debug, PartialEq, Eq)
//! - Const context validation
//! - Discriminant value validation
//! - Realistic command queue scenarios
//! - ARM SMMU v3 specification compliance

use smmu::types::{CommandEntry, CommandType};
use std::collections::HashSet;

// ============================================================================
// CommandType Variant Tests
// ============================================================================

#[test]
fn test_command_type_prefetch_config() {
    let cmd = CommandType::PrefetchConfig;
    assert_eq!(cmd as u8, 0);
}

#[test]
fn test_command_type_prefetch_addr() {
    let cmd = CommandType::PrefetchAddr;
    assert_eq!(cmd as u8, 1);
}

#[test]
fn test_command_type_cfgi_ste() {
    let cmd = CommandType::CfgiSte;
    assert_eq!(cmd as u8, 2);
}

#[test]
fn test_command_type_cfgi_all() {
    let cmd = CommandType::CfgiAll;
    assert_eq!(cmd as u8, 3);
}

#[test]
fn test_command_type_tlbi_nh_all() {
    let cmd = CommandType::TlbiNhAll;
    assert_eq!(cmd as u8, 4);
}

#[test]
fn test_command_type_tlbi_el2_all() {
    let cmd = CommandType::TlbiEl2All;
    assert_eq!(cmd as u8, 5);
}

#[test]
fn test_command_type_tlbi_s12_vmall() {
    let cmd = CommandType::TlbiS12Vmall;
    assert_eq!(cmd as u8, 6);
}

#[test]
fn test_command_type_atc_inv() {
    let cmd = CommandType::AtcInv;
    assert_eq!(cmd as u8, 7);
}

#[test]
fn test_command_type_pri_resp() {
    let cmd = CommandType::PriResp;
    assert_eq!(cmd as u8, 8);
}

#[test]
fn test_command_type_resume() {
    let cmd = CommandType::Resume;
    assert_eq!(cmd as u8, 9);
}

#[test]
fn test_command_type_sync() {
    let cmd = CommandType::Sync;
    assert_eq!(cmd as u8, 10);
}

#[test]
fn test_command_type_all_variants() {
    // Ensure all 11 command types exist
    let commands = vec![
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

    assert_eq!(commands.len(), 11);

    // Verify each has unique discriminant
    let discriminants: Vec<u8> = commands.iter().map(|&c| c as u8).collect();
    assert_eq!(discriminants, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
}

// ============================================================================
// CommandType Default Trait Tests
// ============================================================================

#[test]
fn test_command_type_default() {
    let cmd = CommandType::default();
    assert_eq!(cmd, CommandType::Sync);
    assert_eq!(cmd as u8, 10);
}

// ============================================================================
// CommandType Copy/Clone Trait Tests
// ============================================================================

#[test]
fn test_command_type_copy() {
    let cmd1 = CommandType::CfgiAll;
    let cmd2 = cmd1; // Copy
    assert_eq!(cmd1, cmd2);
    assert_eq!(cmd1 as u8, 3);
    assert_eq!(cmd2 as u8, 3);
}

#[test]
fn test_command_type_clone() {
    let cmd1 = CommandType::TlbiNhAll;
    let cmd2 = cmd1.clone();
    assert_eq!(cmd1, cmd2);
    assert_eq!(cmd1 as u8, 4);
}

// ============================================================================
// CommandType Debug Trait Tests
// ============================================================================

#[test]
fn test_command_type_debug() {
    let cmd = CommandType::PrefetchConfig;
    let debug_string = format!("{cmd:?}");
    assert!(debug_string.contains("PrefetchConfig"));
}

#[test]
fn test_command_type_debug_all_variants() {
    let commands = vec![
        (CommandType::PrefetchConfig, "PrefetchConfig"),
        (CommandType::PrefetchAddr, "PrefetchAddr"),
        (CommandType::CfgiSte, "CfgiSte"),
        (CommandType::CfgiAll, "CfgiAll"),
        (CommandType::TlbiNhAll, "TlbiNhAll"),
        (CommandType::TlbiEl2All, "TlbiEl2All"),
        (CommandType::TlbiS12Vmall, "TlbiS12Vmall"),
        (CommandType::AtcInv, "AtcInv"),
        (CommandType::PriResp, "PriResp"),
        (CommandType::Resume, "Resume"),
        (CommandType::Sync, "Sync"),
    ];

    for (cmd, expected_name) in commands {
        let debug_string = format!("{cmd:?}");
        assert!(debug_string.contains(expected_name));
    }
}

// ============================================================================
// CommandType PartialEq/Eq Trait Tests
// ============================================================================

#[test]
fn test_command_type_equality() {
    let cmd1 = CommandType::CfgiSte;
    let cmd2 = CommandType::CfgiSte;
    let cmd3 = CommandType::CfgiAll;

    assert_eq!(cmd1, cmd2);
    assert_ne!(cmd1, cmd3);
}

#[test]
fn test_command_type_equality_all_pairs() {
    let cmd1 = CommandType::TlbiNhAll;
    let cmd2 = CommandType::TlbiNhAll;
    assert_eq!(cmd1, cmd2);

    let cmd3 = CommandType::TlbiEl2All;
    assert_ne!(cmd1, cmd3);
}

// ============================================================================
// CommandType Hash Trait Tests
// ============================================================================

#[test]
fn test_command_type_hash_set() {
    let mut set = HashSet::new();
    set.insert(CommandType::PrefetchConfig);
    set.insert(CommandType::PrefetchAddr);
    set.insert(CommandType::CfgiAll);
    set.insert(CommandType::CfgiAll); // Duplicate

    assert_eq!(set.len(), 3);
    assert!(set.contains(&CommandType::PrefetchConfig));
    assert!(set.contains(&CommandType::PrefetchAddr));
    assert!(set.contains(&CommandType::CfgiAll));
    assert!(!set.contains(&CommandType::Sync));
}

#[test]
fn test_command_type_hash_all_variants() {
    let mut set = HashSet::new();

    set.insert(CommandType::PrefetchConfig);
    set.insert(CommandType::PrefetchAddr);
    set.insert(CommandType::CfgiSte);
    set.insert(CommandType::CfgiAll);
    set.insert(CommandType::TlbiNhAll);
    set.insert(CommandType::TlbiEl2All);
    set.insert(CommandType::TlbiS12Vmall);
    set.insert(CommandType::AtcInv);
    set.insert(CommandType::PriResp);
    set.insert(CommandType::Resume);
    set.insert(CommandType::Sync);

    assert_eq!(set.len(), 11);
}

// ============================================================================
// CommandEntry Construction Tests
// ============================================================================

#[test]
fn test_command_entry_new() {
    let entry = CommandEntry::new(CommandType::Sync, 100, 200);

    assert_eq!(entry.cmd_type, CommandType::Sync);
    assert_eq!(entry.stream_id, 100);
    assert_eq!(entry.pasid, 200);
    assert_eq!(entry.start_address, 0);
    assert_eq!(entry.end_address, 0);
    assert_eq!(entry.flags, 0);
    assert_eq!(entry.timestamp, 0);
}

#[test]
fn test_command_entry_new_all_command_types() {
    let commands = vec![
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

    for cmd in commands {
        let entry = CommandEntry::new(cmd, 1, 2);
        assert_eq!(entry.cmd_type, cmd);
        assert_eq!(entry.stream_id, 1);
        assert_eq!(entry.pasid, 2);
    }
}

#[test]
fn test_command_entry_new_zero_ids() {
    let entry = CommandEntry::new(CommandType::CfgiAll, 0, 0);

    assert_eq!(entry.cmd_type, CommandType::CfgiAll);
    assert_eq!(entry.stream_id, 0);
    assert_eq!(entry.pasid, 0);
}

#[test]
fn test_command_entry_new_maximum_ids() {
    let entry = CommandEntry::new(CommandType::TlbiNhAll, u32::MAX, u32::MAX);

    assert_eq!(entry.cmd_type, CommandType::TlbiNhAll);
    assert_eq!(entry.stream_id, u32::MAX);
    assert_eq!(entry.pasid, u32::MAX);
}

// ============================================================================
// CommandEntry Field Access Tests
// ============================================================================

#[test]
fn test_command_entry_field_access() {
    let mut entry = CommandEntry::new(CommandType::PrefetchAddr, 10, 20);

    // Modify fields
    entry.start_address = 0x1000;
    entry.end_address = 0x2000;
    entry.flags = 0x1;
    entry.timestamp = 12345;

    assert_eq!(entry.cmd_type, CommandType::PrefetchAddr);
    assert_eq!(entry.stream_id, 10);
    assert_eq!(entry.pasid, 20);
    assert_eq!(entry.start_address, 0x1000);
    assert_eq!(entry.end_address, 0x2000);
    assert_eq!(entry.flags, 0x1);
    assert_eq!(entry.timestamp, 12345);
}

#[test]
fn test_command_entry_modify_all_fields() {
    let mut entry = CommandEntry::new(CommandType::Sync, 0, 0);

    entry.cmd_type = CommandType::AtcInv;
    entry.stream_id = 42;
    entry.pasid = 57;
    entry.start_address = 0xDEADBEEF;
    entry.end_address = 0xCAFEBABE;
    entry.flags = 0xFF;
    entry.timestamp = u64::MAX;

    assert_eq!(entry.cmd_type, CommandType::AtcInv);
    assert_eq!(entry.stream_id, 42);
    assert_eq!(entry.pasid, 57);
    assert_eq!(entry.start_address, 0xDEADBEEF);
    assert_eq!(entry.end_address, 0xCAFEBABE);
    assert_eq!(entry.flags, 0xFF);
    assert_eq!(entry.timestamp, u64::MAX);
}

// ============================================================================
// CommandEntry Copy/Clone Trait Tests
// ============================================================================

#[test]
fn test_command_entry_copy() {
    let entry1 = CommandEntry::new(CommandType::CfgiSte, 100, 200);
    let entry2 = entry1; // Copy

    assert_eq!(entry1.cmd_type, entry2.cmd_type);
    assert_eq!(entry1.stream_id, entry2.stream_id);
    assert_eq!(entry1.pasid, entry2.pasid);
}

#[test]
fn test_command_entry_clone() {
    let mut entry1 = CommandEntry::new(CommandType::TlbiEl2All, 10, 20);
    entry1.start_address = 0x1000;
    entry1.end_address = 0x2000;
    entry1.flags = 0x5;
    entry1.timestamp = 999;

    let entry2 = entry1.clone();

    assert_eq!(entry1.cmd_type, entry2.cmd_type);
    assert_eq!(entry1.stream_id, entry2.stream_id);
    assert_eq!(entry1.pasid, entry2.pasid);
    assert_eq!(entry1.start_address, entry2.start_address);
    assert_eq!(entry1.end_address, entry2.end_address);
    assert_eq!(entry1.flags, entry2.flags);
    assert_eq!(entry1.timestamp, entry2.timestamp);
}

// ============================================================================
// CommandEntry Debug Trait Tests
// ============================================================================

#[test]
fn test_command_entry_debug() {
    let entry = CommandEntry::new(CommandType::Sync, 42, 57);
    let debug_string = format!("{entry:?}");

    assert!(debug_string.contains("CommandEntry"));
    assert!(debug_string.contains("cmd_type"));
    assert!(debug_string.contains("stream_id"));
    assert!(debug_string.contains("pasid"));
}

#[test]
fn test_command_entry_debug_with_all_fields() {
    let mut entry = CommandEntry::new(CommandType::PrefetchConfig, 1, 2);
    entry.start_address = 0x1000;
    entry.end_address = 0x2000;
    entry.flags = 0x1;
    entry.timestamp = 12345;

    let debug_string = format!("{entry:?}");
    assert!(debug_string.contains("CommandEntry"));
}

// ============================================================================
// CommandEntry PartialEq/Eq Trait Tests
// ============================================================================

#[test]
fn test_command_entry_equality() {
    let entry1 = CommandEntry::new(CommandType::CfgiAll, 10, 20);
    let entry2 = CommandEntry::new(CommandType::CfgiAll, 10, 20);
    let entry3 = CommandEntry::new(CommandType::CfgiAll, 10, 21);

    assert_eq!(entry1, entry2);
    assert_ne!(entry1, entry3);
}

#[test]
fn test_command_entry_equality_all_fields() {
    let mut entry1 = CommandEntry::new(CommandType::AtcInv, 100, 200);
    entry1.start_address = 0x1000;
    entry1.end_address = 0x2000;
    entry1.flags = 0x5;
    entry1.timestamp = 999;

    let mut entry2 = CommandEntry::new(CommandType::AtcInv, 100, 200);
    entry2.start_address = 0x1000;
    entry2.end_address = 0x2000;
    entry2.flags = 0x5;
    entry2.timestamp = 999;

    assert_eq!(entry1, entry2);

    // Change one field
    entry2.flags = 0x6;
    assert_ne!(entry1, entry2);
}

// ============================================================================
// Const Context Tests
// ============================================================================

#[test]
fn test_command_entry_const_constructor() {
    const ENTRY: CommandEntry = CommandEntry::new(CommandType::Sync, 42, 57);

    assert_eq!(ENTRY.cmd_type, CommandType::Sync);
    assert_eq!(ENTRY.stream_id, 42);
    assert_eq!(ENTRY.pasid, 57);
    assert_eq!(ENTRY.start_address, 0);
    assert_eq!(ENTRY.end_address, 0);
    assert_eq!(ENTRY.flags, 0);
    assert_eq!(ENTRY.timestamp, 0);
}

// ============================================================================
// ARM SMMU v3 Command Scenarios (Section 6.4)
// ============================================================================

#[test]
fn test_arm_spec_prefetch_config_command() {
    // ARM SMMU v3 Section 6.4: PREFETCH_CONFIG command
    let entry = CommandEntry::new(CommandType::PrefetchConfig, 10, 0);
    assert_eq!(entry.cmd_type, CommandType::PrefetchConfig);
    assert_eq!(entry.cmd_type as u8, 0);
}

#[test]
fn test_arm_spec_prefetch_addr_command() {
    // ARM SMMU v3 Section 6.4: PREFETCH_ADDR command
    let mut entry = CommandEntry::new(CommandType::PrefetchAddr, 10, 5);
    entry.start_address = 0x1000;

    assert_eq!(entry.cmd_type, CommandType::PrefetchAddr);
    assert_eq!(entry.stream_id, 10);
    assert_eq!(entry.pasid, 5);
    assert_eq!(entry.start_address, 0x1000);
}

#[test]
fn test_arm_spec_cfgi_ste_command() {
    // ARM SMMU v3 Section 6.4: CFGI_STE (Stream Table Entry invalidation)
    let entry = CommandEntry::new(CommandType::CfgiSte, 42, 0);

    assert_eq!(entry.cmd_type, CommandType::CfgiSte);
    assert_eq!(entry.stream_id, 42);
}

#[test]
fn test_arm_spec_cfgi_all_command() {
    // ARM SMMU v3 Section 6.4: CFGI_ALL (invalidate all configuration)
    let entry = CommandEntry::new(CommandType::CfgiAll, 0, 0);

    assert_eq!(entry.cmd_type, CommandType::CfgiAll);
}

#[test]
fn test_arm_spec_tlbi_nh_all_command() {
    // ARM SMMU v3 Section 6.4: TLBI_NH_ALL (TLB invalidation non-secure hyp all)
    let entry = CommandEntry::new(CommandType::TlbiNhAll, 0, 0);

    assert_eq!(entry.cmd_type, CommandType::TlbiNhAll);
    assert_eq!(entry.cmd_type as u8, 4);
}

#[test]
fn test_arm_spec_tlbi_el2_all_command() {
    // ARM SMMU v3 Section 6.4: TLBI_EL2_ALL
    let entry = CommandEntry::new(CommandType::TlbiEl2All, 0, 0);

    assert_eq!(entry.cmd_type, CommandType::TlbiEl2All);
    assert_eq!(entry.cmd_type as u8, 5);
}

#[test]
fn test_arm_spec_tlbi_s12_vmall_command() {
    // ARM SMMU v3 Section 6.4: TLBI_S12_VMALL (Stage 1&2 VM all)
    let entry = CommandEntry::new(CommandType::TlbiS12Vmall, 10, 0);

    assert_eq!(entry.cmd_type, CommandType::TlbiS12Vmall);
    assert_eq!(entry.stream_id, 10);
}

#[test]
fn test_arm_spec_atc_inv_command() {
    // ARM SMMU v3 Section 6.4: ATC_INV (Address Translation Cache invalidation)
    let mut entry = CommandEntry::new(CommandType::AtcInv, 15, 20);
    entry.start_address = 0x1000;
    entry.end_address = 0x2000;

    assert_eq!(entry.cmd_type, CommandType::AtcInv);
    assert_eq!(entry.stream_id, 15);
    assert_eq!(entry.pasid, 20);
}

#[test]
fn test_arm_spec_pri_resp_command() {
    // ARM SMMU v3 Section 6.4: PRI_RESP (Page Request Interface response)
    let entry = CommandEntry::new(CommandType::PriResp, 5, 10);

    assert_eq!(entry.cmd_type, CommandType::PriResp);
    assert_eq!(entry.stream_id, 5);
    assert_eq!(entry.pasid, 10);
}

#[test]
fn test_arm_spec_resume_command() {
    // ARM SMMU v3 Section 6.4: RESUME
    let entry = CommandEntry::new(CommandType::Resume, 8, 12);

    assert_eq!(entry.cmd_type, CommandType::Resume);
    assert_eq!(entry.stream_id, 8);
    assert_eq!(entry.pasid, 12);
}

#[test]
fn test_arm_spec_sync_command() {
    // ARM SMMU v3 Section 6.4: SYNC (synchronization barrier)
    let entry = CommandEntry::new(CommandType::Sync, 0, 0);

    assert_eq!(entry.cmd_type, CommandType::Sync);
    assert_eq!(entry.cmd_type as u8, 10);
}

// ============================================================================
// Realistic Command Queue Scenarios
// ============================================================================

#[test]
fn test_realistic_configuration_invalidation_sequence() {
    // Typical configuration change sequence
    let commands = vec![
        CommandEntry::new(CommandType::CfgiSte, 10, 0),
        CommandEntry::new(CommandType::Sync, 0, 0),
    ];

    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0].cmd_type, CommandType::CfgiSte);
    assert_eq!(commands[1].cmd_type, CommandType::Sync);
}

#[test]
fn test_realistic_tlb_invalidation_sequence() {
    // TLB invalidation after page table update
    let commands = vec![
        CommandEntry::new(CommandType::TlbiNhAll, 0, 0),
        CommandEntry::new(CommandType::Sync, 0, 0),
    ];

    assert_eq!(commands[0].cmd_type, CommandType::TlbiNhAll);
    assert_eq!(commands[1].cmd_type, CommandType::Sync);
}

#[test]
fn test_realistic_address_range_prefetch() {
    // Prefetch address range for performance
    let mut entry = CommandEntry::new(CommandType::PrefetchAddr, 5, 10);
    entry.start_address = 0x1000;
    entry.end_address = 0x10000;

    assert_eq!(entry.cmd_type, CommandType::PrefetchAddr);
    assert_eq!(entry.start_address, 0x1000);
    assert_eq!(entry.end_address, 0x10000);
}

#[test]
fn test_realistic_multi_stream_invalidation() {
    // Invalidate multiple streams
    let commands = vec![
        CommandEntry::new(CommandType::CfgiSte, 10, 0),
        CommandEntry::new(CommandType::CfgiSte, 20, 0),
        CommandEntry::new(CommandType::CfgiSte, 30, 0),
        CommandEntry::new(CommandType::Sync, 0, 0),
    ];

    assert_eq!(commands.len(), 4);
    assert_eq!(commands[0].stream_id, 10);
    assert_eq!(commands[1].stream_id, 20);
    assert_eq!(commands[2].stream_id, 30);
}

#[test]
fn test_realistic_command_with_timestamp() {
    // Command with timestamp for ordering
    let mut entry = CommandEntry::new(CommandType::CfgiAll, 0, 0);
    entry.timestamp = 1234567890;

    assert_eq!(entry.timestamp, 1234567890);
}

#[test]
fn test_realistic_command_with_flags() {
    // Command with flags for additional options
    let mut entry = CommandEntry::new(CommandType::AtcInv, 5, 10);
    entry.flags = 0x3; // Some flags set

    assert_eq!(entry.flags, 0x3);
}

// ============================================================================
// Edge Cases and Boundary Tests
// ============================================================================

#[test]
fn test_edge_case_zero_address_range() {
    let mut entry = CommandEntry::new(CommandType::PrefetchAddr, 1, 2);
    entry.start_address = 0;
    entry.end_address = 0;

    assert_eq!(entry.start_address, entry.end_address);
}

#[test]
fn test_edge_case_maximum_address_range() {
    let mut entry = CommandEntry::new(CommandType::AtcInv, 1, 2);
    entry.start_address = 0;
    entry.end_address = u64::MAX;

    assert_eq!(entry.start_address, 0);
    assert_eq!(entry.end_address, u64::MAX);
}

#[test]
fn test_edge_case_inverted_address_range() {
    // Start > End (implementation should handle or validate elsewhere)
    let mut entry = CommandEntry::new(CommandType::PrefetchAddr, 1, 2);
    entry.start_address = 0x2000;
    entry.end_address = 0x1000;

    assert!(entry.start_address > entry.end_address);
}

#[test]
fn test_edge_case_all_flags_set() {
    let mut entry = CommandEntry::new(CommandType::Sync, 0, 0);
    entry.flags = u32::MAX;

    assert_eq!(entry.flags, u32::MAX);
}

// ============================================================================
// Collection Usage Tests
// ============================================================================

#[test]
fn test_command_queue_vec() {
    let mut queue = Vec::new();

    queue.push(CommandEntry::new(CommandType::CfgiAll, 0, 0));
    queue.push(CommandEntry::new(CommandType::TlbiNhAll, 0, 0));
    queue.push(CommandEntry::new(CommandType::Sync, 0, 0));

    assert_eq!(queue.len(), 3);
    assert_eq!(queue[0].cmd_type, CommandType::CfgiAll);
    assert_eq!(queue[1].cmd_type, CommandType::TlbiNhAll);
    assert_eq!(queue[2].cmd_type, CommandType::Sync);
}

#[test]
fn test_command_type_ordering_in_queue() {
    let commands = vec![
        CommandEntry::new(CommandType::PrefetchConfig, 1, 0),
        CommandEntry::new(CommandType::CfgiSte, 2, 0),
        CommandEntry::new(CommandType::TlbiNhAll, 0, 0),
        CommandEntry::new(CommandType::Sync, 0, 0),
    ];

    // Verify order is preserved
    assert_eq!(commands[0].cmd_type as u8, 0);
    assert_eq!(commands[1].cmd_type as u8, 2);
    assert_eq!(commands[2].cmd_type as u8, 4);
    assert_eq!(commands[3].cmd_type as u8, 10);
}

// ============================================================================
// ARM SMMU v3 Specification Compliance Summary
// ============================================================================

#[test]
fn test_spec_compliance_all_command_types_present() {
    // Verify all 11 command types from ARM SMMU v3 Section 6.4 are present
    let command_types = vec![
        CommandType::PrefetchConfig,  // 0
        CommandType::PrefetchAddr,    // 1
        CommandType::CfgiSte,         // 2
        CommandType::CfgiAll,         // 3
        CommandType::TlbiNhAll,       // 4
        CommandType::TlbiEl2All,      // 5
        CommandType::TlbiS12Vmall,    // 6
        CommandType::AtcInv,          // 7
        CommandType::PriResp,         // 8
        CommandType::Resume,          // 9
        CommandType::Sync,            // 10
    ];

    assert_eq!(command_types.len(), 11);

    // Verify discriminants match specification
    for (i, cmd) in command_types.iter().enumerate() {
        assert_eq!(*cmd as u8, i as u8);
    }
}

#[test]
fn test_spec_compliance_command_entry_structure() {
    // Verify CommandEntry has all required fields per ARM SMMU v3
    let entry = CommandEntry::new(CommandType::Sync, 42, 57);

    // Required fields
    let _ = entry.cmd_type;
    let _ = entry.stream_id;
    let _ = entry.pasid;
    let _ = entry.start_address;
    let _ = entry.end_address;
    let _ = entry.flags;
    let _ = entry.timestamp;
}
