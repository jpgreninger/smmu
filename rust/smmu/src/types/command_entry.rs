//! Command queue types for ARM SMMU v3
//!
//! Command queue processing per ARM SMMU v3 specification Section 6.4.

// Note: StreamID, IOVA, PASID types not currently used but available for future expansion

/// Command type enumeration
///
/// Defines all ARM SMMU v3 command types supported by the command queue.
/// Opcode values match ARM IHI0070G.b §4.1.1 exactly.
#[repr(u8)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum CommandType {
    // ---- Configuration prefetch commands (§4.2) ----
    /// Prefetch configuration — CMD_PREFETCH_CONFIG (opcode 0x01)
    PrefetchConfig = 0x01,
    /// Prefetch address — CMD_PREFETCH_ADDR (opcode 0x02)
    PrefetchAddr = 0x02,
    // ---- Configuration invalidation commands (§4.3) ----
    /// Stream Table Entry invalidation — CMD_CFGI_STE (opcode 0x03)
    CfgiSte = 0x03,
    /// All / STE-range configuration invalidation — CMD_CFGI_ALL / CMD_CFGI_STE_RANGE
    /// (opcode 0x04; the two commands share the same opcode per §4.1.1)
    CfgiAll = 0x04,
    /// Context Descriptor invalidation — CMD_CFGI_CD (opcode 0x05)
    ///
    /// Invalidates all information cached from the CD for the specified
    /// SubstreamID (PASID) of the specified stream (ARM §4.3.3).
    CfgiCd = 0x05,
    /// All-CDs invalidation — CMD_CFGI_CD_ALL (opcode 0x06)
    ///
    /// Invalidates all information cached from every CD of the specified
    /// stream (ARM §4.3.4).
    CfgiCdAll = 0x06,
    // ---- TLB invalidation — Non-secure Hyp (§4.4) ----
    /// CMD_TLBI_NH_ALL (opcode 0x10)
    TlbiNhAll = 0x10,
    /// CMD_TLBI_NH_ASID (opcode 0x11)
    TlbiNhAsid = 0x11,
    /// CMD_TLBI_NH_VA (opcode 0x12)
    TlbiNhVa = 0x12,
    /// CMD_TLBI_NH_VAA (opcode 0x13)
    TlbiNhVaa = 0x13,
    // ---- TLB invalidation — EL2 (§4.4) ----
    /// CMD_TLBI_EL2_ALL (opcode 0x20)
    TlbiEl2All = 0x20,
    /// CMD_TLBI_EL2_ASID (opcode 0x21)
    TlbiEl2Asid = 0x21,
    /// CMD_TLBI_EL2_VA (opcode 0x22)
    TlbiEl2Va = 0x22,
    /// CMD_TLBI_EL2_VAA (opcode 0x23)
    TlbiEl2Vaa = 0x23,
    // ---- TLB invalidation — Stage 1&2 (§4.4) ----
    /// CMD_TLBI_S12_VMALL (opcode 0x28)
    TlbiS12Vmall = 0x28,
    /// CMD_TLBI_S2_IPA (opcode 0x2A)
    TlbiS2Ipa = 0x2A,
    // ---- TLB invalidation — Non-secure Non-Hyp (§4.4) ----
    /// CMD_TLBI_NSNH_ALL (opcode 0x30)
    TlbiNsnhAll = 0x30,
    // ---- ATC / PRI commands (§4.5–§4.6) ----
    /// Address Translation Cache invalidation — CMD_ATC_INV (opcode 0x40)
    AtcInv = 0x40,
    /// Page Request Interface response — CMD_PRI_RESP (opcode 0x41)
    PriResp = 0x41,
    // ---- Resume / Stall commands (§4.7) ----
    /// Resume stalled transaction — CMD_RESUME (opcode 0x44)
    Resume = 0x44,
    /// Terminate stalled transaction — CMD_STALL_TERM (opcode 0x45)
    StallTerm = 0x45,
    // ---- Synchronization (§4.8) ----
    /// Synchronization barrier — CMD_SYNC (opcode 0x46)
    Sync = 0x46,
}

impl Default for CommandType {
    fn default() -> Self {
        Self::Sync
    }
}

/// Command entry structure
///
/// Contains all information about a single command in the command queue.
/// Follows ARM SMMU v3 command format.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CommandEntry {
    /// Command type
    pub cmd_type: CommandType,
    /// Target stream identifier (raw u32 for simpler access)
    pub stream_id: u32,
    /// Target Process Address Space ID (raw u32 for simpler access)
    pub pasid: u32,
    /// Start address for range operations (raw u64 for simpler access)
    pub start_address: u64,
    /// End address for range operations (raw u64 for simpler access)
    pub end_address: u64,
    /// Command-specific flags
    pub flags: u32,
    /// Command timestamp
    pub timestamp: u64,
    /// Target ASID for `CMD_TLBI_NH_ASID` / `CMD_TLBI_EL2_ASID` (ARM §4.4).
    /// Ignored by other command types.
    pub asid: u16,

    /// Target VMID for `CMD_TLBI_S12_VMALL` / `CMD_TLBI_S2_IPA` (ARM §4.4, §5.2).
    /// Ignored by other command types.
    pub vmid: u16,

    /// Stall TAG for `CMD_RESUME` / `CMD_STALL_TERM` (ARM §4.6–§4.7, §3.12.2).
    /// Identifies the stalled transaction group to resume or terminate.
    /// Ignored by other command types.
    pub stag: u16,
}

impl CommandEntry {
    /// Create a new command entry
    #[must_use]
    pub const fn new(cmd_type: CommandType, stream_id: u32, pasid: u32) -> Self {
        Self {
            cmd_type,
            stream_id,
            pasid,
            start_address: 0,
            end_address: 0,
            flags: 0,
            timestamp: 0,
            asid: 0,
            vmid: 0,
            stag: 0,
        }
    }
}
