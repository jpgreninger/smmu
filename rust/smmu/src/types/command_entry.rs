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
    /// Secure substream PIDM cache invalidation — CMD_CFGI_VMS_PIDM (opcode 0x07)
    ///
    /// Invalidates PIDM-cached configuration for the specified secure substream
    /// per ARM IHI0070G.b §4.1.1.
    CfgiVmsPidm = 0x07,
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
    // ---- TLB invalidation — EL3 (§4.1.1) ----
    /// CMD_TLBI_EL3_ALL (opcode 0x18) — Invalidate all EL3 TLB entries
    TlbiEl3All = 0x18,
    /// CMD_TLBI_EL3_VA (opcode 0x1A) — Invalidate EL3 TLB entries by VA
    TlbiEl3Va = 0x1A,
    // ---- TLB invalidation — Non-secure Non-Hyp (§4.4) ----
    /// CMD_TLBI_NSNH_ALL (opcode 0x30)
    TlbiNsnhAll = 0x30,
    // ---- TLB invalidation — Secure EL2 (§4.1.1) ----
    /// CMD_TLBI_S_EL2_ALL (opcode 0x50) — Invalidate all Secure EL2 TLB entries
    TlbiSEl2All = 0x50,
    /// CMD_TLBI_S_EL2_ASID (opcode 0x51) — Invalidate Secure EL2 TLB by ASID
    TlbiSEl2Asid = 0x51,
    /// CMD_TLBI_S_EL2_VA (opcode 0x52) — Invalidate Secure EL2 TLB by VA
    TlbiSEl2Va = 0x52,
    /// CMD_TLBI_S_EL2_VAA (opcode 0x53) — Invalidate Secure EL2 TLB by VA, all ASID
    TlbiSEl2Vaa = 0x53,
    /// CMD_TLBI_S_S12_VMALL (opcode 0x58) — Invalidate all Secure S12 TLB by VMID
    TlbiSS12Vmall = 0x58,
    /// CMD_TLBI_S_S2_IPA (opcode 0x5A) — Invalidate Secure S2 TLB by IPA
    TlbiSS2Ipa = 0x5A,
    /// CMD_TLBI_S_NH_ALL (opcode 0x60) — Invalidate all Secure NH TLB entries
    TlbiSnhAll = 0x60,
    // ---- Dirty Page Tracking Invalidation (§4.1.1) ----
    /// CMD_DPTI_ALL (opcode 0x70) — Dirty page tracking invalidation, all
    DptiAll = 0x70,
    /// CMD_DPTI_PA (opcode 0x73) — Dirty page tracking invalidation by PA
    DptiPa = 0x73,
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

impl Default for CommandEntry {
    fn default() -> Self {
        Self::new(CommandType::Sync, 0, 0)
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

    /// Page Request Group index for `CMD_PRI_RESP` (ARM §8.3).
    ///
    /// The SMMU uses this value together with `stream_id` to locate and retire
    /// the matching [`PRIEntry`](crate::types::PRIEntry) in the PRI queue.
    /// Must equal the `prg_index` carried in the original `PRIEntry` (ARM §8.2).
    /// Ignored by all command types other than `PriResp`.
    pub prg_index: u16,

    /// Action (Ac) bit for `CMD_RESUME` (ARM §4.6, Table 4-10).
    ///
    /// When `true` (Ac=1), the SMMU retries the stalled transaction as if it
    /// arrived freshly. When `false` (Ac=0), the transaction is terminated;
    /// the `abort` bit further qualifies the termination outcome.
    /// Ignored by all command types other than `Resume`.
    pub action: bool,

    /// Abort (Ab) bit for `CMD_RESUME` (ARM §4.6, Table 4-10).
    ///
    /// Only meaningful when `action=false` (Ac=0):
    /// `true` (Ab=1) signals a bus error to the originating master;
    /// `false` (Ab=0) terminates the transaction silently (RAZ/WI).
    /// Ignored by all command types other than `Resume`.
    pub abort: bool,

    /// Range field for `CMD_CFGI_STE_RANGE` / `CMD_CFGI_ALL` (ARM §4.3.2).
    ///
    /// Both commands share opcode `0x04` (`CommandType::CfgiAll`); the `range`
    /// value distinguishes them:
    /// - `range == 31`: CMD_CFGI_ALL — invalidate all STE-cached TLB entries globally.
    /// - `range < 31`: CMD_CFGI_STE_RANGE — invalidate only streams whose StreamID
    ///   matches the upper-bit prefix `n[StreamIDSize-1 : range+1]`:
    ///   match condition `(sid >> (range+1)) == (command.stream_id >> (range+1))`.
    ///
    /// Defaults to 31 (CMD_CFGI_ALL semantics).
    /// Ignored by all command types other than `CfgiAll`.
    pub range: u8,

    /// ARM §4.3.1 / §4.3.3: Leaf bit for `CMD_CFGI_STE` and `CMD_CFGI_CD`.
    ///
    /// When `false` (Leaf=0), both the target entry and any cached intermediate
    /// L1ST/L1CD descriptor structures are invalidated.
    /// When `true` (Leaf=1), only the target entry is invalidated; intermediate
    /// structures need not be invalidated.
    ///
    /// This software model does not cache intermediate table structures, so
    /// both values produce equivalent results (semantically a no-op here).
    pub leaf: bool,

    /// ARM §4.8: CS (Completion Signalling) field for `CMD_SYNC`.
    ///
    /// Controls whether and how a completion signal is generated after the SYNC
    /// barrier completes:
    /// - `0b00` (`SIG_NONE`): no completion signal (ignored by SMMU)
    /// - `0b01` (`SIG_IRQ`): MSI write / IRQ completion signal
    /// - `0b10` (`SIG_MSI`): MSI write at `SMMU_GERROR_IRQ_CFG0`
    ///
    /// Ignored by all command types other than `Sync`.
    /// Defaults to 0 (SIG_NONE — no completion signal).
    pub cs: u8,

    // ---- CONF-GAP-8: Range Invalidation Leaf (RIL) fields (§4.4.1.1) ----

    /// Translation Granule (TG) for range-based TLBI (§4.4.1.1).
    ///
    /// - `0` = 4 KB granule
    /// - `1` = 64 KB granule
    /// - `2` = 16 KB granule
    ///
    /// Only used when `ril=true`.  Defaults to 0.
    pub tg: u8,

    /// Number of blocks minus 1 in the range (5 bits, §4.4.1.1).
    ///
    /// `blocks = num + 1`.  Only used when `ril=true`.  Defaults to 0.
    pub num: u8,

    /// Scale factor for the range (3 bits, §4.4.1.1).
    ///
    /// `total_blocks = (num + 1) << scale` (SCALE is used directly as the shift
    /// exponent per ARM IHI0070G.b §4.4.1.1).  Defaults to 0.
    pub scale: u8,

    /// TLB level hint for range invalidation (2 bits, §4.4.1.1).
    ///
    /// Hints at which TLB level the invalidation targets.  Defaults to 0.
    pub ttl: u8,

    /// Range Invalidation Leaf flag (§4.4.1.1).
    ///
    /// When `true`, the VA TLBI commands use range-based invalidation rather
    /// than single-address invalidation.  The range is computed from `tg`,
    /// `num`, and `scale`.  Defaults to `false`.
    pub ril: bool,

    /// Security state of the originating command (ARM §7.3 / §3.10.2.1).
    ///
    /// ARM SMMU v3 requires that command error events (e.g., `C_BAD_STREAMID`,
    /// `C_BAD_STE`) reflect the security state of the command that caused the
    /// error, not a hardcoded default.  This field is propagated into the
    /// `security_state` field of any event record generated when processing
    /// this command fails.
    ///
    /// Defaults to `SecurityState::NonSecure` (least privileged).
    pub security_state: crate::types::SecurityState,

    /// ARM §4.1.6: Security domain bit (SSec) for commands that carry a StreamID.
    ///
    /// On the Non-secure command queue this MUST be `false` (SSec=0). A value of
    /// `true` on the NS queue is ILLEGAL and must raise `CERROR_ILL` and signal
    /// `GERROR_CMDQ_ERR` per ARM §4.1.6.
    ///
    /// Defaults to `false` (Non-secure).
    pub ssec: bool,
}

impl CommandEntry {
    /// Create a new command entry
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::types::{CommandEntry, CommandType};
    ///
    /// let cmd = CommandEntry::new(CommandType::PriResp, 1, 0);
    /// assert_eq!(cmd.prg_index, 0);
    /// ```
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
            prg_index: 0,
            action: false,
            abort: false,
            range: 31,
            leaf: false,
            cs: 0,
            security_state: crate::types::SecurityState::NonSecure,
            tg: 0,
            num: 0,
            scale: 0,
            ttl: 0,
            ssec: false,
            ril: false,
        }
    }

    /// Set the security state of this command (builder pattern).
    ///
    /// Use this to record which security world issued the command so that any
    /// command error event generated during processing carries the correct
    /// `security_state` per ARM §7.3 / §3.10.2.1.
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::types::{CommandEntry, CommandType, SecurityState};
    ///
    /// let cmd = CommandEntry::new(CommandType::CfgiSte, 1, 0)
    ///     .with_security_state(SecurityState::Secure);
    /// assert_eq!(cmd.security_state, SecurityState::Secure);
    /// ```
    #[must_use]
    pub fn with_security_state(mut self, ss: crate::types::SecurityState) -> Self {
        self.security_state = ss;
        self
    }
}
