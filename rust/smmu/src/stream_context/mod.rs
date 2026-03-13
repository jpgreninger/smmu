//! Per-stream state and PASID management
//!
//! This module manages the state associated with each stream (device), including:
//!
//! - PASID (Process Address Space ID) management
//! - Per-PASID address space mappings
//! - Stream configuration and capabilities
//! - Translation context switching
//!
//! # Stream Context
//!
//! Each stream represents a device or logical channel that can access memory.
//! Streams may support multiple PASIDs for virtualization and process isolation.
//!
//! # PASID Support
//!
//! Full PASID support including PASID 0 (default/legacy mode) per ARM SMMU v3 specification.
//!
//! # Thread Safety
//!
//! StreamContext uses DashMap for lock-free concurrent PASID operations and atomic
//! operations for configuration flags, making it safe to share across threads.

#![warn(missing_docs)]

use crate::address_space::{AddressSpace, AddressSpaceError};
use crate::types::{
    AccessType, FaultRecord, FaultType, PagePermissions, SecurityState, StreamConfig, StreamContextError,
    StreamWorld, TranslationData, TranslationError, TranslationResult, IOVA, PA, PASID,
};
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

/// StreamContext - Per-stream state and PASID management
///
/// Manages translation contexts for a single stream (device), supporting multiple
/// PASIDs for process isolation and two-stage translation per ARM SMMU v3 specification.
///
/// # Architecture
///
/// - **Stage-1**: Per-PASID translation (IOVA → IPA or IOVA → PA)
/// - **Stage-2**: Shared translation across PASIDs (IPA → PA)
/// - **Two-Stage**: Combined Stage-1 → Stage-2 translation
/// - **Bypass**: Identity mapping when both stages disabled
///
/// # Thread Safety
///
/// All operations are thread-safe using lock-free DashMap for PASID storage and
/// atomic operations for configuration flags.
///
/// # Examples
///
/// ```
/// use smmu::stream_context::StreamContext;
/// use smmu::types::{PASID, IOVA, PA, PagePermissions, SecurityState, AccessType};
///
/// let stream_context = StreamContext::new();
///
/// // Create PASID and map a page
/// let pasid = PASID::new(1).unwrap();
/// stream_context.create_pasid(pasid).unwrap();
///
/// let iova = IOVA::new(0x1000).unwrap();
/// let pa = PA::new(0x2000).unwrap();
/// let perms = PagePermissions::read_write();
/// stream_context.map_page(pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();
///
/// // Translate address
/// let result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
/// assert!(result.is_ok());
/// ```
#[derive(Debug)]
pub struct StreamContext {
    /// PASID → AddressSpace mapping (Stage-1) for ALL PASIDs including PASID 0
    /// DashMap provides lock-free concurrent access for high performance.
    /// Previously PASID 0 was stored in a separate RwLock, but benchmarking showed
    /// that DashMap is actually faster under contention (15-30ns improvement).
    /// No RwLock on AddressSpace since it's now lock-free with DashMap internally.
    pub(crate) pasid_map: DashMap<u32, Arc<AddressSpace>>,

    /// PASID → ASID mapping — stores the CD.ASID value for each PASID (ARM §3.17).
    /// Used to tag Stage-1 TLB entries and to resolve ASID for ASID-targeted
    /// invalidation commands (`CMD_TLBI_NH_ASID` / `CMD_TLBI_EL2_ASID`).
    /// Default ASID for all PASIDs is 0.
    pub(crate) pasid_asid_map: DashMap<u32, u16>,

    /// Stage-2 AddressSpace (shared across all PASIDs)
    /// RwLock allows concurrent reads with exclusive writes
    stage2_address_space: RwLock<Option<Arc<AddressSpace>>>,

    /// Stage-1 enable flag (atomic for lock-free access)
    stage1_enabled: AtomicBool,

    /// Stage-2 enable flag (atomic for lock-free access)
    stage2_enabled: AtomicBool,

    /// Maximum PASIDs per stream (resource limit)
    max_pasids_per_stream: AtomicUsize,

    /// Stream enabled state (Section 4.2.2)
    enabled: AtomicBool,

    /// BUG-RUST-1 fix: disabling-in-progress flag (ARM IHI0070G.b §7.3.6).
    ///
    /// Set to `true` immediately before `pasid_map.clear()` in `disable()` and
    /// cleared to `false` after `enabled` is stored `false`.  A translator that
    /// passed the `is_enabled()` check just before the enable flag was cleared
    /// will find the `pasid_map` empty; by re-checking this flag it can return
    /// `StreamDisabled` (F_STREAM_DISABLED) rather than the incorrect
    /// `PASIDNotFound`.
    disabling: AtomicBool,

    /// Fault records (Section 4.2.4)
    fault_records: Arc<RwLock<Vec<FaultRecord>>>,

    /// Fault rate limit (max faults to record)
    fault_rate_limit: AtomicUsize,

    /// Fault retry enabled flag
    fault_retry_enabled: AtomicBool,

    /// Monotonic fault timestamp counter (avoids SystemTime overhead)
    fault_timestamp_counter: AtomicUsize,

    /// VMID (Virtual Machine ID) — STE Word 2 bits 63:48 per ARM §5.2.
    /// Tags Stage-2 TLB entries for VMID-targeted invalidation via
    /// `CMD_TLBI_S12_VMALL` / `CMD_TLBI_S2_IPA`.  Default 0.
    vmid: AtomicU16,

    /// Stall fault mode enabled — ARM §3.12.2.
    /// When true, faulting translations stall (returning `Stalled { stag }`)
    /// instead of immediately aborting.  Corresponds to `FaultMode::Stall`.
    stall_enabled: AtomicBool,

    /// Hardware Access Flag management enabled (CD.HA bit 43, ARM SMMU v3 §3.13).
    /// When true, the AF bit in the page table entry is set on first access.
    ha: AtomicBool,

    /// Hardware Dirty State management enabled (CD.HD bit 42, ARM SMMU v3 §3.13).
    /// When true, the dirty bit in the page table entry is set on first write.
    hd: AtomicBool,

    /// STE.S1DSS field (ARM §5.2): controls non-substream (PASID==0) behavior
    /// when the stream is substream-capable (`s1cd_max > 0`).
    /// 0=abort, 1=bypass stage-1 (identity), 2=use CD[0] (default).
    s1dss: AtomicU8,

    /// STE.S1CDMax field (ARM §5.2): number of SubstreamID bits supported.
    /// 0 means not substream-capable; `s1dss` is ignored.
    s1cd_max: AtomicU8,

    /// CD.T0SZ (ARM §5.4): number of address bits excluded from TTBR0 range.
    /// Valid range for SMMUv3.0: 0-39.  Out-of-range generates C_BAD_CD.
    t0sz: AtomicU8,

    /// CD.T1SZ (ARM §5.4): number of address bits excluded from TTBR1 range.
    /// Valid range for SMMUv3.0: 0-39.  Out-of-range generates C_BAD_CD.
    t1sz: AtomicU8,

    /// CD.AA64 (ARM §5.4): AArch64 translation table format selector.
    /// `true` = VMSAv8-64 (AArch64); `false` = VMSAv8-32 LPAE (unsupported).
    aa64: AtomicBool,

    /// STE.Config==0b000 abort mode (ARM §5.2, CT-09).
    ///
    /// When `true`, all translations on this stream are silently aborted
    /// (`TranslationError::StreamDisabled`) without recording any event to the
    /// event queue.  Distinct from the runtime `enabled` flag which is toggled
    /// by `disable_stream()` / `enable_stream()`.
    abort_mode: AtomicBool,

    // ---- GAP-1: STE output-attribute override fields (§5.2 CT-19) ----

    /// §5.2 STE.SHCFG: shareability override (2 bits, 0 = from-translation).
    sh_cfg: AtomicU8,

    /// §5.2 STE.ALLOCCFG: allocation hint override (4 bits).
    alloc_cfg: AtomicU8,

    /// §5.2 STE.MemAttr: memory type attribute (4 bits).
    mem_attr: AtomicU8,

    /// §5.2 STE.INSTCFG: instruction/data attribute override (2 bits).
    inst_cfg: AtomicU8,

    /// §5.2 STE.PRIVCFG: privilege attribute override (2 bits).
    priv_cfg: AtomicU8,

    /// §5.2 STE.NSCFG: non-secure attribute override (2 bits).
    ns_cfg: AtomicU8,

    /// §5.2 STE.MTCFG: memory type override enable.
    ///
    /// When `true`, `mem_attr` overrides the memory type in translated outputs.
    mt_cfg: AtomicBool,

    // ---- GAP-2: STE.STRW privilege check suppression (§5.2 CT-20) ----

    /// §5.2 STE.STRW: Stream World — exception level selection (2 bits).
    ///
    /// EL2 and EL3 suppress the `privileged_only` check on `PagePermissions`.
    /// Stored as the `u8` discriminant of `StreamWorld`.
    strw: AtomicU8,

    /// Stream security state (FINDING-NEW-44).
    ///
    /// Stored as the `u8` discriminant of `SecurityState` for atomic access.
    /// Used when generating `AtcInvalidateCompletion` and `CommandSyncCompletion`
    /// events so those events carry the stream's actual security state rather
    /// than a hardcoded `NonSecure`.  Default: `SecurityState::NonSecure` (0).
    security_state: AtomicU8,

    /// StreamID associated with this context (BUG-RUST-DBGR-10 fix).
    ///
    /// Stored so that fault records generated by `record_fault_internal()` carry
    /// the correct StreamID (§7.3) rather than a placeholder 0.
    /// Set by the SMMU when the stream is configured via `configure_stream()`.
    stream_id: AtomicU32,

    /// Tracks the number of live PASIDs without calling `pasid_map.len()`.
    ///
    /// `DashMap::len()` acquires a read-lock on every shard; calling it while an
    /// `entry()` write-guard is active on any shard causes a re-entrant deadlock
    /// (DashMap documents this constraint).  A separate atomic counter avoids the
    /// issue entirely while keeping the PASID-limit check effectively atomic.
    pasid_count: AtomicUsize,
}

impl StreamContext {
    /// Creates a new StreamContext with default configuration
    ///
    /// Default configuration:
    /// - Stage-1 enabled, Stage-2 disabled
    /// - Maximum 1024 PASIDs per stream
    /// - No PASIDs initially configured
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    ///
    /// let stream_context = StreamContext::new();
    /// assert!(stream_context.is_stage1_enabled());
    /// assert!(!stream_context.is_stage2_enabled());
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            pasid_map: DashMap::new(),
            pasid_asid_map: DashMap::new(),
            stage2_address_space: RwLock::new(None),
            stage1_enabled: AtomicBool::new(true),
            stage2_enabled: AtomicBool::new(false),
            max_pasids_per_stream: AtomicUsize::new(1024),
            enabled: AtomicBool::new(true),
            disabling: AtomicBool::new(false),
            fault_records: Arc::new(RwLock::new(Vec::new())),
            fault_rate_limit: AtomicUsize::new(usize::MAX),
            fault_retry_enabled: AtomicBool::new(false),
            fault_timestamp_counter: AtomicUsize::new(0),
            vmid: AtomicU16::new(0),
            stall_enabled: AtomicBool::new(false),
            ha: AtomicBool::new(false),
            hd: AtomicBool::new(false),
            s1dss: AtomicU8::new(2),
            s1cd_max: AtomicU8::new(0),
            t0sz: AtomicU8::new(16),
            t1sz: AtomicU8::new(16),
            aa64: AtomicBool::new(true),
            abort_mode: AtomicBool::new(false),
            sh_cfg: AtomicU8::new(0),
            alloc_cfg: AtomicU8::new(0),
            mem_attr: AtomicU8::new(0),
            inst_cfg: AtomicU8::new(0),
            priv_cfg: AtomicU8::new(0),
            ns_cfg: AtomicU8::new(0),
            mt_cfg: AtomicBool::new(false),
            strw: AtomicU8::new(StreamWorld::El1El0 as u8),
            security_state: AtomicU8::new(SecurityState::NonSecure as u8),
            stream_id: AtomicU32::new(0),
            pasid_count: AtomicUsize::new(0),
        }
    }

    // ========================================================================
    // PASID Management Operations
    // ========================================================================

    /// Creates a new PASID with a fresh AddressSpace
    ///
    /// # Arguments
    ///
    /// * `pasid` - Process Address Space ID to create
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - PASID already exists (`PASIDAlreadyExists`)
    /// - PASID limit exceeded (`PASIDLimitExceeded`)
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::PASID;
    ///
    /// let stream_context = StreamContext::new();
    /// let pasid = PASID::new(0).unwrap();
    /// assert!(stream_context.create_pasid(pasid).is_ok());
    /// ```
    pub fn create_pasid(&self, pasid: PASID) -> Result<(), StreamContextError> {
        // Bug 6 fix: do NOT guard PASID creation on stream-enabled state.
        // ARM §3.21 commissioning sequence: CDs are fully initialized before STE.V is
        // set to 1 (i.e. before the stream becomes active).  Page-table / PASID setup
        // is independent of stream enable state (ARM §3.4 / §5.2).  The is_enabled()
        // check belongs only in translate() and other transaction-path operations.

        let pasid_value = pasid.as_u32();
        let max_pasids = self.max_pasids_per_stream.load(Ordering::Acquire);

        // BUG-RUST-G fix: Reserve a slot atomically BEFORE acquiring the shard lock.
        // Two threads inserting different PASIDs would each see a different shard lock
        // and could both pass "count < max_pasids" simultaneously.  By doing fetch_add
        // first (under no shard lock), only one of them can claim the Nth slot.
        let prev_count = self.pasid_count.fetch_add(1, Ordering::AcqRel);
        if prev_count >= max_pasids {
            self.pasid_count.fetch_sub(1, Ordering::Release);
            return Err(StreamContextError::PASIDLimitExceeded(prev_count, max_pasids));
        }

        // Use DashMap::entry() to make the duplicate-check and insert atomic within
        // the shard lock.  This eliminates the TOCTOU window that existed between
        // the previous separate contains_key() check and insert() call (BUG-RUST-M04).
        //
        // IMPORTANT: Do NOT call self.pasid_map.len() while an entry() guard is held.
        // DashMap::len() acquires a read-lock on every shard; if the entry guard holds
        // a write-lock on shard N and len() tries to read-lock shard N, the thread
        // deadlocks.  We use pasid_count (AtomicUsize) instead to avoid the re-entrant
        // lock (deadlock fix for the Rust test hang).
        match self.pasid_map.entry(pasid_value) {
            Entry::Occupied(_) => {
                // Roll back: PASID already exists, slot was pre-reserved unnecessarily.
                self.pasid_count.fetch_sub(1, Ordering::Release);
                Err(StreamContextError::PASIDAlreadyExists(pasid_value))
            }
            Entry::Vacant(slot) => {
                let address_space = Arc::new(AddressSpace::new());
                slot.insert(address_space);
                Ok(())
            }
        }
    }

    // ========================================================================
    // ASID Management (CD.ASID per ARM §3.17)
    // ========================================================================

    /// Returns the ASID currently associated with the given PASID.
    ///
    /// Returns `0` if no ASID has been explicitly set (default per spec).
    ///
    /// # Errors
    ///
    /// Returns [`StreamContextError::PASIDNotFound`] if the PASID does not exist.
    pub fn get_pasid_asid(&self, pasid: PASID) -> Result<u16, StreamContextError> {
        let pasid_value = pasid.as_u32();
        if !self.pasid_map.contains_key(&pasid_value) {
            return Err(StreamContextError::PASIDNotFound(pasid_value));
        }
        Ok(self.pasid_asid_map.get(&pasid_value).map(|v| *v).unwrap_or(0))
    }

    /// Sets the ASID (CD.ASID) for the given PASID.
    ///
    /// # Errors
    ///
    /// Returns [`StreamContextError::PASIDNotFound`] if the PASID does not exist.
    pub fn set_pasid_asid(&self, pasid: PASID, asid: u16) -> Result<(), StreamContextError> {
        let pasid_value = pasid.as_u32();
        if !self.pasid_map.contains_key(&pasid_value) {
            return Err(StreamContextError::PASIDNotFound(pasid_value));
        }
        self.pasid_asid_map.insert(pasid_value, asid);
        Ok(())
    }

    /// Returns the ASID for the given PASID, or 0 if not set (infallible fast-path).
    ///
    /// Does not check whether the PASID exists; returns 0 for unknown PASIDs.
    /// Used by the translate fast-path to tag TLB entries.
    #[inline]
    pub(crate) fn get_pasid_asid_or_default(&self, pasid: PASID) -> u16 {
        self.pasid_asid_map.get(&pasid.as_u32()).map(|v| *v).unwrap_or(0)
    }

    // ========================================================================
    // VMID Management (STE.S2VMID per ARM §5.2, §3.8)
    // ========================================================================

    /// Returns the VMID (STE Word 2 bits 63:48) for this stream.
    ///
    /// Default is 0. VMID tags Stage-2 TLB entries for VMID-targeted
    /// invalidation via `CMD_TLBI_S12_VMALL` / `CMD_TLBI_S2_IPA`.
    #[inline]
    pub fn get_vmid(&self) -> u16 {
        self.vmid.load(Ordering::Relaxed)
    }

    /// Sets the VMID for this stream (STE Word 2 bits 63:48, ARM §5.2).
    ///
    /// New TLB entries installed after this call will be tagged with the
    /// new VMID.  Existing cached entries retain the old tag; issue
    /// `CMD_TLBI_S12_VMALL` with the old VMID to evict them.
    #[inline]
    pub fn set_vmid(&self, vmid: u16) {
        self.vmid.store(vmid, Ordering::Release);
    }

    /// Returns whether stall fault mode is enabled for this stream (ARM §3.12.2).
    #[inline]
    pub fn is_stall_enabled(&self) -> bool {
        self.stall_enabled.load(Ordering::Relaxed)
    }

    /// Enables or disables stall fault mode for this stream (ARM §3.12.2).
    ///
    /// When enabled, faulting translations return `Stalled { stag }` instead
    /// of an immediate abort error.
    #[inline]
    pub fn set_stall_enabled(&self, enabled: bool) {
        self.stall_enabled.store(enabled, Ordering::Release);
    }

    /// Returns whether hardware Access Flag management is enabled (CD.HA, ARM SMMU v3 §3.13).
    #[inline]
    pub fn is_ha_enabled(&self) -> bool {
        self.ha.load(Ordering::Relaxed)
    }

    /// Enables or disables hardware Access Flag management (CD.HA bit 43, ARM SMMU v3 §3.13).
    ///
    /// When enabled, the AF bit in the page table entry is set on first access.
    #[inline]
    pub fn set_ha(&self, enabled: bool) {
        self.ha.store(enabled, Ordering::Release);
    }

    /// Returns whether hardware Dirty State management is enabled (CD.HD, ARM SMMU v3 §3.13).
    #[inline]
    pub fn is_hd_enabled(&self) -> bool {
        self.hd.load(Ordering::Relaxed)
    }

    /// Enables or disables hardware Dirty State management (CD.HD bit 42, ARM SMMU v3 §3.13).
    ///
    /// When enabled, the dirty bit in the page table entry is set on first write.
    #[inline]
    pub fn set_hd(&self, enabled: bool) {
        self.hd.store(enabled, Ordering::Release);
    }

    /// Returns the STE.S1DSS field value (0, 1, or 2) (ARM §5.2).
    ///
    /// Controls behavior when a non-substream (PASID==0) transaction arrives on
    /// a substream-capable stage-1 stream (`s1cd_max > 0`).
    /// Default is 2 (use CD\[0\]).
    #[inline]
    pub fn get_s1dss(&self) -> u8 {
        // BUG-NEW-RUST-4 fix: Acquire pairs with the Release store in set_s1dss()
        // and update_configuration() to establish happens-before on non-TSO hardware.
        self.s1dss.load(Ordering::Acquire)
    }

    /// Sets the STE.S1DSS field value (ARM §5.2).
    #[inline]
    pub fn set_s1dss(&self, value: u8) {
        self.s1dss.store(value, Ordering::Release);
    }

    /// Returns the STE.S1CDMax field value (0 = not substream-capable) (ARM §5.2).
    ///
    /// When 0, the stream is not substream-capable and `s1dss` is ignored.
    /// When > 0, the stream supports up to `2^s1cd_max` substreams.
    #[inline]
    pub fn get_s1cd_max(&self) -> u8 {
        // BUG-NEW-RUST-4 fix: Acquire pairs with the Release store in set_s1cd_max().
        self.s1cd_max.load(Ordering::Acquire)
    }

    /// Sets the STE.S1CDMax field value (ARM §5.2).
    #[inline]
    pub fn set_s1cd_max(&self, value: u8) {
        self.s1cd_max.store(value, Ordering::Release);
    }

    /// Returns the CD.T0SZ value (ARM §5.4).
    #[inline]
    #[must_use]
    pub fn get_t0sz(&self) -> u8 {
        // BUG-NEW-RUST-4 fix: Acquire pairs with the Release store in set_t0sz().
        self.t0sz.load(Ordering::Acquire)
    }

    /// Sets the CD.T0SZ value (ARM §5.4).
    #[inline]
    pub fn set_t0sz(&self, value: u8) {
        self.t0sz.store(value, Ordering::Release);
    }

    /// Returns the CD.T1SZ value (ARM §5.4).
    #[inline]
    #[must_use]
    pub fn get_t1sz(&self) -> u8 {
        // BUG-NEW-RUST-4 fix: Acquire pairs with the Release store in set_t1sz().
        self.t1sz.load(Ordering::Acquire)
    }

    /// Sets the CD.T1SZ value (ARM §5.4).
    #[inline]
    pub fn set_t1sz(&self, value: u8) {
        self.t1sz.store(value, Ordering::Release);
    }

    /// Returns true when CD.AA64=1 (AArch64 translation tables, ARM §5.4).
    #[inline]
    #[must_use]
    pub fn get_aa64(&self) -> bool {
        // BUG-NEW-RUST-4 fix: Acquire pairs with the Release store in set_aa64().
        self.aa64.load(Ordering::Acquire)
    }

    /// Sets the CD.AA64 flag (ARM §5.4).
    #[inline]
    pub fn set_aa64(&self, value: bool) {
        self.aa64.store(value, Ordering::Release);
    }

    /// Returns true when STE.Config==0b000 abort mode is active (ARM §5.2, CT-09).
    #[inline]
    #[must_use]
    pub fn is_abort_mode(&self) -> bool {
        // BUG-RUST-DBGR-11 fix: use Acquire ordering to pair with the Release
        // store in set_abort_mode(). The translate hot-path already uses
        // Acquire directly; this makes the public method consistent and sound
        // on weakly-ordered hardware.
        self.abort_mode.load(Ordering::Acquire)
    }

    /// Sets the STE.Config==0b000 abort mode flag (ARM §5.2, CT-09).
    ///
    /// When `true`, all translations on this stream return `StreamDisabled`
    /// without recording any event to the event queue.
    #[inline]
    pub fn set_abort_mode(&self, value: bool) {
        self.abort_mode.store(value, Ordering::Release);
    }

    // ---- GAP-1: STE output-attribute override field accessors (§5.2) ----

    /// Returns the STE.SHCFG shareability override value (ARM §5.2, GAP-1).
    #[inline]
    #[must_use]
    pub fn get_sh_cfg(&self) -> u8 {
        // BUG-NEW-RUST-4 fix: Acquire pairs with the Release store in set_sh_cfg().
        self.sh_cfg.load(Ordering::Acquire)
    }

    /// Sets the STE.SHCFG shareability override (ARM §5.2, GAP-1).
    #[inline]
    pub fn set_sh_cfg(&self, value: u8) {
        self.sh_cfg.store(value, Ordering::Release);
    }

    /// Returns the STE.ALLOCCFG allocation hint override (ARM §5.2, GAP-1).
    #[inline]
    #[must_use]
    pub fn get_alloc_cfg(&self) -> u8 {
        // BUG-NEW-RUST-4 fix: Acquire pairs with the Release store in set_alloc_cfg().
        self.alloc_cfg.load(Ordering::Acquire)
    }

    /// Sets the STE.ALLOCCFG allocation hint override (ARM §5.2, GAP-1).
    #[inline]
    pub fn set_alloc_cfg(&self, value: u8) {
        self.alloc_cfg.store(value, Ordering::Release);
    }

    /// Returns the STE.MemAttr memory type attribute (ARM §5.2, GAP-1).
    #[inline]
    #[must_use]
    pub fn get_mem_attr(&self) -> u8 {
        // BUG-NEW-RUST-4 fix: Acquire pairs with the Release store in set_mem_attr().
        self.mem_attr.load(Ordering::Acquire)
    }

    /// Sets the STE.MemAttr memory type attribute (ARM §5.2, GAP-1).
    #[inline]
    pub fn set_mem_attr(&self, value: u8) {
        self.mem_attr.store(value, Ordering::Release);
    }

    /// Returns the STE.INSTCFG instruction/data attribute override (ARM §5.2, GAP-1).
    #[inline]
    #[must_use]
    pub fn get_inst_cfg(&self) -> u8 {
        // BUG-NEW-RUST-4 fix: Acquire pairs with the Release store in set_inst_cfg().
        self.inst_cfg.load(Ordering::Acquire)
    }

    /// Sets the STE.INSTCFG instruction/data attribute override (ARM §5.2, GAP-1).
    #[inline]
    pub fn set_inst_cfg(&self, value: u8) {
        self.inst_cfg.store(value, Ordering::Release);
    }

    /// Returns the STE.PRIVCFG privilege attribute override (ARM §5.2, GAP-1).
    #[inline]
    #[must_use]
    pub fn get_priv_cfg(&self) -> u8 {
        // BUG-NEW-RUST-4 fix: Acquire pairs with the Release store in set_priv_cfg().
        self.priv_cfg.load(Ordering::Acquire)
    }

    /// Sets the STE.PRIVCFG privilege attribute override (ARM §5.2, GAP-1).
    #[inline]
    pub fn set_priv_cfg(&self, value: u8) {
        self.priv_cfg.store(value, Ordering::Release);
    }

    /// Returns the STE.NSCFG non-secure attribute override (ARM §5.2, GAP-1).
    #[inline]
    #[must_use]
    pub fn get_ns_cfg(&self) -> u8 {
        // BUG-NEW-RUST-4 fix: Acquire pairs with the Release store in set_ns_cfg().
        self.ns_cfg.load(Ordering::Acquire)
    }

    /// Sets the STE.NSCFG non-secure attribute override (ARM §5.2, GAP-1).
    #[inline]
    pub fn set_ns_cfg(&self, value: u8) {
        self.ns_cfg.store(value, Ordering::Release);
    }

    /// Returns whether STE.MTCFG memory type override is enabled (ARM §5.2, GAP-1).
    #[inline]
    #[must_use]
    pub fn is_mt_cfg_enabled(&self) -> bool {
        // BUG-NEW-RUST-4 fix: Acquire pairs with the Release store in set_mt_cfg().
        self.mt_cfg.load(Ordering::Acquire)
    }

    /// Sets the STE.MTCFG memory type override enable flag (ARM §5.2, GAP-1).
    #[inline]
    pub fn set_mt_cfg(&self, value: bool) {
        self.mt_cfg.store(value, Ordering::Release);
    }

    // ---- GAP-2: STE.STRW Stream World accessor (§5.2) ----

    /// Returns the STE.STRW Stream World (exception level selection) (ARM §5.2, GAP-2).
    ///
    /// EL2 and EL3 suppress the `privileged_only` check on translated pages.
    #[inline]
    #[must_use]
    pub fn get_strw(&self) -> StreamWorld {
        // BUG-RUST-3 fix: Acquire ordering so the Release store in
        // update_configuration() is fully visible before this read.
        match self.strw.load(Ordering::Acquire) {
            0x00 => StreamWorld::El1El0,
            0x01 => StreamWorld::El2,
            0x02 => StreamWorld::El2E2h,
            0x03 => StreamWorld::El3,
            _ => StreamWorld::El1El0, // safe fallback
        }
    }

    /// Sets the STE.STRW Stream World (ARM §5.2, GAP-2).
    ///
    /// Uses `Ordering::Release` to pair with the `Ordering::Acquire` load in
    /// `get_strw()`, establishing a happens-before relationship on weakly-ordered
    /// architectures (ARM/POWER).
    #[inline]
    pub fn set_strw(&self, world: StreamWorld) {
        self.strw.store(world as u8, Ordering::Release);
    }

    /// Applies a [`StreamConfig`] to this stream context atomically (GAP-1, GAP-2).
    ///
    /// Updates all configuration fields carried in `StreamConfig` that are relevant
    /// to the translation path, including:
    ///
    /// - Stage enablement and PASID capacity
    /// - STE output-attribute override fields (GAP-1)
    /// - STE.STRW privilege suppression (GAP-2)
    /// - Stall mode, HA/HD flags, VMID, security state, abort mode
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::{StreamConfig, StreamWorld};
    ///
    /// let ctx = StreamContext::new();
    /// let cfg = StreamConfig::builder()
    ///     .translation_enabled(true)
    ///     .stage1_enabled(true)
    ///     .strw(StreamWorld::El2)
    ///     .mt_cfg(true)
    ///     .mem_attr(0x7)
    ///     .build()
    ///     .unwrap();
    /// ctx.update_configuration(cfg);
    /// assert_eq!(ctx.get_strw(), StreamWorld::El2);
    /// assert!(ctx.is_mt_cfg_enabled());
    /// ```
    pub fn update_configuration(&self, cfg: StreamConfig) {
        // BUG-RUST-3 fix: use Release ordering for all config-field stores.
        //
        // Pairing with the Acquire loads in the translate hot-path establishes
        // a happens-before relationship: any translator that loads a config field
        // via Acquire is guaranteed to see all stores that precede this Release
        // sequence as a complete, consistent config snapshot.  SeqCst is
        // stronger than necessary and has higher cost on weakly-ordered CPUs.

        // Abort mode (STE.Config==0b000). BUG-RUST-DBGR-4 fix: compute abort_mode first
        // so we can force both stage flags to false when abort_mode is active.
        // STE.Config==0b000 means ALL stages are disabled; allowing stage1_enabled=true
        // alongside abort_mode=true would be a contradictory state.
        //
        // BUG-R-10 fix: abort_mode is determined solely by cfg.disabled (which represents
        // STE.Config==0b000 per ARM §5.2).  The previous `&& !cfg.translation_enabled`
        // guard was redundant via the StreamConfig builder (which always sets both fields
        // together) but incorrect for direct struct construction: a config with
        // disabled=true and translation_enabled=true would not enter abort mode,
        // allowing translations to proceed contrary to the spec.
        let abort_mode_value = cfg.disabled;
        self.abort_mode.store(abort_mode_value, Ordering::Release);

        // Stage enablement — if abort_mode is set, both stages must be forced off.
        let s1 = if abort_mode_value { false } else { cfg.stage1_enabled };
        let s2 = if abort_mode_value { false } else { cfg.stage2_enabled };
        self.stage1_enabled.store(s1, Ordering::Release);
        self.stage2_enabled.store(s2, Ordering::Release);

        // PASID limits
        if cfg.pasid_enabled {
            self.max_pasids_per_stream
                .store(cfg.max_pasid as usize + 1, Ordering::Release);
        }

        // HA / HD
        self.ha.store(cfg.ha, Ordering::Release);
        self.hd.store(cfg.hd, Ordering::Release);

        // Stall mode
        self.stall_enabled
            .store(cfg.fault_mode == crate::types::FaultMode::Stall, Ordering::Release);

        // VMID
        self.vmid.store(cfg.vmid, Ordering::Release);

        // S1DSS / S1CDMax
        self.s1dss.store(cfg.s1dss, Ordering::Release);
        self.s1cd_max.store(cfg.s1cd_max, Ordering::Release);

        // T0SZ / T1SZ / AA64
        self.t0sz.store(cfg.t0sz, Ordering::Release);
        self.t1sz.store(cfg.t1sz, Ordering::Release);
        self.aa64.store(cfg.aa64, Ordering::Release);

        // Security state
        self.security_state
            .store(cfg.security_state as u8, Ordering::Release);

        // GAP-1: output-attribute override fields
        self.sh_cfg.store(cfg.sh_cfg, Ordering::Release);
        self.alloc_cfg.store(cfg.alloc_cfg, Ordering::Release);
        self.mem_attr.store(cfg.mem_attr, Ordering::Release);
        self.inst_cfg.store(cfg.inst_cfg, Ordering::Release);
        self.priv_cfg.store(cfg.priv_cfg, Ordering::Release);
        self.ns_cfg.store(cfg.ns_cfg, Ordering::Release);
        self.mt_cfg.store(cfg.mt_cfg, Ordering::Release);

        // GAP-2: STRW
        self.strw.store(cfg.strw as u8, Ordering::Release);
    }

    /// Returns the configured security state for this stream (FINDING-NEW-44).
    ///
    /// Used when generating `AtcInvalidateCompletion` and `CommandSyncCompletion`
    /// events so those events carry the stream's actual security state.
    #[inline]
    #[must_use]
    pub fn security_state(&self) -> SecurityState {
        // SAFETY: the stored value is always written via `set_security_state` which
        // validates the value before storing, and initialized to a valid discriminant.
        // All valid `SecurityState` discriminants are 0–3.  If somehow an invalid byte
        // is stored (which cannot happen through the safe API) we fall back to NonSecure.
        SecurityState::from_bits(self.security_state.load(Ordering::Relaxed))
            .unwrap_or(SecurityState::NonSecure)
    }

    /// Sets the security state for this stream (FINDING-NEW-44).
    ///
    /// Controls the `security_state` field in `AtcInvalidateCompletion` and
    /// `CommandSyncCompletion` events generated for this stream.
    #[inline]
    pub fn set_security_state(&self, state: SecurityState) {
        self.security_state.store(state as u8, Ordering::Release);
    }

    /// Returns the StreamID stored in this context (BUG-RUST-DBGR-10).
    ///
    /// Used by `record_fault_internal()` to populate fault record `stream_id` fields.
    #[inline]
    #[must_use]
    pub fn get_stream_id(&self) -> u32 {
        self.stream_id.load(Ordering::Acquire)
    }

    /// Sets the StreamID for this context (BUG-RUST-DBGR-10).
    ///
    /// Called by the SMMU when a stream is configured via `configure_stream()`.
    /// Fault records generated by this context will carry this StreamID per ARM §7.3.
    #[inline]
    pub fn set_stream_id(&self, id: u32) {
        self.stream_id.store(id, Ordering::Release);
    }

    /// Removes a PASID and its associated AddressSpace
    ///
    /// # Arguments
    ///
    /// * `pasid` - Process Address Space ID to remove
    ///
    /// # Errors
    ///
    /// Returns error if PASID not found (`PASIDNotFound`)
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::PASID;
    ///
    /// let stream_context = StreamContext::new();
    /// let pasid = PASID::new(1).unwrap();
    /// stream_context.create_pasid(pasid).unwrap();
    /// assert!(stream_context.remove_pasid(pasid).is_ok());
    /// ```
    pub fn remove_pasid(&self, pasid: PASID) -> Result<(), StreamContextError> {
        let pasid_value = pasid.as_u32();

        // Remove from DashMap (works for all PASIDs including 0)
        if self.pasid_map.remove(&pasid_value).is_none() {
            return Err(StreamContextError::PASIDNotFound(pasid_value));
        }

        // Bug 4 fix: remove the corresponding ASID entry so that if this PASID value
        // is later reallocated, get_pasid_asid_or_default() does not return the old
        // ASID and cause incorrect TLB tagging (ARM §3.17).
        self.pasid_asid_map.remove(&pasid_value);

        self.pasid_count.fetch_sub(1, Ordering::Release);
        Ok(())
    }

    /// Adds a PASID with an existing AddressSpace
    ///
    /// This allows multiple PASIDs to share the same AddressSpace.
    ///
    /// # Arguments
    ///
    /// * `pasid` - Process Address Space ID to add
    /// * `address_space` - Shared AddressSpace reference
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - PASID already exists (`PASIDAlreadyExists`)
    /// - PASID limit exceeded (`PASIDLimitExceeded`)
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::PASID;
    /// use std::sync::{Arc, RwLock};
    ///
    /// let stream_context = StreamContext::new();
    /// let pasid1 = PASID::new(1).unwrap();
    /// stream_context.create_pasid(pasid1).unwrap();
    ///
    /// // Share AddressSpace with another PASID
    /// let addr_space = stream_context.get_pasid_address_space(pasid1).unwrap();
    /// let pasid2 = PASID::new(2).unwrap();
    /// assert!(stream_context.add_pasid(pasid2, addr_space).is_ok());
    /// ```
    pub fn add_pasid(&self, pasid: PASID, address_space: Arc<AddressSpace>) -> Result<(), StreamContextError> {
        let pasid_value = pasid.as_u32();
        let max_pasids = self.max_pasids_per_stream.load(Ordering::Acquire);

        // BUG-NEW2-07 fix: use DashMap::entry() for atomic check-and-insert, eliminating
        // the TOCTOU race between contains_key() and insert(). Mirrors the pattern
        // established for create_pasid() (BUG-RUST-M04).
        //
        // Capacity pre-check: DashMap::len() acquires every shard's read lock in
        // sequence, so it MUST NOT be called while an entry() shard write-lock is
        // already held on this thread — doing so deadlocks.  Instead we snapshot the
        // count before acquiring the entry lock.  The narrow TOCTOU window between
        // this snapshot and the entry() write-lock is acceptable: the worst-case
        // outcome is one extra PASID briefly exceeding the soft cap, and the
        // duplicate-PASID check below remains fully atomic.
        let current_count = self.pasid_count.load(Ordering::Acquire);
        if current_count >= max_pasids {
            return Err(StreamContextError::PASIDLimitExceeded(current_count, max_pasids));
        }

        match self.pasid_map.entry(pasid_value) {
            Entry::Occupied(_) => Err(StreamContextError::PASIDAlreadyExists(pasid_value)),
            Entry::Vacant(slot) => {
                slot.insert(address_space);
                self.pasid_count.fetch_add(1, Ordering::Release);
                Ok(())
            }
        }
    }

    /// Checks if a PASID exists
    ///
    /// # Arguments
    ///
    /// * `pasid` - Process Address Space ID to check
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::PASID;
    ///
    /// let stream_context = StreamContext::new();
    /// let pasid = PASID::new(1).unwrap();
    /// assert!(!stream_context.has_pasid(pasid));
    /// stream_context.create_pasid(pasid).unwrap();
    /// assert!(stream_context.has_pasid(pasid));
    /// ```
    #[must_use]
    pub fn has_pasid(&self, pasid: PASID) -> bool {
        let pasid_value = pasid.as_u32();
        self.pasid_map.contains_key(&pasid_value)
    }

    /// Returns the number of configured PASIDs
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::PASID;
    ///
    /// let stream_context = StreamContext::new();
    /// assert_eq!(stream_context.pasid_count(), 0);
    /// stream_context.create_pasid(PASID::new(1).unwrap()).unwrap();
    /// assert_eq!(stream_context.pasid_count(), 1);
    /// ```
    #[must_use]
    pub fn pasid_count(&self) -> usize {
        self.pasid_count.load(Ordering::Acquire)
    }

    /// Clears all PASIDs and their associated AddressSpaces
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::PASID;
    ///
    /// let stream_context = StreamContext::new();
    /// stream_context.create_pasid(PASID::new(1).unwrap()).unwrap();
    /// stream_context.create_pasid(PASID::new(2).unwrap()).unwrap();
    /// assert_eq!(stream_context.pasid_count(), 2);
    ///
    /// stream_context.clear_all_pasids().unwrap();
    /// assert_eq!(stream_context.pasid_count(), 0);
    /// ```
    pub fn clear_all_pasids(&self) -> Result<(), StreamContextError> {
        // Clear all PASIDs (including PASID 0)
        self.pasid_map.clear();
        // Bug 5 fix: clear ASID map in lock-step so recycled PASID values do not
        // inherit stale ASIDs and cause incorrect TLB tagging (ARM §3.17).
        self.pasid_asid_map.clear();
        self.pasid_count.store(0, Ordering::Release);
        Ok(())
    }

    /// Sets the maximum number of PASIDs per stream
    ///
    /// # Arguments
    ///
    /// * `max` - Maximum PASID count
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    ///
    /// let stream_context = StreamContext::new();
    /// stream_context.set_max_pasids_per_stream(512);
    /// ```
    pub fn set_max_pasids_per_stream(&self, max: usize) {
        self.max_pasids_per_stream.store(max, Ordering::Release);
    }

    /// Gets the AddressSpace for a PASID
    ///
    /// Returns `None` if PASID not found.
    ///
    /// # Arguments
    ///
    /// * `pasid` - Process Address Space ID
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::PASID;
    ///
    /// let stream_context = StreamContext::new();
    /// let pasid = PASID::new(1).unwrap();
    /// stream_context.create_pasid(pasid).unwrap();
    ///
    /// let addr_space = stream_context.get_pasid_address_space(pasid);
    /// assert!(addr_space.is_some());
    /// ```
    #[must_use]
    pub fn get_pasid_address_space(&self, pasid: PASID) -> Option<Arc<AddressSpace>> {
        self.pasid_map.get(&pasid.as_u32()).map(|entry| entry.clone())
    }

    // ========================================================================
    // Stage Configuration Operations
    // ========================================================================

    /// Sets Stage-1 translation enable state
    ///
    /// # Arguments
    ///
    /// * `enabled` - True to enable Stage-1, false to disable
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    ///
    /// let stream_context = StreamContext::new();
    /// stream_context.set_stage1_enabled(false);
    /// assert!(!stream_context.is_stage1_enabled());
    /// ```
    pub fn set_stage1_enabled(&self, enabled: bool) {
        self.stage1_enabled.store(enabled, Ordering::Release);
    }

    /// Sets Stage-2 translation enable state
    ///
    /// # Arguments
    ///
    /// * `enabled` - True to enable Stage-2, false to disable
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    ///
    /// let stream_context = StreamContext::new();
    /// stream_context.set_stage2_enabled(true);
    /// assert!(stream_context.is_stage2_enabled());
    /// ```
    pub fn set_stage2_enabled(&self, enabled: bool) {
        self.stage2_enabled.store(enabled, Ordering::Release);
    }

    /// Checks if Stage-1 translation is enabled
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    ///
    /// let stream_context = StreamContext::new();
    /// assert!(stream_context.is_stage1_enabled());
    /// ```
    #[must_use]
    pub fn is_stage1_enabled(&self) -> bool {
        self.stage1_enabled.load(Ordering::Relaxed)
    }

    /// Checks if Stage-2 translation is enabled
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    ///
    /// let stream_context = StreamContext::new();
    /// assert!(!stream_context.is_stage2_enabled());
    /// ```
    #[must_use]
    pub fn is_stage2_enabled(&self) -> bool {
        self.stage2_enabled.load(Ordering::Relaxed)
    }

    /// Sets the Stage-2 AddressSpace (shared across PASIDs)
    ///
    /// # Arguments
    ///
    /// * `address_space` - Optional Stage-2 AddressSpace
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::address_space::AddressSpace;
    /// use std::sync::Arc;
    ///
    /// let stream_context = StreamContext::new();
    /// let stage2 = Arc::new(AddressSpace::new());
    /// stream_context.set_stage2_address_space(Some(stage2));
    /// ```
    pub fn set_stage2_address_space(&self, address_space: Option<Arc<AddressSpace>>) {
        let mut stage2 = self.stage2_address_space.write().unwrap();
        *stage2 = address_space;
    }

    // ========================================================================
    // Page Operations
    // ========================================================================

    /// Maps a page in the specified PASID's address space
    ///
    /// # Arguments
    ///
    /// * `pasid` - Process Address Space ID
    /// * `iova` - Input/Virtual address
    /// * `pa` - Physical address
    /// * `permissions` - Page permissions
    /// * `security_state` - Security state
    ///
    /// # Errors
    ///
    /// Returns error if PASID not found or mapping fails
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::{PASID, IOVA, PA, PagePermissions, SecurityState};
    ///
    /// let stream_context = StreamContext::new();
    /// let pasid = PASID::new(1).unwrap();
    /// stream_context.create_pasid(pasid).unwrap();
    ///
    /// let iova = IOVA::new(0x1000).unwrap();
    /// let pa = PA::new(0x2000).unwrap();
    /// let perms = PagePermissions::read_write();
    ///
    /// assert!(stream_context.map_page(pasid, iova, pa, perms, SecurityState::NonSecure).is_ok());
    /// ```
    pub fn map_page(
        &self,
        pasid: PASID,
        iova: IOVA,
        pa: PA,
        permissions: PagePermissions,
        security_state: SecurityState,
    ) -> Result<(), AddressSpaceError> {
        // NOTE: ARM §3.4 / §5.2 — page-table setup is independent of stream
        // enable state.  The is_enabled() guard belongs only in translate().
        // Do NOT check is_enabled() here.

        let pasid_value = pasid.as_u32();

        // Get AddressSpace for PASID (works for all PASIDs including 0)
        let addr_space = self.pasid_map.get(&pasid_value).ok_or(AddressSpaceError::InternalError)?;

        // Map page through AddressSpace (no write lock needed - AddressSpace is lock-free)
        addr_space.map_page(iova, pa, permissions, security_state)
    }

    /// Unmaps a page from the specified PASID's address space
    ///
    /// # Arguments
    ///
    /// * `pasid` - Process Address Space ID
    /// * `iova` - Input/Virtual address
    ///
    /// # Errors
    ///
    /// Returns error if PASID not found or unmap fails
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::{PASID, IOVA, PA, PagePermissions, SecurityState};
    ///
    /// let stream_context = StreamContext::new();
    /// let pasid = PASID::new(1).unwrap();
    /// stream_context.create_pasid(pasid).unwrap();
    ///
    /// let iova = IOVA::new(0x1000).unwrap();
    /// let pa = PA::new(0x2000).unwrap();
    /// let perms = PagePermissions::read_write();
    /// stream_context.map_page(pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();
    ///
    /// assert!(stream_context.unmap_page(pasid, iova).is_ok());
    /// ```
    pub fn unmap_page(&self, pasid: PASID, iova: IOVA) -> Result<(), AddressSpaceError> {
        let pasid_value = pasid.as_u32();

        // Get AddressSpace for PASID
        let addr_space = self.pasid_map.get(&pasid_value).ok_or(AddressSpaceError::InternalError)?;

        // Unmap page through AddressSpace (no write lock needed - AddressSpace is lock-free)
        addr_space.unmap_page(iova)
    }

    /// Create and initialize Stage-2 address space
    ///
    /// Creates a new address space for Stage-2 translation (IPA → PA).
    /// Required for two-stage translation setup.
    ///
    /// # Errors
    ///
    /// Returns error if Stage-2 address space already exists.
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    ///
    /// let stream_context = StreamContext::new();
    /// stream_context.set_stage2_enabled(true);
    /// assert!(stream_context.create_stage2_address_space().is_ok());
    /// ```
    pub fn create_stage2_address_space(&self) -> Result<(), StreamContextError> {
        let mut stage2_guard = self.stage2_address_space.write().unwrap();

        // Check if Stage-2 already exists
        if stage2_guard.is_some() {
            return Err(StreamContextError::InternalError(
                "Stage-2 address space already exists".to_string(),
            ));
        }

        // Create new Stage-2 address space
        *stage2_guard = Some(Arc::new(AddressSpace::new()));
        Ok(())
    }

    /// Map a page in the Stage-2 address space (IPA → PA)
    ///
    /// For two-stage translation, this maps Intermediate Physical Addresses
    /// (output of Stage-1) to final Physical Addresses.
    ///
    /// # Arguments
    ///
    /// * `ipa` - Intermediate Physical Address
    /// * `pa` - Physical Address
    /// * `permissions` - Page permissions
    /// * `security_state` - Security state
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Stage-2 address space not initialized
    /// - Mapping fails
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::{IOVA, PA, PagePermissions, SecurityState};
    ///
    /// let stream_context = StreamContext::new();
    /// stream_context.set_stage2_enabled(true);
    /// stream_context.create_stage2_address_space().unwrap();
    ///
    /// let ipa = IOVA::new(0x2000).unwrap();
    /// let pa = PA::new(0x3000).unwrap();
    /// let perms = PagePermissions::read_write();
    ///
    /// assert!(stream_context.map_stage2_page(ipa, pa, perms, SecurityState::NonSecure).is_ok());
    /// ```
    pub fn map_stage2_page(
        &self,
        ipa: IOVA,
        pa: PA,
        permissions: PagePermissions,
        security_state: SecurityState,
    ) -> Result<(), AddressSpaceError> {
        // Get Stage-2 address space (read lock only - AddressSpace is lock-free)
        let stage2_guard = self.stage2_address_space.read().unwrap();
        let stage2 = stage2_guard.as_ref().ok_or(AddressSpaceError::InternalError)?;

        // Map page through lock-free AddressSpace
        stage2.map_page(ipa, pa, permissions, security_state)
    }

    // ========================================================================
    // Translation Operations
    // ========================================================================

    /// Translates an address through configured translation stages
    ///
    /// Supports four translation modes per ARM SMMU v3 specification:
    /// - Stage-1 only: IOVA → PA
    /// - Stage-2 only: IPA → PA (IOVA treated as IPA)
    /// - Two-stage: IOVA → IPA → PA
    /// - Bypass: IOVA = PA (identity mapping)
    ///
    /// # Arguments
    ///
    /// * `pasid` - Process Address Space ID
    /// * `iova` - Input/Virtual address
    /// * `access_type` - Type of memory access (Read/Write/Execute)
    /// * `security_state` - Security state
    ///
    /// # Errors
    ///
    /// Returns error if translation fails due to:
    /// - PASID not found
    /// - Page not mapped
    /// - Permission violation
    /// - Invalid configuration
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::{PASID, IOVA, PA, PagePermissions, SecurityState, AccessType};
    ///
    /// let stream_context = StreamContext::new();
    /// stream_context.set_stage1_enabled(true);
    /// stream_context.set_stage2_enabled(false);
    ///
    /// let pasid = PASID::new(1).unwrap();
    /// stream_context.create_pasid(pasid).unwrap();
    ///
    /// let iova = IOVA::new(0x1000).unwrap();
    /// let pa = PA::new(0x2000).unwrap();
    /// let perms = PagePermissions::read_write();
    /// stream_context.map_page(pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();
    ///
    /// let result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    /// assert!(result.is_ok());
    /// assert_eq!(result.unwrap().physical_address(), pa);
    /// ```
    pub fn translate(
        &self,
        pasid: PASID,
        iova: IOVA,
        access_type: AccessType,
        security_state: SecurityState,
    ) -> TranslationResult {
        // BUG-R-02/R-09 fix: AccessType::None has no ARM SMMU hardware equivalent.
        // Callers must supply a real access type (Read, Write, Execute, etc.).
        // A debug_assert catches mis-use in development builds without altering
        // release behaviour (existing tests that use None for utility purposes
        // bypass this check by calling lower-level helpers directly).
        debug_assert!(
            access_type != AccessType::None,
            "AccessType::None is not a valid ARM SMMU hardware access type; \
             use Read, Write, Execute, or a combination thereof"
        );

        // §5.2 / CT-09: STE.Config==0b000 — abort silently, no event.
        // BUG-RUST-3 fix: use Acquire so the abort_mode store in
        // update_configuration() (Release) happens-before this load.
        // BUG-R-05 fix: return AbortMode (not StreamDisabled) so the SMMU level
        // can distinguish silent-abort (no event) from admin-disabled (F_STREAM_DISABLED).
        if self.abort_mode.load(Ordering::Acquire) {
            return Err(TranslationError::AbortMode);
        }

        // Check if stream is enabled
        if !self.is_enabled() {
            return Err(TranslationError::StreamDisabled);
        }

        // BUG-RUST-3 fix: Acquire ordering ensures the Release stores in
        // update_configuration() are fully visible before these loads.
        let stage1_enabled = self.stage1_enabled.load(Ordering::Acquire);
        let stage2_enabled = self.stage2_enabled.load(Ordering::Acquire);

        // §3.9 / §5.2 STE.S1DSS: non-substream (PASID==0) routing on substream-capable
        // stage-1 streams (s1cd_max > 0). BUG-RUST-DBGR-3/6 fix.
        //
        // When stage-1 is enabled and the stream is substream-capable (s1cd_max > 0)
        // and PASID == 0, S1DSS determines the outcome BEFORE any CD lookup:
        //   0 = abort with F_STREAM_DISABLED
        //   1 = bypass stage-1 (identity mapping PA == IOVA)
        //   2 = use CD[0] (fall through to normal dispatch)
        let s1cd_max = self.s1cd_max.load(Ordering::Acquire);
        if stage1_enabled && s1cd_max > 0 && pasid.as_u32() == 0 {
            let s1dss = self.s1dss.load(Ordering::Acquire);
            match s1dss {
                0 => return Err(TranslationError::StreamDisabled), // F_STREAM_DISABLED
                1 => return self.translate_bypass(iova, security_state), // identity mapping
                _ => {} // 2 = use CD[0], fall through to normal dispatch
            }
        }

        // §3.9: non-zero PASID (SubstreamID) on a stage-2-only or bypass stream is
        // always terminated with an abort; C_BAD_SUBSTREAMID is recorded (§7.3.9).
        if pasid.as_u32() != 0 && !stage1_enabled {
            return Err(TranslationError::BadSubstreamId);
        }

        match (stage1_enabled, stage2_enabled) {
            // Stage-1 only: IOVA → PA
            (true, false) => self.translate_stage1_only(pasid, iova, access_type, security_state),

            // Stage-2 only: IPA → PA (treat IOVA as IPA)
            (false, true) => self.translate_stage2_only(pasid, iova, access_type, security_state),

            // Two-stage: IOVA → IPA → PA
            (true, true) => self.translate_two_stage(pasid, iova, access_type, security_state),

            // Bypass mode: IOVA = PA (identity mapping)
            (false, false) => self.translate_bypass(iova, security_state),
        }
    }

    /// Stage-1 only translation: IOVA → PA
    fn translate_stage1_only(
        &self,
        pasid: PASID,
        iova: IOVA,
        access_type: AccessType,
        security_state: SecurityState,
    ) -> TranslationResult {
        let pasid_value = pasid.as_u32();

        // Get Stage-1 AddressSpace for PASID (lock-free DashMap lookup for all PASIDs).
        // BUG-RUST-1 fix: if the lookup misses, check whether the stream is being
        // concurrently disabled.  A translator that passed is_enabled()==true but
        // then finds an empty map due to a concurrent disable() must return
        // StreamDisabled (ARM §7.3.6 F_STREAM_DISABLED), not PASIDNotFound.
        let addr_space = match self.pasid_map.get(&pasid_value) {
            Some(a) => a,
            None => {
                if self.is_disabling() || !self.is_enabled() {
                    return Err(TranslationError::StreamDisabled);
                }
                return Err(TranslationError::PASIDNotFound);
            }
        };

        // Perform Stage-1 translation (no RwLock needed - AddressSpace is lock-free)
        let result = addr_space.translate_page(iova, access_type, security_state);

        // On successful translation, update Access Flag / Dirty State per ARM §3.13.
        // BUG-RUST-3 fix: Acquire loads for ha/hd config fields.
        if result.is_ok() {
            let ha = self.ha.load(Ordering::Acquire);
            let hd = self.hd.load(Ordering::Acquire);
            if ha || hd {
                addr_space.update_access_flags(iova, ha, hd, access_type);
            }
        }

        // §3.12.2 / BUG-RUST-TWOSTAGE-S1-FAULT-CLASS fix: do NOT record faults
        // here.  Fault recording is the responsibility of the top-level
        // `smmu/mod.rs::translate()` caller via `record_translation_fault()`.
        // Recording here AND in the caller produces two fault records for a
        // single transaction, violating §3.12.2.
        if let Err(ref error) = result {
            return Err(error.clone());
        }

        let data = result.unwrap();

        // §5.2 GAP-2: Check privileged_only against STRW suppression.
        // If the page is privileged-only AND STRW does not suppress (not EL2/EL3),
        // the access is denied with a PermissionFault.
        if data.permissions().privileged_only() && !self.strw_suppresses_priv() {
            return Err(TranslationError::PermissionViolation { access: access_type });
        }

        // §5.2 GAP-1: Apply STE output-attribute overrides.
        Ok(self.apply_output_attrs(data))
    }

    /// Stage-2 only translation: IPA → PA
    fn translate_stage2_only(
        &self,
        _pasid: PASID,
        ipa: IOVA,
        access_type: AccessType,
        security_state: SecurityState,
    ) -> TranslationResult {
        // Get Stage-2 AddressSpace (scope the read-lock tightly)
        let result = {
            let stage2_guard = self.stage2_address_space.read().unwrap();
            let stage2 = stage2_guard.as_ref().ok_or(TranslationError::StreamNotConfigured)?;
            stage2.translate_page(ipa, access_type, security_state)
        };

        // §3.12.2 / BUG-RUST-TWOSTAGE-S1-FAULT-CLASS fix: fault recording is
        // handled exclusively by the SMMU-level caller to avoid double recording.
        if let Err(ref error) = result {
            return Err(error.clone());
        }

        let data = result.unwrap();

        // §5.2 GAP-2: Check privileged_only against STRW suppression.
        // §3.12.2 fix: no record_fault_internal — SMMU caller records the fault.
        if data.permissions().privileged_only() && !self.strw_suppresses_priv() {
            return Err(TranslationError::PermissionViolation { access: access_type });
        }

        // §5.2 GAP-1: Apply STE output-attribute overrides.
        Ok(self.apply_output_attrs(data))
    }

    /// Two-stage translation: IOVA → IPA → PA
    fn translate_two_stage(
        &self,
        pasid: PASID,
        iova: IOVA,
        access_type: AccessType,
        security_state: SecurityState,
    ) -> TranslationResult {
        let pasid_value = pasid.as_u32();

        // Stage-1: IOVA → IPA (lock-free DashMap lookup for all PASIDs).
        // BUG-RUST-1 fix: same disabling check as translate_stage1_only().
        let addr_space = match self.pasid_map.get(&pasid_value) {
            Some(a) => a,
            None => {
                if self.is_disabling() || !self.is_enabled() {
                    return Err(TranslationError::StreamDisabled);
                }
                return Err(TranslationError::PASIDNotFound);
            }
        };

        // Stage-1: IOVA → IPA; use explicit match to avoid fragile unwrap()
        let stage1_result = match addr_space.translate_page(iova, access_type, security_state) {
            Ok(data) => {
                // Update Access Flag / Dirty State per ARM §3.13 after successful Stage-1.
                // BUG-RUST-3 fix: Acquire loads for ha/hd config fields.
                let ha = self.ha.load(Ordering::Acquire);
                let hd = self.hd.load(Ordering::Acquire);
                if ha || hd {
                    addr_space.update_access_flags(iova, ha, hd, access_type);
                }
                data
            },
            Err(error) => {
                // §3.12.2 / BUG-RUST-TWOSTAGE-S1-FAULT-CLASS fix: do NOT call
                // record_fault_internal here.  The top-level smmu/mod.rs caller
                // records the fault via record_translation_fault() using the
                // correct map_translation_error_to_fault_type() mapping (which
                // also properly handles AddressSizeFault, SecurityFault, etc.).
                // Recording here would create a second, potentially misclassified
                // fault record for the same transaction.
                return Err(error);
            },
        };

        // IPA is the physical address from Stage-1
        let ipa =
            IOVA::new(stage1_result.physical_address().as_u64()).map_err(|_| TranslationError::AddressSizeError)?;

        // Stage-2: IPA → PA.
        // BUG-07: Scope the stage2_address_space read-lock tightly so it is
        // released before any call to record_fault_internal (which acquires the
        // fault_records write-lock).  Holding stage2_address_space.read() across
        // record_fault_internal creates an ABBA deadlock risk if a concurrent
        // writer calls set_stage2_address_space() while holding fault_records.write().
        let stage2_result = {
            let stage2_guard = self.stage2_address_space.read().unwrap();
            let stage2 = stage2_guard.as_ref().ok_or(TranslationError::StreamNotConfigured)?;
            // BUG-RUST-1 fix: ARM IHI0070G.b §7.3.16 — "When CLASS == TT, the
            // access is implicitly Data and a read."  The Stage-2 lookup for the
            // page-table walk (PTW) must use AccessType::Read regardless of the
            // original transaction's access type.  The actual permission check is
            // done by the S1 ∩ S2 intersection logic below (lines ~1516-1531).
            //
            // BUG-2 fix: ARM IHI0070G.b §3.10.2.2 — "When stage 1 translation is
            // performed, the NS attribute provided to stage 2 comes from stage 1
            // translation tables."  The NS bit for the Stage-2 lookup must come from
            // the Stage-1 output (stage1_result.security_state()), not the incoming
            // transaction's security_state parameter.
            stage2.translate_page(ipa, AccessType::Read, stage1_result.security_state())
        }; // stage2_address_space read-lock released here

        // §3.12.2 / BUG-RUST-TWOSTAGE-S1-FAULT-CLASS fix: do NOT call
        // record_fault_internal for the Stage-2 error either.  The top-level
        // smmu/mod.rs caller records the single canonical fault record via
        // record_translation_fault().  Recording here would produce a second
        // record for the same transaction, violating §3.12.2.
        // §3.3.1 / FINDING-NEW-29: Effective permissions = intersection of Stage-1 and Stage-2.
        // Stage-2 translation succeeded; intersect its permissions with Stage-1 permissions to
        // derive the final access rights.  If the intersected permissions deny the requested
        // access type, record a PermissionFault and return PermissionViolation.
        let s2_data = stage2_result?;
        let final_perms = stage1_result.permissions().intersection(s2_data.permissions());

        let access_denied = match access_type {
            AccessType::Read => !final_perms.read(),
            AccessType::Write => !final_perms.write(),
            AccessType::Execute => !final_perms.execute(),
            AccessType::ReadWrite => !final_perms.read() || !final_perms.write(),
            AccessType::ReadExecute => !final_perms.read() || !final_perms.execute(),
            AccessType::WriteExecute => !final_perms.write() || !final_perms.execute(),
            AccessType::ReadWriteExecute => !final_perms.read() || !final_perms.write() || !final_perms.execute(),
            AccessType::None => false,
        };
        // §3.12.2 fix: no record_fault_internal — SMMU caller records the fault.
        if access_denied {
            return Err(TranslationError::PermissionViolation { access: access_type });
        }

        // §5.2 GAP-2: Check final effective permissions for privileged_only.
        // §3.12.2 fix: no record_fault_internal — SMMU caller records the fault.
        if final_perms.privileged_only() && !self.strw_suppresses_priv() {
            return Err(TranslationError::PermissionViolation { access: access_type });
        }

        // §5.2 GAP-1: Apply STE output-attribute overrides.
        let result_data = TranslationData::new(s2_data.physical_address(), final_perms, s2_data.security_state());
        Ok(self.apply_output_attrs(result_data))
    }

    /// Bypass mode translation: IOVA = PA (identity mapping)
    fn translate_bypass(&self, iova: IOVA, security_state: SecurityState) -> TranslationResult {
        // Identity mapping: IOVA = PA
        let pa = PA::new(iova.as_u64()).map_err(|_| TranslationError::AddressSizeError)?;

        // Full permissions in bypass mode (privileged_only not applicable)
        let permissions = PagePermissions::new(true, true, true);

        // §5.2 GAP-1: Apply STE output-attribute overrides even in bypass mode.
        let data = TranslationData::new(pa, permissions, security_state);
        Ok(self.apply_output_attrs(data))
    }

    // ---- GAP-1 / GAP-2 helpers ----

    /// Returns `true` if STE.STRW suppresses the `privileged_only` permission check.
    ///
    /// Per ARM §5.2, EL2 and EL3 stream worlds suppress the privilege check; all
    /// other worlds (El1El0, El2E2h) enforce it.
    ///
    /// Made `pub(crate)` so the SMMU TLB fast path (BUG-3 fix) can consult the live
    /// STRW value when deciding whether to enforce the `privileged_only` bit on a
    /// TLB cache hit, matching the slow-path privilege check in `translate_stage1_only`,
    /// `translate_stage2_only`, and `translate_two_stage`.
    #[inline]
    pub(crate) fn strw_suppresses_priv(&self) -> bool {
        matches!(self.get_strw(), StreamWorld::El2 | StreamWorld::El3)
    }

    /// Applies STE output-attribute overrides from the stream configuration to
    /// a successfully translated [`TranslationData`] (§5.2 GAP-1).
    ///
    /// - If `STE.MTCFG` is set, the `mem_type` field is overridden with `STE.MemAttr`.
    /// - `shareability`, `alloc_hint`, `inst_cfg`, `priv_cfg`, and `ns_cfg_out` are
    ///   always written from the corresponding STE fields.
    ///
    /// Made `pub` so that the SMMU TLB cache-hit path (BUG-RUST-DBGR-1 fix) can
    /// re-apply output attributes from the live stream configuration on a cache hit,
    /// without requiring `CacheEntry` to store the six GAP-1 fields.
    #[inline]
    pub fn apply_output_attrs(&self, data: TranslationData) -> TranslationData {
        // BUG-RUST-3 fix: use Acquire ordering for all output-attribute config
        // field loads so the Release stores in update_configuration() are
        // guaranteed visible before these reads on weakly-ordered architectures.
        let mem_type = if self.mt_cfg.load(Ordering::Acquire) {
            self.mem_attr.load(Ordering::Acquire)
        } else {
            0u8
        };
        let shareability = self.sh_cfg.load(Ordering::Acquire);
        let alloc_hint = self.alloc_cfg.load(Ordering::Acquire);
        let inst_cfg = self.inst_cfg.load(Ordering::Acquire);
        let priv_cfg = self.priv_cfg.load(Ordering::Acquire);
        let ns_cfg_out = self.ns_cfg.load(Ordering::Acquire);

        data.with_output_attrs(mem_type, shareability, alloc_hint, inst_cfg, priv_cfg, ns_cfg_out)
    }

    // ========================================================================
    // Section 4.2.1: Configuration Update Operations
    // ========================================================================

    /// Creates a builder for updating stream configuration
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    ///
    /// let ctx = StreamContext::new();
    /// let builder = ctx.update_config_builder();
    /// ```
    #[must_use]
    pub fn update_config_builder(&self) -> StreamConfigBuilder {
        StreamConfigBuilder {
            max_pasids_per_stream: None,
            stage1_enabled: None,
            stage2_enabled: None,
            stage2_address_space: None,
        }
    }

    /// Applies a configuration update transactionally
    ///
    /// Updates are all-or-nothing: either all changes succeed or none are applied.
    ///
    /// # Arguments
    ///
    /// * `builder` - Configuration builder with updates
    ///
    /// # Errors
    ///
    /// Returns error if configuration is invalid or inconsistent
    pub fn apply_config(&self, builder: StreamConfigBuilder) -> Result<(), StreamContextError> {
        // Validate configuration first
        self.validate_config_update(&builder)?;

        // Apply updates atomically
        if let Some(max_pasids) = builder.max_pasids_per_stream {
            self.max_pasids_per_stream.store(max_pasids, Ordering::SeqCst);
        }

        if let Some(stage1) = builder.stage1_enabled {
            self.stage1_enabled.store(stage1, Ordering::SeqCst);
        }

        if let Some(stage2) = builder.stage2_enabled {
            self.stage2_enabled.store(stage2, Ordering::SeqCst);
        }

        if let Some(addr_space) = builder.stage2_address_space {
            let mut stage2 = self.stage2_address_space.write().unwrap();
            *stage2 = addr_space;
        }

        Ok(())
    }

    /// Validates a configuration update before applying
    ///
    /// # Arguments
    ///
    /// * `builder` - Configuration builder to validate
    ///
    /// # Errors
    ///
    /// Returns error if configuration is invalid
    pub fn validate_config_update(&self, builder: &StreamConfigBuilder) -> Result<(), StreamContextError> {
        // Check PASID limit doesn't exceed ARM SMMU v3 maximum (2^20 - 1)
        if let Some(max_pasids) = builder.max_pasids_per_stream {
            if max_pasids > (1 << 20) {
                return Err(StreamContextError::ConfigurationError(
                    "max_pasids_per_stream exceeds ARM SMMU v3 limit (2^20)".to_string(),
                ));
            }

            // Check that new limit isn't less than current PASID count
            let current_count = self.pasid_count.load(Ordering::Acquire);
            if max_pasids < current_count {
                return Err(StreamContextError::ConfigurationError(format!(
                    "Cannot reduce max_pasids_per_stream to {} (current count: {})",
                    max_pasids, current_count
                )));
            }
        }

        // Check Stage-2 configuration consistency
        if let Some(stage2_enabled) = builder.stage2_enabled {
            if stage2_enabled {
                // If enabling Stage-2, must have Stage-2 address space
                let will_have_stage2 = if let Some(ref stage2_opt) = builder.stage2_address_space {
                    stage2_opt.is_some()
                } else {
                    let stage2_guard = self.stage2_address_space.read().unwrap();
                    stage2_guard.is_some()
                };

                if !will_have_stage2 {
                    return Err(StreamContextError::ConfigurationError(
                        "Stage-2 enabled but no Stage-2 AddressSpace configured".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }

    /// Returns the maximum number of PASIDs per stream
    #[must_use]
    pub fn max_pasids_per_stream(&self) -> usize {
        self.max_pasids_per_stream.load(Ordering::Relaxed)
    }

    // ========================================================================
    // Section 4.2.2: State Machine Operations
    // ========================================================================

    /// Enables the stream
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    ///
    /// let ctx = StreamContext::new();
    /// ctx.enable();
    /// assert!(ctx.is_enabled());
    /// ```
    pub fn enable(&self) {
        self.enabled.store(true, Ordering::SeqCst);
    }

    /// Disables the stream
    ///
    /// Automatically clears all PASIDs when disabling.
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    ///
    /// let ctx = StreamContext::new();
    /// ctx.disable();
    /// assert!(!ctx.is_enabled());
    /// ```
    pub fn disable(&self) {
        // BUG-RUST-1 fix (ARM IHI0070G.b §7.3.6 F_STREAM_DISABLED):
        //
        // Correct disable sequence to prevent TOCTOU race with translate():
        //
        //   1. Set `disabling=true` (SeqCst) — translators that pass the
        //      is_enabled() check but then find an empty pasid_map will
        //      detect this flag and return StreamDisabled instead of
        //      the incorrect PASIDNotFound.
        //   2. Clear pasid_map (and the ASID / count mirrors).
        //   3. Store `enabled=false` (SeqCst) — new translators are now
        //      rejected at the is_enabled() fast-path.
        //   4. Clear `disabling=false` (SeqCst) — the in-progress marker
        //      is no longer needed.
        //
        // This four-step sequence ensures that at no point can a translator
        // observe both is_enabled()==true and an empty pasid_map without also
        // seeing is_disabling()==true, which it converts to StreamDisabled.
        self.disabling.store(true, Ordering::SeqCst);
        self.pasid_map.clear();
        // Bug 5 fix: clear ASID map after pasid_map so recycled PASID values do
        // not inherit stale ASIDs on stream re-use.  Order:
        //   disabling=true → pasid_map.clear() → pasid_asid_map.clear()
        //   → enabled=false → disabling=false
        // preserves the invariant that an ASID entry only exists when a
        // corresponding pasid_map entry exists (ARM §3.17).
        self.pasid_asid_map.clear();
        self.pasid_count.store(0, Ordering::Release);
        self.enabled.store(false, Ordering::SeqCst);
        self.disabling.store(false, Ordering::SeqCst);
    }

    /// Checks if the stream is enabled
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    ///
    /// let ctx = StreamContext::new();
    /// assert!(ctx.is_enabled());
    /// ```
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        // BUG-RUST-DBGR-5 fix: use Acquire ordering to pair with the SeqCst
        // store in disable(). A Relaxed load has no happens-before relationship
        // with a Release/SeqCst store on weakly-ordered hardware; Acquire is
        // the correct and sufficient pairing.
        self.enabled.load(Ordering::Acquire)
    }

    /// Returns `true` while `disable()` is actively clearing the PASID map.
    ///
    /// Used by the translation hot-path to distinguish a `pasid_map` miss that
    /// occurred because the stream is being disabled (in which case the correct
    /// error is `StreamDisabled`) from a genuine missing PASID (`PASIDNotFound`).
    ///
    /// See `disable()` for the full synchronisation protocol (BUG-RUST-1 fix).
    #[inline]
    #[must_use]
    fn is_disabling(&self) -> bool {
        self.disabling.load(Ordering::Acquire)
    }

    // ========================================================================
    // Section 4.2.3: State Querying Operations
    // ========================================================================

    /// Returns a query interface for read-only access to stream state
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::PASID;
    ///
    /// let ctx = StreamContext::new();
    /// ctx.create_pasid(PASID::new(1).unwrap()).unwrap();
    ///
    /// let query = ctx.query();
    /// assert_eq!(query.pasid_count(), 1);
    /// ```
    #[must_use]
    pub fn query(&self) -> StreamContextQuery<'_> {
        StreamContextQuery { ctx: self }
    }

    // ========================================================================
    // Section 4.2.4: Fault Handling Operations
    // ========================================================================

    /// Records a fault internally with monotonic timestamp (no SystemTime overhead)
    ///
    /// This is an internal helper that automatically records translation faults
    /// with a monotonic counter-based timestamp for ordering.
    ///
    /// # Arguments
    ///
    /// * `pasid` - PASID that caused the fault
    /// * `iova` - Faulting address
    /// * `fault_type` - Type of fault
    /// * `access_type` - Access type that caused the fault
    /// * `security_state` - Security state
    // §3.12.2 / BUG-RUST-TWOSTAGE-S1-FAULT-CLASS fix: all translate_* methods
    // no longer call this helper directly — fault recording is now handled
    // exclusively by the top-level smmu/mod.rs::translate() caller.  The method
    // is retained for the deprecated public `record_fault()` path and for any
    // future internal use.
    #[allow(dead_code)]
    #[inline]
    fn record_fault_internal(
        &self,
        pasid: PASID,
        iova: IOVA,
        fault_type: FaultType,
        access_type: AccessType,
        security_state: SecurityState,
    ) {
        // Use monotonic counter for timestamp (no SystemTime overhead)
        let timestamp = self.fault_timestamp_counter.fetch_add(1, Ordering::Relaxed);

        // BUG-RUST-DBGR-9 fix: use write() (blocking) instead of try_write().
        // Fault records must never be silently dropped due to lock contention.
        // BUG-RUST-DBGR-10 fix: use stored stream_id (not placeholder 0).
        // The spec (§7.3) requires the StreamID of the requester in fault records.
        let stream_id_val = self.stream_id.load(Ordering::Acquire);
        let stream_id = crate::types::StreamID::new(stream_id_val)
            .unwrap_or_else(|_| crate::types::StreamID::new(0).unwrap());

        if let Ok(mut records) = self.fault_records.write() {
            // Honour the configured rate limit (default: usize::MAX — effectively unlimited).
            let rate_limit = self.fault_rate_limit.load(Ordering::Relaxed);
            if records.len() < rate_limit {
                let fault = FaultRecord::builder()
                    .stream_id(stream_id)
                    .pasid(pasid)
                    .address(iova)
                    .fault_type(fault_type)
                    .access_type(access_type)
                    .security_state(security_state)
                    .timestamp(timestamp as u64)
                    .build();

                records.push(fault);
            }
        }
    }

    /// Records a fault for diagnostic purposes
    ///
    /// **DEPRECATED**: Fault recording has been moved to the SMMU level to eliminate
    /// redundant recording and SystemTime overhead. This method is kept for backward
    /// compatibility but may be removed in a future version.
    ///
    /// # Migration
    ///
    /// Use `SMMU::get_faults()` instead of `StreamContext::get_fault_records()` to
    /// retrieve fault information.
    ///
    /// # Arguments
    ///
    /// * `_pasid` - PASID that caused the fault
    /// * `fault` - Fault record to store
    #[deprecated(
        since = "1.0.4",
        note = "Fault recording moved to SMMU level. Use SMMU::get_faults() instead."
    )]
    pub fn record_fault(&self, _pasid: PASID, fault: FaultRecord) {
        let mut records = self.fault_records.write().unwrap();

        // Check rate limit
        let rate_limit = self.fault_rate_limit.load(Ordering::Relaxed);
        if records.len() < rate_limit {
            records.push(fault);
        }
    }

    /// Returns all recorded faults
    #[must_use]
    pub fn get_fault_records(&self) -> Vec<FaultRecord> {
        let records = self.fault_records.read().unwrap();
        records.clone()
    }

    /// Returns the total number of recorded faults
    #[must_use]
    pub fn get_fault_count(&self) -> usize {
        let records = self.fault_records.read().unwrap();
        records.len()
    }

    /// Clears all fault records
    pub fn clear_fault_records(&self) {
        let mut records = self.fault_records.write().unwrap();
        records.clear();
    }

    /// Returns fault statistics
    #[must_use]
    pub fn get_fault_statistics(&self) -> FaultStatistics {
        let records = self.fault_records.read().unwrap();

        let mut faults_by_type: HashMap<FaultType, u64> = HashMap::new();
        let mut faults_by_pasid: HashMap<u32, u64> = HashMap::new();
        let mut last_fault_time: Option<u64> = None;

        for record in records.iter() {
            *faults_by_type.entry(record.fault_type()).or_insert(0) += 1;
            *faults_by_pasid.entry(record.pasid().as_u32()).or_insert(0) += 1;

            if let Some(last_time) = last_fault_time {
                if record.timestamp() > last_time {
                    last_fault_time = Some(record.timestamp());
                }
            } else {
                last_fault_time = Some(record.timestamp());
            }
        }

        let total_faults = records.len() as u64;
        let page_not_mapped_count = *faults_by_type.get(&FaultType::TranslationFault).unwrap_or(&0);
        let permission_violation_count = *faults_by_type.get(&FaultType::PermissionFault).unwrap_or(&0);

        let rate_limit = self.fault_rate_limit.load(Ordering::Relaxed);
        let rate_limited = total_faults >= rate_limit as u64;

        FaultStatistics {
            total_faults,
            faults_by_type,
            faults_by_pasid,
            last_fault_time,
            page_not_mapped_count,
            permission_violation_count,
            rate_limited,
        }
    }

    /// Resets fault statistics
    pub fn reset_fault_statistics(&self) {
        self.clear_fault_records();
    }

    /// Sets the fault rate limit
    ///
    /// # Arguments
    ///
    /// * `limit` - Maximum number of faults to record
    pub fn set_fault_rate_limit(&self, limit: usize) {
        self.fault_rate_limit.store(limit, Ordering::SeqCst);
    }

    /// Enables or disables fault retry
    ///
    /// # Arguments
    ///
    /// * `enabled` - True to enable retry, false to disable
    pub fn enable_fault_retry(&self, enabled: bool) {
        self.fault_retry_enabled.store(enabled, Ordering::SeqCst);
    }

    /// Translates with retry on fault (if retry enabled)
    pub fn translate_with_retry(
        &self,
        pasid: PASID,
        iova: IOVA,
        access_type: AccessType,
        security_state: SecurityState,
    ) -> TranslationResult {
        self.translate(pasid, iova, access_type, security_state)
    }
}

/// Stream configuration builder for partial updates (Section 4.2.1)
///
/// Provides a fluent interface for building stream configuration updates.
/// All updates are optional and transactional.
///
/// # Examples
///
/// ```
/// use smmu::stream_context::{StreamContext, StreamConfigBuilder};
///
/// let ctx = StreamContext::new();
/// let config = StreamConfigBuilder::new()
///     .max_pasids_per_stream(512)
///     .stage1_enabled(false)
///     .build();
///
/// ctx.apply_config(config).unwrap();
/// ```
#[derive(Debug, Clone, Default)]
pub struct StreamConfigBuilder {
    max_pasids_per_stream: Option<usize>,
    stage1_enabled: Option<bool>,
    stage2_enabled: Option<bool>,
    stage2_address_space: Option<Option<Arc<AddressSpace>>>,
}

impl StreamConfigBuilder {
    /// Creates a new empty configuration builder
    #[must_use]
    pub const fn new() -> Self {
        Self {
            max_pasids_per_stream: None,
            stage1_enabled: None,
            stage2_enabled: None,
            stage2_address_space: None,
        }
    }

    /// Sets the maximum PASIDs per stream
    #[must_use]
    pub const fn max_pasids_per_stream(mut self, max: usize) -> Self {
        self.max_pasids_per_stream = Some(max);
        self
    }

    /// Sets Stage-1 enabled state
    #[must_use]
    pub const fn stage1_enabled(mut self, enabled: bool) -> Self {
        self.stage1_enabled = Some(enabled);
        self
    }

    /// Sets Stage-2 enabled state
    #[must_use]
    pub const fn stage2_enabled(mut self, enabled: bool) -> Self {
        self.stage2_enabled = Some(enabled);
        self
    }

    /// Sets Stage-2 AddressSpace
    #[must_use]
    pub fn stage2_address_space(mut self, addr_space: Option<Arc<AddressSpace>>) -> Self {
        self.stage2_address_space = Some(addr_space);
        self
    }

    /// Builds the configuration
    #[must_use]
    pub const fn build(self) -> Self {
        self
    }
}

/// Read-only query interface for stream context (Section 4.2.3)
///
/// Provides efficient read-only access to stream state without blocking
/// concurrent operations.
///
/// # Examples
///
/// ```
/// use smmu::stream_context::StreamContext;
/// use smmu::types::PASID;
///
/// let ctx = StreamContext::new();
/// ctx.create_pasid(PASID::new(1).unwrap()).unwrap();
///
/// let query = ctx.query();
/// assert_eq!(query.pasid_count(), 1);
/// assert!(query.has_pasid(PASID::new(1).unwrap()));
/// ```
#[derive(Debug)]
pub struct StreamContextQuery<'a> {
    ctx: &'a StreamContext,
}

impl<'a> StreamContextQuery<'a> {
    /// Returns the number of configured PASIDs
    #[must_use]
    pub fn pasid_count(&self) -> usize {
        self.ctx.pasid_count()
    }

    /// Checks if a PASID exists
    #[must_use]
    pub fn has_pasid(&self, pasid: PASID) -> bool {
        self.ctx.has_pasid(pasid)
    }

    /// Returns an iterator over all PASIDs
    pub fn pasids(&self) -> impl Iterator<Item = PASID> + 'a {
        // Collect PASID keys into a Vec to avoid holding DashMap lock
        let pasid_values: Vec<u32> = self.ctx.pasid_map.iter().map(|entry| *entry.key()).collect();

        pasid_values.into_iter().filter_map(|val| PASID::new(val).ok())
    }

    /// Checks if stream is enabled
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.ctx.is_enabled()
    }

    /// Returns fault statistics
    #[must_use]
    pub fn get_stats(&self) -> FaultStatistics {
        self.ctx.get_fault_statistics()
    }

    /// Returns PASIDs filtered by security state.
    ///
    /// Security state is tracked at the stream level (ARM §5.2 STE.SEC_SID classifies
    /// the entire stream and all its substreams/PASIDs into a single security domain).
    /// If the stream's security state matches `security_state`, all configured PASIDs
    /// are returned; otherwise the iterator is empty.
    pub fn pasids_by_security_state(&self, security_state: SecurityState) -> impl Iterator<Item = PASID> + 'a {
        let pasid_values: Vec<u32> = if self.ctx.security_state() == security_state {
            self.ctx.pasid_map.iter().map(|entry| *entry.key()).collect()
        } else {
            Vec::new()
        };
        pasid_values.into_iter().filter_map(|val| PASID::new(val).ok())
    }
}

/// Fault statistics structure (Section 4.2.4)
///
/// Provides comprehensive fault tracking and analysis.
#[derive(Debug, Clone)]
pub struct FaultStatistics {
    /// Total number of faults recorded
    pub total_faults: u64,
    /// Faults grouped by type
    pub faults_by_type: HashMap<FaultType, u64>,
    /// Faults grouped by PASID
    pub faults_by_pasid: HashMap<u32, u64>,
    /// Timestamp of most recent fault
    pub last_fault_time: Option<u64>,
    /// Count of PageNotMapped faults
    pub page_not_mapped_count: u64,
    /// Count of PermissionViolation faults
    pub permission_violation_count: u64,
    /// Whether rate limiting is active
    pub rate_limited: bool,
}

impl Default for StreamContext {
    fn default() -> Self {
        Self::new()
    }
}

// StreamContext is automatically Send + Sync since all components are Send + Sync:
// - DashMap is Send + Sync
// - RwLock is Send + Sync
// - AtomicBool is Send + Sync
// - AtomicUsize is Send + Sync

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_context_new() {
        let ctx = StreamContext::new();
        assert!(ctx.is_stage1_enabled());
        assert!(!ctx.is_stage2_enabled());
        assert_eq!(ctx.pasid_count(), 0);
    }

    #[test]
    fn test_create_pasid_success() {
        let ctx = StreamContext::new();
        let pasid = PASID::new(1).unwrap();
        assert!(ctx.create_pasid(pasid).is_ok());
        assert!(ctx.has_pasid(pasid));
        assert_eq!(ctx.pasid_count(), 1);
    }

    #[test]
    fn test_create_duplicate_pasid() {
        let ctx = StreamContext::new();
        let pasid = PASID::new(1).unwrap();
        ctx.create_pasid(pasid).unwrap();

        let result = ctx.create_pasid(pasid);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StreamContextError::PASIDAlreadyExists(1)));
    }

    #[test]
    fn test_remove_pasid() {
        let ctx = StreamContext::new();
        let pasid = PASID::new(1).unwrap();
        ctx.create_pasid(pasid).unwrap();
        assert!(ctx.remove_pasid(pasid).is_ok());
        assert!(!ctx.has_pasid(pasid));
        assert_eq!(ctx.pasid_count(), 0);
    }

    #[test]
    fn test_stage_configuration() {
        let ctx = StreamContext::new();
        ctx.set_stage1_enabled(false);
        ctx.set_stage2_enabled(true);
        assert!(!ctx.is_stage1_enabled());
        assert!(ctx.is_stage2_enabled());
    }

    // ========================================================================
    // TDD tests for FINDING-M-04: Access Flag and Dirty State simulation
    // These tests will FAIL before implementation.
    // ========================================================================

    #[test]
    fn test_stream_context_ha_sets_af_after_translate() {
        let ctx = StreamContext::new();
        ctx.set_ha(true);

        let pasid = PASID::new(0).unwrap();
        ctx.create_pasid(pasid).unwrap();

        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        ctx.map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        // Translate — should set AF
        let _ = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);

        // Verify AF was set on the address space
        let addr_space_ref = ctx.pasid_map.get(&0).unwrap();
        assert_eq!(addr_space_ref.get_page_access_flag(iova), Some(true));
    }

    #[test]
    fn test_stream_context_hd_sets_dirty_on_write() {
        let ctx = StreamContext::new();
        ctx.set_hd(true);

        let pasid = PASID::new(0).unwrap();
        ctx.create_pasid(pasid).unwrap();

        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        ctx.map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        // Write translate — should set dirty
        let _ = ctx.translate(pasid, iova, AccessType::Write, SecurityState::NonSecure);

        let addr_space_ref = ctx.pasid_map.get(&0).unwrap();
        assert_eq!(addr_space_ref.get_page_dirty(iova), Some(true));
    }

    #[test]
    fn test_stream_context_hd_not_set_on_read() {
        let ctx = StreamContext::new();
        ctx.set_hd(true);

        let pasid = PASID::new(0).unwrap();
        ctx.create_pasid(pasid).unwrap();

        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        ctx.map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        // Read translate — should NOT set dirty
        let _ = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);

        let addr_space_ref = ctx.pasid_map.get(&0).unwrap();
        assert_eq!(addr_space_ref.get_page_dirty(iova), Some(false));
    }

    #[test]
    fn test_two_stage_translation_pasid_0() {
        let ctx = StreamContext::new();

        // Configure for two-stage translation
        ctx.set_stage1_enabled(true);
        ctx.set_stage2_enabled(true);

        // Create PASID 0
        let pasid = PASID::new(0).unwrap();
        ctx.create_pasid(pasid).unwrap();

        // Create Stage-2 address space
        ctx.create_stage2_address_space().unwrap();

        // Map Stage-1: IOVA -> IPA
        let iova = IOVA::new(0x1000).unwrap();
        let ipa = PA::new(0x2000).unwrap();
        ctx.map_page(pasid, iova, ipa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        // Map Stage-2: IPA -> PA
        let ipa_as_iova = IOVA::new(ipa.as_u64()).unwrap();
        let final_pa = PA::new(0x3000).unwrap();
        ctx.map_stage2_page(ipa_as_iova, final_pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        // Translate with PASID 0 - this should NOT fail with PASIDNotFound
        let result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
        assert!(result.is_ok(), "Two-stage translation with PASID 0 should succeed: {:?}", result);
        assert_eq!(result.unwrap().physical_address(), final_pa);
    }

    // ── BUG-RUST-M04: create_pasid() limit check must be atomic (TOCTOU) ────

    /// Regression guard: `create_pasid()` must enforce the max-PASID limit atomically
    /// using `entry()` so that no concurrent inserter can slip a PASID in between
    /// the count check and the actual insert.
    ///
    /// Single-threaded proof: set max_pasids to 2, insert PASID 1 and PASID 2 to fill
    /// the limit exactly, then verify that a third insert is rejected with
    /// `PASIDLimitExceeded`.  Before the fix the check (`pasid_map.len()`) was
    /// performed outside the shard lock, creating a window for concurrent overcount.
    #[test]
    fn bug_rust_m04_create_pasid_enforces_limit_atomically() {
        let ctx = StreamContext::new();
        ctx.set_max_pasids_per_stream(2);

        let p1 = PASID::new(1).unwrap();
        let p2 = PASID::new(2).unwrap();
        let p3 = PASID::new(3).unwrap();

        assert!(ctx.create_pasid(p1).is_ok(), "first PASID must succeed");
        assert!(ctx.create_pasid(p2).is_ok(), "second PASID must succeed (fills limit)");

        let result = ctx.create_pasid(p3);
        assert!(
            matches!(result, Err(StreamContextError::PASIDLimitExceeded(2, 2))),
            "BUG-RUST-M04: third PASID must be rejected with PASIDLimitExceeded; got {result:?}"
        );

        // Confirm neither PASID 1 nor PASID 2 was corrupted by the failed insert attempt.
        assert!(ctx.has_pasid(p1), "PASID 1 must still exist after failed third insert");
        assert!(ctx.has_pasid(p2), "PASID 2 must still exist after failed third insert");
        assert!(!ctx.has_pasid(p3), "PASID 3 must NOT have been inserted");
        assert_eq!(ctx.pasid_count(), 2, "pasid_count must remain 2 after failed insert");
    }

    /// Regression guard: duplicate PASID insertion must be rejected atomically.
    /// The `entry()` API ensures the `Occupied` branch is taken with the shard
    /// lock held, preventing a second concurrent caller from inserting the same key.
    #[test]
    fn bug_rust_m04_create_pasid_rejects_duplicate_atomically() {
        let ctx = StreamContext::new();
        let pasid = PASID::new(42).unwrap();

        assert!(ctx.create_pasid(pasid).is_ok());
        let result = ctx.create_pasid(pasid);
        assert!(
            matches!(result, Err(StreamContextError::PASIDAlreadyExists(42))),
            "BUG-RUST-M04: duplicate create_pasid must return PASIDAlreadyExists; got {result:?}"
        );
        assert_eq!(ctx.pasid_count(), 1, "pasid_count must remain 1 after duplicate rejection");
    }

    // ── BUG-NEW2-07: add_pasid() must be atomic (TOCTOU fix) ─────────────────

    /// Regression guard: `add_pasid()` must reject a duplicate PASID atomically.
    ///
    /// Before the fix, `add_pasid()` used a non-atomic `contains_key()` + `insert()`
    /// sequence.  Two concurrent callers with the same PASID could both pass the
    /// `contains_key()` check and then race to `insert()`, silently overwriting the
    /// first entry without returning `PASIDAlreadyExists`.  The fix uses
    /// `DashMap::entry()` so the check and insert happen under the same shard lock.
    ///
    /// Note: this test uses `add_pasid()` exclusively (no `create_pasid()`) to avoid
    /// triggering the pre-existing deadlock in `create_pasid()`'s capacity check,
    /// which calls `pasid_map.len()` while holding a DashMap shard write-lock.
    #[test]
    fn bug_new2_07_add_pasid_rejects_duplicate_atomically() {
        use crate::address_space::AddressSpace;

        let ctx = StreamContext::new();
        let pasid1 = PASID::new(1).unwrap();
        let pasid2 = PASID::new(2).unwrap();

        // Seed pasid1 via add_pasid to avoid any create_pasid deadlock.
        let as1 = Arc::new(AddressSpace::new());
        ctx.add_pasid(pasid1, Arc::clone(&as1)).unwrap();

        let as2 = Arc::new(AddressSpace::new());

        // First add_pasid for pasid2 must succeed.
        assert!(
            ctx.add_pasid(pasid2, Arc::clone(&as2)).is_ok(),
            "BUG-NEW2-07: first add_pasid must succeed"
        );

        // Second add_pasid for the same PASID must be rejected.
        let result = ctx.add_pasid(pasid2, Arc::clone(&as2));
        assert!(
            matches!(result, Err(StreamContextError::PASIDAlreadyExists(2))),
            "BUG-NEW2-07: duplicate add_pasid must return PASIDAlreadyExists; got {result:?}"
        );
        assert_eq!(ctx.pasid_count(), 2, "pasid_count must remain 2 after duplicate rejection");
    }

    /// Regression guard: `add_pasid()` must enforce the max-PASID limit atomically.
    ///
    /// Set the cap to 2, fill it with `add_pasid`, then verify that a third
    /// `add_pasid` is rejected with `PASIDLimitExceeded` rather than silently
    /// inserting past the limit due to the former non-atomic count-then-insert
    /// sequence.
    ///
    /// Note: this test uses `add_pasid()` exclusively (no `create_pasid()`) to avoid
    /// triggering the pre-existing deadlock in `create_pasid()`'s capacity check,
    /// which calls `pasid_map.len()` while holding a DashMap shard write-lock.
    #[test]
    fn bug_new2_07_add_pasid_enforces_limit_atomically() {
        use crate::address_space::AddressSpace;

        let ctx = StreamContext::new();
        ctx.set_max_pasids_per_stream(2);

        let p1 = PASID::new(1).unwrap();
        let p2 = PASID::new(2).unwrap();
        let p3 = PASID::new(3).unwrap();

        ctx.add_pasid(p1, Arc::new(AddressSpace::new())).unwrap();
        ctx.add_pasid(p2, Arc::new(AddressSpace::new())).unwrap();

        let result = ctx.add_pasid(p3, Arc::new(AddressSpace::new()));
        assert!(
            matches!(result, Err(StreamContextError::PASIDLimitExceeded(2, 2))),
            "BUG-NEW2-07: add_pasid past limit must return PASIDLimitExceeded; got {result:?}"
        );
        assert!(!ctx.has_pasid(p3), "PASID 3 must NOT have been inserted");
        assert_eq!(ctx.pasid_count(), 2, "pasid_count must remain 2 after failed add_pasid");
    }

    // ── BUG-RUST-G: create_pasid() must atomically reserve slot via fetch_add ─

    /// BUG-RUST-G: The old code checked pasid_count INSIDE the shard lock,
    /// which means two threads inserting different PASIDs (different shards) can
    /// both pass the count < max_pasids check before either increments the atomic.
    ///
    /// This single-threaded test verifies the structural correctness of the fix:
    /// after a duplicate PASID is rejected, pasid_count must not have increased.
    ///
    /// With the BUG-RUST-G fix, fetch_add runs BEFORE the entry() check.
    /// When at capacity, a duplicate returns PASIDLimitExceeded (the limit
    /// check fires first).  When NOT at capacity, a duplicate returns
    /// PASIDAlreadyExists.  We test the not-at-capacity case here.
    #[test]
    fn bug_rust_g_create_pasid_duplicate_does_not_leak_count() {
        let ctx = StreamContext::new();
        // Set limit to 3 so we have room — the duplicate is p1 at count=1 (not at limit).
        ctx.set_max_pasids_per_stream(3);

        let p1 = PASID::new(1).unwrap();
        let p2 = PASID::new(2).unwrap();

        ctx.create_pasid(p1).unwrap();
        ctx.create_pasid(p2).unwrap();
        assert_eq!(ctx.pasid_count(), 2, "pre-condition: count must be 2");

        // Attempt to insert PASID 1 again — below limit so the entry check fires.
        // With BUG-RUST-G fix: fetch_add (count=3, prev=2 < 3), then entry() finds
        // Occupied, rolls back, returns PASIDAlreadyExists.
        let result = ctx.create_pasid(p1);
        assert!(
            matches!(result, Err(StreamContextError::PASIDAlreadyExists(1))),
            "BUG-RUST-G: duplicate create_pasid (below limit) must return PASIDAlreadyExists; got {result:?}"
        );
        // Count must roll back to 2 after the duplicate attempt.
        assert_eq!(
            ctx.pasid_count(),
            2,
            "BUG-RUST-G: pasid_count must not increase after duplicate create_pasid"
        );
    }

    /// BUG-RUST-G: When the limit is exceeded, create_pasid must roll back the
    /// pre-reserved atomic slot so pasid_count remains at the limit.
    #[test]
    fn bug_rust_g_create_pasid_limit_rolls_back_atomic() {
        let ctx = StreamContext::new();
        ctx.set_max_pasids_per_stream(2);

        let p1 = PASID::new(1).unwrap();
        let p2 = PASID::new(2).unwrap();
        let p3 = PASID::new(3).unwrap();

        ctx.create_pasid(p1).unwrap();
        ctx.create_pasid(p2).unwrap();
        assert_eq!(ctx.pasid_count(), 2, "pre-condition: count must be 2");

        // Third PASID must be rejected by the limit check.
        let result = ctx.create_pasid(p3);
        assert!(
            result.is_err(),
            "BUG-RUST-G: create_pasid beyond limit must fail; got {result:?}"
        );
        assert!(
            !ctx.has_pasid(p3),
            "BUG-RUST-G: PASID 3 must not have been inserted"
        );
        // Count must remain at 2 — the pre-reserved slot must have been rolled back.
        assert_eq!(
            ctx.pasid_count(),
            2,
            "BUG-RUST-G: pasid_count must roll back to 2 after limit-exceeded create_pasid"
        );
    }
}
