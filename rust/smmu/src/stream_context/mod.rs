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

    // ---- CONF-GAP-14: STE.MEV merged event flag (§5.2) ----

    /// §5.2 STE.MEV: Merged Event.
    ///
    /// When `true`, duplicate fault events (same type + stream_id) are suppressed
    /// — only the first occurrence is recorded.  Default: `false`.
    mev: AtomicBool,

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

    // ---- NEW-3: STE.S2T0SZ Stage-2 IPA input range (§3.4/§5.2) ----

    /// §5.2 STE.S2T0SZ: Stage-2 T0SZ — input IPA range (6 bits, 0-63).
    ///
    /// When > 0, any IPA at or above `2^(64-S2T0SZ)` generates F_TRANSLATION.
    /// Default 16 (48-bit IPA range).
    s2_t0sz: AtomicU8,

    // ---- NEW-3/NEW-8: STE.S2PS Stage-2 output PA size (§3.4/§5.2/§7.3.14) ----

    /// §5.2 STE.S2PS: Stage-2 output physical address size (3 bits).
    ///
    /// Encoding: 0=32-bit, 1=36-bit, 2=40-bit, 3=42-bit, 4=44-bit, 5=48-bit, 6=52-bit.
    /// After stage-2 translation, the output PA must be within this range (F_ADDR_SIZE).
    /// Default 5 (48-bit PA).
    s2_ps: AtomicU8,

    // ---- NEW-7: CD.EPD0 TTBR0 translation table walk disable (§5.4) ----

    /// §5.4 CD.EPD0: when `true`, all TTBR0 (stage-1) translation table walks are
    /// disabled and generate F_TRANSLATION.  Default `false`.
    epd0: AtomicBool,

    // ---- GAP-E: CD.TBI top-byte-ignore (§3.4.1/§5.4) ----

    /// §5.4 CD.TBI: top-byte-ignore; VA bits[63:56] masked before T0SZ range
    /// check (§3.4.1).  Default `false`.
    tbi: AtomicBool,

    // ---- GAP-F: CD.IPS per-CD stage-1 output IPA size (§5.4/§3.4) ----

    /// §5.4 CD.IPS: stage-1 output IPA size encoding (3 bits, same encoding as
    /// STE.S2PS).  After stage-1 produces an IPA the IPA must be within this
    /// range (F_ADDR_SIZE).  Default 5 (48-bit).
    ips: AtomicU8,

    // ---- NEW-12: STE.EATS — Enhanced Address Translation Security (§5.2, §3.9) ----

    /// §5.2 STE.EATS: ATS support level; 0 = no ATS.  Default 0.
    eats: AtomicU8,

    // ---- GAP-NEW-G: STE.S1STALLD — stall-disabled override (§5.2) ----

    /// §5.2 STE.S1STALLD: when `true`, abort semantics are used even when
    /// `stall_enabled=true` (CD.S=1).  Default `false`.
    s1_stalld: AtomicBool,

    // ---- NEW-GAP-J: Access Flag Fault management (§3.13.2) ----

    /// CD.AFFD: Access Flag Fault Disable — when true, suppresses F_ACCESS even when AF=0.
    affd: AtomicBool,
    /// STE.S2AFFD: Stage-2 Access Flag Fault Disable.
    s2affd: AtomicBool,
    /// STE.S2HA: Stage-2 Hardware Access flag update enable.
    s2ha: AtomicBool,
    /// STE.S2HD: Stage-2 Hardware Dirty flag update enable.
    s2hd: AtomicBool,

    // ---- NEW-GAP-K: WXN/UWXN permission modifiers (§5.4) ----

    /// CD.WXN: Write eXecute Never — writable pages are execute-never.
    wxn: AtomicBool,
    /// CD.UWXN: Unprivileged Write eXecute Never for privileged accesses.
    uwxn: AtomicBool,

    // ---- NEW-GAP-L: S2PTW protected table walk (§5.2) ----

    /// STE.S2PTW: Protected Table Walk — device-memory stage-2 page causes F_PERMISSION.
    s2ptw: AtomicBool,

    // ---- BUG-QA-12: STE.S2S — Stage-2 Stall (§5.5) ----

    /// STE.S2S: when `true`, stage-2 faults stall (independently of CD.S / FaultMode).
    s2_stall: AtomicBool,

    // ---- BUG-QA-13: STE.S2R — Stage-2 Record (§5.5) ----

    /// STE.S2R: when `false` (and s2_stall=false), stage-2 fault events are suppressed.
    s2_record: AtomicBool,
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
            mev: AtomicBool::new(false),
            s2_t0sz: AtomicU8::new(16),
            s2_ps: AtomicU8::new(5),
            epd0: AtomicBool::new(false),
            tbi: AtomicBool::new(false),
            ips: AtomicU8::new(5),
            eats: AtomicU8::new(0),
            s1_stalld: AtomicBool::new(false),
            affd: AtomicBool::new(false),
            s2affd: AtomicBool::new(false),
            s2ha: AtomicBool::new(false),
            s2hd: AtomicBool::new(false),
            wxn: AtomicBool::new(false),
            uwxn: AtomicBool::new(false),
            s2ptw: AtomicBool::new(false),
            s2_stall: AtomicBool::new(false),
            s2_record: AtomicBool::new(true),
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

    /// Returns whether stall fault mode is active for this stream (ARM §3.12.2).
    ///
    /// Stall is active only when `CD.S=1` (`stall_enabled=true`) AND
    /// `STE.S1STALLD=0` (`s1_stalld=false`).  When `S1STALLD=1` the STE forces
    /// abort semantics even if the CD requests stall (ARM §5.2 STE.S1STALLD).
    #[inline]
    pub fn is_stall_enabled(&self) -> bool {
        self.stall_enabled.load(Ordering::Relaxed)
            && !self.s1_stalld.load(Ordering::Relaxed)
    }

    /// Enables or disables stall fault mode for this stream (ARM §3.12.2).
    ///
    /// When enabled, faulting translations return `Stalled { stag }` instead
    /// of an immediate abort error.
    #[inline]
    pub fn set_stall_enabled(&self, enabled: bool) {
        self.stall_enabled.store(enabled, Ordering::Release);
    }

    /// Returns whether the S1STALLD override is set (GAP-NEW-G, ARM §5.2).
    ///
    /// When `true`, stall semantics are suppressed regardless of `CD.S`.
    #[inline]
    #[must_use]
    pub fn is_s1_stalld_set(&self) -> bool {
        self.s1_stalld.load(Ordering::Relaxed)
    }

    /// Sets the S1STALLD override flag (GAP-NEW-G, ARM §5.2).
    #[inline]
    pub fn set_s1_stalld(&self, v: bool) {
        self.s1_stalld.store(v, Ordering::Release);
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

    /// Returns the effective access type after applying INSTCFG and PRIVCFG overrides.
    ///
    /// This mirrors the transformations applied inside `translate()` and
    /// `translate_and_get_stage2_ipa()`, so callers (e.g. smmu/mod.rs fault
    /// recording) can derive the correct `ind`/`rnw` values for event entries.
    ///
    /// The order matches the implementation: INSTCFG is applied first, then PRIVCFG.
    ///
    /// BUG-2 fix: §13.1.2 — INSTCFG applies to ALL reads, privileged or not.
    /// BUG-3 fix: §3.3.4/§13.5 — PRIVCFG applied to the INSTCFG-adjusted type.
    /// BUG-AUDIT-133 fix: §5.2 — correct INSTCFG encoding:
    ///   0b00 (0) = Use Incoming (passthrough)
    ///   0b01 (1) = Reserved — behaves as 0b00 (passthrough)
    ///   0b10 (2) = Force Data
    ///   0b11 (3) = Force Instruction
    #[inline]
    pub(crate) fn effective_access_type(&self, access_type: AccessType) -> AccessType {
        // Step 1: apply INSTCFG
        let after_instcfg = match self.inst_cfg.load(Ordering::Acquire) {
            // 0b11 (3) = Force Instruction: ARM §13.1.2 — INSTCFG can only change
            // the instruction/data marking of *reads*.  Writes are always considered
            // Data on input, prior to any translation table permission checks.
            // Therefore only Read → Execute and ReadPrivileged → ExecutePrivileged;
            // Write and WritePrivileged fall through unchanged (§13.1.2).
            3 => match access_type {
                AccessType::Read           => AccessType::Execute,
                AccessType::ReadPrivileged => AccessType::ExecutePrivileged,
                _ => access_type,
            },
            2 => match access_type {
                AccessType::Execute               => AccessType::Read,
                AccessType::ExecutePrivileged     => AccessType::ReadPrivileged,
                // BUG-RUST-4 INSTCFG fix: §5.2 INSTCFG=2 (Force-Data) must also
                // demote compound-execute types that carry an execute component.
                // ReadExecute → Read  (execute component stripped).
                // ReadExecutePrivileged → ReadPrivileged.
                AccessType::ReadExecute           => AccessType::Read,
                AccessType::ReadExecutePrivileged => AccessType::ReadPrivileged,
                _ => access_type,
            },
            _ => access_type,
        };
        // Step 2: apply PRIVCFG
        match self.priv_cfg.load(Ordering::Acquire) {
            2 => match after_instcfg {
                AccessType::ReadPrivileged             => AccessType::Read,
                AccessType::WritePrivileged            => AccessType::Write,
                AccessType::ExecutePrivileged          => AccessType::Execute,
                AccessType::ReadWritePrivileged        => AccessType::ReadWrite,
                // BUG-RUST-5 fix: demote compound-execute privileged variants.
                AccessType::ReadExecutePrivileged      => AccessType::ReadExecute,
                AccessType::WriteExecutePrivileged     => AccessType::WriteExecute,
                AccessType::ReadWriteExecutePrivileged => AccessType::ReadWriteExecute,
                other => other,
            },
            3 => match after_instcfg {
                AccessType::Read             => AccessType::ReadPrivileged,
                AccessType::Write            => AccessType::WritePrivileged,
                AccessType::Execute          => AccessType::ExecutePrivileged,
                AccessType::ReadWrite        => AccessType::ReadWritePrivileged,
                // BUG-RUST-5 fix: promote compound-execute access types.
                AccessType::ReadExecute      => AccessType::ReadExecutePrivileged,
                AccessType::WriteExecute     => AccessType::WriteExecutePrivileged,
                AccessType::ReadWriteExecute => AccessType::ReadWriteExecutePrivileged,
                other => other,
            },
            _ => after_instcfg,
        }
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

        // CONF-GAP-14: MEV (Merged Event) flag
        self.mev.store(cfg.mev, Ordering::Release);

        // NEW-3: S2T0SZ / S2PS Stage-2 translation parameters (§3.4/§5.2)
        self.s2_t0sz.store(cfg.s2_t0sz, Ordering::Release);
        self.s2_ps.store(cfg.s2_ps, Ordering::Release);

        // NEW-7: CD.EPD0 — TTBR0 table walk disable (§5.4)
        self.epd0.store(cfg.epd0, Ordering::Release);

        // GAP-E: CD.TBI — top-byte-ignore for VA range check (§3.4.1/§5.4)
        self.tbi.store(cfg.tbi, Ordering::Release);

        // GAP-F: CD.IPS — stage-1 output IPA size (§5.4/§3.4)
        self.ips.store(cfg.ips, Ordering::Release);

        // NEW-12: STE.EATS — ATS support level (§5.2, §3.9)
        self.eats.store(cfg.eats, Ordering::Release);

        // GAP-NEW-G: STE.S1STALLD — stall-disabled override (§5.2)
        self.s1_stalld.store(cfg.s1_stalld, Ordering::Release);

        // NEW-GAP-J: Access Flag Fault management (§3.13.2)
        self.affd.store(cfg.affd, Ordering::Release);
        self.s2affd.store(cfg.s2affd, Ordering::Release);
        self.s2ha.store(cfg.s2ha, Ordering::Release);
        self.s2hd.store(cfg.s2hd, Ordering::Release);

        // NEW-GAP-K: WXN/UWXN permission modifiers (§5.4)
        self.wxn.store(cfg.wxn, Ordering::Release);
        self.uwxn.store(cfg.uwxn, Ordering::Release);

        // NEW-GAP-L: S2PTW protected table walk (§5.2)
        self.s2ptw.store(cfg.s2ptw, Ordering::Release);

        // BUG-QA-12: STE.S2S — stage-2 stall enable (§5.5)
        self.s2_stall.store(cfg.s2_stall, Ordering::Release);

        // BUG-QA-13: STE.S2R — stage-2 record (§5.5)
        self.s2_record.store(cfg.s2_record, Ordering::Release);
    }

    /// Returns whether STE.S2S (Stage-2 Stall) is enabled (BUG-QA-12, §5.5).
    ///
    /// When `true`, stage-2 translation faults cause stall semantics independently
    /// of `FaultMode` / CD.S.
    #[inline]
    #[must_use]
    pub fn get_s2_stall(&self) -> bool {
        self.s2_stall.load(Ordering::Acquire)
    }

    /// Sets STE.S2S (Stage-2 Stall) for this stream (BUG-QA-12, §5.5).
    #[inline]
    pub fn set_s2_stall(&self, val: bool) {
        self.s2_stall.store(val, Ordering::Release);
    }

    /// Returns whether STE.S2R (Stage-2 Record) is enabled (BUG-QA-13, §5.5).
    ///
    /// When `false` and `s2_stall=false`, stage-2 fault events are suppressed
    /// from the event queue.
    #[inline]
    #[must_use]
    pub fn get_s2_record(&self) -> bool {
        self.s2_record.load(Ordering::Acquire)
    }

    /// Sets STE.S2R (Stage-2 Record) for this stream (BUG-QA-13, §5.5).
    #[inline]
    pub fn set_s2_record(&self, val: bool) {
        self.s2_record.store(val, Ordering::Release);
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

    /// Returns the MEV (Merged Event) flag for this stream (CONF-GAP-14, §5.2).
    ///
    /// When `true`, the SMMU will suppress duplicate fault events (same type +
    /// stream_id) for this stream per ARM §5.2 STE.MEV semantics.
    #[inline]
    #[must_use]
    pub fn mev(&self) -> bool {
        self.mev.load(Ordering::Relaxed)
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

    // ---- NEW-3: S2T0SZ / S2PS accessors (§3.4/§5.2) ----

    /// Returns the STE.S2T0SZ Stage-2 IPA input address range (NEW-3, ARM §5.2).
    ///
    /// When > 0, any IPA at or above `2^(64-S2T0SZ)` must generate F_TRANSLATION.
    #[inline]
    #[must_use]
    pub fn get_s2_t0sz(&self) -> u8 {
        self.s2_t0sz.load(Ordering::Acquire)
    }

    /// Sets the STE.S2T0SZ Stage-2 IPA input address range (ARM §5.2).
    #[inline]
    pub fn set_s2_t0sz(&self, value: u8) {
        self.s2_t0sz.store(value, Ordering::Release);
    }

    /// Returns the STE.S2PS Stage-2 output PA size encoding (NEW-3/NEW-8, ARM §5.2).
    ///
    /// Encoding: 0=32-bit, 1=36-bit, 2=40-bit, 3=42-bit, 4=44-bit, 5=48-bit, 6=52-bit.
    #[inline]
    #[must_use]
    pub fn get_s2_ps(&self) -> u8 {
        self.s2_ps.load(Ordering::Acquire)
    }

    /// Sets the STE.S2PS Stage-2 output PA size encoding (ARM §5.2).
    #[inline]
    pub fn set_s2_ps(&self, value: u8) {
        self.s2_ps.store(value, Ordering::Release);
    }

    // ---- NEW-7: EPD0 accessor (§5.4) ----

    /// Returns the CD.EPD0 flag (NEW-7, ARM §5.4).
    ///
    /// When `true`, TTBR0 stage-1 translation table walks are disabled and
    /// generate F_TRANSLATION.
    #[inline]
    #[must_use]
    pub fn get_epd0(&self) -> bool {
        self.epd0.load(Ordering::Acquire)
    }

    /// Sets the CD.EPD0 flag (ARM §5.4).
    #[inline]
    pub fn set_epd0(&self, value: bool) {
        self.epd0.store(value, Ordering::Release);
    }

    // ---- GAP-E: CD.TBI accessors (§3.4.1/§5.4) ----

    /// Returns the CD.TBI flag (GAP-E, ARM §3.4.1/§5.4).
    ///
    /// When `true`, VA bits[63:56] are masked to zero before the T0SZ VA range
    /// check.  The translation itself uses the original unmasked IOVA.
    #[inline]
    #[must_use]
    pub fn get_tbi(&self) -> bool {
        self.tbi.load(Ordering::Acquire)
    }

    /// Sets the CD.TBI flag (ARM §3.4.1/§5.4).
    #[inline]
    pub fn set_tbi(&self, value: bool) {
        self.tbi.store(value, Ordering::Release);
    }

    // ---- GAP-F: CD.IPS accessors (§5.4/§3.4) ----

    /// Returns the CD.IPS stage-1 output IPA size encoding (GAP-F, ARM §5.4).
    ///
    /// Encoding: 0=32-bit, 1=36-bit, 2=40-bit, 3=42-bit, 4=44-bit,
    ///           5=48-bit (default), 6=52-bit.
    #[inline]
    #[must_use]
    pub fn get_ips(&self) -> u8 {
        self.ips.load(Ordering::Acquire)
    }

    /// Sets the CD.IPS stage-1 output IPA size encoding (ARM §5.4).
    #[inline]
    pub fn set_ips(&self, value: u8) {
        self.ips.store(value, Ordering::Release);
    }

    /// NEW-12: Get STE.EATS — ATS support level (§5.2, §3.9).
    ///
    /// Returns `0` when ATS is not supported for this stream.
    #[inline]
    #[must_use]
    pub fn get_eats(&self) -> u8 {
        self.eats.load(Ordering::Acquire)
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
        // BUG-RUST-8 fix: also clear stage2_address_space to prevent stale
        // stage-2 mappings from surviving a full reconfiguration cycle.
        // Per ARM §3.4: the stage-2 address space is logically associated with
        // the stream, not with individual PASIDs; however, when all PASIDs are
        // cleared in preparation for a complete reconfigure, any previously set
        // stage-2 AS must also be reset so that a subsequent two-stage
        // translation does not inadvertently use old mappings.
        if let Ok(mut s2) = self.stage2_address_space.write() {
            *s2 = None;
        }
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

    /// Maps a page in "unaccessed" state (AF=false) for Access Flag Fault testing (NEW-GAP-J §3.13.2).
    pub fn map_page_unaccessed(
        &self,
        pasid: PASID,
        iova: IOVA,
        pa: PA,
        permissions: PagePermissions,
        security_state: SecurityState,
    ) -> Result<(), AddressSpaceError> {
        let pasid_value = pasid.as_u32();
        let addr_space = self.pasid_map.get(&pasid_value).ok_or(AddressSpaceError::InternalError)?;
        addr_space.map_page_unaccessed(iova, pa, permissions, security_state)
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

    /// Maps a Stage-2 page with AF=false (unaccessed) for AF fault testing (BUG-RUST-2).
    ///
    /// Creates a stage-2 IPA→PA mapping with the Access Flag set to false.
    /// Used to verify that S2AFFD/S2HA govern stage-2 AF fault behaviour per ARM §3.13.2.
    pub fn map_stage2_page_unaccessed(
        &self,
        ipa: IOVA,
        pa: PA,
        permissions: PagePermissions,
        security_state: SecurityState,
    ) -> Result<(), AddressSpaceError> {
        let stage2_guard = self.stage2_address_space.read().unwrap();
        let stage2 = stage2_guard.as_ref().ok_or(AddressSpaceError::InternalError)?;
        stage2.map_page_unaccessed(ipa, pa, permissions, security_state)
    }

    /// Maps a device-memory page into the Stage-2 address space (NEW-GAP-L: §5.2 S2PTW).
    ///
    /// Used to test STE.S2PTW: when `s2ptw=true`, a translation through a device-memory
    /// stage-2 page causes F_PERMISSION.
    pub fn map_stage2_device_page(
        &self,
        ipa: IOVA,
        pa: PA,
        permissions: PagePermissions,
        security_state: SecurityState,
    ) -> Result<(), AddressSpaceError> {
        let stage2_guard = self.stage2_address_space.read().unwrap();
        let stage2 = stage2_guard.as_ref().ok_or(AddressSpaceError::InternalError)?;
        stage2.map_page_device(ipa, pa, permissions, security_state)
    }

    /// Read the Access Flag of a stage-2 page (for testing §3.13.2 HTTU behavior).
    ///
    /// Returns `Some(true)` if AF=1, `Some(false)` if AF=0, or `None` if the
    /// page does not exist in the stage-2 address space.
    #[must_use]
    pub fn get_stage2_page_access_flag(&self, ipa: IOVA) -> Option<bool> {
        let guard = self.stage2_address_space.read().unwrap();
        guard.as_ref()?.get_page_access_flag(ipa)
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
        self.translate_internal(pasid, iova, access_type, security_state, false)
    }

    /// Internal translation entry point that accepts an `is_atos` flag.
    ///
    /// When `is_atos=true` (ARM §13.1.4 ATOS), the INSTCFG and PRIVCFG overrides
    /// are skipped — InD and PnU are taken directly from the request, not the STE.
    fn translate_internal(
        &self,
        pasid: PASID,
        iova: IOVA,
        access_type: AccessType,
        security_state: SecurityState,
        is_atos: bool,
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

        // BUG-RUST-4 STRW fix / BUG-RUST-7 / BUG-AUDIT-79:
        // §5.2 STE.STRW=El2/El3 promotes unprivileged access types to their
        // privileged equivalents.  This must happen BEFORE INSTCFG so that the
        // promotion is reflected in the access type that INSTCFG sees.
        // BUG-RUST-7: WriteExecute → WriteExecutePrivileged must be included.
        // BUG-AUDIT-79 fix: the previous guard (`!stage2_enabled`) incorrectly
        // skipped STRW-based promotion for two-stage streams.  Spec §5.2 does
        // not restrict STRW promotion to single-stage configurations; remove the
        // stage-2 guard so that promotion applies in all translation modes.
        let access_type = if matches!(self.get_strw(), StreamWorld::El2 | StreamWorld::El3)
        {
            match access_type {
                AccessType::Read             => AccessType::ReadPrivileged,
                AccessType::Write            => AccessType::WritePrivileged,
                AccessType::Execute          => AccessType::ExecutePrivileged,
                AccessType::ReadWrite        => AccessType::ReadWritePrivileged,
                AccessType::ReadExecute      => AccessType::ReadExecutePrivileged,
                AccessType::WriteExecute     => AccessType::WriteExecutePrivileged,     // BUG-RUST-7
                AccessType::ReadWriteExecute => AccessType::ReadWriteExecutePrivileged,
                other => other, // already privileged or None
            }
        } else {
            access_type
        };

        // §13.1.4: For ATOS operations, INSTCFG and PRIVCFG are ignored —
        // InD and PnU are taken directly from ATOS_ADDR (Table 13.4).
        // Skip both blocks when is_atos=true.
        //
        // BUG-AUDIT-133 fix: §5.2 — correct INSTCFG encoding:
        //   inst_cfg==0b11 (3): Force-Instruction — Read/Write become Execute.
        //   inst_cfg==0b10 (2): Force-Data         — Execute accesses become Read.
        //   inst_cfg==0b01 (1): Reserved — behaves as 0b00 (pass through unchanged).
        //   inst_cfg==0b00 (0): Use Incoming — pass through unchanged.
        //
        // BUG-2 fix: §13.1.2 — INSTCFG applies to ALL reads, privileged or not.
        // INSTCFG=3: ReadPrivileged → ExecutePrivileged (not just Read → Execute).
        // INSTCFG=2: ExecutePrivileged → ReadPrivileged (not just Execute → Read).
        let effective_access = if is_atos {
            access_type
        } else {
            match self.inst_cfg.load(Ordering::Acquire) {
            3 => match access_type {
                AccessType::Read => AccessType::Execute,
                AccessType::ReadPrivileged => AccessType::ExecutePrivileged,
                // NOTE: ReadWrite, ReadExecute, WriteExecute, ReadWriteExecute are
                // intentionally left unchanged. ARM §5.2: "INSTCFG only affects reads;
                // writes are always considered Data." A compound access type cannot have
                // its read component independently forced to Execute without abandoning
                // its write semantics.
                _ => access_type,
            },
            2 => match access_type {
                AccessType::Execute               => AccessType::Read,
                AccessType::ExecutePrivileged     => AccessType::ReadPrivileged,
                // BUG-RUST-4 INSTCFG fix: §5.2 INSTCFG=2 (Force-Data) must also
                // strip the execute component from ReadExecute and ReadExecutePrivileged.
                AccessType::ReadExecute           => AccessType::Read,
                AccessType::ReadExecutePrivileged => AccessType::ReadPrivileged,
                _ => access_type,
            },
            // 0b01 (Reserved) and 0b00 (Use Incoming) both pass through unchanged.
            _ => access_type,
            }
        };

        // BUG-RUST-1 fix: §3.3.4/§13.5 — STE.PRIVCFG override applied to effective_access
        // before dispatching to translate_two_stage / translate_stage2_only.
        // The translate_and_get_stage2_ipa() path already applies this; translate() did not.
        // PRIVCFG=2 (Force Unprivileged): demote privileged access-type variants to unprivileged.
        // PRIVCFG=3 (Force Privileged): promote unprivileged access-type variants to privileged.
        // PRIVCFG=0/1 (Use Incoming): pass effective_access unchanged.
        let effective_access = if is_atos {
            effective_access
        } else {
            match self.priv_cfg.load(Ordering::Acquire) {
            2 => match effective_access {
                AccessType::ReadPrivileged             => AccessType::Read,
                AccessType::WritePrivileged            => AccessType::Write,
                AccessType::ExecutePrivileged          => AccessType::Execute,
                AccessType::ReadWritePrivileged        => AccessType::ReadWrite,
                // BUG-RUST-5 fix: demote compound-execute privileged variants.
                AccessType::ReadExecutePrivileged      => AccessType::ReadExecute,
                AccessType::WriteExecutePrivileged     => AccessType::WriteExecute,
                AccessType::ReadWriteExecutePrivileged => AccessType::ReadWriteExecute,
                other => other,
            },
            3 => match effective_access {
                AccessType::Read             => AccessType::ReadPrivileged,
                AccessType::Write            => AccessType::WritePrivileged,
                AccessType::Execute          => AccessType::ExecutePrivileged,
                AccessType::ReadWrite        => AccessType::ReadWritePrivileged,
                // BUG-RUST-5 fix: promote compound-execute access types.
                AccessType::ReadExecute      => AccessType::ReadExecutePrivileged,
                AccessType::WriteExecute     => AccessType::WriteExecutePrivileged,
                AccessType::ReadWriteExecute => AccessType::ReadWriteExecutePrivileged,
                other => other,
            },
            _ => effective_access,
            } // close match priv_cfg
        }; // close if is_atos / else

        // NEW-7 fix: §5.4 CD.EPD0=1 — TTBR0 translation table walk disabled → F_TRANSLATION.
        // SW model uses a single address space per PASID (TTBR0-equivalent).
        // EPD1 (TTBR1 upper half) is not modelled separately.
        if stage1_enabled && self.epd0.load(Ordering::Acquire) {
            return Err(TranslationError::PageNotMapped);
        }

        match (stage1_enabled, stage2_enabled) {
            // Stage-1 only: IOVA → PA
            (true, false) => self.translate_stage1_only(pasid, iova, effective_access, security_state),

            // Stage-2 only: IPA → PA (treat IOVA as IPA)
            (false, true) => self.translate_stage2_only(pasid, iova, effective_access, security_state),

            // Two-stage: IOVA → IPA → PA
            (true, true) => self.translate_two_stage(pasid, iova, effective_access, security_state),

            // Bypass mode: IOVA = PA (identity mapping)
            (false, false) => self.translate_bypass(iova, security_state),
        }
    }

    /// Translate an address and also return the Stage-2 IPA if a two-stage fault occurred.
    ///
    /// This is the SMMU-internal entry point used by `smmu/mod.rs` to populate the
    /// `S2` and `IPA` fields of the event record per ARM IHI0070G.b §7.3.13.
    ///
    /// Returns `(result, stage2_ipa)` where:
    /// - `result` is the same value as [`translate()`] would return.
    /// - `stage2_ipa` is `Some(ipa_addr)` when a two-stage translation was attempted
    ///   and the fault occurred at Stage-2 (i.e. Stage-1 succeeded and produced an IPA,
    ///   but Stage-2 failed to map that IPA to a PA).  It is `None` for all other
    ///   translation modes or when Stage-1 itself faulted.
    ///
    /// # Note
    ///
    /// The `ipa_addr` is the raw u64 value of the stage-1 output physical address
    /// (the IPA fed into Stage-2), NOT a validated `IOVA` or `PA` — it may exceed
    /// hardware limits.  The caller is responsible for propagating it directly into
    /// the event record without further arithmetic.
    pub(crate) fn translate_and_get_stage2_ipa(
        &self,
        pasid: PASID,
        iova: IOVA,
        access_type: AccessType,
        security_state: SecurityState,
        is_atos: bool,
    ) -> (TranslationResult, Option<u64>) {
        // GAP NEW-2: §7.3.13 — S2 and IPA fields.
        // Determine whether this is a two-stage stream so we can detect stage-2 faults.
        let stage1_enabled = self.stage1_enabled.load(Ordering::Acquire);
        let stage2_enabled = self.stage2_enabled.load(Ordering::Acquire);

        if stage1_enabled && stage2_enabled {
            // BUG-NEW-17 fix: §3.9 / §5.2 STE.S1DSS — apply S1DSS routing BEFORE
            // entering the two-stage translation path.  The translate() function checks
            // S1DSS at line ~1834, but translate_and_get_stage2_ipa() previously called
            // translate_two_stage_with_ipa() directly, bypassing that check entirely.
            //
            // For a substream-capable stream (s1cd_max > 0) with PASID==0:
            //   S1DSS=0: abort with F_STREAM_DISABLED (return StreamDisabled here so
            //            smmu/mod.rs records the event on the normal S1DSS path).
            //   S1DSS=1: bypass stage-1 (delegate to translate_bypass), returning
            //            the IPA as None (stage-1 not attempted, so no stage-2 fault
            //            IPA to report from this path; the smmu-level S1DSS=1 branch
            //            in mod.rs will do the stage-2 call instead).
            //   S1DSS=2: use CD[0], fall through to normal two-stage path.
            let s1cd_max = self.s1cd_max.load(Ordering::Acquire);
            if s1cd_max > 0 && pasid.as_u32() == 0 {
                let s1dss = self.s1dss.load(Ordering::Acquire);
                match s1dss {
                    0 => return (Err(TranslationError::StreamDisabled), None),
                    1 => {
                        // Stage-1 bypassed; IOVA is forwarded as IPA to stage-2.
                        // Return a sentinel that tells smmu/mod.rs to apply S1DSS=1 routing.
                        // We return StreamDisabled here so the outer S1DSS block in
                        // smmu/mod.rs (which already handles S1DSS=1 correctly via
                        // translate_stage2_only) takes over.  The None IPA signals that
                        // this is not a "stage-2 fault after stage-1 succeeded" case.
                        return (Err(TranslationError::StreamDisabled), None);
                    }
                    _ => {} // S1DSS=2: use CD[0], fall through
                }
            }

            // NEW-04 fix: §5.4 CD.EPD0=1 — TTBR0 translation table walk disabled.
            // The single-stage translate() path already checks EPD0, but this two-stage
            // path calls translate_two_stage_with_ipa() directly, bypassing that check.
            // Guard here mirrors the check in translate() above.
            if self.epd0.load(Ordering::Acquire) {
                return (Err(TranslationError::PageNotMapped), None);
            }

            // Two-stage path: delegate to the specialised helper that returns the IPA.
            // §13.1.4: For ATOS operations, INSTCFG and PRIVCFG are ignored.
            // BUG-AUDIT-133 fix: §5.2 — correct INSTCFG encoding applied before
            // permission checks inside translate_two_stage_with_ipa.
            //
            // BUG-2 fix: §13.1.2 — INSTCFG applies to ALL reads, privileged or not.
            // INSTCFG=3 (Force Instruction): ReadPrivileged → ExecutePrivileged.
            // INSTCFG=2 (Force Data): ExecutePrivileged → ReadPrivileged.
            // INSTCFG=1 (Reserved): behaves as 0b00 (passthrough).
            let effective_access = if is_atos {
                access_type
            } else {
                match self.inst_cfg.load(Ordering::Acquire) {
                3 => match access_type {
                    AccessType::Read => AccessType::Execute,
                    AccessType::ReadPrivileged => AccessType::ExecutePrivileged,
                    _ => access_type,
                },
                2 => match access_type {
                    AccessType::Execute               => AccessType::Read,
                    AccessType::ExecutePrivileged     => AccessType::ReadPrivileged,
                    // BUG-RUST-4 INSTCFG fix: also strip execute from compound types.
                    AccessType::ReadExecute           => AccessType::Read,
                    AccessType::ReadExecutePrivileged => AccessType::ReadPrivileged,
                    _ => access_type,
                },
                // 0b01 (Reserved) and 0b00 (Use Incoming) both pass through unchanged.
                _ => access_type,
                }
            };
            // BUG-3 fix: §3.3.4/§13.5 — apply STE.PRIVCFG override to effective_access
            // so that the access type flowing into translate_two_stage_with_ipa reflects
            // the PRIVCFG promotion/demotion, consistent with the translate() path.
            // PRIVCFG=2 (Force Unprivileged): demote privileged variants.
            // PRIVCFG=3 (Force Privileged): promote unprivileged variants.
            let effective_access = if is_atos {
                effective_access
            } else {
                match self.priv_cfg.load(Ordering::Acquire) {
                2 => match effective_access {
                    AccessType::ReadPrivileged             => AccessType::Read,
                    AccessType::WritePrivileged            => AccessType::Write,
                    AccessType::ExecutePrivileged          => AccessType::Execute,
                    AccessType::ReadWritePrivileged        => AccessType::ReadWrite,
                    // BUG-RUST-5 fix: demote compound-execute privileged variants.
                    AccessType::ReadExecutePrivileged      => AccessType::ReadExecute,
                    AccessType::WriteExecutePrivileged     => AccessType::WriteExecute,
                    AccessType::ReadWriteExecutePrivileged => AccessType::ReadWriteExecute,
                    other => other,
                },
                3 => match effective_access {
                    AccessType::Read             => AccessType::ReadPrivileged,
                    AccessType::Write            => AccessType::WritePrivileged,
                    AccessType::Execute          => AccessType::ExecutePrivileged,
                    AccessType::ReadWrite        => AccessType::ReadWritePrivileged,
                    // BUG-RUST-5 fix: promote compound-execute access types.
                    AccessType::ReadExecute      => AccessType::ReadExecutePrivileged,
                    AccessType::WriteExecute     => AccessType::WriteExecutePrivileged,
                    AccessType::ReadWriteExecute => AccessType::ReadWriteExecutePrivileged,
                    other => other,
                },
                _ => effective_access,
                }
            };
            let (result, stage2_ipa) =
                self.translate_two_stage_with_ipa(pasid, iova, effective_access, security_state);
            (result, stage2_ipa)
        } else if !stage1_enabled && stage2_enabled {
            // BUG-1 fix: §7.3.13 — Stage-2-only: the IOVA is the IPA.
            // Any fault on a stage-2-only stream is a stage-2 fault; return Some(iova)
            // so the SMMU caller emits s2=true and ipa=<IOVA> in the EventEntry.
            let result = self.translate_internal(pasid, iova, access_type, security_state, is_atos);
            let ipa_opt = if result.is_err() { Some(iova.as_u64()) } else { None };
            (result, ipa_opt)
        } else {
            // Stage-1-only or bypass: delegate to translate_internal() with is_atos flag
            // so INSTCFG/PRIVCFG overrides are correctly skipped for ATOS.
            (self.translate_internal(pasid, iova, access_type, security_state, is_atos), None)
        }
    }

    /// Two-stage translation that also returns the IPA on stage-2 failure.
    ///
    /// Mirrors `translate_two_stage` but additionally returns `Some(ipa)` when
    /// Stage-2 translation fails, so the SMMU can populate the IPA field in the
    /// event record per ARM §7.3.13.
    #[allow(clippy::too_many_lines)]
    fn translate_two_stage_with_ipa(
        &self,
        pasid: PASID,
        iova: IOVA,
        access_type: AccessType,
        security_state: SecurityState,
    ) -> (TranslationResult, Option<u64>) {
        let pasid_value = pasid.as_u32();

        // Abort-mode / disabled checks are already done in the public translate().
        // Here we re-check only what is needed to get the IPA.
        let addr_space = match self.pasid_map.get(&pasid_value) {
            Some(a) => a,
            None => {
                let err = if self.is_disabling() || !self.is_enabled() {
                    TranslationError::StreamDisabled
                } else {
                    TranslationError::PASIDNotFound
                };
                return (Err(err), None);
            }
        };

        // NEW-GAP-J: §3.13.2 — Access Flag fault check (two-stage path).
        {
            let ha = self.ha.load(Ordering::Acquire);
            let affd = self.affd.load(Ordering::Acquire);
            if !ha && !affd && addr_space.get_page_access_flag(iova) == Some(false) {
                return (Err(TranslationError::AccessFlagFault), None);
            }
        }

        // Stage-1: IOVA → IPA
        let stage1_result = match addr_space.translate_page(iova, access_type, security_state) {
            Ok(data) => {
                let ha = self.ha.load(Ordering::Acquire);
                let hd = self.hd.load(Ordering::Acquire);
                if ha || hd {
                    addr_space.update_access_flags(iova, ha, hd, access_type);
                }
                data
            }
            Err(e) => return (Err(e), None), // Stage-1 fault — no IPA
        };

        // IPA = stage-1 output PA address
        let ipa_raw = stage1_result.physical_address().as_u64();
        let ipa = match IOVA::new(ipa_raw) {
            Ok(i) => i,
            Err(_) => return (Err(TranslationError::AddressSizeError), None),
        };

        // NEW-GAP-K: §5.4 — WXN/UWXN permission check on stage-1 result (two-stage path).
        // Bug-4 fix: use can_execute() so ExecutePrivileged is correctly detected.
        {
            if access_type.can_execute() {
                let perms = stage1_result.permissions();
                // WXN: any writable page is execute-never (applies to all privilege levels).
                if self.wxn.load(Ordering::Acquire) && perms.write() {
                    return (Err(TranslationError::WxnFault), None);
                }
                // UWXN (§5.4): privileged execute on a page writable by unprivileged code → F_PERMISSION.
                // Bug-3 fix: UWXN is IGNORED when StreamWorld=EL2/EL3 (all-privileged).
                // Per §5.4: "In configurations for which all accesses are considered privileged,
                // for example StreamWorld == any-EL2 or StreamWorld == EL3, this field has no effect
                // and is IGNORED."  The previous condition (effective_priv_suppresses_check) was
                // inverted — it fired UWXN for EL2/EL3 streams when it should be ignored.
                // Correct condition: UWXN=1 AND stream is NOT all-privileged
                //                    AND the access is effectively privileged (PRIVCFG or STRW)
                //                    AND the page is writable AND NOT privileged-only.
                if self.uwxn.load(Ordering::Acquire)
                    && !self.stream_is_all_privileged()
                    && self.is_access_effectively_privileged(access_type)
                    && perms.write()
                    && !perms.privileged_only()
                {
                    return (Err(TranslationError::WxnFault), None);
                }
            }
        }

        // GAP-F fix: §5.4/§3.4 — CD.IPS: stage-1 output IPA must be within IPS range → F_ADDR_SIZE.
        // The IPS field bounds the stage-1 output address (IPA).  Violation maps to
        // AddressSizeError which the SMMU caller records as F_ADDR_SIZE.
        {
            let ips = self.ips.load(Ordering::Acquire);
            let ipa_bits = Self::s2ps_to_bits(ips); // reuse existing S2PS helper (same encoding)
            if ipa_bits < 52 {
                let ipa_limit: u64 = 1u64 << ipa_bits;
                if ipa_raw >= ipa_limit {
                    return (Err(TranslationError::AddressSizeError), None);
                }
            }
        }

        // NEW-3 fix: §3.4 — S2T0SZ IPA input range check → F_TRANSLATION.
        // When S2T0SZ > 0, any IPA at or above 2^(64-S2T0SZ) is out of range.
        // PageNotMapped → FaultType::TranslationFault; the SMMU caller records the event.
        // NEW-03 fix: §7.3.13 — return Some(ipa_raw) so the SMMU caller emits the event
        // with s2=true and the actual IPA value (not s2=false, ipa=0).
        {
            let s2_t0sz = self.s2_t0sz.load(Ordering::Acquire).min(48);
            if s2_t0sz > 0 {
                let ipa_limit: u64 = 1u64 << (64u32 - u32::from(s2_t0sz));
                if ipa_raw >= ipa_limit {
                    return (Err(TranslationError::PageNotMapped), Some(ipa_raw));
                }
            }
        }

        // BUG-RUST-2 fix: §3.13.2 — Stage-2 Access Flag fault check (two-stage-with-ipa path).
        // Uses STE.S2HA and STE.S2AFFD (stage-2 specific fields), NOT stage-1 HA/AFFD.
        // This check takes priority over permission faults per ARM §3.13.2.
        {
            let s2ha = self.s2ha.load(Ordering::Acquire);
            let s2affd = self.s2affd.load(Ordering::Acquire);
            if !s2ha && !s2affd {
                let stage2_guard = self.stage2_address_space.read().unwrap();
                if let Some(stage2) = stage2_guard.as_ref() {
                    if stage2.get_page_access_flag(ipa) == Some(false) {
                        return (Err(TranslationError::AccessFlagFault), Some(ipa_raw));
                    }
                }
            }
        }

        // Stage-2: IPA → PA.
        let stage2_result = {
            let stage2_guard = self.stage2_address_space.read().unwrap();
            let stage2 = match stage2_guard.as_ref() {
                Some(s) => s,
                None => return (Err(TranslationError::StreamNotConfigured), Some(ipa_raw)),
            };
            // BUG-AUDIT-90 fix: use the actual access_type for the data translation.
            // Stage-2 IPA→PA is the real data access — permission checks must reflect the
            // transaction's actual access type so a Write to a read-only page generates a
            // write fault (rnw=false), not a spurious read fault.
            //
            // BUG-2 fix: ARM IHI0070G.b §3.10.2.2 — "When stage 1 translation is
            // performed, the NS attribute provided to stage 2 comes from stage 1
            // translation tables."  The NS bit for the Stage-2 lookup must come from
            // the Stage-1 output (stage1_result.security_state()), not the incoming
            // transaction's security_state parameter.
            stage2.translate_page(ipa, access_type, stage1_result.security_state())
        };

        // Stage-2 failed → return the IPA so the SMMU can populate the event record.
        let s2_data = match stage2_result {
            Ok(d) => {
                // BUG-AUDIT-128 fix: §3.13.2 — when S2HA (or S2HD) is enabled, atomically
                // set AF=1 in the stage-2 descriptor after a successful translation.
                // Mirrors the stage-1 update_access_flags call above (line ~2143).
                // §3.13.2: hardware AF update for stage-2 when S2HA or S2HD enabled.
                let do_s2_af_update = self.s2ha.load(Ordering::Acquire);
                let do_s2_dirty_update = self.s2hd.load(Ordering::Acquire);
                if do_s2_af_update || do_s2_dirty_update {
                    let stage2_guard = self.stage2_address_space.read().unwrap();
                    if let Some(stage2) = stage2_guard.as_ref() {
                        stage2.update_access_flags(ipa, do_s2_af_update, do_s2_dirty_update, access_type);
                    }
                }
                d
            }
            Err(e) => return (Err(e), Some(ipa_raw)),
        };

        // NEW-8 fix: §3.4/§7.3.14 — stage-2 output PA must be within S2PS range → F_ADDR_SIZE.
        // Return the IPA in the error pair so the event record is properly populated.
        {
            let s2_ps = self.s2_ps.load(Ordering::Acquire);
            let pa_bits = Self::s2ps_to_bits(s2_ps);
            if pa_bits < 52 {
                let pa_limit: u64 = 1u64 << pa_bits;
                if s2_data.physical_address().as_u64() >= pa_limit {
                    return (Err(TranslationError::AddressSizeError), Some(ipa_raw));
                }
            }
        }

        // NEW-GAP-L: §5.2 S2PTW — protected table walk (two-stage-with-ipa path).
        if self.s2ptw.load(Ordering::Acquire) {
            let stage2_guard = self.stage2_address_space.read().unwrap();
            if let Some(stage2) = stage2_guard.as_ref() {
                if stage2.get_page_device_memory(ipa) {
                    return (Err(TranslationError::S2PtwFault), Some(ipa_raw));
                }
            }
        }

        // Permission intersection (same as translate_two_stage)
        let final_perms = stage1_result.permissions().intersection(s2_data.permissions());
        // Bug-4 fix: use can_read/write/execute() to handle privileged variants correctly.
        let access_denied = (access_type.can_read() && !final_perms.read())
            || (access_type.can_write() && !final_perms.write())
            || (access_type.can_execute() && !final_perms.execute());
        if access_denied {
            // Permission fault occurred AFTER stage-2 succeeded — IPA is the stage-2 input.
            return (Err(TranslationError::PermissionViolation { access: access_type }), Some(ipa_raw));
        }
        // NEW-4: §3.3.4/§13.5 — STE.PRIVCFG overrides STRW for privileged_only check.
        if final_perms.privileged_only() && !self.effective_priv_suppresses_check() {
            return (Err(TranslationError::PermissionViolation { access: access_type }), Some(ipa_raw));
        }

        // BUG-AUDIT-49 fix: carry page_attr from the stage-2 result so that
        // gatos_translate() can return the correct ATTR/SH for device-memory pages.
        // TranslationData::new() defaults page_attr=0xFF; transfer the stage-2 value.
        let mut result_data = TranslationData::new(s2_data.physical_address(), final_perms, s2_data.security_state());
        result_data.page_attr = s2_data.page_attr;
        // Return Some(ipa_raw) even on success so the SMMU can tag the TLB entry
        // with the correct IPA for CMD_TLBI_S2_IPA selective invalidation (§4.4.3.1 / §3.17).
        (Ok(self.apply_output_attrs(result_data)), Some(ipa_raw))
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

        // NEW-GAP-J: §3.13.2 — Access Flag fault check.
        // If AF=0 and ha=false (no HTTU) and affd=false (AF fault not disabled) → F_ACCESS.
        // This takes priority over permission faults per the spec.
        {
            let ha = self.ha.load(Ordering::Acquire);
            let affd = self.affd.load(Ordering::Acquire);
            if !ha && !affd && addr_space.get_page_access_flag(iova) == Some(false) {
                return Err(TranslationError::AccessFlagFault);
            }
        }

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

        // NEW-GAP-K: §5.4 — WXN permission modifier check.
        // WXN=1: any writable page is execute-never.
        // UWXN (§5.4): privileged execute on a page writable by unprivileged code → F_PERMISSION.
        // Bug-4 fix: use can_execute() so ExecutePrivileged is correctly detected.
        {
            if access_type.can_execute() {
                let perms = data.permissions();
                // WXN: applies to all privilege levels.
                if self.wxn.load(Ordering::Acquire) && perms.write() {
                    return Err(TranslationError::WxnFault);
                }
                // UWXN: privileged execute on an unprivileged-writable page.
                // Bug-3 fix: UWXN is IGNORED when StreamWorld=EL2/EL3 (all-privileged).
                // See translate_two_stage_with_ipa for the full rationale.
                if self.uwxn.load(Ordering::Acquire)
                    && !self.stream_is_all_privileged()
                    && self.is_access_effectively_privileged(access_type)
                    && perms.write()
                    && !perms.privileged_only()
                {
                    return Err(TranslationError::WxnFault);
                }
            }
        }

        // §5.2 GAP-2 / NEW-4: Check privileged_only against STRW/PRIVCFG suppression.
        // PRIVCFG=3 (Force Privileged) overrides STRW to always suppress.
        // PRIVCFG=2 (Force Unprivileged) overrides STRW to never suppress.
        if data.permissions().privileged_only() && !self.effective_priv_suppresses_check() {
            return Err(TranslationError::PermissionViolation { access: access_type });
        }

        // §5.2 GAP-1: Apply STE output-attribute overrides.
        Ok(self.apply_output_attrs(data))
    }

    /// Stage-2 only translation: IPA → PA
    ///
    /// Exposed as `pub(crate)` so that the SMMU-level S1DSS=0b01 handler can
    /// forward the IOVA as an IPA through stage-2 when both stage-1 and stage-2
    /// are enabled (ARM §3.9 — S1DSS=0b01 passes input address directly to
    /// stage-2 as the IPA, BUG-RUST-3 fix).
    pub(crate) fn translate_stage2_only(
        &self,
        _pasid: PASID,
        ipa: IOVA,
        access_type: AccessType,
        security_state: SecurityState,
    ) -> TranslationResult {
        // NEW-3 fix: §3.4 — S2T0SZ IPA input range check → F_TRANSLATION.
        // For stage-2-only streams, the incoming IOVA is the IPA.
        // PageNotMapped → FaultType::TranslationFault; the SMMU caller records the event.
        {
            let s2_t0sz = self.s2_t0sz.load(Ordering::Acquire).min(48);
            if s2_t0sz > 0 {
                let ipa_limit: u64 = 1u64 << (64u32 - u32::from(s2_t0sz));
                if ipa.as_u64() >= ipa_limit {
                    return Err(TranslationError::PageNotMapped);
                }
            }
        }

        // BUG-RUST-2 fix: §3.13.2 — Stage-2 Access Flag fault check (stage-2-only path).
        // Uses STE.S2HA and STE.S2AFFD, NOT the stage-1 HA/AFFD fields.
        // Per ARM §3.13.2 and §5.2:
        //   S2HA=1: SMMU sets AF=1 in stage-2 entries (HTTU); no F_ACCESS fault.
        //   S2HA=0 and S2AFFD=1: stage-2 AF faults suppressed; always treat AF as 1.
        //   S2HA=0 and S2AFFD=0: stage-2 AF=0 → F_ACCESS fault.
        // This check must happen before the translate_page call (like stage-1) to take
        // priority over permission faults per the spec.
        {
            let s2ha = self.s2ha.load(Ordering::Acquire);
            let s2affd = self.s2affd.load(Ordering::Acquire);
            if !s2ha && !s2affd {
                let stage2_guard = self.stage2_address_space.read().unwrap();
                if let Some(stage2) = stage2_guard.as_ref() {
                    if stage2.get_page_access_flag(ipa) == Some(false) {
                        return Err(TranslationError::AccessFlagFault);
                    }
                }
            }
        }

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

        // NEW-8 fix: §3.4/§7.3.14 — stage-2 output PA must be within S2PS range → F_ADDR_SIZE.
        // SMMU caller maps AddressSizeError → FaultType::AddressSizeFault and records event.
        {
            let s2_ps = self.s2_ps.load(Ordering::Acquire);
            let pa_bits = Self::s2ps_to_bits(s2_ps);
            if pa_bits < 52 {
                let pa_limit: u64 = 1u64 << pa_bits;
                if data.physical_address().as_u64() >= pa_limit {
                    return Err(TranslationError::AddressSizeError);
                }
            }
        }

        // §5.2 GAP-2 / NEW-4: Check privileged_only against STRW/PRIVCFG suppression.
        // §3.12.2 fix: no record_fault_internal — SMMU caller records the fault.
        if data.permissions().privileged_only() && !self.effective_priv_suppresses_check() {
            return Err(TranslationError::PermissionViolation { access: access_type });
        }

        // §5.2 GAP-1: Apply STE output-attribute overrides.
        Ok(self.apply_output_attrs(data))
    }

    /// Two-stage translation: IOVA → IPA → PA
    #[allow(clippy::too_many_lines)]
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

        // NEW-GAP-J: §3.13.2 — Access Flag fault check (stage-1 path).
        {
            let ha = self.ha.load(Ordering::Acquire);
            let affd = self.affd.load(Ordering::Acquire);
            if !ha && !affd && addr_space.get_page_access_flag(iova) == Some(false) {
                return Err(TranslationError::AccessFlagFault);
            }
        }

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

        // NEW-GAP-K: §5.4 — WXN/UWXN permission modifier check on stage-1 result.
        // Bug-4 fix: use can_execute() so ExecutePrivileged is correctly detected.
        {
            if access_type.can_execute() {
                let perms = stage1_result.permissions();
                // WXN: applies to all privilege levels.
                if self.wxn.load(Ordering::Acquire) && perms.write() {
                    return Err(TranslationError::WxnFault);
                }
                // UWXN (§5.4): privileged execute on an unprivileged-writable page.
                // Bug-3 fix: UWXN is IGNORED when StreamWorld=EL2/EL3 (all-privileged).
                // See translate_two_stage_with_ipa for the full rationale.
                if self.uwxn.load(Ordering::Acquire)
                    && !self.stream_is_all_privileged()
                    && self.is_access_effectively_privileged(access_type)
                    && perms.write()
                    && !perms.privileged_only()
                {
                    return Err(TranslationError::WxnFault);
                }
            }
        }

        // IPA is the physical address from Stage-1
        let ipa_raw = stage1_result.physical_address().as_u64();
        let ipa =
            IOVA::new(ipa_raw).map_err(|_| TranslationError::AddressSizeError)?;

        // BUG-RUST-6 fix: §5.4/§3.4 — CD.IPS: stage-1 output IPA must be within IPS
        // range → F_ADDR_SIZE.  This check is present in translate_two_stage_with_ipa
        // but was missing from translate_two_stage, creating a gap where the slow
        // (non-GATOS) path silently accepted out-of-range IPAs.
        {
            let ips = self.ips.load(Ordering::Acquire);
            let ipa_bits = Self::s2ps_to_bits(ips); // reuse existing S2PS helper (same encoding)
            if ipa_bits < 52 {
                let ipa_limit: u64 = 1u64 << ipa_bits;
                if ipa_raw >= ipa_limit {
                    return Err(TranslationError::AddressSizeError);
                }
            }
        }

        // NEW-3 fix: §3.4 — S2T0SZ IPA input range check → F_TRANSLATION.
        // When S2T0SZ > 0, any IPA at or above 2^(64-S2T0SZ) is out of range.
        // PageNotMapped → FaultType::TranslationFault; the SMMU caller records the event.
        {
            let s2_t0sz = self.s2_t0sz.load(Ordering::Acquire).min(48);
            if s2_t0sz > 0 {
                let ipa_limit: u64 = 1u64 << (64u32 - u32::from(s2_t0sz));
                if ipa_raw >= ipa_limit {
                    return Err(TranslationError::PageNotMapped);
                }
            }
        }

        // BUG-RUST-2 fix: §3.13.2 — Stage-2 Access Flag fault check (two-stage path).
        // Uses STE.S2HA and STE.S2AFFD (stage-2 specific fields), NOT stage-1 HA/AFFD.
        // This check takes priority over permission faults per ARM §3.13.2.
        {
            let s2ha = self.s2ha.load(Ordering::Acquire);
            let s2affd = self.s2affd.load(Ordering::Acquire);
            if !s2ha && !s2affd {
                let stage2_guard = self.stage2_address_space.read().unwrap();
                if let Some(stage2) = stage2_guard.as_ref() {
                    if stage2.get_page_access_flag(ipa) == Some(false) {
                        return Err(TranslationError::AccessFlagFault);
                    }
                }
            }
        }

        // Stage-2: IPA → PA.
        // BUG-07: Scope the stage2_address_space read-lock tightly so it is
        // released before any call to record_fault_internal (which acquires the
        // fault_records write-lock).  Holding stage2_address_space.read() across
        // record_fault_internal creates an ABBA deadlock risk if a concurrent
        // writer calls set_stage2_address_space() while holding fault_records.write().
        let stage2_result = {
            let stage2_guard = self.stage2_address_space.read().unwrap();
            let stage2 = stage2_guard.as_ref().ok_or(TranslationError::StreamNotConfigured)?;
            // BUG-AUDIT-90 fix: use the actual access_type for the Stage-2 data translation.
            // This is the IPA→PA lookup for the actual transaction, NOT a page-table walk
            // (PTW) fetch.  A PTW lookup would use AccessType::Read per §7.3.16, but this
            // code path performs the data access itself — permission checks must use the
            // real access type so that a Write to a read-only S2 page generates a write
            // fault (rnw=false) rather than a spurious read fault.
            //
            // BUG-2 fix: ARM IHI0070G.b §3.10.2.2 — "When stage 1 translation is
            // performed, the NS attribute provided to stage 2 comes from stage 1
            // translation tables."  The NS bit for the Stage-2 lookup must come from
            // the Stage-1 output (stage1_result.security_state()), not the incoming
            // transaction's security_state parameter.
            stage2.translate_page(ipa, access_type, stage1_result.security_state())
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

        // BUG-AUDIT-130 fix: §3.13.2 / §3.13.4 — when S2HA (or S2HD) is enabled,
        // atomically set AF=1 in the stage-2 descriptor after a successful translation.
        // Mirrors the fix in translate_two_stage_with_ipa (BUG-AUDIT-128).
        {
            let do_s2_af_update = self.s2ha.load(Ordering::Acquire);
            let do_s2_dirty_update = self.s2hd.load(Ordering::Acquire);
            if do_s2_af_update || do_s2_dirty_update {
                let stage2_guard = self.stage2_address_space.read().unwrap();
                if let Some(stage2) = stage2_guard.as_ref() {
                    stage2.update_access_flags(ipa, do_s2_af_update, do_s2_dirty_update, access_type);
                }
            }
        }

        // NEW-8 fix: §3.4/§7.3.14 — stage-2 output PA must be within S2PS range → F_ADDR_SIZE.
        {
            let s2_ps = self.s2_ps.load(Ordering::Acquire);
            let pa_bits = Self::s2ps_to_bits(s2_ps);
            if pa_bits < 52 {
                let pa_limit: u64 = 1u64 << pa_bits;
                if s2_data.physical_address().as_u64() >= pa_limit {
                    return Err(TranslationError::AddressSizeError);
                }
            }
        }

        // NEW-GAP-L: §5.2 S2PTW — protected table walk.
        // If STE.S2PTW=1 and the stage-2 page is device memory, deny the translation
        // (prevents TTW hitting device MMIO) → F_PERMISSION.
        if self.s2ptw.load(Ordering::Acquire) {
            let stage2_guard = self.stage2_address_space.read().unwrap();
            if let Some(stage2) = stage2_guard.as_ref() {
                if stage2.get_page_device_memory(ipa) {
                    return Err(TranslationError::S2PtwFault);
                }
            }
        }

        let final_perms = stage1_result.permissions().intersection(s2_data.permissions());

        // Bug-4 fix: use can_read/write/execute() to handle privileged variants correctly.
        let access_denied = (access_type.can_read() && !final_perms.read())
            || (access_type.can_write() && !final_perms.write())
            || (access_type.can_execute() && !final_perms.execute());
        // §3.12.2 fix: no record_fault_internal — SMMU caller records the fault.
        if access_denied {
            return Err(TranslationError::PermissionViolation { access: access_type });
        }

        // §5.2 GAP-2 / NEW-4: Check final effective permissions for privileged_only.
        // §3.12.2 fix: no record_fault_internal — SMMU caller records the fault.
        if final_perms.privileged_only() && !self.effective_priv_suppresses_check() {
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

    /// Returns `true` if the effective privilege level suppresses the `privileged_only`
    /// permission check, taking STE.PRIVCFG into account (NEW-4, ARM §3.3.4/§13.5).
    ///
    /// PRIVCFG=3 (Force Privileged) overrides STRW: treat the access as privileged
    ///   regardless of STRW — suppress the `privileged_only` check.
    /// PRIVCFG=2 (Force Unprivileged) overrides STRW: treat the access as unprivileged
    ///   regardless of STRW — never suppress the `privileged_only` check.
    /// PRIVCFG=0/1 (pass-through): fall back to STRW-based suppression.
    ///
    /// Made `pub(crate)` so the SMMU TLB fast path can apply the same logic.
    #[inline]
    pub(crate) fn effective_priv_suppresses_check(&self) -> bool {
        match self.priv_cfg.load(Ordering::Acquire) {
            3 => true,  // Force Privileged: suppress privileged_only check
            2 => false, // Force Unprivileged: never suppress privileged_only check
            _ => self.strw_suppresses_priv(),
        }
    }

    /// Returns `true` when the access is privileged for PnU wire-format purposes
    /// (ARM IHI0070G.b §7.3).
    ///
    /// Bug-4 fix: For PRIVCFG=Use Incoming (0b00/0b01), pnu must reflect the
    /// AxPROT\[1\] bit of the incoming transaction, represented by `access_type.is_privileged()`.
    ///
    /// - PRIVCFG=3 (Force Privileged): always privileged → `true`
    /// - PRIVCFG=2 (Force Unprivileged): always unprivileged → `false`
    /// - PRIVCFG=0/1 (Use Incoming): delegates to `is_access_effectively_privileged(access_type)`,
    ///   which uses the incoming `AccessType`'s `is_privileged()` flag.
    #[inline]
    pub(crate) fn is_access_privileged(&self, access_type: AccessType) -> bool {
        self.is_access_effectively_privileged(access_type)
    }

    // ---- Bug-3 fix: UWXN §5.4 — stream_is_all_privileged / is_access_effectively_privileged ----

    /// Returns `true` when StreamWorld is entirely privileged (EL2 or EL3), making UWXN
    /// irrelevant per ARM IHI0070G.b §5.4.
    ///
    /// ARM §5.4: "In configurations for which all accesses are considered privileged,
    /// for example StreamWorld == any-EL2 or StreamWorld == EL3, this field has no
    /// effect and is IGNORED."
    ///
    /// Note: `El2E2h` (VHE) retains EL1-like privilege separation (AP\[1\] is meaningful)
    /// and is NOT all-privileged per ARM §D5.1.3.  Only the pure EL2 and EL3 worlds
    /// are unconditionally all-privileged.
    /// `PRIVCFG=3` (Force Privileged) is NOT included here: it promotes individual
    /// accesses to privileged but does not make the entire StreamWorld all-privileged.
    #[inline]
    pub(crate) fn stream_is_all_privileged(&self) -> bool {
        // Bug-4 fix: El2E2h removed — VHE maintains EL1-like privilege separation,
        // so UWXN and privilege checks remain active for El2E2h streams.
        matches!(
            self.get_strw(),
            StreamWorld::El2 | StreamWorld::El3
        )
    }

    /// Returns `true` when the access should be treated as privileged for UWXN purposes.
    ///
    /// This combines stream-level (STRW), per-transaction (PRIVCFG), and incoming
    /// access type privilege (Bug-4 fix):
    ///
    /// - STRW ∈ {El2, El3}: all accesses are privileged → `true`
    /// - PRIVCFG=3 (Force Privileged): override to privileged → `true`
    /// - PRIVCFG=2 (Force Unprivileged): override to unprivileged → `false`
    /// - PRIVCFG=0/1 (Use Incoming): `access_type.is_privileged()` — reflects AxPROT\[1\]
    ///   of the incoming transaction (Bug-4 fix: ARM IHI0070G.b §7.3 EventEntry.PnU)
    ///
    /// Used to determine whether the UWXN check should fire, and (after Bug-4 fix)
    /// to derive the pnu field of the EventEntry for PRIVCFG=Use Incoming streams.
    #[inline]
    pub(crate) fn is_access_effectively_privileged(&self, access_type: AccessType) -> bool {
        match self.priv_cfg.load(Ordering::Acquire) {
            3 => true,  // Force Privileged — overrides incoming access type
            2 => false, // Force Unprivileged — overrides incoming access type
            _ => {
                // PRIVCFG=0/1 (Use Incoming): use the stream-level STRW if it's all-privileged;
                // otherwise fall back to the incoming transaction's privilege level.
                if self.stream_is_all_privileged() {
                    true
                } else {
                    // Bug-4 fix: use the AxPROT[1] bit from the incoming access type.
                    access_type.is_privileged()
                }
            }
        }
    }

    // ---- NEW-8 helper: S2PS encoding → PA bit width ----

    /// Converts a STE.S2PS 3-bit encoding to the number of output PA bits (NEW-8, ARM §5.2).
    ///
    /// Encoding: 0=32-bit, 1=36-bit, 2=40-bit, 3=42-bit, 4=44-bit, 5=48-bit, 6=52-bit.
    /// Reserved values default to 48 bits.
    #[inline]
    fn s2ps_to_bits(s2_ps: u8) -> u32 {
        match s2_ps {
            0 => 32,
            1 => 36,
            2 => 40,
            3 => 42,
            4 => 44,
            5 => 48,
            6 => 52,
            _ => 48,
        }
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
        let mt_cfg_enabled = self.mt_cfg.load(Ordering::Acquire);
        let mem_type = if mt_cfg_enabled {
            self.mem_attr.load(Ordering::Acquire)
        } else {
            0u8
        };
        // §13.1.4: "Device or Normal-iNC-oNC memory types always OSH regardless
        // of any SHCFG override."  Determine the effective attribute: when MTCFG
        // overrides the memory type use that byte; otherwise use the per-page attr
        // from the translation tables so the rule applies even without MTCFG.
        let effective_attr = if mt_cfg_enabled {
            mem_type
        } else {
            data.page_attr
        };
        // ARM MAIR encoding: Device types are 0x00 (nGnRnE), 0x04 (nGnRE), 0x08 (nGRE),
        // 0x0C (GRE). Normal-iNC-oNC encodes as 0x44. All must force OSH per §13.1.4.
        let is_device_or_nc = matches!(effective_attr, 0x00 | 0x04 | 0x08 | 0x0C | 0x44);
        let shareability = if is_device_or_nc {
            0b10_u8 // OSH — forced for Device and Normal-iNC-oNC
        } else {
            self.sh_cfg.load(Ordering::Acquire)
        };
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

    // ── BUG-RUST-5: PRIVCFG=3 must promote compound execute access types ──────

    /// BUG-RUST-5: effective_access_type(ReadExecute) with PRIVCFG=3 must return
    /// a privileged variant (is_privileged()==true), not leave it as ReadExecute.
    ///
    /// Per ARM §13.1/Table 13.4, PRIVCFG=Force Privileged applies to ALL
    /// transaction types.  Before the fix, ReadExecute / WriteExecute /
    /// ReadWriteExecute fall through to `other => other` in the PRIVCFG=3 arm.
    #[test]
    fn bug_rust5_privcfg3_promotes_read_execute_via_effective_access_type() {
        let ctx = StreamContext::new();
        ctx.set_priv_cfg(3); // Force Privileged
        let promoted = ctx.effective_access_type(AccessType::ReadExecute);
        assert!(
            promoted.is_privileged(),
            "BUG-RUST-5: effective_access_type(ReadExecute) with PRIVCFG=3 must return \
             a privileged variant (bit-3 set). Got {promoted:?}"
        );
    }

    /// BUG-RUST-5: WriteExecute must also be promoted.
    #[test]
    fn bug_rust5_privcfg3_promotes_write_execute_via_effective_access_type() {
        let ctx = StreamContext::new();
        ctx.set_priv_cfg(3);
        let promoted = ctx.effective_access_type(AccessType::WriteExecute);
        assert!(
            promoted.is_privileged(),
            "BUG-RUST-5: effective_access_type(WriteExecute) with PRIVCFG=3 must return \
             a privileged variant. Got {promoted:?}"
        );
    }

    /// BUG-RUST-5: ReadWriteExecute must also be promoted.
    #[test]
    fn bug_rust5_privcfg3_promotes_read_write_execute_via_effective_access_type() {
        let ctx = StreamContext::new();
        ctx.set_priv_cfg(3);
        let promoted = ctx.effective_access_type(AccessType::ReadWriteExecute);
        assert!(
            promoted.is_privileged(),
            "BUG-RUST-5: effective_access_type(ReadWriteExecute) with PRIVCFG=3 must return \
             a privileged variant. Got {promoted:?}"
        );
    }

    /// BUG-RUST-5: PRIVCFG=2 (Force Unprivileged) must demote already-privileged compound
    /// types. This ensures symmetry of the fix.
    #[test]
    fn bug_rust5_privcfg2_demotes_read_execute_privileged() {
        // ReadExecutePrivileged doesn't exist yet — but ReadPrivileged does and is demoted.
        // Test that existing variants still demote correctly (regression guard).
        let ctx = StreamContext::new();
        ctx.set_priv_cfg(2); // Force Unprivileged
        // ReadPrivileged → Read (already works)
        let demoted = ctx.effective_access_type(AccessType::ReadPrivileged);
        assert!(
            !demoted.is_privileged(),
            "PRIVCFG=2 must demote ReadPrivileged. Got {demoted:?}"
        );
        // ReadExecute stays as ReadExecute under PRIVCFG=2 (already unprivileged — correct)
        let unchanged = ctx.effective_access_type(AccessType::ReadExecute);
        assert!(
            !unchanged.is_privileged(),
            "PRIVCFG=2 on ReadExecute must stay unprivileged. Got {unchanged:?}"
        );
    }
}
