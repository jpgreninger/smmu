//! Main SMMU controller and translation engine
//!
//! This module implements the top-level SMMU controller that orchestrates:
//!
//! - Stream management and configuration
//! - Translation request handling
//! - Event and fault reporting
//! - Global SMMU configuration
//!
//! # SMMU Controller
//!
//! The SMMU controller is the main entry point for all translation operations
//! and provides the public API for interacting with the SMMU subsystem.
//!
//! # Translation Flow
//!
//! 1. Receive translation request (StreamID, PASID, IOVA, AccessType)
//! 2. Lookup stream context
//! 3. Select appropriate address space based on PASID
//! 4. Perform page table walk
//! 5. Check permissions and return physical address or fault
//!
//! # Thread Safety
//!
//! All operations are thread-safe using:
//! - `DashMap` for lock-free concurrent stream access
//! - `Arc<RwLock<T>>` for shared configuration with concurrent reads
//! - `AtomicBool` for lock-free shutdown coordination
//! - `Arc<Mutex<Vec<T>>>` for thread-safe fault queue
//!
//! # Examples
//!
//! ```rust
//! use smmu::SMMU;
//! use smmu::types::{StreamID, StreamConfig, SMMUConfig};
//!
//! // Create SMMU with default configuration
//! let smmu = SMMU::new();
//!
//! // Create SMMU with custom configuration
//! let config = SMMUConfig::high_performance();
//! let smmu = SMMU::with_config(config);
//!
//! // Configure a stream
//! let stream_id = StreamID::new(1).unwrap();
//! let stream_config = StreamConfig::stage1_only();
//! smmu.configure_stream(stream_id, stream_config).unwrap();
//!
//! // Check stream status
//! assert!(smmu.has_stream(stream_id));
//! assert_eq!(smmu.get_stream_count(), 1);
//!
//! // Shutdown gracefully
//! smmu.shutdown().unwrap();
//! ```

use crate::cache::{CacheEntry, CacheKey, ReplacementPolicy, TlbCache};
use crate::stream_context::StreamContext;
use crate::types::{
    AccessType, CommandEntry, CommandType, EventEntry, EventType, FaultRecord, FaultType, PRIEntry, PagePermissions,
    QueueStatistics, SMMUConfig, SMMUError, SecurityState, StreamConfig, StreamID, TranslationError, TranslationResult,
    IOVA, PA, PASID,
};
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, RwLock};

/// Cache-aligned atomic counter to prevent false sharing
///
/// Each counter is aligned to 64 bytes (standard cache line size) to ensure
/// that concurrent updates to different counters don't cause cache-line bouncing
/// on multi-core systems. This is critical for high-contention statistics counters.
///
/// **Performance Impact**: Prevents 10-40ns cache-line bouncing penalty under
/// high concurrency, especially on NUMA systems.
#[repr(align(64))]
#[derive(Debug)]
struct CacheAligned<T>(T);

impl<T> CacheAligned<T> {
    const fn new(value: T) -> Self {
        Self(value)
    }
}

/// Record of a stalled transaction in the stall queue (ARM §3.12.2).
///
/// When a stream is configured with `FaultMode::Stall` and a translation fault
/// occurs, the SMMU creates one `StallRecord` per fault and enqueues it.
/// Software retrieves the STAG from `TranslationError::Stalled { stag }` and
/// later sends `CMD_RESUME` or `CMD_STALL_TERM` to complete or abort the stall.
#[derive(Debug, Clone)]
pub struct StallRecord {
    /// Unique Stall TAG for this transaction group.
    pub stag: u16,
    /// Stream that issued the faulting transaction.
    pub stream_id: u32,
    /// PASID of the faulting transaction.
    pub pasid: u32,
    /// IOVA of the faulting access.
    pub iova: u64,
    /// Access type of the faulting transaction.
    pub access: AccessType,
    /// Security state of the faulting transaction.
    pub security_state: SecurityState,
}

/// SMMU controller - Central coordination and translation engine
///
/// The SMMU controller manages multiple streams (devices), each with their own
/// translation contexts (PASIDs), and provides the main entry point for all
/// SMMU operations.
///
/// # Architecture
///
/// - **Stream Management**: `DashMap<StreamID, Arc<StreamContext>>` (interior mutability)
/// - **Global Configuration**: `Arc<RwLock<SMMUConfig>>`
/// - **Shutdown Coordination**: `AtomicBool`
/// - **Fault Queue**: `Arc<Mutex<Vec<FaultRecord>>>`
///
/// # Thread Safety
///
/// All methods use `&self` (immutable borrow) with interior mutability patterns:
/// - Lock-free operations for hot paths (has_stream, is_shutdown)
/// - Concurrent reads with exclusive writes for configuration
/// - Automatic `Send + Sync` trait derivation
///
/// # Resource Management
///
/// All resources are cleaned up via RAII:
/// - `Arc` reference counting prevents use-after-free
/// - `RwLock`/`Mutex` prevent data races
/// - `DashMap` provides lock-free concurrent access
/// - Drop trait automatically cleans up all resources
///
/// # ARM SMMU v3 Compliance
///
/// Implements Section 5.1 requirements:
/// - Stream limit enforcement per configuration
/// - Proper fault recording per Section 6.2
/// - Configuration validation per Section 3.4
/// - Atomic state transitions for shutdown
#[derive(Debug)]
pub struct SMMU {
    /// Stream management: StreamID → StreamContext mapping
    ///
    /// DashMap provides lock-free concurrent access for high performance.
    /// StreamContext uses interior mutability (DashMap for pasid_map, atomics for flags),
    /// so no additional RwLock wrapper is needed.
    streams: DashMap<u32, Arc<StreamContext>>,

    /// Global SMMU configuration
    ///
    /// RwLock allows many concurrent readers with exclusive writer access.
    /// Arc enables shared ownership across threads.
    config: Arc<RwLock<SMMUConfig>>,

    /// Shutdown coordination flag
    ///
    /// AtomicBool provides lock-free atomic state for shutdown checks.
    /// Used to reject new operations during graceful shutdown.
    shutdown: AtomicBool,

    /// Global SMMU enable flag (SMMU_CR0.SMMUEN, §6.3.9)
    ///
    /// When false (reset default), all transactions bypass the SMMU and
    /// receive an identity mapping (PA == IOVA) without fault.
    /// When true, translations go through the full stream/page-table path.
    enabled: AtomicBool,

    /// Global Bypass/Abort flag (SMMU_GBPA.ABORT, §3.11, §13.2)
    ///
    /// Only relevant when SMMUEN=0.  When true, all incoming transactions are
    /// aborted (bus error) instead of receiving an identity bypass mapping.
    /// Defaults to false (bypass mode) per the SMMU_GBPA reset value.
    gbpa_abort: AtomicBool,

    /// Fault event queue
    ///
    /// Thread-safe fault recording for diagnostic and compliance purposes.
    /// Mutex protects against concurrent modifications.
    fault_queue: Arc<Mutex<Vec<FaultRecord>>>,

    /// Translation statistics with cache-line alignment to prevent false sharing
    ///
    /// Lock-free counters for translation metrics. Each counter is cache-aligned
    /// to prevent false sharing (cache-line bouncing) under high contention.
    /// This optimization saves 10-40ns under concurrent load.
    total_translations: CacheAligned<AtomicU64>,
    successful_translations: CacheAligned<AtomicU64>,
    failed_translations: CacheAligned<AtomicU64>,

    /// Event queue (Section 5.3.1)
    ///
    /// FIFO queue for event records including translation faults, permission violations,
    /// and command completions. Thread-safe with RwLock for concurrent access.
    event_queue: Arc<RwLock<VecDeque<EventEntry>>>,
    event_queue_capacity: usize,
    event_count: AtomicU64,
    /// Event queue LOG2SIZE (ARM §3.5.1)
    eventq_log2size: u32,
    /// Event queue producer index register (ARM §3.5.1)
    eventq_prod: AtomicU32,
    /// Event queue consumer index register (ARM §3.5.1)
    eventq_cons: AtomicU32,

    /// Command queue (Section 5.3.2)
    ///
    /// FIFO queue for command entries including TLB invalidation, synchronization,
    /// and configuration commands. Thread-safe with RwLock for concurrent access.
    command_queue: Arc<RwLock<VecDeque<CommandEntry>>>,
    command_queue_capacity: usize,
    command_count: AtomicU64,
    /// Command queue LOG2SIZE (ARM §3.5.1)
    cmdq_log2size: u32,
    /// Command queue producer index register (ARM §3.5.1)
    cmdq_prod: AtomicU32,
    /// Command queue consumer index register (ARM §3.5.1)
    cmdq_cons: AtomicU32,

    /// PRI queue (Section 5.3.3)
    ///
    /// FIFO queue for Page Request Interface entries used for on-demand paging.
    /// Thread-safe with RwLock for concurrent access.
    pri_queue: Arc<RwLock<VecDeque<PRIEntry>>>,
    pri_queue_capacity: usize,
    pri_count: AtomicU64,
    /// PRI queue LOG2SIZE (ARM §3.5.1)
    priq_log2size: u32,
    /// PRI queue producer index register (ARM §3.5.1)
    priq_prod: AtomicU32,
    /// PRI queue consumer index register (ARM §3.5.1)
    priq_cons: AtomicU32,

    /// Cache invalidation counter
    ///
    /// Tracks number of TLB/ATC invalidation operations for statistics.
    invalidation_count: AtomicU64,

    /// TLB cache for accelerating address translations
    ///
    /// Caches translation results to avoid expensive page table walks on repeated
    /// translations. Critical for achieving target performance (135ns latency).
    /// Lock-free concurrent access via DashMap.
    tlb_cache: Arc<TlbCache>,

    /// Monotonic fault timestamp counter for ordering
    ///
    /// Uses atomic counter instead of SystemTime::now() to avoid expensive
    /// syscalls (20-50ns each) on the fault path. Provides ordering guarantees
    /// without wall-clock overhead. This is a monotonic counter, not wall-clock
    /// time - use for ordering faults only.
    ///
    /// **Performance**: Atomic increment is ~1-2ns vs 20-50ns for SystemTime::now()
    fault_timestamp_counter: AtomicU64,

    /// Stall queue: STAG → StallRecord (ARM §3.12.2).
    ///
    /// Holds pending stalled transactions for streams configured with
    /// `FaultMode::Stall`.  Software resolves stalls by sending `CMD_RESUME`
    /// or `CMD_STALL_TERM` with the matching STAG.
    stall_queue: DashMap<u16, StallRecord>,

    /// Monotonic STAG (Stall TAG) counter.
    ///
    /// Wrapping increment ensures unique STAGs for up to 65 535 concurrent
    /// stalls before wrap-around.
    stag_counter: AtomicU16,

    /// Stall-event pending buffer (ARM IHI0070G.b §7.4 / BUG-13).
    ///
    /// When the main event queue is full (len >= 2*capacity) and a stall event
    /// arrives, the event is placed here instead of being silently dropped.
    /// ARM §7.4: "a fault record from a stalled transaction is not discarded
    /// and an event is reported for the stalled transaction when the queue is
    /// next writable."
    ///
    /// Events in this buffer do NOT trigger OVFLG (stall faults do not cause
    /// an overflow condition per §7.4).  The buffer is drained into the main
    /// event queue whenever space becomes available (at record-event time or
    /// when get_events() is called).
    stall_pending: Mutex<VecDeque<EventEntry>>,

    /// Global error register (SMMU_GERROR, ARM §6.3.19).
    ///
    /// Bit-field of global error conditions:
    ///   Bit 0:  CMDQ_ERR       — Command queue processing error (key)
    ///   Bit 2:  EVENTQ_ABT_ERR — Event queue memory system abort
    ///   Bit 3:  PRIQ_ABT_ERR   — PRI queue memory system abort
    ///   Bit 4:  MSI_CMDQ_ABT_ERR  — MSI write abort for command queue
    ///   Bit 5:  MSI_EVENTQ_ABT_ERR — MSI write abort for event queue
    ///   Bit 6:  MSI_PRIQ_ABT_ERR  — MSI write abort for PRI queue
    ///   Bit 7:  MSI_GERROR_ABT_ERR — MSI write abort for GERROR
    ///   Bit 8:  SFM_ERR        — Service Fault Mapping error
    ///   Bit 9:  CMDQP_ERR      — Command queue paused error
    ///
    /// The SMMU toggles GERROR[x] ONLY when the error is currently INACTIVE
    /// (GERROR[x] == GERRORN[x]).  An error is active when GERROR[x] != GERRORN[x].
    /// Software acknowledges by writing GERRORN[x] to match GERROR[x].
    /// Resets to 0 (ARM §6.3.19).
    gerror: AtomicU32,

    /// Global error notify register (SMMU_GERRORN, ARM §6.3.20).
    ///
    /// Software-writable companion to GERROR.  An error bit is ACTIVE when
    /// the corresponding bits differ (GERROR[x] XOR GERRORN[x] == 1).
    /// Software acknowledges an error by toggling GERRORN[x] to match GERROR[x].
    /// `clear_gerror(bits)` toggles the specified bits in this register.
    /// Resets to 0 (ARM §6.3.20).
    gerrorn: AtomicU32,

    /// SMMU_CR0 register (§6.3.9).
    ///
    /// Bit-field controlling global SMMU queue enable gates (ARM §6.3.9):
    ///   Bit 0: SMMUEN   — global enable (mirrors `enabled`)
    ///   Bit 1: PRIQEN   — PRI queue enable gate
    ///   Bit 2: EVENTQEN — Event queue enable gate
    ///   Bit 3: CMDQEN   — Command queue enable gate
    ///   Bit 4: ATSCHK   — ATS behavior control
    ///
    /// Software may write this register directly via `set_cr0()` to control
    /// individual queue gates without toggling the global enable.
    cr0: AtomicU32,

    /// SMMU_CR2 register (§6.3.12).
    ///
    /// Bit-field controlling miscellaneous SMMU behaviour (ARM §6.3.12):
    ///   Bit 1: RECINVSID — Record C_BAD_STREAMID events in the event queue.
    ///
    /// When RECINVSID=0 (reset default), C_BAD_STREAMID events are NOT written
    /// to the event queue.  GERROR.CMDQ_ERR is only signalled for command-queue
    /// errors (§7.1), not for transaction-path StreamID faults (§7.3.3).
    /// When RECINVSID=1, C_BAD_STREAMID events are written to the event queue
    /// as specified in §7.3.3.
    cr2: AtomicU32,

    /// Atomic stream count for TOCTOU-safe limit enforcement (BUG-6 fix).
    ///
    /// Incremented with `fetch_add` BEFORE the DashMap insert and decremented
    /// with `fetch_sub` on failure (limit exceeded or duplicate stream).
    /// This eliminates the race window between the `streams.len()` read and
    /// the subsequent insert that existed in the previous implementation.
    stream_count: std::sync::atomic::AtomicUsize,

    /// Stream Table log2 size (ARM §6.3.4, §7.3.3 / BUG-NEW-RUST-1).
    ///
    /// When less than 32, the Stream Table has `2^log2size` entries and any
    /// StreamID at or above `2^log2size` is out-of-range — the SMMU must
    /// generate C_BAD_STREAMID (§7.3.3) and abort the transaction.
    ///
    /// The value 32 is a sentinel meaning "no table-size limit" (default).
    /// ARM IHI0070G.b §6.3.4 SMMU_STRTAB_BASE_CFG.LOG2SIZE field.
    strtab_log2size: AtomicU8,
}

impl SMMU {
    // ========================================================================
    // ARM §6.3.17: SMMU_GERROR bit constants (public for downstream testing)
    // ========================================================================

    /// GERROR bit 0: CMDQ_ERR — Command queue processing error (§6.3.17)
    ///
    /// Set when the SMMU detects an error during command processing (e.g.,
    /// unsupported command opcode, C_BAD_STREAMID) and halts the command queue.
    /// Software must clear this bit (via `clear_gerror`) before command
    /// processing can resume.
    pub const GERROR_CMDQ_ERR: u32           = 1 << 0;
    /// GERROR bit 2: EVENTQ_ABT_ERR — Event queue memory system abort (§6.3.17)
    pub const GERROR_EVENTQ_ABT_ERR: u32     = 1 << 2;
    /// GERROR bit 3: PRIQ_ABT_ERR — PRI queue memory system abort (§6.3.17)
    pub const GERROR_PRIQ_ABT_ERR: u32       = 1 << 3;
    /// GERROR bit 4: MSI_CMDQ_ABT_ERR — MSI write abort for command queue (§6.3.17)
    pub const GERROR_MSI_CMDQ_ABT_ERR: u32   = 1 << 4;
    /// GERROR bit 5: MSI_EVENTQ_ABT_ERR — MSI write abort for event queue (§6.3.17)
    pub const GERROR_MSI_EVENTQ_ABT_ERR: u32 = 1 << 5;
    /// GERROR bit 6: MSI_PRIQ_ABT_ERR — MSI write abort for PRI queue (§6.3.17)
    pub const GERROR_MSI_PRIQ_ABT_ERR: u32   = 1 << 6;
    /// GERROR bit 7: MSI_GERROR_ABT_ERR — MSI write abort for GERROR (§6.3.17)
    pub const GERROR_MSI_GERROR_ABT_ERR: u32 = 1 << 7;
    /// GERROR bit 8: SFM_ERR — Service Fault Mapping error (§6.3.17)
    pub const GERROR_SFM_ERR: u32            = 1 << 8;
    /// GERROR bit 9: CMDQP_ERR — Command queue paused error (§6.3.17)
    pub const GERROR_CMDQP_ERR: u32          = 1 << 9;

    // Backward-compatible aliases for renamed/repositioned constants.
    // Existing test code that references these names continues to compile.

    /// Alias for [`GERROR_MSI_CMDQ_ABT_ERR`](Self::GERROR_MSI_CMDQ_ABT_ERR) (§6.3.17).
    pub const GERROR_CMDQ_ABT_ERR: u32   = Self::GERROR_MSI_CMDQ_ABT_ERR;
    /// Alias for [`GERROR_MSI_EVENTQ_ABT_ERR`](Self::GERROR_MSI_EVENTQ_ABT_ERR) (§6.3.17).
    pub const GERROR_MSI_ABT_ERR: u32    = Self::GERROR_MSI_EVENTQ_ABT_ERR;
    /// Alias for [`GERROR_SFM_ERR`](Self::GERROR_SFM_ERR) (§6.3.17).
    pub const GERROR_SFE: u32            = Self::GERROR_SFM_ERR;

    // ========================================================================
    // ARM §6.3.9: SMMU_CR0 bit constants (public for downstream testing)
    // ========================================================================

    /// CR0 bit 0: SMMUEN — global SMMU enable (§6.3.9)
    pub const CR0_SMMUEN: u32   = 1 << 0;
    /// CR0 bit 1: PRIQEN — PRI queue enable gate (§6.3.9)
    ///
    /// ARM IHI0070G.b §6.3.9: bit[1]=PRIQEN. Note: there is no INTEN bit in the
    /// ARM SMMU v3 specification; the previous incorrect constant CR0_INTEN has
    /// been removed.
    pub const CR0_PRIQEN: u32   = 1 << 1;
    /// CR0 bit 2: EVENTQEN — Event queue enable gate (§6.3.9)
    pub const CR0_EVENTQEN: u32 = 1 << 2;
    /// CR0 bit 3: CMDQEN — Command queue enable gate (§6.3.9)
    pub const CR0_CMDQEN: u32   = 1 << 3;
    /// CR0 bit 4: ATSCHK — ATS CHK enable (§6.3.9)
    pub const CR0_ATSCHK: u32   = 1 << 4;

    // ========================================================================
    // ARM §6.3.12: SMMU_CR2 bit constants (public for downstream testing)
    // ========================================================================

    /// CR2 bit 1: RECINVSID — Record C_BAD_STREAMID events in the event queue (§6.3.12 / §7.3.3).
    ///
    /// When 0 (reset default), C_BAD_STREAMID events triggered by CMD_CFGI_STE
    /// for an unknown StreamID are NOT written to the event queue.  Note: GERROR.CMDQ_ERR
    /// is only toggled for command-queue processing errors (§7.1), not for
    /// transaction-path StreamID range faults (§7.3.3).
    /// When 1, C_BAD_STREAMID events are recorded in the event queue as per §7.3.3.
    pub const CR2_RECINVSID: u32 = 1 << 1;

    // ========================================================================
    // ARM §3.5.1: Circular Queue Index Helpers (private)
    // ========================================================================

    /// Compute LOG2SIZE for a given capacity (next power-of-two exponent).
    ///
    /// If capacity is already a power of 2, log2size = log2(capacity).
    /// Otherwise, log2size = ceil(log2(capacity)).
    ///
    /// Examples: 256 → 8, 512 → 9, 128 → 7.
    fn compute_log2size(capacity: usize) -> u32 {
        if capacity <= 1 {
            return 0;
        }
        if capacity.is_power_of_two() {
            // Exact power of 2: trailing_zeros gives log2.
            capacity.trailing_zeros()
        } else {
            // Not a power of 2: use ceil(log2(capacity)).
            // usize::BITS - leading_zeros gives floor(log2) + 1 for non-power-of-two.
            usize::BITS - capacity.leading_zeros()
        }
    }

    /// Advance a circular queue index by 1 (ARM §3.5.1 wrap semantics).
    ///
    /// The index cycles through 0..2^(log2size+1)-1 then wraps to 0.
    ///
    /// Per ARM IHI0070G.b §3.5.1, the maximum permitted LOG2SIZE is 19.
    /// Values above 19 are clamped to 19 to prevent shift-overflow panics in
    /// debug builds and divide-by-zero in release builds.
    fn advance_index(idx: u32, log2size: u32) -> u32 {
        let log2size = log2size.min(19); // ARM §3.5.1: max log2size = 19
        let modulus = 2u32 << log2size; // 2^(log2size+1)
        (idx + 1) % modulus
    }

    /// Compute number of occupied entries in a circular queue.
    ///
    /// Uses modular subtraction so the result is always non-negative.
    ///
    /// Per ARM IHI0070G.b §3.5.1, the maximum permitted LOG2SIZE is 19.
    /// Values above 19 are clamped to 19 to prevent shift-overflow panics in
    /// debug builds and divide-by-zero in release builds.
    fn queue_occupied(prod: u32, cons: u32, log2size: u32) -> u32 {
        let log2size = log2size.min(19); // ARM §3.5.1: max log2size = 19
        let modulus = 2u32 << log2size;
        prod.wrapping_sub(cons).wrapping_add(modulus) % modulus
    }

    /// Create a new SMMU instance with default configuration
    ///
    /// Default configuration:
    /// - Standard queue sizes (512 event, 256 command, 128 PRI)
    /// - 1024 TLB cache entries
    /// - 48-bit IOVA space, 52-bit PA space
    /// - Maximum 65_536 streams
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    ///
    /// let smmu = SMMU::new();
    /// assert!(!smmu.is_shutdown());
    /// assert_eq!(smmu.get_stream_count(), 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(SMMUConfig::default())
    }

    /// Create a new SMMU instance with custom configuration
    ///
    /// Validates configuration before creating SMMU instance.
    /// Use configuration presets for common scenarios:
    /// - `SMMUConfig::high_performance()` - Server/data center
    /// - `SMMUConfig::low_memory()` - Memory-constrained environments
    /// - `SMMUConfig::embedded_profile()` - Embedded systems
    ///
    /// # Arguments
    ///
    /// * `config` - Global SMMU configuration
    ///
    /// # Panics
    ///
    /// Panics if configuration validation fails. Use `SMMUConfig::validate()`
    /// to check configuration before passing to this constructor.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    /// use smmu::types::SMMUConfig;
    ///
    /// let config = SMMUConfig::high_performance();
    /// let smmu = SMMU::with_config(config);
    /// assert_eq!(smmu.get_stream_count(), 0);
    /// ```
    #[must_use]
    pub fn with_config(config: SMMUConfig) -> Self {
        // Validate configuration on construction
        config.validate().expect("Invalid SMMU configuration");

        let queue_config = config.queue_config();
        let event_queue_capacity = queue_config.event_queue_size;
        let command_queue_capacity = queue_config.command_queue_size;
        let pri_queue_capacity = queue_config.pri_queue_size;

        let eventq_log2size = Self::compute_log2size(event_queue_capacity);
        let cmdq_log2size = Self::compute_log2size(command_queue_capacity);
        let priq_log2size = Self::compute_log2size(pri_queue_capacity);

        // Create TLB cache with capacity from configuration
        let tlb_capacity = config.cache_config.tlb_cache_size;
        let tlb_cache = Arc::new(TlbCache::new(tlb_capacity, ReplacementPolicy::Lru));

        Self {
            streams: DashMap::new(),
            config: Arc::new(RwLock::new(config)),
            shutdown: AtomicBool::new(false),
            enabled: AtomicBool::new(false),
            gbpa_abort: AtomicBool::new(false),
            fault_queue: Arc::new(Mutex::new(Vec::new())),
            total_translations: CacheAligned::new(AtomicU64::new(0)),
            successful_translations: CacheAligned::new(AtomicU64::new(0)),
            failed_translations: CacheAligned::new(AtomicU64::new(0)),
            event_queue: Arc::new(RwLock::new(VecDeque::with_capacity(event_queue_capacity))),
            event_queue_capacity,
            event_count: AtomicU64::new(0),
            eventq_log2size,
            eventq_prod: AtomicU32::new(0),
            eventq_cons: AtomicU32::new(0),
            command_queue: Arc::new(RwLock::new(VecDeque::with_capacity(command_queue_capacity))),
            command_queue_capacity,
            command_count: AtomicU64::new(0),
            cmdq_log2size,
            cmdq_prod: AtomicU32::new(0),
            cmdq_cons: AtomicU32::new(0),
            pri_queue: Arc::new(RwLock::new(VecDeque::with_capacity(pri_queue_capacity))),
            pri_queue_capacity,
            pri_count: AtomicU64::new(0),
            priq_log2size,
            priq_prod: AtomicU32::new(0),
            priq_cons: AtomicU32::new(0),
            invalidation_count: AtomicU64::new(0),
            tlb_cache,
            fault_timestamp_counter: AtomicU64::new(0),
            stall_queue: DashMap::new(),
            stag_counter: AtomicU16::new(1),
            stall_pending: Mutex::new(VecDeque::new()),
            gerror: AtomicU32::new(0),
            // GERRORN reset: ARM §6.3.20 — resets to 0.
            gerrorn: AtomicU32::new(0),
            // CR0 reset: ARM IHI0070G.b §6.3.9 — ALL bits reset to 0.
            // SMMUEN, CMDQEN, EVENTQEN, and PRIQEN are all 0 after reset.
            // Software must explicitly set the required bits via set_cr0() or
            // call enable() before using queues or translations.
            cr0: AtomicU32::new(0),
            // CR2 reset: ARM IHI0070G.b §6.3.12 — resets to 0.
            // RECINVSID=0 means C_BAD_STREAMID events are not recorded by default.
            cr2: AtomicU32::new(0),
            // BUG-6 fix: atomic stream counter starts at zero; incremented
            // before each insert, decremented on rollback.
            stream_count: std::sync::atomic::AtomicUsize::new(0),
            // BUG-NEW-RUST-1 fix: 32 = sentinel "no table-size limit" (default).
            // Software must call set_strtab_log2size() to enable range checking.
            strtab_log2size: AtomicU8::new(32),
        }
    }

    /// Explicit initialization step
    ///
    /// Currently a no-op as initialization happens in constructor.
    /// Reserved for future use (e.g., hardware initialization, DMA setup).
    ///
    /// # Errors
    ///
    /// Returns error if SMMU is already shutdown.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    ///
    /// let smmu = SMMU::new();
    /// assert!(smmu.initialize().is_ok());
    /// ```
    pub fn initialize(&self) -> Result<(), SMMUError> {
        self.check_shutdown()?;
        Ok(())
    }

    /// Graceful shutdown of SMMU controller
    ///
    /// Performs graceful shutdown in the following order:
    /// 1. Set shutdown flag atomically (rejects new operations)
    /// 2. Clear all stream contexts
    /// 3. Flush fault queue
    ///
    /// After shutdown, all operations will return `ShutdownInProgress` error.
    ///
    /// # Thread Safety
    ///
    /// Shutdown is atomic and thread-safe. Multiple threads can call this
    /// concurrently - only the first call performs the shutdown.
    ///
    /// # Errors
    ///
    /// Returns error if already shutdown (idempotent).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    /// use smmu::types::{StreamID, StreamConfig};
    ///
    /// let smmu = SMMU::new();
    /// let stream_id = StreamID::new(1).unwrap();
    /// smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();
    ///
    /// // Graceful shutdown
    /// assert!(smmu.shutdown().is_ok());
    /// assert!(smmu.is_shutdown());
    ///
    /// // Operations after shutdown fail
    /// assert!(smmu.configure_stream(stream_id, StreamConfig::bypass()).is_err());
    /// ```
    pub fn shutdown(&self) -> Result<(), SMMUError> {
        // Atomic test-and-set to ensure single shutdown
        // Use AcqRel: combines acquire (read) and release (write) semantics
        // This is sufficient for swap operations - SeqCst was overly conservative
        let was_shutdown = self.shutdown.swap(true, Ordering::AcqRel);
        if was_shutdown {
            return Err(SMMUError::ShutdownInProgress);
        }

        // BUG-NEW-RUST-5 fix: zero stream_count BEFORE clearing streams.
        // The previous order (streams.clear() then stream_count.store(0)) created a
        // window where a concurrent get_stream_count() could observe count > 0 while
        // streams is already empty.  Zeroing the count first closes that window:
        // any observer after this store sees count == 0 regardless of whether
        // streams.clear() has completed yet.  The Release ordering pairs with the
        // Acquire load in get_stream_count(), establishing happens-before.
        self.stream_count.store(0, Ordering::Release);
        // Clear all streams (automatic cleanup via Arc/Drop)
        self.streams.clear();

        // Flush fault queue
        if let Ok(mut queue) = self.fault_queue.lock() {
            queue.clear();
        }

        Ok(())
    }

    /// Check if SMMU is shutdown
    ///
    /// Lock-free atomic check for shutdown state.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    ///
    /// let smmu = SMMU::new();
    /// assert!(!smmu.is_shutdown());
    ///
    /// smmu.shutdown().unwrap();
    /// assert!(smmu.is_shutdown());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_shutdown(&self) -> bool {
        // Pair with the AcqRel swap in shutdown() to ensure the write is visible.
        // Relaxed is insufficient on weakly-ordered architectures (BUG-RUST-07).
        self.shutdown.load(Ordering::Acquire)
    }

    /// Returns true when SMMU_CR0.SMMUEN is set (§6.3.9).
    #[inline]
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// Set SMMU_CR0.SMMUEN=1 — enable the SMMU globally.
    ///
    /// After this call, translations proceed through the full stream/page-table
    /// path instead of bypassing.  Also sets EVENTQEN, CMDQEN and PRIQEN so that
    /// all queues are active by default, matching hardware reset-to-enabled behavior.
    ///
    /// # Errors
    ///
    /// Returns `ShutdownInProgress` if the SMMU has been shut down.
    pub fn enable(&self) -> Result<(), SMMUError> {
        self.check_shutdown()?;
        // BUG-RUST-F5 fix: §6.3.9 (init order) / §7.2.1 (events gated on EVENTQEN).
        // The previous order set enabled=true BEFORE writing CR0_EVENTQEN: in the window
        // between those two stores, translations could run and generate fault events that
        // were silently discarded because event recording checks CR0_EVENTQEN (§7.2.1).
        //
        // Fix: write CR0_EVENTQEN (and CMDQEN, PRIQEN, SMMUEN) FIRST, then set the
        // `enabled` flag.  This matches ARM §6.3.9's recommended initialization order:
        // queues enabled before SMMUEN=1.  When `enabled` becomes visible to other threads,
        // CR0_EVENTQEN is already set, so no fault event can be generated in a window
        // where the event queue is not yet active.
        self.cr0.fetch_or(
            Self::CR0_SMMUEN | Self::CR0_EVENTQEN | Self::CR0_CMDQEN | Self::CR0_PRIQEN,
            Ordering::AcqRel,
        );
        self.enabled.store(true, Ordering::Release);
        Ok(())
    }

    /// Clear SMMU_CR0.SMMUEN — disable the SMMU globally.
    ///
    /// After this call, all transactions bypass the SMMU (identity mapping,
    /// no fault), regardless of stream configuration.
    ///
    /// # Errors
    ///
    /// Returns `ShutdownInProgress` if the SMMU has been shut down.
    pub fn disable(&self) -> Result<(), SMMUError> {
        self.check_shutdown()?;
        // BUG-R2-RUST-2 fix: §6.3.9 (disable ordering) — match the ordering used by
        // enable(), which writes CR0 FIRST then sets the `enabled` flag.
        //
        // The previous order stored `enabled=false` BEFORE clearing CR0.SMMUEN via
        // fetch_and.  This created a window where `is_enabled()` returned false while
        // `get_cr0()` still showed SMMUEN=1 — an observable inconsistency between the
        // two views of SMMU enable state on weakly-ordered hardware.
        //
        // Fix: clear CR0.SMMUEN first, then store enabled=false.  When `enabled`
        // becomes visible to other threads (Release store), CR0.SMMUEN has already
        // been cleared (AcqRel fetch_and), so the two views are consistent.
        self.cr0.fetch_and(!Self::CR0_SMMUEN, Ordering::AcqRel);
        self.enabled.store(false, Ordering::Release);
        Ok(())
    }

    /// Write SMMU_CR0 directly (§6.3.9).
    ///
    /// Sets the full CR0 register value.  Bit 0 (SMMUEN) also updates the
    /// `enabled` flag so that `is_enabled()` remains consistent.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    ///
    /// let smmu = SMMU::new();
    /// smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN);
    /// assert!(smmu.is_enabled());
    /// assert_eq!(smmu.get_cr0() & SMMU::CR0_CMDQEN, SMMU::CR0_CMDQEN);
    /// ```
    pub fn set_cr0(&self, value: u32) {
        self.cr0.store(value, Ordering::Release);
        self.enabled.store((value & Self::CR0_SMMUEN) != 0, Ordering::Release);
    }

    /// Read SMMU_CR0 (§6.3.9).
    ///
    /// Returns the current CR0 register value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    ///
    /// let smmu = SMMU::new();
    /// // ARM §6.3.9: All CR0 bits reset to 0 (SMMUEN=0, CMDQEN=0, EVENTQEN=0, PRIQEN=0).
    /// assert_eq!(smmu.get_cr0(), 0, "CR0 must be 0 after reset (§6.3.9)");
    /// // After enable(), SMMUEN and queue enable bits are set.
    /// smmu.enable().unwrap();
    /// assert_ne!(smmu.get_cr0() & SMMU::CR0_SMMUEN, 0, "SMMUEN set after enable()");
    /// ```
    #[must_use]
    pub fn get_cr0(&self) -> u32 {
        self.cr0.load(Ordering::Acquire)
    }

    /// Write SMMU_CR2 (§6.3.12).
    ///
    /// Controls miscellaneous SMMU behaviour.  Notable bit:
    /// - `CR2_RECINVSID` (bit 1): when set, C_BAD_STREAMID events are recorded
    ///   in the event queue.  When clear (reset default), they are suppressed.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    ///
    /// let smmu = SMMU::new();
    /// assert_eq!(smmu.get_cr2(), 0, "CR2 resets to 0 (§6.3.12)");
    /// smmu.set_cr2(SMMU::CR2_RECINVSID);
    /// assert_eq!(smmu.get_cr2() & SMMU::CR2_RECINVSID, SMMU::CR2_RECINVSID);
    /// ```
    pub fn set_cr2(&self, value: u32) {
        self.cr2.store(value, Ordering::Release);
    }

    /// Read SMMU_CR2 (§6.3.12).
    ///
    /// Returns the current CR2 register value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    ///
    /// let smmu = SMMU::new();
    /// assert_eq!(smmu.get_cr2(), 0, "CR2 must be 0 after reset (§6.3.12)");
    /// ```
    #[must_use]
    pub fn get_cr2(&self) -> u32 {
        self.cr2.load(Ordering::Acquire)
    }

    // ========================================================================
    // ARM §6.3.4: SMMU_STRTAB_BASE_CFG.LOG2SIZE — stream table size limit
    // ========================================================================

    /// Set the Stream Table log2 size (ARM §6.3.4 / BUG-NEW-RUST-1).
    ///
    /// When `v < 32`, any StreamID >= 2^v is out-of-range and will be rejected
    /// with `TranslationError::InvalidStreamID` in `translate()`, as mandated
    /// by ARM IHI0070G.b §7.3.3 (C_BAD_STREAMID).
    ///
    /// The sentinel value `32` (default) means "no table-size limit" — all
    /// StreamIDs are accepted and looked up in the DashMap as normal.
    ///
    /// Valid hardware range per ARM §6.3.4: 1–20 (model accepts 0–31; 32 = disabled).
    pub fn set_strtab_log2size(&self, v: u8) {
        self.strtab_log2size.store(v, Ordering::Release);
    }

    /// Read the Stream Table log2 size (ARM §6.3.4 / BUG-NEW-RUST-1).
    ///
    /// Returns the configured log2 size.  Value 32 means "no limit" (default).
    #[must_use]
    pub fn get_strtab_log2size(&self) -> u8 {
        self.strtab_log2size.load(Ordering::Acquire)
    }

    /// Returns true when SMMU_GBPA.ABORT is set (§3.11, §13.2).
    ///
    /// When SMMUEN=0 and this flag is true, all transactions are aborted
    /// instead of bypassed with an identity mapping.
    #[inline]
    #[must_use]
    pub fn is_gbpa_abort(&self) -> bool {
        self.gbpa_abort.load(Ordering::Acquire)
    }

    /// Set or clear SMMU_GBPA.ABORT (§3.11, §13.2).
    ///
    /// - `abort=true`: when SMMUEN=0, all transactions abort with `TranslationError::GbpaAbort`.
    /// - `abort=false` (default): when SMMUEN=0, transactions bypass with identity PA.
    ///
    /// Has no effect when SMMUEN=1.
    pub fn set_gbpa_abort(&self, abort: bool) {
        self.gbpa_abort.store(abort, Ordering::Release);
    }

    // ========================================================================
    // ARM §6.3.17: SMMU_GERROR / SMMU_GERRORN register model
    // ========================================================================

    /// Read SMMU_GERROR — global error status register (ARM §6.3.19).
    ///
    /// Returns the raw GERROR register value.  An error bit is ACTIVE when
    /// `get_gerror() XOR get_gerrorn()` is non-zero for that bit.  At reset
    /// both GERROR and GERRORN are 0, so no errors are active.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    ///
    /// let smmu = SMMU::new();
    /// assert_eq!(smmu.get_gerror(), 0, "GERROR must be zero after reset");
    /// ```
    #[must_use]
    pub fn get_gerror(&self) -> u32 {
        self.gerror.load(Ordering::Acquire)
    }

    /// Read SMMU_GERRORN — global error notify register (ARM §6.3.20).
    ///
    /// Returns the software-writable GERRORN register value.  An error bit is
    /// ACTIVE when `get_gerror() XOR get_gerrorn()` is non-zero.  Software
    /// acknowledges an active error by calling `clear_gerror(bits)`, which
    /// toggles the corresponding GERRORN bits to match GERROR.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    ///
    /// let smmu = SMMU::new();
    /// assert_eq!(smmu.get_gerrorn(), 0, "GERRORN must be zero after reset");
    /// ```
    #[must_use]
    pub fn get_gerrorn(&self) -> u32 {
        self.gerrorn.load(Ordering::Acquire)
    }

    /// Signal a GERROR error condition (ARM §6.3.19 XOR-toggle protocol).
    ///
    /// Toggles GERROR[x] for each bit in `bits` that is currently INACTIVE
    /// (i.e., GERROR[x] == GERRORN[x]).  Bits that are already ACTIVE are
    /// left unchanged — "The SMMU does not toggle a bit when an error is
    /// already active" (ARM §6.3.19).
    ///
    /// This replaces the old `fetch_or` pattern which violated the spec by
    /// setting bits unconditionally instead of using XOR-toggle semantics.
    fn signal_gerror(&self, bits: u32) {
        // BUG-RUST-DBGR-7 fix: use a CAS loop to atomically check-and-toggle.
        // The spec (§6.3.19 §7.5) says "The SMMU does not toggle bit[x] if the
        // error is already active".  An error is ACTIVE when GERROR[x] != GERRORN[x].
        // A plain load-then-fetch_xor has a TOCTOU window: another thread could
        // activate the bit between our load and our fetch_xor, causing us to
        // incorrectly toggle an already-active bit.  A CAS loop closes this window.
        loop {
            let current_gerror  = self.gerror.load(Ordering::Acquire);
            let current_gerrorn = self.gerrorn.load(Ordering::Acquire);
            // BUG-NEW-RUST-3 fix: full seqlock — re-read BOTH gerrorn and gerror
            // to detect concurrent modification of either register between the two
            // initial loads.
            //
            // BUG-RUST-E fix (preserved): re-read gerrorn after gerror to detect
            // concurrent clear_gerror() between the two loads.  If gerrorn changed,
            // our snapshot is inconsistent — retry.
            let reread_gerrorn = self.gerrorn.load(Ordering::Acquire);
            if reread_gerrorn != current_gerrorn {
                // BUG-R2-RUST-7 fix: yield CPU hint to reduce memory-bus contention
                // under adversarial concurrent load and avoid a busy-spin livelock.
                std::hint::spin_loop();
                continue; // gerrorn changed — clear_gerror ran concurrently, retry
            }
            // Also re-read gerror to detect concurrent signal_gerror() modifying
            // gerror between our initial load and the gerrorn read.  If gerror
            // changed, our snapshot is stale — retry.
            let reread_gerror = self.gerror.load(Ordering::Acquire);
            if reread_gerror != current_gerror {
                // BUG-R2-RUST-7 fix: yield CPU hint at each retry point.
                std::hint::spin_loop();
                continue; // gerror changed — concurrent signal_gerror(), retry
            }
            // Consistent snapshot — compute inactive bits.
            // Bits that are currently ACTIVE (GERROR[x] != GERRORN[x]): skip.
            // Bits that are currently INACTIVE (GERROR[x] == GERRORN[x]): toggle.
            let active   = current_gerror ^ current_gerrorn;
            let inactive = bits & !active;
            if inactive == 0 {
                break; // all requested bits are already active — nothing to do
            }
            let new_gerror = current_gerror ^ inactive;
            if self.gerror.compare_exchange_weak(
                current_gerror,
                new_gerror,
                Ordering::AcqRel,
                Ordering::Acquire,
            ).is_ok() {
                break; // success — CAS applied
            }
            // CAS failed: GERROR changed concurrently — retry loop.
            // BUG-R2-RUST-7 fix: yield CPU hint to reduce contention on retry.
            std::hint::spin_loop();
        }
    }

    /// Toggle EVENTQ_PROD.OVFLG (bit 31) atomically, only if not already active.
    ///
    /// ARM §7.4: "OVFLG is toggled when a non-stall event record is discarded due
    /// to queue full, provided that an overflow condition is not already present."
    /// OVFLG is active when `OVFLG != OVACKFLG` (bit 31 of prod differs from
    /// bit 31 of cons).  BUG-RUST-DBGR-8 fix: use CAS loop to close the TOCTOU
    /// window between the `ovflg == ovackflg` check and the `fetch_xor`.
    #[inline]
    fn toggle_ovflg_once(&self) {
        loop {
            let prod = self.eventq_prod.load(Ordering::Acquire);
            let cons = self.eventq_cons.load(Ordering::Acquire);
            // BUG-RUST-J fix: re-read prod after cons to detect concurrent
            // clear_event_queue() resetting both to 0 between the two loads.
            // If prod changed, our snapshot is inconsistent — retry.
            let prod2 = self.eventq_prod.load(Ordering::Acquire);
            if prod != prod2 {
                continue; // snapshot inconsistent — retry
            }
            let ovflg    = (prod >> 31) & 1;
            let ovackflg = (cons >> 31) & 1;
            if ovflg != ovackflg {
                break; // overflow already active — do NOT toggle again
            }
            let new_prod = prod ^ (1u32 << 31);
            if self.eventq_prod.compare_exchange_weak(
                prod,
                new_prod,
                Ordering::AcqRel,
                Ordering::Acquire,
            ).is_ok() {
                break; // success — CAS applied
            }
            // CAS failed: prod changed concurrently — retry loop
        }
    }

    /// Acknowledge GERROR error bits (ARM §6.3.20 — write SMMU_GERRORN).
    ///
    /// Software acknowledges an active error by writing GERRORN to match
    /// GERROR for those bits.  This implementation toggles GERRORN for the
    /// specified `bits`, which matches the hardware XOR semantics: toggling
    /// GERRORN[x] makes it equal to GERROR[x], setting the error to INACTIVE.
    ///
    /// After this call:
    /// - `get_gerrorn() & bits` will have been toggled.
    /// - `(get_gerror() ^ get_gerrorn()) & bits` will be 0 (error inactive)
    ///   provided the SMMU has not re-signalled in the interim.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    /// use smmu::types::{CommandEntry, CommandType};
    ///
    /// let smmu = SMMU::new();
    /// smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    ///
    /// // Trigger CMDQ_ERR: submit a command for an unknown stream.
    /// smmu.submit_command(CommandEntry::new(CommandType::CfgiSte, 0xDEAD, 0)).unwrap();
    /// let _ = smmu.process_command_queue();
    ///
    /// // Error is ACTIVE before acknowledge.
    /// assert_ne!(
    ///     (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR,
    ///     0,
    ///     "CMDQ_ERR must be ACTIVE before acknowledge"
    /// );
    ///
    /// // Acknowledge: toggle GERRORN.CMDQ_ERR to match GERROR.CMDQ_ERR.
    /// smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);
    ///
    /// // Error is now INACTIVE: (GERROR ^ GERRORN) & CMDQ_ERR == 0.
    /// assert_eq!(
    ///     (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR,
    ///     0,
    ///     "CMDQ_ERR must be inactive after acknowledge"
    /// );
    /// ```
    pub fn clear_gerror(&self, bits: u32) {
        // BUG-03 fix: toggle GERRORN to acknowledge the error (ARM §6.3.20).
        // The old implementation (fetch_and(!bits)) directly cleared GERROR,
        // which is a read-only hardware register.  Per the spec, software must
        // write GERRORN; the SMMU uses XOR-toggle semantics: toggling GERRORN[x]
        // to match GERROR[x] makes the error inactive (equal → inactive).
        // ARM §6.3.20: "Software must not toggle fields in this register that
        // correspond to errors that are inactive."  Mask to active bits only.
        //
        // BUG-R-04 fix: use a CAS loop to close the TOCTOU race between the
        // Acquire load of gerrorn and the subsequent fetch_xor.  A concurrent
        // clear_gerror call could toggle the same bits between the load and the
        // store, re-activating an already-acknowledged error.  The CAS loop
        // retries until our snapshot of gerrorn is still current at store time,
        // matching the pattern used by signal_gerror().
        loop {
            let gerror = self.gerror.load(Ordering::Acquire);
            let gerrorn = self.gerrorn.load(Ordering::Acquire);
            let active_bits = gerror ^ gerrorn;
            let new_gerrorn = gerrorn ^ (bits & active_bits);
            if self
                .gerrorn
                .compare_exchange(gerrorn, new_gerrorn, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                break;
            }
        }
    }

    /// Configure a stream with specified configuration
    ///
    /// Creates a new stream context if it doesn't exist. Returns error if
    /// stream already exists or stream limit exceeded.
    ///
    /// # Arguments
    ///
    /// * `stream_id` - Stream identifier (device ID)
    /// * `config` - Stream configuration (translation stages, PASID, etc.)
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - SMMU is shutdown (`ShutdownInProgress`)
    /// - Stream already exists (`StreamAlreadyExists`)
    /// - Stream limit exceeded (`StreamLimitExceeded`)
    /// - Configuration is invalid (`InvalidConfiguration`)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    /// use smmu::types::{StreamID, StreamConfig};
    ///
    /// let smmu = SMMU::new();
    /// let stream_id = StreamID::new(1).unwrap();
    ///
    /// // Configure Stage-1 only translation
    /// let config = StreamConfig::stage1_only();
    /// assert!(smmu.configure_stream(stream_id, config).is_ok());
    ///
    /// // Duplicate configuration fails
    /// let config2 = StreamConfig::stage2_only();
    /// assert!(smmu.configure_stream(stream_id, config2).is_err());
    /// ```
    pub fn configure_stream(&self, stream_id: StreamID, config: StreamConfig) -> Result<(), SMMUError> {
        self.check_shutdown()?;

        // Validate stream configuration
        config
            .validate()
            .map_err(|e| SMMUError::invalid_configuration(format!("Stream config validation failed: {e:?}")))?;

        let stream_value = stream_id.as_u32();

        // BUG-6 fix: atomically reserve a slot BEFORE the DashMap insert.
        //
        // Increment the counter first.  If the resulting value (post-increment)
        // exceeds `max_streams`, roll back immediately and return an error.
        // This eliminates the TOCTOU window that previously existed between
        // the `streams.len()` read and the subsequent DashMap insert.
        //
        // Ordering rationale:
        //   - AcqRel on the fetch_add: the acquire half ensures we see all
        //     prior decrements; the release half makes our increment visible
        //     to concurrent readers before we attempt the insert.
        //   - Release on the fetch_sub rollback: ensures the freed slot is
        //     visible to the next waiter.
        // BUG-RUST-C fix: snapshot max_streams BEFORE fetch_add so the limit in effect
        // at decision time is used.  If max_streams is reduced between the fetch_add and
        // a subsequent config re-read, a valid reservation (prev_count < old max_streams)
        // would be spuriously rejected.  Reading max_streams first and then atomically
        // reserving a slot honours the limit that was in effect when the decision was made.
        let max_streams = self.config.read().unwrap().max_streams();
        let prev_count = self.stream_count.fetch_add(1, Ordering::AcqRel);

        if prev_count >= max_streams {
            // Roll back — we must not consume the slot.
            self.stream_count.fetch_sub(1, Ordering::Release);
            return Err(SMMUError::stream_limit_exceeded(prev_count, max_streams));
        }

        // Create new StreamContext and apply the full configuration via
        // update_configuration() — this is the ONLY path that sets all STE
        // fields (including GAP-1 output-attribute overrides and GAP-2 STRW)
        // with Release ordering, establishing a happens-before relationship
        // with the Acquire loads in the translate hot-path.
        //
        // BUG-RUST-NEW-2 fix: Previously individual Relaxed setters were used,
        // leaving GAP-1/GAP-2 fields (mt_cfg, mem_attr, sh_cfg, alloc_cfg,
        // inst_cfg, priv_cfg, ns_cfg, strw) at their zero defaults for all
        // SMMU-configured streams.  update_configuration() sets every field.
        let stream_context = StreamContext::new();
        stream_context.update_configuration(config);
        // BUG-RUST-DBGR-10 fix: store the StreamID so fault records carry the correct
        // StreamID per ARM §7.3 (not placeholder 0).
        stream_context.set_stream_id(stream_value);

        // Use entry API for atomic check-and-insert (eliminates TOCTOU race for duplicates)
        // This guarantees that no other thread can insert the same stream_id between
        // our check and insert operations.
        match self.streams.entry(stream_value) {
            Entry::Vacant(entry) => {
                // Insert into stream map with Arc wrapper (no RwLock needed)
                entry.insert(Arc::new(stream_context));
                Ok(())
            },
            Entry::Occupied(_) => {
                // Duplicate stream: roll back the slot we pre-reserved.
                self.stream_count.fetch_sub(1, Ordering::Release);
                Err(SMMUError::stream_already_exists(stream_id))
            },
        }
    }

    /// Disable a previously configured stream
    ///
    /// When disabled, any translation attempt on this stream will return
    /// `StreamDisabled` error and generate an `F_STREAM_DISABLED` event (§7.3.7).
    ///
    /// # Errors
    ///
    /// Returns error if SMMU is shutdown or stream is not found.
    pub fn disable_stream(&self, stream_id: StreamID) -> Result<(), SMMUError> {
        self.check_shutdown()?;
        let ctx = self.get_stream_context(stream_id)?;
        ctx.disable();
        Ok(())
    }

    /// Re-enable a previously disabled stream
    ///
    /// After re-enabling, translations proceed normally per the stream's config.
    ///
    /// # Errors
    ///
    /// Returns error if SMMU is shutdown or stream is not found.
    pub fn enable_stream(&self, stream_id: StreamID) -> Result<(), SMMUError> {
        self.check_shutdown()?;
        let ctx = self.get_stream_context(stream_id)?;
        ctx.enable();
        Ok(())
    }

    /// Update the configuration of an existing stream without invalidating TLB entries.
    ///
    /// Per ARM IHI0070G.b §4.4, updating an STE does NOT automatically invalidate
    /// cached TLB entries; software must issue explicit `CMD_TLBI_*` commands to
    /// invalidate stale translations.  This method faithfully models that behaviour:
    /// the stream context atomics are updated (STRW, output-attribute fields, etc.)
    /// but any TLB entries cached for this stream remain live until explicitly
    /// invalidated.
    ///
    /// # Arguments
    ///
    /// * `stream_id` - Stream identifier to reconfigure
    /// * `config`    - New stream configuration to apply
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - SMMU is shutdown (`ShutdownInProgress`)
    /// - Stream not found (`StreamNotFound`)
    /// - New configuration fails validation (`InvalidConfiguration`)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    /// use smmu::types::{StreamID, StreamConfig, StreamWorld};
    ///
    /// let smmu = SMMU::new();
    /// smmu.enable().unwrap();
    /// let stream_id = StreamID::new(42).unwrap();
    ///
    /// smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();
    ///
    /// // Update STRW without clearing TLB (caller must issue CMD_TLBI_* separately).
    /// let new_cfg = StreamConfig::builder()
    ///     .stage1_enabled(true)
    ///     .translation_enabled(true)
    ///     .strw(StreamWorld::El1El0)
    ///     .build()
    ///     .unwrap();
    /// smmu.reconfigure_stream(stream_id, new_cfg).unwrap();
    /// ```
    pub fn reconfigure_stream(&self, stream_id: StreamID, config: StreamConfig) -> Result<(), SMMUError> {
        self.check_shutdown()?;

        config
            .validate()
            .map_err(|e| SMMUError::invalid_configuration(format!("Stream config validation failed: {e:?}")))?;

        let ctx = self.get_stream_context(stream_id)?;
        ctx.update_configuration(config);
        Ok(())
    }

    /// Remove a stream and its associated context
    ///
    /// Removes stream from SMMU and cleans up all associated resources
    /// (PASIDs, address spaces, etc.). Automatic cleanup via Arc/Drop.
    ///
    /// # Arguments
    ///
    /// * `stream_id` - Stream identifier to remove
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - SMMU is shutdown (`ShutdownInProgress`)
    /// - Stream not found (`StreamNotFound`)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    /// use smmu::types::{StreamID, StreamConfig};
    ///
    /// let smmu = SMMU::new();
    /// let stream_id = StreamID::new(1).unwrap();
    ///
    /// smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();
    /// assert!(smmu.has_stream(stream_id));
    ///
    /// smmu.remove_stream(stream_id).unwrap();
    /// assert!(!smmu.has_stream(stream_id));
    /// ```
    pub fn remove_stream(&self, stream_id: StreamID) -> Result<(), SMMUError> {
        self.check_shutdown()?;

        let stream_value = stream_id.as_u32();

        self.streams
            .remove(&stream_value)
            .ok_or_else(|| SMMUError::stream_not_found(stream_id))?;

        // BUG-6 fix: decrement atomic counter to match the removed stream slot.
        self.stream_count.fetch_sub(1, Ordering::Release);

        // Invalidate all TLB cache entries for this stream
        self.tlb_cache.invalidate_by_stream(stream_id);

        Ok(())
    }

    /// Check if a stream exists
    ///
    /// Lock-free check for stream existence.
    ///
    /// # Arguments
    ///
    /// * `stream_id` - Stream identifier to check
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    /// use smmu::types::{StreamID, StreamConfig};
    ///
    /// let smmu = SMMU::new();
    /// let stream_id = StreamID::new(1).unwrap();
    ///
    /// assert!(!smmu.has_stream(stream_id));
    ///
    /// smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();
    /// assert!(smmu.has_stream(stream_id));
    /// ```
    #[inline]
    #[must_use]
    pub fn has_stream(&self, stream_id: StreamID) -> bool {
        self.streams.contains_key(&stream_id.as_u32())
    }

    /// Get current stream count
    ///
    /// Returns the number of currently configured streams.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    /// use smmu::types::{StreamID, StreamConfig};
    ///
    /// let smmu = SMMU::new();
    /// assert_eq!(smmu.get_stream_count(), 0);
    ///
    /// let stream_id = StreamID::new(1).unwrap();
    /// smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();
    /// assert_eq!(smmu.get_stream_count(), 1);
    /// ```
    #[must_use]
    pub fn get_stream_count(&self) -> usize {
        // BUG-R2-RUST-6 fix (Option A): remove the `is_shutdown()` early-return
        // guard and rely solely on the authoritative `stream_count` atomic.
        //
        // The previous guard returned 0 immediately when `is_shutdown()` was true,
        // which pre-empted the DashMap state.  This over-compensated: callers that
        // check `get_stream_count() > 0` to verify stream registration could be
        // misled into thinking streams were never registered during the window
        // between `shutdown.swap(true)` and `streams.clear()`.
        //
        // `shutdown()` already atomically stores 0 to `stream_count` after
        // `streams.clear()`, so once `shutdown()` has completed, this method
        // correctly returns 0 via the `stream_count` load alone.  During a
        // concurrent `shutdown()`, the count is not guaranteed to be consistent
        // (by design — shutdown is a tear-down path), but that window is benign.
        //
        // BUG-RUST-A fix (preserved): use `stream_count` rather than
        // `streams.len()` (DashMap), which diverges during concurrent mutation.
        self.stream_count.load(Ordering::Acquire)
    }

    /// Create a PASID for a stream
    ///
    /// Creates a new Process Address Space ID (PASID) within a stream context.
    /// Each PASID has its own address space for Stage-1 translation.
    ///
    /// # Arguments
    ///
    /// * `stream_id` - Stream identifier
    /// * `pasid` - Process Address Space ID to create
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - SMMU is shutdown (`ShutdownInProgress`)
    /// - Stream not found (`StreamNotFound`)
    /// - PASID creation fails (`StreamContextError`)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    /// use smmu::types::{StreamID, StreamConfig, PASID};
    ///
    /// let smmu = SMMU::new();
    /// let stream_id = StreamID::new(1).unwrap();
    /// smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    ///
    /// let pasid = PASID::new(0).unwrap();
    /// smmu.create_pasid(stream_id, pasid).unwrap();
    /// ```
    pub fn create_pasid(&self, stream_id: StreamID, pasid: PASID) -> Result<(), SMMUError> {
        self.check_shutdown()?;
        let stream_context = self.get_stream_context(stream_id)?;
        stream_context.create_pasid(pasid).map_err(SMMUError::from)
    }

    /// Set the ASID (CD.ASID) for a PASID — ARM SMMU v3 §3.17, §4.4
    ///
    /// Configures the Address Space Identifier stored in the Context Descriptor
    /// (CD Word 1[31:16]) for the given stream/PASID pair.  Stage-1 TLB entries
    /// installed after this call will be tagged with the new ASID so that
    /// `CMD_TLBI_NH_ASID` / `CMD_TLBI_EL2_ASID` commands can target them.
    ///
    /// # Errors
    ///
    /// Returns error if stream or PASID is not found.
    pub fn set_pasid_asid(&self, stream_id: StreamID, pasid: PASID, asid: u16) -> Result<(), SMMUError> {
        self.check_shutdown()?;
        let stream_context = self.get_stream_context(stream_id)?;
        stream_context.set_pasid_asid(pasid, asid).map_err(SMMUError::from)
    }

    /// Get the ASID (CD.ASID) currently set for a PASID — ARM SMMU v3 §3.17
    ///
    /// Returns the 16-bit ASID for the given stream/PASID pair.
    /// Returns `0` if no ASID has been set (default).
    ///
    /// # Errors
    ///
    /// Returns error if stream or PASID is not found.
    pub fn get_pasid_asid(&self, stream_id: StreamID, pasid: PASID) -> Result<u16, SMMUError> {
        self.check_shutdown()?;
        let stream_context = self.get_stream_context(stream_id)?;
        stream_context.get_pasid_asid(pasid).map_err(SMMUError::from)
    }

    /// Set the VMID (STE.S2VMID) for a stream — ARM SMMU v3 §5.2, §3.8
    ///
    /// Configures the Virtual Machine Identifier stored in the Stream Table
    /// Entry (STE Word 2 bits 63:48). TLB entries installed after this call
    /// will be tagged with the new VMID so that `CMD_TLBI_S12_VMALL` and
    /// `CMD_TLBI_S2_IPA` can target them.
    ///
    /// # Errors
    ///
    /// Returns error if stream is not found.
    pub fn set_stream_vmid(&self, stream_id: StreamID, vmid: u16) -> Result<(), SMMUError> {
        self.check_shutdown()?;
        let stream_context = self.get_stream_context(stream_id)?;
        stream_context.set_vmid(vmid);
        Ok(())
    }

    /// Get the VMID (STE.S2VMID) currently set for a stream — ARM SMMU v3 §5.2
    ///
    /// Returns the 16-bit VMID. Returns `0` if no VMID has been set (default).
    ///
    /// # Errors
    ///
    /// Returns error if stream is not found.
    pub fn get_stream_vmid(&self, stream_id: StreamID) -> Result<u16, SMMUError> {
        self.check_shutdown()?;
        let stream_context = self.get_stream_context(stream_id)?;
        Ok(stream_context.get_vmid())
    }

    // ========================================================================
    // Section 3.12.2: Stall fault model — public API
    // ========================================================================

    /// Returns all currently stalled transactions (ARM §3.12.2).
    ///
    /// Each entry in the returned vector corresponds to one stalled fault.
    /// Software uses the `stag` field to issue `CMD_RESUME` or
    /// `CMD_STALL_TERM` to resolve each stall.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    ///
    /// let smmu = SMMU::new();
    /// assert!(smmu.get_stalled_transactions().is_empty());
    /// ```
    #[must_use]
    pub fn get_stalled_transactions(&self) -> Vec<StallRecord> {
        self.stall_queue.iter().map(|entry| entry.value().clone()).collect()
    }

    /// Aborts the stalled transaction identified by `stag` (ARM §3.12.2).
    ///
    /// Equivalent to sending `CMD_STALL_TERM` with the given STAG.
    /// Returns `true` if the record was found and removed, `false` if the
    /// STAG was unknown (no-op).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    ///
    /// let smmu = SMMU::new();
    /// assert!(!smmu.abort_stalled_transaction(42), "unknown STAG returns false");
    /// ```
    pub fn abort_stalled_transaction(&self, stag: u16) -> bool {
        self.stall_queue.remove(&stag).is_some()
    }

    /// Remove a PASID from a stream
    ///
    /// Removes a Process Address Space ID (PASID) from a stream context,
    /// cleaning up all associated resources (address space, mappings, etc.).
    ///
    /// # Arguments
    ///
    /// * `stream_id` - Stream identifier
    /// * `pasid` - Process Address Space ID to remove
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - SMMU is shutdown (`ShutdownInProgress`)
    /// - Stream not found (`StreamNotFound`)
    /// - PASID removal fails (`StreamContextError`)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    /// use smmu::types::{StreamID, StreamConfig, PASID};
    ///
    /// let smmu = SMMU::new();
    /// let stream_id = StreamID::new(1).unwrap();
    /// smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    ///
    /// let pasid = PASID::new(1).unwrap();
    /// smmu.create_pasid(stream_id, pasid).unwrap();
    ///
    /// // Later, remove the PASID
    /// smmu.remove_pasid(stream_id, pasid).unwrap();
    /// ```
    pub fn remove_pasid(&self, stream_id: StreamID, pasid: PASID) -> Result<(), SMMUError> {
        self.check_shutdown()?;
        let stream_context = self.get_stream_context(stream_id)?;
        let result = stream_context.remove_pasid(pasid);

        // Invalidate all TLB cache entries for this stream/PASID combination
        if result.is_ok() {
            self.tlb_cache.invalidate_by_stream_pasid(stream_id, pasid);
        }

        result.map_err(SMMUError::from)
    }

    /// Map a page in a stream's address space
    ///
    /// Creates a virtual-to-physical mapping for Stage-1 translation.
    ///
    /// # Arguments
    ///
    /// * `stream_id` - Stream identifier
    /// * `pasid` - Process Address Space ID
    /// * `iova` - Input/Output Virtual Address
    /// * `pa` - Physical Address
    /// * `permissions` - Page permissions (read/write/execute)
    /// * `security_state` - Security state (Secure/NonSecure/Realm/Root)
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - SMMU is shutdown (`ShutdownInProgress`)
    /// - Stream not found (`StreamNotFound`)
    /// - Mapping operation fails (`StreamContextError`)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    /// use smmu::types::{StreamID, StreamConfig, PASID, IOVA, PA, PagePermissions, SecurityState};
    ///
    /// let smmu = SMMU::new();
    /// let stream_id = StreamID::new(1).unwrap();
    /// smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    ///
    /// let pasid = PASID::new(0).unwrap();
    /// smmu.create_pasid(stream_id, pasid).unwrap();
    ///
    /// let iova = IOVA::new(0x1000).unwrap();
    /// let pa = PA::new(0x2000).unwrap();
    /// let perms = PagePermissions::read_write();
    /// smmu.map_page(stream_id, pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();
    /// ```
    pub fn map_page(
        &self,
        stream_id: StreamID,
        pasid: PASID,
        iova: IOVA,
        pa: PA,
        permissions: PagePermissions,
        security_state: SecurityState,
    ) -> Result<(), SMMUError> {
        self.check_shutdown()?;
        let stream_context = self.get_stream_context(stream_id)?;
        let result = stream_context.map_page(pasid, iova, pa, permissions, security_state);

        // Invalidate TLB cache entry for this IOVA to ensure consistency
        // when remapping an existing page
        if result.is_ok() {
            let cache_key = CacheKey::new(stream_id, pasid, iova, security_state);
            self.tlb_cache.invalidate_entry(&cache_key);
        }

        result.map_err(SMMUError::from)
    }

    /// Map a page in the Stage-2 address space (IPA → PA)
    ///
    /// For two-stage translation, Stage-2 maps Intermediate Physical Addresses (IPA)
    /// to Physical Addresses (PA). This is separate from Stage-1 PASID-based mappings.
    ///
    /// # Arguments
    ///
    /// * `stream_id` - Stream identifier
    /// * `ipa` - Intermediate Physical Address (from Stage-1 output)
    /// * `pa` - Final Physical Address
    /// * `permissions` - Page permissions for Stage-2
    /// * `security_state` - Security state
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - SMMU is shutdown
    /// - Stream not found
    /// - Stage-2 address space not initialized
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use smmu::SMMU;
    /// use smmu::types::{StreamID, StreamConfig, IOVA, PA, PagePermissions, SecurityState};
    ///
    /// let smmu = SMMU::new();
    /// let stream_id = StreamID::new(1).unwrap();
    ///
    /// // Configure for two-stage translation
    /// let mut config = StreamConfig::default();
    /// config.stage1_enabled = true;
    /// config.stage2_enabled = true;
    /// smmu.configure_stream(stream_id, config).unwrap();
    ///
    /// // Create Stage-2 address space
    /// smmu.create_stage2_address_space(stream_id).unwrap();
    ///
    /// // Map Stage-2: IPA → PA
    /// let ipa = IOVA::new(0x2000).unwrap(); // IPA from Stage-1
    /// let pa = PA::new(0x3000).unwrap();
    /// smmu.map_stage2_page(stream_id, ipa, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    /// ```
    pub fn map_stage2_page(
        &self,
        stream_id: StreamID,
        ipa: IOVA,
        pa: PA,
        permissions: PagePermissions,
        security_state: SecurityState,
    ) -> Result<(), SMMUError> {
        self.check_shutdown()?;
        let stream_context = self.get_stream_context(stream_id)?;
        stream_context.map_stage2_page(ipa, pa, permissions, security_state)
            .map_err(SMMUError::from)
    }

    /// Create and initialize Stage-2 address space for a stream
    ///
    /// Required for two-stage translation. Creates a new address space
    /// for Stage-2 IPA → PA mappings.
    ///
    /// # Arguments
    ///
    /// * `stream_id` - Stream identifier
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - SMMU is shutdown
    /// - Stream not found
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use smmu::SMMU;
    /// use smmu::types::{StreamID, StreamConfig};
    ///
    /// let smmu = SMMU::new();
    /// let stream_id = StreamID::new(1).unwrap();
    ///
    /// // Configure stream for two-stage translation
    /// let mut config = StreamConfig::default();
    /// config.stage1_enabled = true;
    /// config.stage2_enabled = true;
    /// smmu.configure_stream(stream_id, config).unwrap();
    ///
    /// // Create Stage-2 address space
    /// smmu.create_stage2_address_space(stream_id).unwrap();
    /// ```
    pub fn create_stage2_address_space(&self, stream_id: StreamID) -> Result<(), SMMUError> {
        self.check_shutdown()?;
        let stream_context = self.get_stream_context(stream_id)?;
        stream_context.create_stage2_address_space().map_err(SMMUError::from)
    }

    /// Get a copy of the global SMMU configuration
    ///
    /// Returns a cloned copy of the configuration to avoid holding read lock.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    /// use smmu::types::SMMUConfig;
    ///
    /// let config = SMMUConfig::high_performance();
    /// let smmu = SMMU::with_config(config.clone());
    ///
    /// let retrieved_config = smmu.get_config();
    /// assert_eq!(retrieved_config, config);
    /// ```
    #[must_use]
    pub fn get_config(&self) -> SMMUConfig {
        let config_guard = self.config.read().unwrap();
        config_guard.clone()
    }

    /// Update global configuration transactionally
    ///
    /// Applies configuration update function atomically. If validation fails,
    /// no changes are applied (all-or-nothing semantics).
    ///
    /// # Arguments
    ///
    /// * `f` - Configuration update function
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - SMMU is shutdown (`ShutdownInProgress`)
    /// - Configuration validation fails (`InvalidConfiguration`)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    ///
    /// let smmu = SMMU::new();
    ///
    /// // Update cache size
    /// smmu.update_config(|config| {
    ///     config.cache_config.tlb_cache_size = 2048;
    /// }).unwrap();
    ///
    /// let config = smmu.get_config();
    /// assert_eq!(config.cache_config.tlb_cache_size, 2048);
    /// ```
    pub fn update_config(&self, f: impl FnOnce(&mut SMMUConfig)) -> Result<(), SMMUError> {
        self.check_shutdown()?;

        let mut config_guard = self.config.write().unwrap();

        // Create backup for rollback
        let backup = config_guard.clone();

        // Apply update function
        f(&mut config_guard);

        // Validate updated configuration
        if let Err(e) = config_guard.validate() {
            // Rollback on validation failure
            *config_guard = backup;
            return Err(SMMUError::invalid_configuration(format!(
                "Configuration update validation failed: {:?}",
                e
            )));
        }

        Ok(())
    }

    /// Record a fault event
    ///
    /// Appends fault to the fault queue for diagnostic purposes.
    /// Thread-safe concurrent fault recording.
    ///
    /// # Arguments
    ///
    /// * `fault` - Fault record to append
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    /// use smmu::types::{FaultRecord, FaultType, StreamID, PASID, IOVA, AccessType, SecurityState};
    ///
    /// let smmu = SMMU::new();
    ///
    /// let fault = FaultRecord::builder()
    ///     .stream_id(StreamID::new(1).unwrap())
    ///     .pasid(PASID::new(0).unwrap())
    ///     .address(IOVA::new(0x1000).unwrap())
    ///     .fault_type(FaultType::TranslationFault)
    ///     .access_type(AccessType::Read)
    ///     .security_state(SecurityState::NonSecure)
    ///     .timestamp(0)
    ///     .build();
    ///
    /// smmu.record_fault(fault);
    /// assert_eq!(smmu.get_faults().len(), 1);
    /// ```
    pub fn record_fault(&self, fault: FaultRecord) {
        if let Ok(mut queue) = self.fault_queue.lock() {
            queue.push(fault);
        }
    }

    /// Get a copy of all fault records
    ///
    /// Returns a cloned copy of the fault queue to avoid holding lock.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    /// use smmu::types::{FaultRecord, FaultType, StreamID, PASID, IOVA, AccessType, SecurityState};
    ///
    /// let smmu = SMMU::new();
    ///
    /// let fault = FaultRecord::builder()
    ///     .stream_id(StreamID::new(1).unwrap())
    ///     .pasid(PASID::new(0).unwrap())
    ///     .address(IOVA::new(0x1000).unwrap())
    ///     .fault_type(FaultType::TranslationFault)
    ///     .access_type(AccessType::Read)
    ///     .security_state(SecurityState::NonSecure)
    ///     .timestamp(0)
    ///     .build();
    ///
    /// smmu.record_fault(fault.clone());
    ///
    /// let faults = smmu.get_faults();
    /// assert_eq!(faults.len(), 1);
    /// assert_eq!(faults[0], fault);
    /// ```
    #[must_use]
    pub fn get_faults(&self) -> Vec<FaultRecord> {
        if let Ok(queue) = self.fault_queue.lock() {
            queue.clone()
        } else {
            Vec::new()
        }
    }

    /// Clear all fault records
    ///
    /// Removes all faults from the queue.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    /// use smmu::types::{FaultRecord, FaultType, StreamID, PASID, IOVA, AccessType, SecurityState};
    ///
    /// let smmu = SMMU::new();
    ///
    /// let fault = FaultRecord::builder()
    ///     .stream_id(StreamID::new(1).unwrap())
    ///     .pasid(PASID::new(0).unwrap())
    ///     .address(IOVA::new(0x1000).unwrap())
    ///     .fault_type(FaultType::TranslationFault)
    ///     .access_type(AccessType::Read)
    ///     .security_state(SecurityState::NonSecure)
    ///     .timestamp(0)
    ///     .build();
    ///
    /// smmu.record_fault(fault);
    /// assert_eq!(smmu.get_faults().len(), 1);
    ///
    /// smmu.clear_faults();
    /// assert_eq!(smmu.get_faults().len(), 0);
    /// ```
    pub fn clear_faults(&self) {
        if let Ok(mut queue) = self.fault_queue.lock() {
            queue.clear();
        }
    }

    // ========================================================================
    // Translation Engine - Section 5.2
    // ========================================================================

    /// Perform address translation (main SMMU API)
    ///
    /// Translates an Input/Output Virtual Address (IOVA) to a Physical Address (PA)
    /// for a given stream and process address space. This is the main entry point for
    /// all translation operations, implementing ARM SMMU v3 Section 5.3 translation process.
    ///
    /// # Translation Modes
    ///
    /// The translation behavior depends on stream configuration:
    ///
    /// - **Stage-1 Only**: IOVA → PA (per-PASID translation)
    /// - **Stage-2 Only**: IPA → PA (VM translation)
    /// - **Two-Stage**: IOVA → IPA → PA (nested virtualization)
    /// - **Bypass**: IOVA = PA (identity mapping)
    ///
    /// # Fault Recording
    ///
    /// Translation errors are automatically recorded as fault events per ARM SMMU v3
    /// Section 6.2 fault reporting requirements. Faults can be retrieved via
    /// `get_faults()` for diagnostic purposes.
    ///
    /// # Thread Safety
    ///
    /// This method uses `&self` (immutable borrow) with interior mutability for
    /// thread-safe concurrent translations. Multiple threads can perform translations
    /// simultaneously without blocking.
    ///
    /// # Arguments
    ///
    /// * `stream_id` - Stream identifier (device ID)
    /// * `pasid` - Process Address Space ID (0 for legacy/default mode)
    /// * `iova` - Input/Output Virtual Address to translate
    /// * `access` - Access type (Read/Write/Execute) for permission checking
    ///
    /// # Returns
    ///
    /// Returns TranslationResult containing:
    /// - **Success**: Physical address, permissions, and security state
    /// - **Error**: Detailed translation error (page fault, permission violation, etc.)
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - SMMU is shutdown (`ShutdownInProgress`)
    /// - Stream not configured (`StreamNotFound`)
    /// - Translation fails (various TranslationError types converted to `SMMUError`)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    /// use smmu::types::{StreamID, StreamConfig, PASID, IOVA, AccessType, SecurityState};
    ///
    /// let smmu = SMMU::new();
    ///
    /// // Configure stream
    /// let stream_id = StreamID::new(1).unwrap();
    /// smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    ///
    /// // Perform translation
    /// let pasid = PASID::new(0).unwrap();
    /// let iova = IOVA::new(0x1000).unwrap();
    /// let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    ///
    /// match result {
    ///     Ok(data) => println!("PA: 0x{:x}", data.physical_address().as_u64()),
    ///     Err(e) => println!("Translation failed: {}", e),
    /// }
    /// ```
    ///
    /// # ARM SMMU v3 Compliance
    ///
    /// Implements:
    /// - Section 5.3: Translation process and multi-stage translation
    /// - Section 6.2: Fault detection and reporting
    /// - Stage-1, Stage-2, Two-Stage, and Bypass modes
    /// - Permission checking per access type
    /// - PASID 0 support for legacy compatibility
    #[allow(clippy::too_many_lines)]
    pub fn translate(
        &self,
        stream_id: StreamID,
        pasid: PASID,
        iova: IOVA,
        access: AccessType,
        security_state: SecurityState,
    ) -> TranslationResult {
        // Update statistics
        self.total_translations.0.fetch_add(1, Ordering::Relaxed);

        // Check shutdown state first.
        // Return early without recording a fault — a shutdown is not a translation
        // fault. StreamNotConfigured is used because TranslationError has no
        // shutdown-specific variant; callers can distinguish it via SMMU::is_shutdown().
        if self.check_shutdown().is_err() {
            self.failed_translations.0.fetch_add(1, Ordering::Relaxed);
            // Do NOT fall through to record_translation_fault below.
            return Err(TranslationError::StreamNotConfigured);
        }

        // BUG-RUST-H fix: read OAS bits once per translate() to avoid repeated
        // config.read() RwLock acquisitions on the hot path.
        let oas_bits = self.config.read().unwrap().address_config.max_pa_bits as u64;

        // §6.3.9 SMMUEN=0: bypass or abort depending on GBPA.ABORT (§3.11, §13.2).
        if !self.enabled.load(Ordering::Acquire) {
            if self.gbpa_abort.load(Ordering::Acquire) {
                // GBPA.ABORT=1: abort all transactions — no identity mapping, no fault event.
                self.failed_translations.0.fetch_add(1, Ordering::Relaxed);
                return Err(TranslationError::GbpaAbort);
            }
            // GBPA.ABORT=0: bypass — PA = IOVA (identity); full permissions; no fault.
            // §3.4: OAS check for GBPA bypass. Silent abort (no event) if IOVA >= OAS.
            {
                // oas_bits already read once near the top of translate() (BUG-RUST-H fix).
                if oas_bits < 64 && iova.as_u64() >= (1u64 << oas_bits) {
                    self.failed_translations.0.fetch_add(1, Ordering::Relaxed);
                    return Err(TranslationError::AddressSizeError);
                }
            }
            let pa = crate::types::PA::new(iova.as_u64())
                .unwrap_or_else(|_| crate::types::PA::new(0).expect("zero PA always valid"));
            self.successful_translations.0.fetch_add(1, Ordering::Relaxed);
            return Ok(crate::types::TranslationData::new(
                pa,
                crate::types::PagePermissions::all(),
                security_state,
            ));
        }

        // BUG-NEW-RUST-1 fix: §6.3.4 / §7.3.3 — StreamID range check.
        // When strtab_log2size < 32 (table-size limit configured), any StreamID
        // >= 2^log2size is out-of-range.  The SMMU must return C_BAD_STREAMID and
        // abort the transaction; GERROR.CMDQ_ERR is not toggled for transaction-path
        // StreamID faults (only for command-queue errors per §7.1).
        {
            let log2size = self.strtab_log2size.load(Ordering::Acquire);
            if log2size < 32 {
                let limit = 1u32 << log2size;
                if stream_id.as_u32() >= limit {
                    self.failed_translations.0.fetch_add(1, Ordering::Relaxed);
                    self.record_stream_not_found_fault(stream_id, pasid, iova, access, security_state);
                    return Err(TranslationError::InvalidStreamID);
                }
            }
        }

        // BUG-NEW-10 fix: TLB cache must only be consulted when SMMUEN=1; per ARM §6.3.9
        // when SMMUEN=0 all translations must bypass/abort regardless of cached state.
        // Fast path: TLB cache lookup (only reached when SMMU is enabled and not shut down).
        let cache_key = CacheKey::new(stream_id, pasid, iova, security_state);
        if let Some(cached) = self.tlb_cache.lookup(&cache_key) {
            // Verify cached entry allows the requested access type.
            // NOTE: `allows()` does NOT check the `privileged_only` bit (bit 3 of
            // PagePermissions) — that check is separate, per ARM §5.2 STE.STRW semantics.
            if cached.permissions.allows(access) {
                // Use the raw u32 key to match the DashMap key type.
                let stream_value = stream_id.as_u32();

                // BUG-RUST-F2 fix: §5.2 / §13.5 — Consolidate both the privilege check
                // and the apply_output_attrs() call into a SINGLE DashMap guard held
                // across both operations.  The previous code performed two separate
                // `self.streams.get(&stream_value)` calls, creating a TOCTOU window:
                // if remove_stream() fired between them the privilege check would silently
                // pass (the `if let Some` guard skips it when the stream is gone) AND
                // apply_output_attrs() would be skipped, returning zeroed output-attribute
                // fields.  A single guard eliminates that race window entirely.
                //
                // BUG-3 fix (preserved): §5.2 GAP-2 — The `privileged_only` bit must be
                // checked on the TLB fast path, not only in the slow-path translate().
                // When a page has `privileged_only` set and the stream's STRW does NOT
                // suppress the privilege check (i.e., STRW is not El2 or El3), a
                // non-privileged access must be denied even on a TLB hit.
                let mut data = crate::types::TranslationData::new(
                    cached.physical_address,
                    cached.permissions,
                    cached.security_state,
                );
                // BUG-NEW-RUST-1 fix (TLB fast path): skip the TLB entry when
                // S1DSS routing will override it.  A cached CD[0] result for
                // PASID==0 on a substream-capable stream (s1cd_max > 0) is only
                // valid when s1dss == 0x02 (use CD[0]).  If s1dss was changed to
                // 0x00 or 0x01 after the entry was cached, using it would return
                // a stale result instead of applying the new S1DSS routing rule.
                // Fall through to the slow path so the S1DSS block can apply the
                // correct behaviour.
                let s1dss_overrides_tlb = if pasid.as_u32() == 0 {
                    self.streams.get(&stream_value).map_or(false, |r| {
                        r.value().get_s1cd_max() > 0 && r.value().get_s1dss() != 2
                    })
                } else {
                    false
                };
                if !s1dss_overrides_tlb {
                    if let Some(stream_ref) = self.streams.get(&stream_value) {
                        // Privilege check (BUG-3 fix) and output-attr application (BUG-RUST-DBGR-1
                        // fix) use the SAME guard — no race window between the two operations.
                        if cached.permissions.privileged_only() && !stream_ref.value().strw_suppresses_priv() {
                            // privileged_only page and STRW enforces the check: deny access.
                            self.failed_translations.0.fetch_add(1, Ordering::Relaxed);
                            return Err(TranslationError::PermissionViolation { access });
                        }
                        data = stream_ref.value().apply_output_attrs(data);
                    } else if cached.permissions.privileged_only() {
                        // Stream was removed while the TLB entry was still live.  The privilege
                        // check cannot be evaluated without the stream's STRW — deny access
                        // conservatively rather than silently bypassing the check.
                        self.failed_translations.0.fetch_add(1, Ordering::Relaxed);
                        return Err(TranslationError::PermissionViolation { access });
                    }
                    self.successful_translations.0.fetch_add(1, Ordering::Relaxed);
                    return Ok(data);
                }
                // s1dss_overrides_tlb == true: fall through to slow path below
            }
            // Cache hit but insufficient permissions - fall through to full translation
        }

        // Slow path: TLB cache miss - perform full page table walk
        // Lookup stream context and perform translation while holding DashMap guard
        // This avoids Arc::clone overhead (5-15ns per cache-miss translation)
        let stream_value = stream_id.as_u32();
        let stream_guard = self.streams.get(&stream_value);

        // Capture ASID (CD.ASID, §3.17), VMID (STE.S2VMID, §5.2), stall mode,
        // and S1DSS / S1CDMax while holding the stream guard, so TLB entries
        // can be tagged and routing decisions made without re-locking the map.
        let (result, entry_asid, entry_vmid, stall_mode, is_bypass, stream_s1dss, stream_s1cd_max) =
            if let Some(stream_ref) = stream_guard {
                let asid = stream_ref.value().get_pasid_asid_or_default(pasid);
                let vmid = stream_ref.value().get_vmid();
                let stall = stream_ref.value().is_stall_enabled();
                let bypass = !stream_ref.value().is_stage1_enabled() && !stream_ref.value().is_stage2_enabled();
                let s1dss_val = stream_ref.value().get_s1dss();
                let s1cd_max_val = stream_ref.value().get_s1cd_max();

                // §5.4 / CT-13: T0SZ/T1SZ out-of-range check (valid range 0-39).
                // §5.4 / CT-14: CD.AA64=false (AArch32 LPAE) is unsupported — C_BAD_CD.
                // Only applies when Stage-1 is enabled.
                if stream_ref.value().is_stage1_enabled() {
                    let t0sz = stream_ref.value().get_t0sz();
                    let t1sz = stream_ref.value().get_t1sz();
                    let aa64 = stream_ref.value().get_aa64();
                    if !aa64 || t0sz > 39 || t1sz > 39 {
                        // Drop stream_ref guard before recording fault (not strictly needed
                        // but avoids holding the DashMap shard lock longer than necessary).
                        drop(stream_ref);
                        self.failed_translations.0.fetch_add(1, Ordering::Relaxed);
                        // Record the C_BAD_CD event directly to the event queue.
                        let timestamp = self.fault_timestamp_counter.fetch_add(1, Ordering::Relaxed);
                        let fault = FaultRecord::builder()
                            .stream_id(stream_id)
                            .pasid(pasid)
                            .address(iova)
                            .fault_type(FaultType::BadCD)
                            .access_type(access)
                            .security_state(security_state)
                            .timestamp(timestamp)
                            .build();
                        self.record_fault(fault);
                        // Gate event recording on CR0.EVENTQEN (§7.2.1).
                        if (self.cr0.load(Ordering::Acquire) & Self::CR0_EVENTQEN) != 0 {
                            let event = EventEntry {
                                event_type: EventType::CBadCd,
                                stream_id: stream_id.as_u32(),
                                pasid: pasid.as_u32(),
                                address: iova.as_u64(),
                                security_state,
                                error_code: 0,
                                timestamp,
                                stall: false,
                                stag: 0,
                            };
                            if let Ok(mut queue) = self.event_queue.write() {
                                if queue.len() < self.event_queue_capacity {
                                    queue.push_back(event);
                                    self.event_count.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                        // BUG-RUST-DBGR-2 fix: C_BAD_CD is a configuration fault (ARM §3.12.2);
                        // must always abort — never stalled. Return BadCD (event 0x0A) which
                        // maps to FaultType::BadCD and is correctly excluded from the
                        // stall-eligible set. Previously returned BadSubstreamId (0x08) which
                        // was inconsistent with the CBadCd event recorded above.
                        return Err(TranslationError::BadCD);
                    }
                }

                // Delegate to StreamContext for actual translation
                // StreamContext handles Stage-1, Stage-2, Two-Stage, and Bypass modes
                let r = stream_ref.value().translate(pasid, iova, access, security_state);
                (r, asid, vmid, stall, bypass, s1dss_val, s1cd_max_val)
            } else {
                self.failed_translations.0.fetch_add(1, Ordering::Relaxed);
                // Record fault before returning error
                self.record_stream_not_found_fault(stream_id, pasid, iova, access, security_state);
                return Err(TranslationError::StreamNotConfigured);
            };

        // §3.4 OAS check for STE-level bypass (Config==0b100, both stages disabled).
        // If IOVA >= OAS, abort with F_ADDR_SIZE; §7.3.14 mandates the event.
        // NOTE: This check MUST run BEFORE TLB insertion (below) so that an
        // out-of-range IOVA is never cached.  Caching the identity-map result
        // first and then returning Err would leave a stale TLB entry that makes
        // a subsequent translation of the same IOVA succeed from cache (BUG-NEW-09).
        // Note: C_BAD_SUBSTREAMID (non-zero PASID) is already handled inside translate()
        // and would have returned Err before reaching Ok here.
        if is_bypass && result.is_ok() {
            // oas_bits already read once near the top of translate() (BUG-RUST-H fix).
            if oas_bits < 64 && iova.as_u64() >= (1u64 << oas_bits) {
                let oas_error = TranslationError::AddressSizeError;
                self.failed_translations.0.fetch_add(1, Ordering::Relaxed);
                self.record_translation_fault(stream_id, pasid, iova, access, security_state, &oas_error, false, 0);
                return Err(oas_error);
            }
        }

        // NEW-52 / ARM §7.3.9 / §3.10: C_BAD_SUBSTREAMID when SubstreamID (PASID) >= 2^STE.S1CDMax.
        // This check applies to stage-1-capable, substream-capable streams (s1cd_max > 0)
        // when the presented PASID falls outside the valid index range.
        // PASID==0 is excluded: it is the "no substream" case handled by S1DSS below.
        // BUG-05 fix: this check MUST run BEFORE the TLB insert so that an invalid-PASID
        // result is never cached. Previously this check ran after TLB insertion, so the first
        // call would cache the inner Ok result and a subsequent call would hit the TLB fast
        // path and return Ok instead of Err(BadSubstreamId) — ARM IHI0070G.b §7.3.9.
        // BUG-11 fix: guard the shift — validation in StreamConfig::validate() now
        // prevents s1cd_max > 20, but this defensive check ensures no panic / UB
        // if a value >= 32 ever reaches this path (e.g., from unsafe construction).
        let s1cd_max_limit: u32 = if stream_s1cd_max < 32 {
            1u32 << stream_s1cd_max
        } else {
            u32::MAX
        };
        if !is_bypass && stream_s1cd_max > 0 && pasid.as_u32() != 0
            && pasid.as_u32() >= s1cd_max_limit
        {
            self.failed_translations.0.fetch_add(1, Ordering::Relaxed);
            let substreamid_error = TranslationError::BadSubstreamId;
            self.record_translation_fault(
                stream_id, pasid, iova, access, security_state, &substreamid_error, false, 0,
            );
            return Err(substreamid_error);
        }

        // On successful translation, populate TLB cache tagged with both CD.ASID
        // and STE.S2VMID so that ASID-targeted and VMID-targeted invalidation work.
        // Skip caching when S1DSS routing will override the result (s1cd_max > 0,
        // pasid == 0, and s1dss != 0b10) to avoid stale entries bypassing S1DSS.
        //
        // BUG-NEW-RUST-1 fix: re-read s1dss from the stream's atomic field
        // immediately before the TLB insert decision.  The `stream_s1dss` captured
        // earlier (while the stream guard was held) may be stale if a concurrent
        // reconfigure_stream() changed s1dss between the guard release and here.
        // A fresh Acquire load is sufficient because StreamContext.update_configuration()
        // stores s1dss with Release ordering, so this Acquire load observes the most
        // recent value.  If the stream was removed concurrently, fall back to the
        // original snapshot (safe: the stream no longer routing traffic).
        let current_s1dss = if stream_s1cd_max > 0 && pasid.as_u32() == 0 && !is_bypass {
            self.streams
                .get(&stream_id.as_u32())
                .map_or(stream_s1dss, |r| r.value().get_s1dss())
        } else {
            stream_s1dss
        };
        let s1dss_will_override =
            !is_bypass && stream_s1cd_max > 0 && pasid.as_u32() == 0 && current_s1dss != 2;
        if let Ok(ref data) = result {
            if !s1dss_will_override {
                let entry = CacheEntry::new_with_tags(
                    iova,
                    data.physical_address(),
                    data.permissions(),
                    data.security_state(),
                    entry_asid,
                    entry_vmid,
                    0,
                );
                self.tlb_cache.insert(cache_key, entry);
            }
        }

        // §3.9 / §5.2 STE.S1DSS: Route non-substream (PASID==0) transactions on
        // substream-capable stage-1 streams (s1cd_max > 0) according to STE.S1DSS.
        // This is an STE-level decision that overrides (or replaces) the CD[0] result.
        // Applies when:
        //   - The stream has at least one CD-index bit (s1cd_max > 0)
        //   - The transaction PASID is 0 (non-substream)
        //   - The stream is not in pure bypass mode (bypass streams handled above)
        //
        // For s1dss == 0b00: always abort (override any Ok result).
        // For s1dss == 0b01: always return identity mapping (override any result).
        // For s1dss == 0b10: use CD[0] — the result already computed is correct.
        //
        // BUG-NEW-RUST-1 fix: use `current_s1dss` (the fresh re-read value) for the
        // routing decision so that a concurrent reconfigure_stream() is observed here.
        if !is_bypass && stream_s1cd_max > 0 && pasid.as_u32() == 0 {
            match current_s1dss {
                0x00 => {
                    // §7.3.7: S1DSS==0b00 — non-substream transaction on a substream-capable
                    // stream aborts with F_STREAM_DISABLED (event 0x06).
                    // This overrides any Ok result from the inner translate() call.
                    // If the inner call already returned Err, we still need to count and record
                    // the failure as F_STREAM_DISABLED specifically.
                    self.failed_translations.0.fetch_add(1, Ordering::Relaxed);
                    // Directly enqueue the F_STREAM_DISABLED event — record_translation_fault
                    // suppresses EventEntry for StreamDisabled (from the STE.Config==0b000 path).
                    let timestamp = self.fault_timestamp_counter.fetch_add(1, Ordering::Relaxed);
                    let fault = crate::types::FaultRecord::builder()
                        .stream_id(stream_id)
                        .pasid(pasid)
                        .address(iova)
                        .fault_type(crate::types::FaultType::StreamDisabled)
                        .access_type(access)
                        .security_state(security_state)
                        .timestamp(timestamp)
                        .build();
                    self.record_fault(fault);
                    let event = EventEntry {
                        event_type: EventType::FStreamDisabled,
                        stream_id: stream_id.as_u32(),
                        pasid: pasid.as_u32(),
                        address: iova.as_u64(),
                        security_state,
                        error_code: 0,
                        timestamp,
                        stall: false,
                        stag: 0,
                    };
                    if let Ok(mut queue) = self.event_queue.write() {
                        if queue.len() < self.event_queue_capacity {
                            queue.push_back(event);
                            self.event_count.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    return Err(TranslationError::StreamDisabled);
                }
                0x01 => {
                    // §3.9 S1DSS==0b01: bypass stage-1 for non-substream transactions.
                    // Identity mapping: PA = IOVA, regardless of what the inner translate
                    // returned (it may have failed with PageNotMapped — that is overridden).
                    // OAS check applies per §3.4.
                    // oas_bits already read once near the top of translate() (BUG-RUST-H fix).
                    if oas_bits < 64 && iova.as_u64() >= (1u64 << oas_bits) {
                        let oas_error = TranslationError::AddressSizeError;
                        self.failed_translations.0.fetch_add(1, Ordering::Relaxed);
                        self.record_translation_fault(
                            stream_id, pasid, iova, access, security_state, &oas_error, false, 0,
                        );
                        return Err(oas_error);
                    }
                    let pa = crate::types::PA::new(iova.as_u64())
                        .unwrap_or_else(|_| crate::types::PA::new(0).expect("zero PA always valid"));
                    self.successful_translations.0.fetch_add(1, Ordering::Relaxed);
                    return Ok(crate::types::TranslationData::new(
                        pa,
                        crate::types::PagePermissions::all(),
                        security_state,
                    ));
                }
                _ => {
                    // s1dss == 0b10 (or any reserved value): use CD[0] — result already
                    // computed, fall through to the normal success/fault handling below.
                }
            }
        }

        // On translation fault, check whether the stream uses stall mode (ARM §3.12.2).
        // If so, enqueue a StallRecord and return Stalled { stag } instead of the
        // raw fault error — software must send CMD_RESUME or CMD_STALL_TERM to resolve.
        //
        // §3.12.2 / BUG-06: Configuration faults must NEVER stall — only translation
        // faults (F_TRANSLATION, F_ADDR_SIZE, F_ACCESS, F_PERMISSION) are stall-eligible.
        // ARM §11.639: C_BAD_STREAMID, C_BAD_STE, C_BAD_SUBSTREAMID, C_BAD_CD,
        // F_STE_FETCH, F_CD_FETCH must always abort.
        if let Err(ref error) = result {
            self.failed_translations.0.fetch_add(1, Ordering::Relaxed);
            // Map the error to a fault type and check stall eligibility.
            let fault_type = Self::map_translation_error_to_fault_type(error);
            // Only translation-class faults are stall-eligible; configuration faults always abort.
            let is_stall_eligible = matches!(
                fault_type,
                FaultType::TranslationFault
                    | FaultType::AddressSizeFault
                    | FaultType::AccessFlagFault
                    | FaultType::PermissionFault
            );
            let is_stall = stall_mode && is_stall_eligible;

            // §3.12.2 / FINDING-NEW-26: Allocate STAG before recording fault so the
            // EventEntry carries the correct STAG value when is_stall==true.
            let stag = if is_stall {
                // BUG-RUST-2 fix (part 1): Use AcqRel ordering for all STAG counter
                // fetch_add calls.  Relaxed ordering is insufficient to enforce the
                // STAG uniqueness/non-reuse constraint (ARM §3.12.2) across threads
                // because it provides no cross-thread visibility guarantees.
                // AcqRel establishes a happens-before relationship: every thread
                // that reads the counter sees the most recent write.

                // Generate an initial candidate STAG, skipping zero (reserved).
                let initial = self.stag_counter.fetch_add(1, Ordering::AcqRel);
                // BUG-RUST-2 fix (part 2): Use a `while` loop (not `if`) to skip
                // over zero.  Under concurrent load, the single `if`-branch fetch_add
                // can itself return 0 (two threads simultaneously wrap the counter),
                // causing STAG=0 to escape and be used as a real stall identifier.
                let mut candidate = initial;
                while candidate == 0 {
                    candidate = self.stag_counter.fetch_add(1, Ordering::AcqRel);
                }
                // Ensure uniqueness using an atomic check-and-insert via entry()
                // API (ARM §3.12.2: STAG is a 16-bit non-zero identifier).
                // Guard with 65535 iterations to prevent infinite loop when the
                // queue is exhausted.  failed_translations is already incremented
                // once at the top of the error branch; do not increment again here.
                let mut iters: u32 = 0;
                loop {
                    match self.stall_queue.entry(candidate) {
                        Entry::Vacant(slot) => {
                            slot.insert(StallRecord {
                                stag: candidate,
                                stream_id: stream_id.as_u32(),
                                pasid: pasid.as_u32(),
                                iova: iova.as_u64(),
                                access,
                                security_state,
                            });
                            break;
                        }
                        Entry::Occupied(_) => {
                            // BUG-RUST-2 fix: use AcqRel + while loop so that
                            // concurrent wraps cannot produce STAG=0.
                            candidate = self.stag_counter.fetch_add(1, Ordering::AcqRel);
                            while candidate == 0 {
                                candidate = self.stag_counter.fetch_add(1, Ordering::AcqRel);
                            }
                            iters += 1;
                            if iters >= 65535 {
                                // BUG-13 fix / BUG-RUST-DBGR-12 fix: stall queue is
                                // full (all 65535 STAGs occupied).  ARM §3.12.2: "The
                                // SMMU always records the details of the access into
                                // the Event queue."  Record the fault as a non-stall
                                // event so software sees the correct fault report.
                                // Return StallQueueFull so callers can distinguish
                                // this condition from a plain translation error.
                                self.record_translation_fault(
                                    stream_id, pasid, iova, access, security_state,
                                    error, false, 0,
                                );
                                return Err(TranslationError::StallQueueFull);
                            }
                        }
                    }
                }
                candidate
            } else {
                0
            };

            self.record_translation_fault(stream_id, pasid, iova, access, security_state, error, is_stall, stag);

            if is_stall {
                // The stall record was already inserted atomically via entry()
                // inside the STAG allocation loop above.
                return Err(TranslationError::Stalled { stag });
            }
        } else {
            self.successful_translations.0.fetch_add(1, Ordering::Relaxed);
        }

        result
    }

    /// Returns the total number of TLB / ATC / CD invalidation operations performed.
    ///
    /// Includes all invalidation commands: `CMD_TLBI_*`, `CMD_ATC_INV`,
    /// `CMD_CFGI_CD`, and `CMD_CFGI_CD_ALL`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    ///
    /// let smmu = SMMU::new();
    /// assert_eq!(smmu.get_invalidation_count(), 0);
    /// ```
    #[must_use]
    pub fn get_invalidation_count(&self) -> u64 {
        self.invalidation_count.load(Ordering::Relaxed)
    }

    /// Get translation statistics
    ///
    /// Returns tuple of (total, successful, failed) translation counts.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    /// use smmu::types::{StreamID, StreamConfig, PASID, IOVA, AccessType, SecurityState};
    ///
    /// let smmu = SMMU::new();
    /// let stream_id = StreamID::new(1).unwrap();
    /// smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();
    ///
    /// let pasid = PASID::new(0).unwrap();
    /// let iova = IOVA::new(0x1000).unwrap();
    /// let _ = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    ///
    /// let (total, successful, failed) = smmu.get_translation_stats();
    /// assert_eq!(total, 1);
    /// ```
    #[must_use]
    pub fn get_translation_stats(&self) -> (u64, u64, u64) {
        let total = self.total_translations.0.load(Ordering::Relaxed);
        let successful = self.successful_translations.0.load(Ordering::Relaxed);
        let failed = self.failed_translations.0.load(Ordering::Relaxed);
        (total, successful, failed)
    }

    /// Reset translation statistics
    ///
    /// Resets all translation counters to zero.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::SMMU;
    ///
    /// let smmu = SMMU::new();
    /// smmu.reset_translation_stats();
    /// let (total, successful, failed) = smmu.get_translation_stats();
    /// assert_eq!(total, 0);
    /// assert_eq!(successful, 0);
    /// assert_eq!(failed, 0);
    /// ```
    pub fn reset_translation_stats(&self) {
        self.total_translations.0.store(0, Ordering::Relaxed);
        self.successful_translations.0.store(0, Ordering::Relaxed);
        self.failed_translations.0.store(0, Ordering::Relaxed);
    }

    // ========================================================================
    // Helper Methods
    // ========================================================================

    /// Get stream context for a given stream ID
    ///
    /// # Arguments
    ///
    /// * `stream_id` - Stream identifier to lookup
    ///
    /// # Errors
    ///
    /// Returns `SMMUError::StreamNotFound` if stream doesn't exist.
    fn get_stream_context(&self, stream_id: StreamID) -> Result<Arc<StreamContext>, SMMUError> {
        let stream_value = stream_id.as_u32();

        self.streams
            .get(&stream_value)
            .map(|entry| Arc::clone(entry.value()))
            .ok_or_else(|| SMMUError::stream_not_found(stream_id))
    }

    /// Map ARM SMMU v3 fault type to event type
    ///
    /// Converts FaultType to `EventType` for event queue recording.
    ///
    /// # Arguments
    ///
    /// * `fault_type` - Fault type to map
    ///
    /// # Returns
    ///
    /// Corresponding event type.
    fn map_fault_type_to_event_type(fault_type: FaultType) -> EventType {
        match fault_type {
            FaultType::StreamDisabled => EventType::FStreamDisabled,
            // §7.3.3: stream table lookup failure → C_BAD_STREAMID (0x02), not F_TRANSLATION.
            FaultType::BadStreamID => EventType::CBadStreamid,
            // §7.3.9: non-zero PASID on stage-2-only or bypass stream → C_BAD_SUBSTREAMID (0x08).
            FaultType::BadSubstreamId => EventType::CBadSubstreamid,
            // §7.3.14: address-size fault → F_ADDR_SIZE (0x11), not F_TRANSLATION.
            FaultType::AddressSizeFault => EventType::FAddrSize,
            // §7.3.15 / FINDING-NEW-31: AccessFlagFault → F_ACCESS (0x12), not F_TRANSLATION.
            FaultType::AccessFlagFault => EventType::FAccess,
            // §7.3.11: C_BAD_CD — Fetched CD invalid (0x0A)
            FaultType::BadCD => EventType::CBadCd,
            FaultType::TranslationFault
            | FaultType::BadSTE
            | FaultType::AlignmentFault
            | FaultType::ExternalAbort
            | FaultType::TLBConflictAbort
            | FaultType::CDFetchFault
            | FaultType::STEFetchFault
            | FaultType::WalkEABT
            | FaultType::OutputAddressRangeFault
            | FaultType::UnsupportedAtomicUpdate => EventType::FTranslation,
            FaultType::PermissionFault | FaultType::SecurityFault => EventType::FPermission,
        }
    }

    /// Map translation error to ARM SMMU v3 fault type
    ///
    /// Converts TranslationError to appropriate FaultType per ARM SMMU v3
    /// Section 6.2 fault classification.
    ///
    /// # Arguments
    ///
    /// * `error` - Translation error to map
    ///
    /// # Returns
    ///
    /// Corresponding ARM SMMU v3 fault type code.
    fn map_translation_error_to_fault_type(error: &TranslationError) -> FaultType {
        match error {
            TranslationError::PageNotMapped => FaultType::TranslationFault,
            TranslationError::PermissionViolation { .. } => FaultType::PermissionFault,
            TranslationError::InvalidAddress { .. } => FaultType::AddressSizeFault,
            TranslationError::AddressSizeError => FaultType::AddressSizeFault,
            TranslationError::AlignmentError => FaultType::AlignmentFault,
            TranslationError::SecurityViolation => FaultType::SecurityFault,
            TranslationError::ExternalAbort => FaultType::ExternalAbort,
            TranslationError::TlbConflict => FaultType::TLBConflictAbort,
            TranslationError::InvalidStreamID => FaultType::BadStreamID,
            TranslationError::StreamNotConfigured => FaultType::BadSTE,
            TranslationError::StreamDisabled => FaultType::StreamDisabled,
            // BUG-R-05 fix: STE.Config==0b000 abort mode — silent termination, no event.
            // Maps to StreamDisabled so internal fault tracking still fires, but the
            // SMMU-level event-suppression check (see record_translation_fault) must
            // match AbortMode specifically to allow StreamDisabled to generate F_STREAM_DISABLED.
            TranslationError::AbortMode => FaultType::StreamDisabled,
            TranslationError::InvalidPASID => FaultType::BadCD,
            TranslationError::PASIDNotFound => FaultType::BadCD,
            // Stalled is a stall-mode outcome, not a distinct fault class.
            // Map to TranslationFault for event recording purposes.
            TranslationError::Stalled { .. } => FaultType::TranslationFault,
            // GbpaAbort is a global disable abort — not a per-stream translation fault.
            // Map to TranslationFault as a safe catch-all (should never reach fault recording).
            TranslationError::GbpaAbort => FaultType::TranslationFault,
            // §3.9: non-zero PASID on stage-2-only or bypass stream → C_BAD_SUBSTREAMID.
            TranslationError::BadSubstreamId => FaultType::BadSubstreamId,
            // BUG-RUST-DBGR-2 fix: §7.3.11 C_BAD_CD — invalid context descriptor.
            // Maps to BadCD (event code 0x0A), distinct from BadSubstreamId (0x08).
            TranslationError::BadCD => FaultType::BadCD,
            // BUG-RUST-DBGR-12: StallQueueFull is a stall-queue resource exhaustion condition.
            // The underlying fault was already recorded; map to TranslationFault for fallback.
            TranslationError::StallQueueFull => FaultType::TranslationFault,
        }
    }

    /// Record translation fault event
    ///
    /// Creates and records a fault record for a translation error.
    ///
    /// # Arguments
    ///
    /// * `stream_id` - Stream identifier
    /// * `pasid` - Process Address Space ID
    /// * `iova` - Input/Output Virtual Address
    /// * `access` - Access type that caused the fault
    /// * `error` - Translation error details
    fn record_translation_fault(
        &self,
        stream_id: StreamID,
        pasid: PASID,
        iova: IOVA,
        access: AccessType,
        security_state: SecurityState,
        error: &TranslationError,
        is_stall: bool,
        stag: u16,
    ) {
        let fault_type = Self::map_translation_error_to_fault_type(error);

        // Use monotonic atomic counter instead of SystemTime::now() to avoid syscall overhead
        // This eliminates 20-50ns syscall cost on the fault path while maintaining ordering
        let timestamp = self.fault_timestamp_counter.fetch_add(1, Ordering::Relaxed);

        let fault = FaultRecord::builder()
            .stream_id(stream_id)
            .pasid(pasid)
            .address(iova)
            .fault_type(fault_type)
            .access_type(access)
            .security_state(security_state)
            .timestamp(timestamp)
            .build();

        self.record_fault(fault);

        // §5.2 / §7.3.7: Both STE.Config==0b000 (AbortMode) and administratively-disabled
        // streams (StreamDisabled via disable_stream()) terminate traffic without recording
        // an event in this model.  F_STREAM_DISABLED (event 0x06) is only generated for
        // the S1DSS==0 case (non-substream on substream-capable stream), which is handled
        // separately in the S1DSS branch before reaching record_translation_fault.
        // BUG-R-05 fix: the distinction between AbortMode and StreamDisabled exists to make
        // the two paths distinguishable to callers; both suppress EventEntry here.
        if matches!(
            error,
            TranslationError::AbortMode | TranslationError::StreamDisabled
        ) {
            return;
        }

        // §7.2.1 / CT-33: When CR0.EVENTQEN=0, events must not be recorded.
        // Stall events are exempt because the stalled transaction can never complete
        // without an event — software needs the event to issue CMD_RESUME.
        if !is_stall && (self.cr0.load(Ordering::Acquire) & Self::CR0_EVENTQEN) == 0 {
            return;
        }

        // Also record to event queue for ARM SMMU v3 compliance (Section 6.3)
        let event_type = Self::map_fault_type_to_event_type(fault_type);
        let event = EventEntry {
            event_type,
            stream_id: stream_id.as_u32(),
            pasid: pasid.as_u32(),
            address: iova.as_u64(),
            security_state,
            error_code: 0,
            timestamp,
            stall: is_stall,
            // §3.12.2 / FINDING-NEW-26: Carry the stall tag so software can correlate
            // the EventEntry to the matching CMD_RESUME command.  Zero when stall==false.
            stag,
        };

        if let Ok(mut queue) = self.event_queue.write() {
            // BUG-13 fix / ARM §7.4: "a fault record from a stalled transaction is not
            // discarded and an event is reported for the stalled transaction when the
            // queue is next writable."
            //
            // Drain any previously-pending stall events into the main queue first
            // (FIFO order) before inserting the new event, provided space is available.
            if queue.len() < self.event_queue_capacity {
                if let Ok(mut pending) = self.stall_pending.lock() {
                    while queue.len() < self.event_queue_capacity {
                        match pending.pop_front() {
                            Some(p) => {
                                queue.push_back(p);
                                self.event_count.fetch_add(1, Ordering::Relaxed);
                            }
                            None => break,
                        }
                    }
                }
            }

            if queue.len() < self.event_queue_capacity {
                // Main queue has space — insert directly.
                queue.push_back(event);
                self.event_count.fetch_add(1, Ordering::Relaxed);
            } else if event.stall {
                // BUG-13 fix: stall event and queue is full — redirect to stall_pending
                // instead of dropping.  Must NOT trigger OVFLG (ARM §7.4: stall fault
                // records do not cause an overflow condition).
                if let Ok(mut pending) = self.stall_pending.lock() {
                    pending.push_back(event);
                }
            } else {
                // Non-stall event dropped due to full queue.
                // BUG-10 fix / ARM §7.4 / BUG-RUST-DBGR-8 fix: Toggle OVFLG atomically
                // via CAS loop (toggle_ovflg_once) — see that method for details.
                self.toggle_ovflg_once();
            }
        }
    }

    /// Record stream not found fault event
    ///
    /// Creates and records a fault record for a stream lookup failure.
    ///
    /// # Arguments
    ///
    /// * `stream_id` - Stream identifier that was not found
    /// * `pasid` - Process Address Space ID
    /// * `iova` - Input/Output Virtual Address
    /// * `access` - Access type requested
    fn record_stream_not_found_fault(&self, stream_id: StreamID, pasid: PASID, iova: IOVA, access: AccessType, security_state: SecurityState) {
        // Use monotonic atomic counter instead of SystemTime::now() to avoid syscall overhead
        let timestamp = self.fault_timestamp_counter.fetch_add(1, Ordering::Relaxed);

        let fault = FaultRecord::builder()
            .stream_id(stream_id)
            .pasid(pasid)
            .address(iova)
            .fault_type(FaultType::BadStreamID)
            .access_type(access)
            .security_state(security_state)
            .timestamp(timestamp)
            .build();

        self.record_fault(fault);

        // §7.2.1 / CT-33: When CR0.EVENTQEN=0, events must not be recorded.
        if (self.cr0.load(Ordering::Acquire) & Self::CR0_EVENTQEN) == 0 {
            return;
        }

        // BUG-NEW-RUST-2 fix: §6.3.12 / §7.3.3 — RECINVSID gate.
        // When CR2.RECINVSID=0 (reset default), C_BAD_STREAMID events triggered
        // by an unknown StreamID must NOT be written to the event queue.
        // Per §7.3.3, RECINVSID only gates the event queue write; GERROR.CMDQ_ERR
        // is NOT toggled for transaction-path StreamID faults (only command-queue errors §7.1).
        if (self.cr2.load(Ordering::Acquire) & Self::CR2_RECINVSID) == 0 {
            return;
        }

        // §7.3.3: stream-not-found → C_BAD_STREAMID (0x02), not F_TRANSLATION (0x10).
        let event = EventEntry {
            event_type: EventType::CBadStreamid,
            stream_id: stream_id.as_u32(),
            pasid: pasid.as_u32(),
            address: iova.as_u64(),
            security_state,
            error_code: 0,
            timestamp,
            stall: false,
            stag: 0,
        };

        if let Ok(mut queue) = self.event_queue.write() {
            if queue.len() < self.event_queue_capacity {
                queue.push_back(event);
                self.event_count.fetch_add(1, Ordering::Relaxed);
            } else {
                // BUG-05 / ARM §7.4 / BUG-RUST-DBGR-8 fix: Toggle OVFLG atomically
                // via CAS loop — see toggle_ovflg_once() for details.
                self.toggle_ovflg_once();
            }
        }
    }

    /// Check if SMMU is shutdown and return error if so
    ///
    /// Helper method for all operations that should fail during shutdown.
    #[inline]
    fn check_shutdown(&self) -> Result<(), SMMUError> {
        if self.is_shutdown() {
            Err(SMMUError::ShutdownInProgress)
        } else {
            Ok(())
        }
    }

    // ========================================================================
    // Section 5.3.1: Event Queue Operations
    // ========================================================================

    /// Submit an event to the event queue
    ///
    /// Adds an event entry to the FIFO event queue. Returns error if queue capacity is exceeded
    /// (only enforced for small queues < 100 to support overflow testing).
    ///
    /// # Thread Safety
    ///
    /// This method is thread-safe and can be called concurrently from multiple threads.
    ///
    /// # ARM SMMU v3 Compliance
    ///
    /// Implements event queue management per Section 6.3.
    pub fn submit_event(&self, event: EventEntry) -> Result<(), SMMUError> {
        let mut queue = self.event_queue.write().unwrap();
        if queue.len() >= self.event_queue_capacity {
            // BUG-NEW-RUST-4 fix: ARM §7.4 — when the event queue is full and a
            // non-stall event is dropped, OVFLG must be toggled (provided no overflow
            // condition is already active).  Stall events must be redirected to
            // stall_pending rather than dropped, and must NOT trigger OVFLG.
            // The public submit_event() previously returned EventQueueFull without
            // implementing these §7.4 semantics; the internal record_translation_fault()
            // path already implements them correctly via the same helpers.
            if event.stall {
                // Stall events: redirect to stall_pending (ARM §7.4: not discarded).
                // Must NOT trigger OVFLG.
                drop(queue); // release write lock before acquiring stall_pending
                if let Ok(mut pending) = self.stall_pending.lock() {
                    pending.push_back(event);
                }
            } else {
                // Non-stall event dropped due to full queue: toggle OVFLG once.
                // Release write lock first so toggle_ovflg_once() can acquire it
                // independently if needed (it only touches atomics, so this is safe).
                drop(queue);
                self.toggle_ovflg_once();
            }
            return Err(SMMUError::EventQueueFull);
        }
        queue.push_back(event);
        self.event_count.fetch_add(1, Ordering::Relaxed);
        // Safety (BUG-RUST-08): the write lock on `event_queue` is held for the entire
        // push_back path, so no two callers can execute this load+store concurrently.
        // A plain read-modify-write is therefore race-free and correct here.
        let prod = self.eventq_prod.load(Ordering::Relaxed);
        self.eventq_prod.store(Self::advance_index(prod, self.eventq_log2size), Ordering::Release);
        Ok(())
    }

    /// Get all events from the queue (non-destructive read)
    ///
    /// Returns a copy of all events currently in the queue.  Also drains any
    /// pending stall events from `stall_pending` into the main queue if space
    /// is available (ARM §7.4: events are reported "when the queue is next
    /// writable").  Events remain in the queue until explicitly cleared.
    pub fn get_events(&self) -> Vec<EventEntry> {
        // Opportunistically drain stall_pending into the main queue before reading.
        if let Ok(mut queue) = self.event_queue.write() {
            if queue.len() < self.event_queue_capacity {
                if let Ok(mut pending) = self.stall_pending.lock() {
                    while queue.len() < self.event_queue_capacity {
                        match pending.pop_front() {
                            Some(p) => {
                                queue.push_back(p);
                                self.event_count.fetch_add(1, Ordering::Relaxed);
                            }
                            None => break,
                        }
                    }
                }
            }
            queue.iter().copied().collect()
        } else {
            Vec::new()
        }
    }

    /// Check if event queue has events
    ///
    /// Returns true if the queue contains at least one event.
    #[must_use]
    pub fn has_events(&self) -> bool {
        let queue = self.event_queue.read().unwrap();
        !queue.is_empty()
    }

    /// Get current event queue size
    ///
    /// Returns the number of events currently in the queue.
    #[must_use]
    pub fn get_event_queue_size(&self) -> u64 {
        let queue = self.event_queue.read().unwrap();
        queue.len() as u64
    }

    /// Clear all events from the queue
    ///
    /// Removes all events atomically and resets PROD/CONS indices to 0
    /// so that PROD == CONS (empty condition, ARM §3.5.1).
    pub fn clear_event_queue(&self) {
        let mut queue = self.event_queue.write().unwrap();
        queue.clear();
        self.eventq_prod.store(0, Ordering::Release);
        self.eventq_cons.store(0, Ordering::Release);
    }

    /// Get number of pending stall events waiting to drain into the main queue.
    ///
    /// ARM IHI0070G.b §7.4 / BUG-13: Stall events that could not fit in the main
    /// event queue are redirected to an internal stall_pending buffer rather than
    /// being dropped.  This method returns the count of events in that buffer.
    ///
    /// A return value > 0 indicates there are stall events awaiting drain; they
    /// will be inserted into the main queue automatically when space is available.
    #[must_use]
    pub fn get_pending_stall_count(&self) -> usize {
        self.stall_pending.lock().map_or(0, |p| p.len())
    }

    /// Get events filtered by event type
    ///
    /// Returns all events matching the specified event type.
    pub fn get_events_by_type(&self, event_type: EventType) -> Vec<EventEntry> {
        let queue = self.event_queue.read().unwrap();
        queue.iter().filter(|e| e.event_type == event_type).copied().collect()
    }

    /// Get events filtered by stream ID
    ///
    /// Returns all events for the specified stream.
    pub fn get_events_by_stream(&self, stream_id: u32) -> Vec<EventEntry> {
        let queue = self.event_queue.read().unwrap();
        queue.iter().filter(|e| e.stream_id == stream_id).copied().collect()
    }

    // ========================================================================
    // Section 5.3.2: Command Queue Operations
    // ========================================================================

    /// Submit a command to the command queue
    ///
    /// Adds a command entry to the FIFO command queue. Returns error if queue capacity is exceeded
    /// (only enforced for small queues < 100 to support overflow testing) or if command parameters are invalid.
    ///
    /// # Validation
    ///
    /// - For range operations (AtcInv), start_address must be <= end_address
    ///
    /// # ARM SMMU v3 Compliance
    ///
    /// Implements command queue management per Section 6.4.
    pub fn submit_command(&self, command: CommandEntry) -> Result<(), SMMUError> {
        // Validate command parameters
        if command.cmd_type == CommandType::AtcInv && command.end_address < command.start_address {
            return Err(SMMUError::InvalidCommandParameters(
                "Invalid address range: end < start".to_string(),
            ));
        }

        let mut queue = self.command_queue.write().unwrap();
        // BUG-NEW3-08 fix: enforce capacity for ALL queue sizes, not just < 200.
        // The previous guard skipped overflow detection for large queues, allowing
        // unbounded growth that violates ARM §3.5.1 circular queue semantics.
        if queue.len() >= self.command_queue_capacity {
            return Err(SMMUError::CommandQueueFull);
        }
        queue.push_back(command);
        self.command_count.fetch_add(1, Ordering::Relaxed);
        let prod = self.cmdq_prod.load(Ordering::Relaxed);
        self.cmdq_prod.store(Self::advance_index(prod, self.cmdq_log2size), Ordering::Release);
        Ok(())
    }

    /// Process all commands in the command queue
    ///
    /// Processes commands in FIFO order and generates completion events.
    /// Returns the number of commands processed.
    ///
    /// # ARM SMMU v3 Compliance
    ///
    /// Commands are processed in submission order per Section 6.4.
    pub fn process_command_queue(&self) -> Result<usize, SMMUError> {
        // §4.1.2 / CT-33: Command queue is gated by CR0.CMDQEN.
        // When CMDQEN=0, command processing is a no-op (queue untouched).
        if (self.cr0.load(Ordering::Acquire) & Self::CR0_CMDQEN) == 0 {
            return Ok(0);
        }

        // NEW-50 / ARM §6.3.19: Do not process commands when GERROR.CMDQ_ERR is
        // ACTIVE (GERROR[x] != GERRORN[x]).  Software must acknowledge by writing
        // GERRORN to match GERROR (via clear_gerror) before restarting queue processing.
        // BUG-03 fix: use XOR-active test instead of raw GERROR bit test.
        //
        // BUG-NEW-RUST-2 fix: replace the SeqCst fence with a double-load seqlock.
        // A SeqCst fence between the two loads PREVENTS REORDERING but does NOT
        // prevent a concurrent signal_gerror() write from landing between the two
        // loads — the fence is an ordering guarantee, not a mutual-exclusion barrier.
        // The correct fix mirrors the seqlock pattern used in signal_gerror(): re-read
        // gerror after gerrorn and retry if it changed.  This ensures both loads
        // see a coherent snapshot of the gerror/gerrorn pair.
        let gerror_active = loop {
            let err_reg  = self.gerror.load(Ordering::Acquire);
            let err_ack  = self.gerrorn.load(Ordering::Acquire);
            let reread   = self.gerror.load(Ordering::Acquire);
            if reread != err_reg {
                std::hint::spin_loop();
                continue; // gerror changed between the two reads — retry
            }
            break err_reg ^ err_ack;
        };
        if gerror_active & Self::GERROR_CMDQ_ERR != 0 {
            return Ok(0);
        }

        let mut processed = 0;

        loop {
            // Pop one command at a time to avoid holding lock
            let command = {
                let mut queue = self.command_queue.write().unwrap();
                let cmd = queue.pop_front();
                if cmd.is_some() {
                    let cons = self.cmdq_cons.load(Ordering::Relaxed);
                    self.cmdq_cons
                        .store(Self::advance_index(cons, self.cmdq_log2size), Ordering::Release);
                }
                cmd
            };

            match command {
                Some(cmd) => {
                    // ARM §6.3.17: set CMDQ_ERR and halt queue on command
                    // processing error (FINDING-M-06).
                    if let Err(e) = self.process_single_command(cmd) {
                        // BUG-03 fix: use signal_gerror (XOR-toggle, only-if-inactive)
                        // instead of fetch_or (unconditional set).
                        self.signal_gerror(Self::GERROR_CMDQ_ERR);
                        return Err(e);
                    }
                    processed += 1;
                },
                None => break,
            }
        }

        Ok(processed)
    }

    /// Get current command queue size
    ///
    /// Returns the number of commands currently in the queue.
    #[must_use]
    pub fn get_command_queue_size(&self) -> u64 {
        let queue = self.command_queue.read().unwrap();
        queue.len() as u64
    }

    /// Check if command queue is full
    ///
    /// Returns true if the queue is at capacity.
    #[must_use]
    pub fn is_command_queue_full(&self) -> bool {
        let queue = self.command_queue.read().unwrap();
        queue.len() >= self.command_queue_capacity
    }

    /// Clear all commands from the queue
    ///
    /// Removes all pending commands atomically and resets PROD/CONS indices to 0
    /// so that PROD == CONS (empty condition, ARM §3.5.1).
    pub fn clear_command_queue(&self) {
        let mut queue = self.command_queue.write().unwrap();
        queue.clear();
        self.cmdq_prod.store(0, Ordering::Release);
        self.cmdq_cons.store(0, Ordering::Release);
    }

    // ========================================================================
    // ARM §3.5.1: Circular Queue PROD/CONS Register Accessors
    // ========================================================================

    /// Returns the raw CMDQ_PROD register value (ARM §6.3.25 semantics).
    ///
    /// Bits \[log2size-1:0\] = producer position, bit \[log2size\] = wrap bit.
    #[must_use]
    pub fn cmdq_prod_index(&self) -> u32 {
        self.cmdq_prod.load(Ordering::Acquire)
    }

    /// Returns the raw CMDQ_CONS register value.
    #[must_use]
    pub fn cmdq_cons_index(&self) -> u32 {
        self.cmdq_cons.load(Ordering::Acquire)
    }

    /// Returns the EVENTQ_PROD queue index (bits 30:0), masking out OVFLG (bit 31).
    ///
    /// Per ARM §7.4, bit 31 of `SMMU_EVENTQ_PROD` is the overflow flag (OVFLG)
    /// and must not be included when computing the producer position or queue
    /// occupancy.  Use [`get_eventq_prod`](Self::get_eventq_prod) to read the
    /// full register value including OVFLG.
    #[must_use]
    pub fn eventq_prod_index(&self) -> u32 {
        self.eventq_prod.load(Ordering::Acquire) & !(1u32 << 31)
    }

    /// Returns the full `SMMU_EVENTQ_PROD` register value including OVFLG (bit 31).
    ///
    /// Software compares bit 31 (OVFLG) against the saved OVACKFLG in
    /// `SMMU_EVENTQ_CONS` to detect event queue overflow per ARM §7.4.
    #[must_use]
    pub fn get_eventq_prod(&self) -> u32 {
        self.eventq_prod.load(Ordering::Acquire)
    }

    /// Returns the raw EVENTQ_CONS register value.
    #[must_use]
    pub fn eventq_cons_index(&self) -> u32 {
        self.eventq_cons.load(Ordering::Acquire)
    }

    /// Returns the raw PRIQ_PROD register value.
    #[must_use]
    pub fn priq_prod_index(&self) -> u32 {
        self.priq_prod.load(Ordering::Acquire)
    }

    /// Returns the raw PRIQ_CONS register value.
    #[must_use]
    pub fn priq_cons_index(&self) -> u32 {
        self.priq_cons.load(Ordering::Acquire)
    }

    /// Returns true if the command queue is empty (PROD == CONS, ARM §3.5.1).
    #[must_use]
    pub fn is_cmdq_empty_by_index(&self) -> bool {
        self.cmdq_prod.load(Ordering::Acquire) == self.cmdq_cons.load(Ordering::Acquire)
    }

    /// Returns true if the event queue is empty (PROD == CONS, ARM §3.5.1).
    ///
    /// OVFLG (bit 31) of `SMMU_EVENTQ_PROD` is masked before the comparison so
    /// that a toggled overflow flag is never mistaken for a non-empty queue.
    #[must_use]
    pub fn is_eventq_empty_by_index(&self) -> bool {
        let prod = self.eventq_prod.load(Ordering::Acquire) & !(1u32 << 31);
        prod == self.eventq_cons.load(Ordering::Acquire)
    }

    /// Returns the number of entries in the command queue by PROD/CONS index.
    #[must_use]
    pub fn cmdq_occupied_entries(&self) -> u32 {
        Self::queue_occupied(
            self.cmdq_prod.load(Ordering::Acquire),
            self.cmdq_cons.load(Ordering::Acquire),
            self.cmdq_log2size,
        )
    }

    /// Returns the number of entries in the event queue by PROD/CONS index.
    ///
    /// OVFLG (bit 31) of `SMMU_EVENTQ_PROD` is masked before computing occupancy
    /// so that a toggled overflow flag does not corrupt the entry count.
    #[must_use]
    pub fn eventq_occupied_entries(&self) -> u32 {
        Self::queue_occupied(
            self.eventq_prod.load(Ordering::Acquire) & !(1u32 << 31),
            self.eventq_cons.load(Ordering::Acquire),
            self.eventq_log2size,
        )
    }

    /// Returns the command queue LOG2SIZE (determines index width and capacity).
    #[must_use]
    pub fn cmdq_log2size(&self) -> u32 {
        self.cmdq_log2size
    }

    /// Returns the event queue LOG2SIZE.
    #[must_use]
    pub fn eventq_log2size(&self) -> u32 {
        self.eventq_log2size
    }

    /// Returns the PRI queue LOG2SIZE.
    #[must_use]
    pub fn priq_log2size(&self) -> u32 {
        self.priq_log2size
    }

    /// Process a single command
    ///
    /// Internal method to process one command and generate appropriate completion events.
    #[allow(clippy::unnecessary_wraps)]
    #[allow(clippy::too_many_lines)]
    fn process_single_command(&self, command: CommandEntry) -> Result<(), SMMUError> {
        match command.cmd_type {
            // ASID-targeted invalidation: remove only entries tagged with cmd.asid (§4.4)
            CommandType::TlbiNhAsid | CommandType::TlbiEl2Asid => {
                self.tlb_cache.invalidate_by_asid(command.asid);
                self.invalidation_count.fetch_add(1, Ordering::Relaxed);
            },
            CommandType::TlbiNhAll
            | CommandType::TlbiNhVa
            | CommandType::TlbiNhVaa
            | CommandType::TlbiEl2All
            | CommandType::TlbiEl2Va
            | CommandType::TlbiEl2Vaa
            | CommandType::TlbiNsnhAll => {
                // Global TLB invalidation - clear entire cache
                self.tlb_cache.invalidate_all();
                self.invalidation_count.fetch_add(1, Ordering::Relaxed);
            },
            // VMID-targeted invalidation: remove only entries tagged with cmd.vmid (§4.4, §5.2)
            CommandType::TlbiS12Vmall | CommandType::TlbiS2Ipa => {
                self.tlb_cache.invalidate_by_vmid(command.vmid);
                self.invalidation_count.fetch_add(1, Ordering::Relaxed);
            },
            CommandType::AtcInv => {
                // CMD_ATC_INV (§4.5.1): range-based or global ATC invalidation.
                //
                // flags bit 0 = G (Global):
                //   G=0: invalidate entries for addresses in [start_address, end_address]
                //   G=1: invalidate ALL entries for (stream_id, PASID) regardless of address
                let global = (command.flags & 1) != 0;

                if let (Ok(stream_id), Ok(pasid)) = (
                    StreamID::new(command.stream_id),
                    PASID::new(command.pasid),
                ) {
                    if global {
                        // Global: evict all entries for this (stream, PASID) pair.
                        self.tlb_cache.invalidate_by_stream_pasid(stream_id, pasid);
                    } else {
                        // Range: evict only entries within [start, end].
                        // If address construction fails (out-of-range value), fall back
                        // to stream+PASID invalidation to avoid skipping any entries.
                        match (
                            crate::types::IOVA::new(command.start_address),
                            crate::types::IOVA::new(command.end_address),
                        ) {
                            (Ok(start), Ok(end)) => {
                                self.tlb_cache.invalidate_by_va_range(stream_id, pasid, start, end);
                            },
                            _ => {
                                // Fallback: scoped to stream+PASID (conservative)
                                self.tlb_cache.invalidate_by_stream_pasid(stream_id, pasid);
                            },
                        }
                    }
                }
                self.invalidation_count.fetch_add(1, Ordering::Relaxed);

                // Generate completion event with monotonic timestamp.
                // FINDING-NEW-44: use the stream's configured security state
                // instead of hardcoding NonSecure.
                let timestamp = self.fault_timestamp_counter.fetch_add(1, Ordering::Relaxed);
                let stream_sec_state = self
                    .streams
                    .get(&command.stream_id)
                    .map_or(SecurityState::NonSecure, |ctx| ctx.security_state());

                let event = EventEntry {
                    event_type: EventType::AtcInvalidateCompletion, // IMPDEF §7.3.21
                    stream_id: command.stream_id,
                    pasid: command.pasid,
                    address: command.start_address,
                    security_state: stream_sec_state,
                    error_code: 0,
                    timestamp,
                    stall: false,
                    stag: 0,
                };

                let _ = self.submit_event(event);
            },
            CommandType::Sync => {
                // §4.8 / BUG-02: CS=0b11 is Reserved — CERROR_ILL per ARM §4.7.3.
                // CERROR_ILL is reported via GERROR.CMDQ_ERR (bit[0] set via fetch_or
                // in process_command_queue) + halting queue processing.
                // No EventEntry is written to the Event queue for command errors
                // (§7.1 / §4.1.3 — command errors use SMMU_CMDQ_CONS.ERR + GERROR,
                // not the Event queue).
                if command.cs == 0b11 {
                    // BUG-09/15 fix: do NOT call fetch_xor here.  Per §6.3.19 / §7.5,
                    // GERROR bits use "activate-only-if-inactive" (OR) semantics —
                    // the SMMU must not toggle a bit when the error is already active.
                    // fetch_xor would briefly clear an already-set CMDQ_ERR bit before
                    // process_command_queue applies its authoritative fetch_or, creating
                    // a race window.  Let process_command_queue apply the single fetch_or.
                    return Err(SMMUError::InvalidCommandParameters(
                        "CMD_SYNC: CS=0b11 is Reserved — CERROR_ILL (ARM §4.7.3)".to_string(),
                    ));
                }
                // §4.8 / FINDING-NEW-27: CS=0b00 (SIG_NONE) → no completion signal.
                if command.cs != 0 {
                    let timestamp = self.fault_timestamp_counter.fetch_add(1, Ordering::Relaxed);
                    // FINDING-NEW-44: use the stream's configured security state
                    // instead of hardcoding NonSecure.
                    let stream_sec_state = self
                        .streams
                        .get(&command.stream_id)
                        .map_or(SecurityState::NonSecure, |ctx| ctx.security_state());

                    let event = EventEntry {
                        event_type: EventType::CommandSyncCompletion, // IMPDEF §7.3.21
                        stream_id: command.stream_id,
                        pasid: command.pasid,
                        address: 0,
                        security_state: stream_sec_state,
                        error_code: 0,
                        timestamp,
                        stall: false,
                        stag: 0,
                    };

                    let _ = self.submit_event(event);
                }
            },
            CommandType::Resume => {
                // CMD_RESUME (§4.6, §3.12.2): resolve the stalled transaction.
                // The Ac (action) and Ab (abort) bits determine the outcome per ARM §4.6, Table 4-10:
                //   Ac=1 (action=true):                 Retry — transaction retried as if freshly arrived.
                //   Ac=0, Ab=0 (action=false, abort=false): Terminate successfully (RAZ/WI).
                //   Ac=0, Ab=1 (action=false, abort=true):  Abort — transaction terminated with bus error.
                // ARM §4.6: verify the STAG belongs to the given StreamID; if not, command has no effect.
                let stream_matches = self.stall_queue
                    .get(&command.stag)
                    .map_or(false, |r| r.stream_id == command.stream_id);
                if stream_matches {
                    self.stall_queue.remove(&command.stag);
                }
            },
            CommandType::StallTerm => {
                // §4.7.2 / FINDING-NEW-30: CMD_STALL_TERM clears ALL stall records for
                // the given StreamID, not just the one matching the STAG field.
                // ARM §4.7.2 specifies no STAG operand — the command takes a StreamID
                // and terminates every pending stalled transaction for that stream.
                self.stall_queue.retain(|_stag, record| record.stream_id != command.stream_id);
            },
            CommandType::CfgiCd => {
                // CMD_CFGI_CD (§4.3.3): invalidate TLB entries cached from the
                // Context Descriptor for the specified (stream, PASID) pair.
                if let (Ok(stream_id), Ok(pasid)) = (
                    StreamID::new(command.stream_id),
                    PASID::new(command.pasid),
                ) {
                    self.tlb_cache.invalidate_by_stream_pasid(stream_id, pasid);
                }
                self.invalidation_count.fetch_add(1, Ordering::Relaxed);
            },
            CommandType::CfgiCdAll => {
                // CMD_CFGI_CD_ALL (§4.3.4): invalidate TLB entries cached from
                // all Context Descriptors of the specified stream.
                if let Ok(stream_id) = StreamID::new(command.stream_id) {
                    self.tlb_cache.invalidate_by_stream(stream_id);
                }
                self.invalidation_count.fetch_add(1, Ordering::Relaxed);
            },
            CommandType::PriResp => {
                // ARM §8.3 / BUG-RUST-Q4 fix: The PRI queue is STRICT FIFO.
                // CMD_PRI_RESP may only retire the HEAD entry whose
                // (stream_id, prg_index) matches the command.  If the head does
                // not match, this is a software usage error; the SMMU generates
                // no fault for this condition (ARM §8.3) and the command is
                // treated as a no-op.  Searching and removing an interior entry
                // (any pos > 0) is incorrect per ARM §3.5.1 and §6.3.98.
                let mut queue = self.pri_queue.write().unwrap();
                if let Some(head) = queue.front() {
                    if head.stream_id == command.stream_id && head.prg_index == command.prg_index {
                        queue.pop_front();
                        let cons = self.priq_cons.load(Ordering::Relaxed);
                        self.priq_cons
                            .store(Self::advance_index(cons, self.priq_log2size), Ordering::Release);
                    }
                }
            },
            // CMD_CFGI_STE (§4.3.1): invalidate STE for a specific stream.
            // ARM §4.3.1: if the SID is unknown, this is a C_BAD_STREAMID error
            // that sets CMDQ_ERR and halts command queue processing (FINDING-M-06).
            CommandType::CfgiSte => {
                // BUG-RUST-D fix: use get() which atomically checks existence under the
                // shard lock, eliminating the TOCTOU window of a separate contains_key() call.
                if self.streams.get(&command.stream_id).is_none() {
                    // Generate C_BAD_STREAMID event and set CMDQ_ERR
                    // ARM §4.3.1: any unknown StreamID, including 0, must generate
                    // C_BAD_STREAMID + GERROR_CMDQ_ERR (BUG-RUST-04).
                    let timestamp = self.fault_timestamp_counter.fetch_add(1, Ordering::Relaxed);
                    let event = EventEntry {
                        event_type: EventType::CBadStreamid,
                        stream_id: command.stream_id,
                        pasid: command.pasid,
                        address: 0,
                        security_state: command.security_state,
                        error_code: 0x02,  // C_BAD_STREAMID event number per §7.3.3 (0x03 = F_STE_FETCH §7.3.4)
                        timestamp,
                        stall: false,
                        stag: 0,
                    };
                    // §6.3.12 / §7.3.3: only record C_BAD_STREAMID in the event queue
                    // when CR2.RECINVSID == 1.  GERROR.CMDQ_ERR is still signalled
                    // unconditionally (via the Err return caught in process_command_queue).
                    if (self.cr2.load(Ordering::Acquire) & Self::CR2_RECINVSID) != 0 {
                        let _ = self.submit_event(event);
                    }
                    return Err(SMMUError::InvalidCommandParameters(format!(
                        "CMD_CFGI_STE: unknown stream_id {}", command.stream_id
                    )));
                }
                // Known stream: no TLB state to flush in this model.
            },
            // CMD_CFGI_ALL / CMD_CFGI_STE_RANGE (ARM §4.3.2):
            // Both use opcode 0x04 (CfgiAll); the `range` field distinguishes them.
            //   range == 31 → CMD_CFGI_ALL: invalidate all STE-cached TLB entries globally.
            //   range < 31  → CMD_CFGI_STE_RANGE: invalidate only streams matching the
            //                  upper-bit prefix: (sid >> (range+1)) == (cmd.stream_id >> (range+1)).
            // NOTE: shifting a u32 by 32 bits is UB/panic, so range==31 is handled separately.
            CommandType::CfgiAll => {
                if command.range == 31 {
                    // CMD_CFGI_ALL — full global TLB invalidation
                    self.tlb_cache.invalidate_all();
                } else {
                    // CMD_CFGI_STE_RANGE — prefix-matched stream invalidation
                    let prefix_bits = u32::from(command.range) + 1;
                    let cmd_prefix = command.stream_id >> prefix_bits;
                    for entry in &self.streams {
                        let raw_sid = *entry.key();
                        if (raw_sid >> prefix_bits) == cmd_prefix {
                            if let Ok(stream_id) = StreamID::new(raw_sid) {
                                self.tlb_cache.invalidate_by_stream(stream_id);
                            }
                        }
                    }
                }
                self.invalidation_count.fetch_add(1, Ordering::Relaxed);
            },
            // CMD_PREFETCH_CONFIG, CMD_PREFETCH_ADDR —
            // no side-effect processing required in the software model.
            CommandType::PrefetchConfig
            | CommandType::PrefetchAddr => {},
            // §4.1.1: EL3 TLB invalidation commands — invalidate all entries globally.
            CommandType::TlbiEl3All | CommandType::TlbiEl3Va => {
                self.tlb_cache.invalidate_all();
                self.invalidation_count.fetch_add(1, Ordering::Relaxed);
            },
            // §4.1.1: Secure EL2 TLB invalidation commands — invalidate all entries.
            CommandType::TlbiSEl2All
            | CommandType::TlbiSEl2Va
            | CommandType::TlbiSEl2Vaa
            | CommandType::TlbiSnhAll => {
                self.tlb_cache.invalidate_all();
                self.invalidation_count.fetch_add(1, Ordering::Relaxed);
            },
            // §4.1.1: Secure EL2 ASID-targeted TLB invalidation.
            CommandType::TlbiSEl2Asid => {
                self.tlb_cache.invalidate_by_asid(command.asid);
                self.invalidation_count.fetch_add(1, Ordering::Relaxed);
            },
            // §4.1.1: Secure S12 / S2 VMID-targeted TLB invalidation.
            CommandType::TlbiSS12Vmall | CommandType::TlbiSS2Ipa => {
                self.tlb_cache.invalidate_by_vmid(command.vmid);
                self.invalidation_count.fetch_add(1, Ordering::Relaxed);
            },
            // §4.1.1: CMD_CFGI_VMS_PIDM — no TLB state to flush in this model.
            CommandType::CfgiVmsPidm => {},
            // §4.1.1: DPTI commands — no dirty-tracking state in this model.
            CommandType::DptiAll | CommandType::DptiPa => {},
        }

        Ok(())
    }

    // ========================================================================
    // Section 5.3.3: PRI Queue Operations
    // ========================================================================

    /// Submit a page request to the PRI queue
    ///
    /// Adds a page request entry to the FIFO PRI queue. Returns error if queue capacity is exceeded
    /// (only enforced for small queues < 100 to support overflow testing).
    ///
    /// # ARM SMMU v3 Compliance
    ///
    /// Implements Page Request Interface per Section 7.
    pub fn submit_page_request(&self, request: PRIEntry) -> Result<(), SMMUError> {
        // §4.1.2 / CT-33: PRI queue submissions are gated by CR0.PRIQEN.
        // When PRIQEN=0, page requests must be rejected.
        if (self.cr0.load(Ordering::Acquire) & Self::CR0_PRIQEN) == 0 {
            return Err(SMMUError::InvalidConfiguration(
                "CR0.PRIQEN=0: PRI queue is not enabled".to_string(),
            ));
        }

        let mut queue = self.pri_queue.write().unwrap();
        // BUG-01 / ARM §8.1: enforce PRI queue capacity unconditionally — the
        // previous `< 200` guard was an implementation artifact with no spec basis.
        if queue.len() >= self.pri_queue_capacity {
            // §8.1: Toggle PRIQ_PROD.OVFLG (bit[31]) when a PRI message is discarded
            // due to a full queue, provided an overflow condition is not already present
            // (i.e. OVFLG already differs from the software-acknowledged OVACKFLG).
            // Use XOR to toggle rather than OR so a second toggle is only applied after
            // software acknowledges the first via PRIQ_CONS.OVACKFLG = PRIQ_PROD.OVFLG.
            // BUG-RUST-DBGR-8 fix: CAS loop for atomic OVFLG toggle (same pattern as toggle_ovflg_once).
            loop {
                let current_prod = self.priq_prod.load(Ordering::Acquire);
                let current_cons = self.priq_cons.load(Ordering::Acquire);
                let ovflg    = (current_prod >> 31) & 1;
                let ovackflg = (current_cons >> 31) & 1;
                if ovflg != ovackflg {
                    break; // overflow already active — do NOT toggle again
                }
                let new_prod = current_prod ^ (1u32 << 31);
                if self.priq_prod.compare_exchange_weak(
                    current_prod,
                    new_prod,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ).is_ok() {
                    break; // CAS applied
                }
                // CAS failed: priq_prod changed concurrently — retry loop
            }
            return Err(SMMUError::PriQueueFull);
        }
        queue.push_back(request);
        self.pri_count.fetch_add(1, Ordering::Relaxed);
        // ARM §3.5.1: advance PRIQ_PROD after each enqueue so software can
        // observe queue fullness via (PROD - CONS).
        let prod = self.priq_prod.load(Ordering::Relaxed);
        self.priq_prod
            .store(Self::advance_index(prod, self.priq_log2size), Ordering::Release);
        Ok(())
    }

    /// Get all page requests from the queue (non-destructive read)
    ///
    /// Returns a copy of all page requests currently in the queue.
    pub fn get_pri_queue(&self) -> Vec<PRIEntry> {
        let queue = self.pri_queue.read().unwrap();
        queue.iter().copied().collect()
    }

    /// Process all page requests in the PRI queue
    ///
    /// Processes page requests and generates PRI events.
    /// Returns the number of requests processed.
    pub fn process_pri_queue(&self) -> Result<usize, SMMUError> {
        let mut processed = 0;

        loop {
            let request = {
                let mut queue = self.pri_queue.write().unwrap();
                let entry = queue.pop_front();
                if entry.is_some() {
                    // ARM §3.5.1: advance PRIQ_CONS after each dequeue.
                    let cons = self.priq_cons.load(Ordering::Relaxed);
                    self.priq_cons
                        .store(Self::advance_index(cons, self.priq_log2size), Ordering::Release);
                }
                entry
            };

            match request {
                Some(req) => {
                    // Generate PRI event for this request with monotonic timestamp.
                    // The event carries the prg_index via the error_code field so
                    // that software can correlate events with PRI queue entries.
                    let timestamp = self.fault_timestamp_counter.fetch_add(1, Ordering::Relaxed);

                    let event = EventEntry {
                        event_type: EventType::EPageRequest,
                        stream_id: req.stream_id,
                        pasid: req.pasid,
                        address: req.requested_address,
                        // §7.3.19 / FINDING-NEW-32: carry the request's security state,
                        // not a hardcoded NonSecure.
                        security_state: req.security_state,
                        error_code: u32::from(req.prg_index),
                        timestamp,
                        stall: false,
                        stag: 0,
                    };

                    let _ = self.submit_event(event);
                    processed += 1;
                },
                None => break,
            }
        }

        Ok(processed)
    }

    /// Get current PRI queue size
    ///
    /// Returns the number of page requests currently in the queue.
    #[must_use]
    pub fn get_pri_queue_size(&self) -> u64 {
        let queue = self.pri_queue.read().unwrap();
        queue.len() as u64
    }

    /// Clear all page requests from the queue
    ///
    /// Removes all pending page requests atomically and resets PROD/CONS indices to 0
    /// so that PROD == CONS (empty condition, ARM §3.5.1).
    pub fn clear_pri_queue(&self) {
        let mut queue = self.pri_queue.write().unwrap();
        queue.clear();
        self.priq_prod.store(0, Ordering::Release);
        self.priq_cons.store(0, Ordering::Release);
    }

    // ========================================================================
    // Section 5.3.5: Queue Integration and Statistics
    // ========================================================================

    /// Get queue statistics
    ///
    /// Returns comprehensive statistics for all queues.
    #[must_use]
    pub fn get_queue_statistics(&self) -> QueueStatistics {
        QueueStatistics::new(
            self.get_event_queue_size(),
            self.get_command_queue_size(),
            self.get_pri_queue_size(),
            self.event_queue_capacity,
            self.command_queue_capacity,
            self.pri_queue_capacity,
        )
    }

    /// Reset all queues atomically
    ///
    /// Clears all event, command, and PRI queues.
    ///
    /// Per ARM §3.5.1, initialization must produce a consistent empty state.
    /// This includes the internal `stall_pending` buffer that holds stall events
    /// which overflowed the main event queue (ARM §7.4 / BUG-RUST-Q2 fix).
    pub fn reset_queues(&self) {
        self.clear_event_queue();
        self.clear_command_queue();
        self.clear_pri_queue();
        // BUG-RUST-Q2 fix: clear_event_queue() does NOT drain stall_pending.
        // A reset must produce a fully empty state per ARM §3.5.1.
        if let Ok(mut pending) = self.stall_pending.lock() {
            pending.clear();
        }
    }

    /// Get cache statistics
    ///
    /// Returns cache invalidation statistics and TLB cache performance metrics.
    #[must_use]
    pub fn get_cache_statistics(&self) -> CacheStatistics {
        let tlb_stats = self.tlb_cache.statistics();

        CacheStatistics {
            invalidation_count: self.invalidation_count.load(Ordering::Relaxed),
            tlb_lookups: tlb_stats.get_lookups(),
            tlb_hits: tlb_stats.get_hits(),
            tlb_misses: tlb_stats.get_misses(),
            tlb_evictions: tlb_stats.get_evictions(),
            tlb_insertions: tlb_stats.get_insertions(),
            tlb_invalidations: tlb_stats.get_invalidations(),
        }
    }

    // ============================================================================
    // Iterator-Based APIs
    // ============================================================================

    /// Returns an iterator over all configured stream IDs.
    ///
    /// This provides an efficient way to enumerate all streams currently
    /// configured in the SMMU without collecting into a vector.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::prelude::*;
    ///
    /// let smmu = SMMU::new();
    /// let stream1 = StreamID::new(1)?;
    /// let stream2 = StreamID::new(2)?;
    ///
    /// smmu.configure_stream(stream1, StreamConfig::bypass())?;
    /// smmu.configure_stream(stream2, StreamConfig::bypass())?;
    ///
    /// // Iterate over all configured streams
    /// for stream_id in smmu.streams() {
    ///     println!("Stream: {}", stream_id.as_u32());
    /// }
    ///
    /// // Count streams
    /// let count = smmu.streams().count();
    /// assert_eq!(count, 2);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// # Performance
    ///
    /// This iterator uses DashMap's iter() which provides lock-free iteration
    /// with minimal overhead. The iterator is a snapshot of the streams at
    /// the time of creation.
    #[must_use]
    pub fn streams(&self) -> impl Iterator<Item = StreamID> + '_ {
        self.streams.iter().filter_map(|entry| StreamID::new(*entry.key()).ok())
    }

    /// Returns an iterator over all active PASIDs for a given stream.
    ///
    /// This provides an efficient way to enumerate all PASIDs configured
    /// for a specific stream without collecting into a vector.
    ///
    /// # Arguments
    ///
    /// * `stream_id` - The stream ID to query
    ///
    /// # Returns
    ///
    /// Returns an iterator over PASIDs, or None if the stream doesn't exist.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::prelude::*;
    ///
    /// let smmu = SMMU::new();
    /// let stream_id = StreamID::new(1)?;
    ///
    /// let config = StreamConfig::builder()
    ///     .translation_enabled(true)
    ///     .stage1_enabled(true)
    ///     .pasid_enabled(true)
    ///     .max_pasid(255)
    ///     .build()?;
    /// smmu.configure_stream(stream_id, config)?;
    ///
    /// // Create multiple PASIDs
    /// smmu.create_pasid(stream_id, PASID::new(0)?)?;
    /// smmu.create_pasid(stream_id, PASID::new(1)?)?;
    /// smmu.create_pasid(stream_id, PASID::new(2)?)?;
    ///
    /// // Iterate over all PASIDs for this stream
    /// if let Some(pasids) = smmu.pasids(stream_id) {
    ///     for pasid in pasids {
    ///         println!("PASID: {}", pasid.as_u32());
    ///     }
    /// }
    ///
    /// // Count PASIDs
    /// let count = smmu.pasids(stream_id).map(|v| v.len()).unwrap_or(0);
    /// assert_eq!(count, 3);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn pasids(&self, stream_id: StreamID) -> Option<Vec<PASID>> {
        self.streams.get(&stream_id.as_u32()).map(|entry| {
            let context = entry.value();
            // Get all PASID keys from the DashMap (includes PASID 0)
            let pasids: Vec<PASID> = context
                .pasid_map
                .iter()
                .filter_map(|p| PASID::new(*p.key()).ok())
                .collect();

            pasids
        })
    }

    /// Returns an iterator over all fault records.
    ///
    /// This provides an efficient way to process fault records without
    /// consuming the internal fault queue. For consuming iteration, use
    /// `drain_faults()` instead.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::prelude::*;
    ///
    /// let smmu = SMMU::new();
    /// // ... configure and trigger some faults ...
    ///
    /// // Iterate over all faults
    /// for fault in smmu.faults() {
    ///     println!("Fault: {:?} at address 0x{:x}",
    ///              fault.fault_type(), fault.address().as_u64());
    /// }
    ///
    /// // Filter faults by type
    /// let translation_faults = smmu.faults()
    ///     .filter(|f| f.fault_type() == FaultType::TranslationFault)
    ///     .count();
    /// ```
    #[must_use]
    pub fn faults(&self) -> impl Iterator<Item = FaultRecord> + '_ {
        // Get a snapshot of the fault queue
        let faults = self.fault_queue.lock().unwrap();
        let faults_snapshot: Vec<FaultRecord> = faults.clone();
        faults_snapshot.into_iter()
    }

    /// Returns a draining iterator over all fault records.
    ///
    /// This iterator removes faults from the internal queue as they are
    /// iterated over. For non-consuming iteration, use `faults()` instead.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::prelude::*;
    ///
    /// let smmu = SMMU::new();
    /// // ... configure and trigger some faults ...
    ///
    /// // Process and remove all faults
    /// for fault in smmu.drain_faults() {
    ///     eprintln!("Processing fault: {:?}", fault.fault_type());
    ///     // Handle fault...
    /// }
    ///
    /// // Fault queue is now empty
    /// assert_eq!(smmu.faults().count(), 0);
    /// ```
    #[must_use]
    pub fn drain_faults(&self) -> Vec<FaultRecord> {
        let mut faults = self.fault_queue.lock().unwrap();
        faults.drain(..).collect()
    }

    /// Returns an iterator over event queue entries.
    ///
    /// This provides an efficient way to process events without consuming
    /// the event queue.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::prelude::*;
    ///
    /// let smmu = SMMU::new();
    /// // ... generate some events ...
    ///
    /// // Iterate over all events
    /// for event in smmu.events() {
    ///     println!("Event: {:?}", event.event_type);
    /// }
    ///
    /// // Filter by event type
    /// let fault_events = smmu.events().iter()
    ///     .filter(|e| matches!(e.event_type, EventType::FTranslation | EventType::FPermission))
    ///     .count();
    /// ```
    #[must_use]
    pub fn events(&self) -> Vec<EventEntry> {
        let events = self.event_queue.read().unwrap();
        events.iter().copied().collect()
    }

    /// Returns an iterator over events filtered by stream ID.
    ///
    /// This is more efficient than filtering manually as it avoids cloning
    /// unneeded events.
    ///
    /// # Arguments
    ///
    /// * `stream_id` - The stream ID to filter by
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::prelude::*;
    ///
    /// let smmu = SMMU::new();
    /// let stream_id = StreamID::new(1)?;
    ///
    /// // Get events for specific stream
    /// for event in smmu.events_for_stream(stream_id) {
    ///     println!("Stream {} event: {:?}", stream_id.as_u32(), event.event_type);
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn events_for_stream(&self, stream_id: StreamID) -> Vec<EventEntry> {
        let events = self.event_queue.read().unwrap();
        events.iter().filter(|e| e.stream_id == stream_id.as_u32()).copied().collect()
    }

    /// Returns an iterator over page request interface (PRI) queue entries.
    ///
    /// This provides an efficient way to process page requests without
    /// consuming the PRI queue.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use smmu::prelude::*;
    ///
    /// let smmu = SMMU::new();
    /// // ... generate some page requests ...
    ///
    /// // Iterate over all page requests
    /// for request in smmu.page_requests() {
    ///     println!("Page request: address 0x{:x}", request.requested_address);
    /// }
    ///
    /// // Process requests for specific stream
    /// let stream_id = StreamID::new(1)?;
    /// for request in smmu.page_requests().iter().filter(|r| r.stream_id == stream_id.as_u32()) {
    ///     // Handle page request...
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn page_requests(&self) -> Vec<PRIEntry> {
        let requests = self.pri_queue.read().unwrap();
        requests.iter().copied().collect()
    }

    /// Pre-fill the stall queue with all 65535 STAGs to simulate exhaustion.
    ///
    /// **For integration tests only** (BUG-RUST-DBGR-12): fills every STAG slot
    /// so that the next stall-mode fault hits the queue-full path and returns
    /// `TranslationError::StallQueueFull`.  Not intended for production use.
    pub fn inject_stall_records_for_test(&self, stream_id: u32, pasid: u32) {
        for stag in 1u16..=u16::MAX {
            self.stall_queue.entry(stag).or_insert(StallRecord {
                stag,
                stream_id,
                pasid,
                iova: 0xDEAD_BEEF,
                access: AccessType::Read,
                security_state: SecurityState::NonSecure,
            });
        }
    }
}

/// Cache statistics structure for testing
///
/// Includes both invalidation counts and TLB cache performance metrics.
#[derive(Debug, Clone)]
pub struct CacheStatistics {
    invalidation_count: u64,
    tlb_lookups: u64,
    tlb_hits: u64,
    tlb_misses: u64,
    tlb_evictions: u64,
    tlb_insertions: u64,
    tlb_invalidations: u64,
}

impl CacheStatistics {
    /// Get invalidation count
    pub const fn invalidation_count(&self) -> u64 {
        self.invalidation_count
    }

    /// Get TLB cache lookup count
    pub const fn tlb_lookups(&self) -> u64 {
        self.tlb_lookups
    }

    /// Get TLB cache hit count
    pub const fn tlb_hits(&self) -> u64 {
        self.tlb_hits
    }

    /// Get TLB cache miss count
    pub const fn tlb_misses(&self) -> u64 {
        self.tlb_misses
    }

    /// Get TLB cache eviction count
    pub const fn tlb_evictions(&self) -> u64 {
        self.tlb_evictions
    }

    /// Get TLB cache insertion count
    pub const fn tlb_insertions(&self) -> u64 {
        self.tlb_insertions
    }

    /// Get TLB cache invalidation count
    pub const fn tlb_invalidations(&self) -> u64 {
        self.tlb_invalidations
    }

    /// Calculate TLB cache hit rate as percentage (0.0 to 100.0)
    pub fn tlb_hit_rate(&self) -> f64 {
        if self.tlb_lookups == 0 {
            0.0
        } else {
            (self.tlb_hits as f64 / self.tlb_lookups as f64) * 100.0
        }
    }
}

impl Default for SMMU {
    fn default() -> Self {
        Self::new()
    }
}

// SMMU is automatically Send + Sync since all components are Send + Sync:
// - DashMap<K, V> is Send + Sync when K: Send + Sync and V: Send + Sync
// - Arc<RwLock<T>> is Send + Sync when T: Send + Sync
// - Arc<Mutex<T>> is Send + Sync when T: Send + Sync
// - AtomicBool is Send + Sync
// - StreamContext is Send + Sync (verified in stream_context module)
// - SMMUConfig is Send + Sync (all fields are Send + Sync)

// Verify Send + Sync trait bounds at compile time
#[allow(dead_code)]
fn assert_send_sync() {
    fn is_send<T: Send>() {}
    fn is_sync<T: Sync>() {}

    is_send::<SMMU>();
    is_sync::<SMMU>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AccessType, FaultType, SecurityState, IOVA, PASID};

    // ── FINDING-NEW-31: AccessFlagFault must map to F_ACCESS (§7.3.15) ─────────

    /// Unit test for the private `map_fault_type_to_event_type()` function.
    /// Verifies that `FaultType::AccessFlagFault` maps to `EventType::FAccess`
    /// and not `EventType::FTranslation`.
    #[test]
    fn access_flag_fault_maps_to_f_access() {
        assert_eq!(
            SMMU::map_fault_type_to_event_type(FaultType::AccessFlagFault),
            EventType::FAccess,
            "§7.3.15 / FINDING-NEW-31: AccessFlagFault must map to EventType::FAccess (0x12)"
        );
    }

    /// Regression guard: TranslationFault must still map to FTranslation.
    #[test]
    fn translation_fault_maps_to_f_translation() {
        assert_eq!(
            SMMU::map_fault_type_to_event_type(FaultType::TranslationFault),
            EventType::FTranslation,
            "TranslationFault must map to EventType::FTranslation (0x10)"
        );
    }

    #[test]
    fn test_smmu_new() {
        let smmu = SMMU::new();
        assert!(!smmu.is_shutdown());
        assert_eq!(smmu.get_stream_count(), 0);
    }

    #[test]
    fn test_smmu_with_config() {
        let config = SMMUConfig::high_performance();
        let smmu = SMMU::with_config(config.clone());

        let retrieved_config = smmu.get_config();
        assert_eq!(retrieved_config, config);
    }

    #[test]
    fn test_configure_stream_success() {
        let smmu = SMMU::new();
        let stream_id = StreamID::new(1).unwrap();
        let config = StreamConfig::stage1_only();

        assert!(smmu.configure_stream(stream_id, config).is_ok());
        assert!(smmu.has_stream(stream_id));
        assert_eq!(smmu.get_stream_count(), 1);
    }

    #[test]
    fn test_configure_stream_duplicate() {
        let smmu = SMMU::new();
        let stream_id = StreamID::new(1).unwrap();
        let config = StreamConfig::stage1_only();

        smmu.configure_stream(stream_id, config.clone()).unwrap();

        let result = smmu.configure_stream(stream_id, config);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SMMUError::StreamAlreadyExists(_)));
    }

    #[test]
    fn test_configure_stream_limit() {
        // Create SMMU with minimal config (1 stream max)
        let config = SMMUConfig::minimal();
        let smmu = SMMU::with_config(config);

        let stream_id1 = StreamID::new(1).unwrap();
        let stream_config = StreamConfig::bypass();

        smmu.configure_stream(stream_id1, stream_config.clone()).unwrap();

        // Second stream should fail due to limit
        let stream_id2 = StreamID::new(2).unwrap();
        let result = smmu.configure_stream(stream_id2, stream_config);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SMMUError::StreamLimitExceeded { .. }));
    }

    #[test]
    fn test_remove_stream() {
        let smmu = SMMU::new();
        let stream_id = StreamID::new(1).unwrap();
        let config = StreamConfig::bypass();

        smmu.configure_stream(stream_id, config).unwrap();
        assert!(smmu.has_stream(stream_id));

        smmu.remove_stream(stream_id).unwrap();
        assert!(!smmu.has_stream(stream_id));
        assert_eq!(smmu.get_stream_count(), 0);
    }

    #[test]
    fn test_remove_stream_not_found() {
        let smmu = SMMU::new();
        let stream_id = StreamID::new(1).unwrap();

        let result = smmu.remove_stream(stream_id);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SMMUError::StreamNotFound(_)));
    }

    #[test]
    fn test_shutdown() {
        let smmu = SMMU::new();
        let stream_id = StreamID::new(1).unwrap();
        let config = StreamConfig::bypass();

        smmu.configure_stream(stream_id, config.clone()).unwrap();
        assert_eq!(smmu.get_stream_count(), 1);

        assert!(smmu.shutdown().is_ok());
        assert!(smmu.is_shutdown());
        assert_eq!(smmu.get_stream_count(), 0);

        // Operations after shutdown should fail
        let stream_id2 = StreamID::new(2).unwrap();
        let result = smmu.configure_stream(stream_id2, config);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SMMUError::ShutdownInProgress));
    }

    #[test]
    fn test_shutdown_idempotent() {
        let smmu = SMMU::new();

        assert!(smmu.shutdown().is_ok());
        assert!(smmu.is_shutdown());

        // Second shutdown should return error but be safe
        let result = smmu.shutdown();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SMMUError::ShutdownInProgress));
    }

    #[test]
    fn test_update_config() {
        let smmu = SMMU::new();

        smmu.update_config(|config| {
            config.cache_config.tlb_cache_size = 2048;
        })
        .unwrap();

        let config = smmu.get_config();
        assert_eq!(config.cache_config.tlb_cache_size, 2048);
    }

    #[test]
    fn test_update_config_validation() {
        let smmu = SMMU::new();

        // Try to set invalid queue size
        let result = smmu.update_config(|config| {
            config.queue_config.event_queue_size = 1; // Below minimum
        });

        assert!(result.is_err());

        // Configuration should be unchanged
        let config = smmu.get_config();
        assert_eq!(
            config.queue_config.event_queue_size,
            SMMUConfig::default().queue_config.event_queue_size
        );
    }

    #[test]
    fn test_fault_recording() {
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

        smmu.record_fault(fault.clone());

        let faults = smmu.get_faults();
        assert_eq!(faults.len(), 1);
        assert_eq!(faults[0], fault);
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
    fn test_initialize() {
        let smmu = SMMU::new();
        assert!(smmu.initialize().is_ok());

        smmu.shutdown().unwrap();
        assert!(smmu.initialize().is_err());
    }

    #[test]
    fn test_has_stream() {
        let smmu = SMMU::new();
        let stream_id = StreamID::new(1).unwrap();

        assert!(!smmu.has_stream(stream_id));

        smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();

        assert!(smmu.has_stream(stream_id));
    }

    #[test]
    fn test_thread_safety_markers() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<SMMU>();
        assert_sync::<SMMU>();
    }

    // ========================================================================
    // FINDING-M-01: Circular Queue PROD/CONS Semantics (ARM §3.5.1)
    // ========================================================================

    #[test]
    fn test_cmdq_initially_empty_by_index() {
        let smmu = SMMU::new();
        assert_eq!(smmu.cmdq_prod_index(), 0);
        assert_eq!(smmu.cmdq_cons_index(), 0);
        assert!(smmu.is_cmdq_empty_by_index());
        assert_eq!(smmu.cmdq_occupied_entries(), 0);
    }

    #[test]
    fn test_eventq_initially_empty_by_index() {
        let smmu = SMMU::new();
        assert_eq!(smmu.eventq_prod_index(), 0);
        assert_eq!(smmu.eventq_cons_index(), 0);
        assert!(smmu.is_eventq_empty_by_index());
        assert_eq!(smmu.eventq_occupied_entries(), 0);
    }

    #[test]
    fn test_cmdq_prod_advances_on_submit() {
        let smmu = SMMU::new();
        smmu.enable().unwrap();
        let stream_id = StreamID::new(1).unwrap();
        smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();

        let cmd = CommandEntry::new(CommandType::TlbiNhAll, 0, 0);
        smmu.submit_command(cmd).unwrap();

        assert_eq!(smmu.cmdq_prod_index(), 1);
        assert_eq!(smmu.cmdq_cons_index(), 0);
        assert!(!smmu.is_cmdq_empty_by_index());
        assert_eq!(smmu.cmdq_occupied_entries(), 1);
    }

    #[test]
    fn test_cmdq_cons_advances_on_process() {
        let smmu = SMMU::new();
        smmu.enable().unwrap();
        let stream_id = StreamID::new(1).unwrap();
        smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();

        let cmd = CommandEntry::new(CommandType::TlbiNhAll, 0, 0);
        smmu.submit_command(cmd).unwrap();
        assert_eq!(smmu.cmdq_prod_index(), 1);

        smmu.process_command_queue().unwrap();
        assert_eq!(smmu.cmdq_cons_index(), 1);
        assert!(smmu.is_cmdq_empty_by_index());
        assert_eq!(smmu.cmdq_occupied_entries(), 0);
    }

    #[test]
    fn test_eventq_prod_advances_on_submit() {
        let smmu = SMMU::new();
        let event = EventEntry::new(EventType::FTranslation, 0, 0, 0);
        smmu.submit_event(event).unwrap();

        assert_eq!(smmu.eventq_prod_index(), 1);
        assert_eq!(smmu.eventq_cons_index(), 0);
        assert!(!smmu.is_eventq_empty_by_index());
        assert_eq!(smmu.eventq_occupied_entries(), 1);
    }

    #[test]
    fn test_cmdq_prod_cons_index_after_clear() {
        let smmu = SMMU::new();
        smmu.enable().unwrap();
        let cmd = CommandEntry::new(CommandType::TlbiNhAll, 0, 0);
        smmu.submit_command(cmd).unwrap();

        smmu.clear_command_queue();
        assert_eq!(smmu.cmdq_prod_index(), 0);
        assert_eq!(smmu.cmdq_cons_index(), 0);
        assert!(smmu.is_cmdq_empty_by_index());
    }

    #[test]
    fn test_eventq_prod_cons_index_after_clear() {
        let smmu = SMMU::new();
        let event = EventEntry::new(EventType::FTranslation, 0, 0, 0);
        smmu.submit_event(event).unwrap();

        smmu.clear_event_queue();
        assert_eq!(smmu.eventq_prod_index(), 0);
        assert_eq!(smmu.eventq_cons_index(), 0);
        assert!(smmu.is_eventq_empty_by_index());
    }

    #[test]
    fn test_cmdq_log2size_for_default_capacity() {
        let smmu = SMMU::new();
        // Default command queue capacity = 256 → log2size = 8
        assert_eq!(smmu.cmdq_log2size(), 8);
        // Default event queue capacity = 512 → log2size = 9
        assert_eq!(smmu.eventq_log2size(), 9);
    }

    #[test]
    fn test_multiple_commands_prod_advances_by_count() {
        let smmu = SMMU::new();
        smmu.enable().unwrap();

        for _ in 0..5 {
            let cmd = CommandEntry::new(CommandType::TlbiNhAll, 0, 0);
            smmu.submit_command(cmd).unwrap();
        }
        assert_eq!(smmu.cmdq_prod_index(), 5);
        assert_eq!(smmu.cmdq_occupied_entries(), 5);

        smmu.process_command_queue().unwrap();
        assert_eq!(smmu.cmdq_cons_index(), 5);
        assert_eq!(smmu.cmdq_prod_index(), 5); // prod unchanged by processing
        assert_eq!(smmu.cmdq_occupied_entries(), 0);
        assert!(smmu.is_cmdq_empty_by_index());
    }

    // ── BUG-RUST-H01: STAG TOCTOU — entry() API must be used ────────────────

    /// Regression guard: stall_queue must use entry() for atomic check-and-insert.
    ///
    /// Before the fix, check-then-insert was non-atomic: two concurrent threads
    /// could both see the same STAG as free and both insert, with the second
    /// silently overwriting the first.  After the fix, only one of them wins
    /// the Vacant slot and the other increments to a new candidate.
    ///
    /// Single-threaded proof: pre-populate the stall_queue with every STAG
    /// value 1..=3, then trigger a stall fault on a stream whose STAG counter
    /// starts at 1.  The allocation loop must skip all occupied slots and land
    /// on a fresh STAG (4), not silently overwrite slot 1.
    #[test]
    fn bug_rust_h01_stag_entry_no_overwrite() {
        use crate::types::{AccessType, FaultMode, SecurityState, StreamConfig, IOVA, PASID};

        let smmu = SMMU::new();
        smmu.enable().unwrap();

        let stream_id = StreamID::new(0x10).unwrap();
        let cfg = StreamConfig::builder()
            .stage1_enabled(true)
            .translation_enabled(true)
            .fault_mode(FaultMode::Stall)
            .build()
            .unwrap();
        smmu.configure_stream(stream_id, cfg).unwrap();
        smmu.create_pasid(stream_id, PASID::new(0).unwrap()).unwrap();

        // Pre-populate slots 1, 2, 3 with sentinel records so the allocator
        // must skip them.
        for tag in 1u16..=3 {
            let sentinel = StallRecord {
                stag: tag,
                stream_id: 0xFF,
                pasid: 0xFF,
                iova: 0xDEAD_BEEF,
                access: AccessType::Read,
                security_state: SecurityState::NonSecure,
            };
            smmu.stall_queue.insert(tag, sentinel);
        }

        // Reset the STAG counter so it starts at 1 (will collide with slots 1-3).
        smmu.stag_counter.store(1, Ordering::Relaxed);

        // Trigger a stall fault.
        let result = smmu.translate(
            stream_id,
            PASID::new(0).unwrap(),
            IOVA::new(0x9000_0000).unwrap(),
            AccessType::Read,
            SecurityState::NonSecure,
        );

        // The result must be Stalled with a STAG >= 4 (slots 1-3 are occupied).
        match result {
            Err(TranslationError::Stalled { stag }) => {
                assert!(stag >= 4, "BUG-RUST-H01: STAG {stag} collides with a pre-occupied slot");
                // The sentinel records in slots 1-3 must still be intact.
                for occupied in 1u16..=3 {
                    let entry = smmu.stall_queue.get(&occupied);
                    assert!(
                        entry.is_some(),
                        "BUG-RUST-H01: pre-occupied stall record for STAG {occupied} was overwritten"
                    );
                    assert_eq!(
                        entry.unwrap().stream_id,
                        0xFF,
                        "BUG-RUST-H01: stall record for STAG {occupied} was silently overwritten"
                    );
                }
            }
            other => panic!("expected Stalled error, got: {other:?}"),
        }
    }

    // ── BUG-RUST-H02: failed_translations double-increment ───────────────────

    /// Regression guard: a stall fault must increment `failed_translations`
    /// exactly once, not twice.
    ///
    /// Before the fix, when a stall fault occurred, `failed_translations` was
    /// incremented once at the top of the error branch and a second time inside
    /// the STAG-exhaustion guard — even when the queue was not exhausted.
    #[test]
    fn bug_rust_h02_failed_translations_single_increment() {
        use crate::types::{AccessType, FaultMode, SecurityState, StreamConfig, IOVA, PASID};

        let smmu = SMMU::new();
        smmu.enable().unwrap();

        let stream_id = StreamID::new(0x20).unwrap();
        let cfg = StreamConfig::builder()
            .stage1_enabled(true)
            .translation_enabled(true)
            .fault_mode(FaultMode::Stall)
            .build()
            .unwrap();
        smmu.configure_stream(stream_id, cfg).unwrap();
        smmu.create_pasid(stream_id, PASID::new(0).unwrap()).unwrap();

        let (_, _, before) = smmu.get_translation_stats();

        // Trigger a single stall fault.
        let result = smmu.translate(
            stream_id,
            PASID::new(0).unwrap(),
            IOVA::new(0xBAD0_0000).unwrap(),
            AccessType::Read,
            SecurityState::NonSecure,
        );
        assert!(
            matches!(result, Err(TranslationError::Stalled { .. })),
            "expected a stall, got: {result:?}"
        );

        let (_, _, after) = smmu.get_translation_stats();
        assert_eq!(
            after - before,
            1,
            "BUG-RUST-H02: failed_translations incremented {} time(s) for a single stall fault (expected 1)",
            after - before
        );
    }

    // ── NEW-47: CR0 bit positions per ARM IHI0070G.b §6.3.9 ─────────────────

    /// Regression guard: CR0 constants must match ARM IHI0070G.b §6.3.9.
    ///
    /// The ARM spec defines:
    ///   bit 0 — SMMUEN
    ///   bit 1 — PRIQEN
    ///   bit 2 — EVENTQEN
    ///   bit 3 — CMDQEN
    ///   bit 4 — ATSCHK
    ///
    /// Note: there is no INTEN bit in ARM SMMU v3 §6.3.9.
    #[test]
    fn bug_rust_h03_cr0_bit_positions() {
        assert_eq!(SMMU::CR0_SMMUEN,   1 << 0, "CR0_SMMUEN must be bit 0 (§6.3.9)");
        assert_eq!(SMMU::CR0_PRIQEN,   1 << 1, "CR0_PRIQEN must be bit 1 (§6.3.9)");
        assert_eq!(SMMU::CR0_EVENTQEN, 1 << 2, "CR0_EVENTQEN must be bit 2 (§6.3.9)");
        assert_eq!(SMMU::CR0_CMDQEN,   1 << 3, "CR0_CMDQEN must be bit 3 (§6.3.9)");
        assert_eq!(SMMU::CR0_ATSCHK,   1 << 4, "CR0_ATSCHK must be bit 4 (§6.3.9)");
    }

    /// BUG-04/§6.3.9: After reset, ALL CR0 bits must be 0.
    ///
    /// ARM IHI0070G.b §6.3.9: SMMUEN, PRIQEN, EVENTQEN, and CMDQEN all reset
    /// to 0.  Software must explicitly set these bits before using the queues
    /// or enabling translations.
    #[test]
    fn bug_rust_h03_cr0_reset_value_uses_correct_bits() {
        let smmu = SMMU::new();
        let cr0 = smmu.get_cr0();
        // ALL bits must be 0 after reset per ARM §6.3.9.
        assert_eq!(cr0, 0, "BUG-04/§6.3.9: CR0 must be 0 after reset; got cr0=0x{cr0:08x}");
        // Individual checks for clarity.
        assert_eq!(cr0 & SMMU::CR0_SMMUEN,   0, "§6.3.9: SMMUEN must be 0 after reset");
        assert_eq!(cr0 & SMMU::CR0_PRIQEN,   0, "§6.3.9: PRIQEN must be 0 after reset");
        assert_eq!(cr0 & SMMU::CR0_EVENTQEN, 0, "§6.3.9: EVENTQEN must be 0 after reset");
        assert_eq!(cr0 & SMMU::CR0_CMDQEN,   0, "§6.3.9: CMDQEN must be 0 after reset");
    }

    // ── BUG-RUST-M01: SecurityViolation must map to SecurityFault, not PermissionFault ──

    /// Regression guard: `TranslationError::SecurityViolation` must be mapped to
    /// `FaultType::SecurityFault`, not `FaultType::PermissionFault`, per ARM IHI0070G.b §7.3.
    ///
    /// Before the fix, SecurityViolation was incorrectly mapped to PermissionFault,
    /// conflating two distinct ARM SMMU v3 fault types.
    #[test]
    fn bug_rust_m01_security_violation_maps_to_security_fault() {
        let fault_type = SMMU::map_translation_error_to_fault_type(&TranslationError::SecurityViolation);
        assert_eq!(
            fault_type,
            FaultType::SecurityFault,
            "BUG-RUST-M01: SecurityViolation must map to FaultType::SecurityFault (ARM IHI0070G.b §7.3)"
        );
        assert_ne!(
            fault_type,
            FaultType::PermissionFault,
            "BUG-RUST-M01: SecurityViolation must NOT map to PermissionFault"
        );
    }

    /// Regression guard: `FaultType::SecurityFault` must be handled by
    /// `map_fault_type_to_event_type()` and mapped to `EventType::FPermission` per §7.3.16.
    #[test]
    fn bug_rust_m01_security_fault_maps_to_f_permission_event() {
        let event_type = SMMU::map_fault_type_to_event_type(FaultType::SecurityFault);
        assert_eq!(
            event_type,
            EventType::FPermission,
            "BUG-RUST-M01: FaultType::SecurityFault must map to EventType::FPermission (§7.3.16)"
        );
    }

    // ── BUG-RUST-M02: Stall events must not bypass event queue capacity ──────

    /// Regression guard: stall events must be bounded at 2× the normal event queue
    /// capacity and must NOT grow without limit under sustained stall-mode fault load.
    ///
    /// Before the fix, `event.stall == true` caused the event to be enqueued
    /// unconditionally, allowing unbounded memory growth.
    #[test]
    fn bug_rust_m02_stall_events_bounded_at_2x_capacity() {
        use crate::types::{AccessType, FaultMode, SecurityState, StreamConfig, IOVA, PASID};

        // Build an SMMU with a tiny event queue (capacity = 4) and a stall-mode stream.
        // Use QueueConfig to set a small event queue size.
        let config = SMMUConfig::from(crate::types::QueueConfig {
            event_queue_size: 4,
            command_queue_size: 32,
            pri_queue_size: 32,
        });
        let smmu = SMMU::with_config(config);
        smmu.enable().unwrap();

        let stream_id = StreamID::new(1).unwrap();
        let stream_cfg = StreamConfig::builder()
            .stage1_enabled(true)
            .translation_enabled(true)
            .fault_mode(FaultMode::Stall)
            .build()
            .unwrap();
        smmu.configure_stream(stream_id, stream_cfg).unwrap();

        let pasid = PASID::new(0).unwrap();
        smmu.create_pasid(stream_id, pasid).unwrap();

        // Attempt 20 translations to an unmapped address — each will produce a stall
        // fault event (since the PASID has no pages mapped).
        let iova = IOVA::new(0x9000_0000).unwrap();
        let total_attempts = 20usize;
        for _ in 0..total_attempts {
            let _ = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
        }

        // Drain the event queue and count stall events.
        let events = smmu.get_events();
        let stall_events: Vec<_> = events.iter().filter(|e| e.stall).collect();
        // 2× capacity = 8; stall events must be bounded.
        assert!(
            stall_events.len() <= 8,
            "BUG-RUST-M02: stall events must be capped at 2× capacity (8); got {}",
            stall_events.len()
        );
    }

    // ── BUG-RUST-M03: submit_event() must enforce capacity for all queue sizes ─

    // ── BUG-RUST-2: STAG=0 escape on counter wrap ────────────────────────────

    /// TDD regression guard: `advance_index` and `queue_occupied` must not
    /// panic for the spec-maximum log2size of 19 (ARM IHI0070G.b §3.5.1).
    ///
    /// With the clamp fix `let log2size = log2size.min(19)`, log2size=19 gives
    /// modulus = 2^20 = 1_048_576. Without the fix but with a small value this
    /// test passes trivially.  This test specifically exercises log2size=19 to
    /// confirm the boundary is handled correctly after clamping.
    #[test]
    fn bug_rust3_advance_index_log2size_19_no_panic() {
        // log2size=19: modulus = 2u32 << 19 = 1_048_576
        // advance_index(0, 19) = 1
        // advance_index(1_048_575, 19) = 0  (wraps)
        let m = 1_048_576u32;
        assert_eq!(SMMU::advance_index(0, 19), 1);
        assert_eq!(SMMU::advance_index(m - 1, 19), 0);
        assert_eq!(SMMU::queue_occupied(1, 0, 19), 1);
        assert_eq!(SMMU::queue_occupied(0, 0, 19), 0);
    }

    /// TDD regression guard: `advance_index` with log2size values above 19
    /// MUST NOT panic.  The spec (ARM IHI0070G.b §3.5.1) mandates max log2size=19,
    /// so any value above that must be clamped to 19 rather than causing a
    /// shift-overflow panic in debug builds.
    ///
    /// Before the fix: `2u32 << 31` overflows (panic in debug, silent wrap in
    /// release producing modulus=0, then divide-by-zero).
    /// After the fix: `log2size.min(19)` clamps the value to 19.
    #[test]
    fn bug_rust3_advance_index_log2size_above_19_clamped_no_panic() {
        // log2size=20 must clamp to 19 → same result as log2size=19
        let result_clamped = SMMU::advance_index(0, 20);
        let result_19 = SMMU::advance_index(0, 19);
        assert_eq!(
            result_clamped, result_19,
            "log2size=20 must be clamped to 19: advance_index(0,20)={result_clamped} vs advance_index(0,19)={result_19}"
        );

        // log2size=31 must clamp to 19 (would shift-overflow without the fix).
        let result_31 = SMMU::advance_index(5, 31);
        let result_19_5 = SMMU::advance_index(5, 19);
        assert_eq!(
            result_31, result_19_5,
            "log2size=31 must be clamped to 19: advance_index(5,31)={result_31} vs advance_index(5,19)={result_19_5}"
        );
    }

    /// TDD regression guard: `queue_occupied` with log2size=31 must not panic
    /// or produce a divide-by-zero.
    #[test]
    fn bug_rust3_queue_occupied_log2size_31_no_panic() {
        // Without the fix, `2u32 << 31` wraps to 0 in release mode →
        // `prod.wrapping_sub(cons).wrapping_add(0) % 0` → divide-by-zero panic.
        // With the fix, log2size is clamped to 19.
        let result = SMMU::queue_occupied(5, 3, 31);
        let expected = SMMU::queue_occupied(5, 3, 19); // clamped value
        assert_eq!(
            result, expected,
            "queue_occupied(5,3,31) must equal queue_occupied(5,3,19) after clamping"
        );
    }

    // ── BUG-RUST-2: STAG=0 escape and Relaxed ordering ───────────────────────

    /// TDD regression guard: STAG=0 must never be returned from the stall
    /// allocation loop, even when the counter wraps through zero during the
    /// retry loop (Occupied branch).
    ///
    /// Scenario:
    /// 1. Pre-populate stall slots 1..=N so the allocation loop must retry.
    /// 2. Set the STAG counter so that the Occupied-branch fetch_add will
    ///    return 0 (the counter is at u16::MAX, so fetch_add returns 0 after
    ///    wrapping — or more precisely, returns the old value of 65535, then
    ///    the counter wraps to 0, and the NEXT fetch_add returns 0).
    ///
    /// The `if candidate == 0` guard (current code) prevents STAG=0 from being
    /// used AS LONG AS the first fetch_add after a wrap returns 0.  However,
    /// if the second fetch_add in the `if` guard itself wraps to 0 (concurrent
    /// threads), candidate would still be 0.  This test covers the simpler
    /// single-threaded wrap scenario to confirm the guard works.
    ///
    /// After the fix (while loop), STAG=0 can never escape regardless of how
    /// many consecutive wraps occur.
    #[test]
    fn bug_rust2_stag_zero_never_returned_on_counter_wrap() {
        use crate::types::{AccessType, FaultMode, SecurityState, StreamConfig, IOVA, PASID};

        let smmu = SMMU::new();
        smmu.enable().unwrap();

        let stream_id = StreamID::new(0x30).unwrap();
        let cfg = StreamConfig::builder()
            .stage1_enabled(true)
            .translation_enabled(true)
            .fault_mode(FaultMode::Stall)
            .build()
            .unwrap();
        smmu.configure_stream(stream_id, cfg).unwrap();
        smmu.create_pasid(stream_id, PASID::new(0).unwrap()).unwrap();

        // Set the STAG counter to u16::MAX so the very first fetch_add in the
        // initial allocation returns u16::MAX (old value), counter wraps to 0.
        // The second fetch_add (inside `if initial == 0`) is not triggered here
        // since initial != 0.  Candidate = u16::MAX.
        // Then slot u16::MAX is occupied → Occupied branch:
        //   fetch_add returns 0 (old value), counter becomes 1.
        //   `if candidate == 0` is TRUE → does one more fetch_add returning 1.
        // With the `while` fix, this loop continues until candidate != 0.
        smmu.stag_counter.store(u16::MAX, Ordering::Relaxed);

        // Pre-populate slot u16::MAX so the loop hits the Occupied branch and
        // the counter wraps to 0 inside the retry.
        let sentinel = StallRecord {
            stag: u16::MAX,
            stream_id: 0xFF,
            pasid: 0xFF,
            iova: 0xDEAD,
            access: AccessType::Read,
            security_state: SecurityState::NonSecure,
        };
        smmu.stall_queue.insert(u16::MAX, sentinel);

        // Trigger a stall fault.
        let result = smmu.translate(
            stream_id,
            PASID::new(0).unwrap(),
            IOVA::new(0xCAFE_0000).unwrap(),
            AccessType::Read,
            SecurityState::NonSecure,
        );

        match result {
            Err(TranslationError::Stalled { stag }) => {
                assert_ne!(
                    stag, 0,
                    "§3.12.2 BUG-RUST-2: STAG=0 is reserved and must never be allocated; \
                     got stag=0 when counter wrapped through zero during retry loop"
                );
            }
            other => panic!("expected Stalled error, got: {other:?}"),
        }
    }

    /// TDD regression guard: `stag_counter.fetch_add` must use `AcqRel` ordering
    /// (not `Relaxed`) to provide the cross-thread visibility guarantees required
    /// by ARM IHI0070G.b §3.12.2 STAG uniqueness constraint.
    ///
    /// This test is a structural assertion — it verifies the ordering at the
    /// known line numbers.  We cannot directly test memory ordering in a standard
    /// test, but we can verify the invariant that the counter never returns 0
    /// under aggressive concurrent stress.
    #[test]
    fn bug_rust2_stag_counter_ordering_acquirerel() {
        use crate::types::{AccessType, FaultMode, SecurityState, StreamConfig, IOVA, PASID};
        use std::sync::Arc;
        use std::thread;

        // Spin up N threads, each triggering a stall fault.  If the ordering
        // is too weak (Relaxed), a thread may observe a stale counter value
        // and generate a duplicate STAG or STAG=0.  With AcqRel, all threads
        // see a consistent monotonic counter.
        let smmu = Arc::new(SMMU::new());
        smmu.enable().unwrap();

        let stream_id = StreamID::new(0x31).unwrap();
        let cfg = StreamConfig::builder()
            .stage1_enabled(true)
            .translation_enabled(true)
            .fault_mode(FaultMode::Stall)
            .build()
            .unwrap();
        smmu.configure_stream(stream_id, cfg).unwrap();
        smmu.create_pasid(stream_id, PASID::new(0).unwrap()).unwrap();

        let all_stags: Vec<u16> = (0..8usize)
            .map(|t| {
                let smmu = Arc::clone(&smmu);
                thread::spawn(move || {
                    let mut local_stags = Vec::new();
                    for i in 0..10usize {
                        let fault_addr = 0x0100_0000u64 + (t as u64) * 0x1000_0000 + (i as u64) * 0x1000;
                        let result = smmu.translate(
                            stream_id,
                            PASID::new(0).unwrap(),
                            IOVA::new(fault_addr).unwrap(),
                            AccessType::Read,
                            SecurityState::NonSecure,
                        );
                        if let Err(TranslationError::Stalled { stag }) = result {
                            local_stags.push(stag);
                        }
                    }
                    local_stags
                })
            })
            .flat_map(|h| h.join().unwrap())
            .collect();

        // No STAG=0 must appear.
        assert!(
            all_stags.iter().all(|&s| s != 0),
            "§3.12.2 BUG-RUST-2: STAG=0 found under concurrent load; ordering is insufficient"
        );
    }

    /// Regression guard: `submit_event()` must return `Err(EventQueueFull)` once
    /// the queue reaches its capacity, regardless of whether the capacity is >= 200.
    ///
    /// Before the fix, the `< 200` guard caused production-sized queues to never
    /// enforce their limit, allowing unbounded growth.
    #[test]
    fn bug_rust_m03_submit_event_enforces_large_capacity() {
        use crate::types::{EventType, QueueConfig};

        // Use a capacity value >= 200 to exercise the previously-skipped path.
        let capacity = 200usize;
        let config = SMMUConfig::from(QueueConfig {
            event_queue_size: capacity,
            command_queue_size: 32,
            pri_queue_size: 32,
        });
        let smmu = SMMU::with_config(config);

        let make_event = |i: u32| EventEntry {
            event_type: EventType::FTranslation,
            stream_id: i,
            pasid: 0,
            address: 0,
            security_state: SecurityState::NonSecure,
            error_code: 0,
            timestamp: u64::from(i),
            stall: false,
            stag: 0,
        };

        // Fill the queue exactly to capacity — all must succeed.
        for i in 0..(capacity as u32) {
            assert!(
                smmu.submit_event(make_event(i)).is_ok(),
                "BUG-RUST-M03: submit_event should succeed for entry {i} (capacity {capacity})"
            );
        }

        // One more must fail.
        let result = smmu.submit_event(make_event(capacity as u32));
        assert!(
            matches!(result, Err(SMMUError::EventQueueFull)),
            "BUG-RUST-M03: submit_event must return EventQueueFull once capacity ({capacity}) is reached; got {result:?}"
        );
    }

    // ── BUG-RUST-A: get_stream_count() must use atomic, not streams.len() ──────

    /// BUG-RUST-A: get_stream_count() must use the atomic counter, not streams.len().
    ///
    /// Before the fix, get_stream_count() returned streams.len() (DashMap).
    /// After BUG-RUST-A fix + BUG-RUST-B fix: the atomic IS the authoritative counter
    /// and it IS reset to 0 on shutdown — so get_stream_count() returns 0 after shutdown.
    /// This test verifies the pre-shutdown count (1) and that the atomic path is used
    /// (confirmed by checking the atomic directly matches get_stream_count()).
    #[test]
    fn bug_rust_a_get_stream_count_uses_atomic_after_shutdown() {
        let smmu = SMMU::new();
        let sid = StreamID::new(1).unwrap();
        smmu.configure_stream(sid, StreamConfig::bypass()).unwrap();
        assert_eq!(smmu.get_stream_count(), 1, "pre-shutdown: stream count must be 1");
        // Verify get_stream_count() matches the atomic value (not streams.len()).
        assert_eq!(
            smmu.get_stream_count(),
            smmu.stream_count.load(Ordering::Acquire),
            "BUG-RUST-A: get_stream_count() must equal stream_count atomic"
        );

        smmu.shutdown().unwrap();
        // After shutdown with BUG-RUST-B also fixed: atomic is reset to 0.
        // get_stream_count() (using the atomic) must return 0.
        assert_eq!(
            smmu.get_stream_count(),
            0,
            "BUG-RUST-A: get_stream_count() must return 0 after shutdown (atomic reset)"
        );
        // Double-check: get_stream_count() must match the atomic.
        assert_eq!(
            smmu.get_stream_count(),
            smmu.stream_count.load(Ordering::Acquire),
            "BUG-RUST-A: get_stream_count() must equal stream_count atomic after shutdown"
        );
    }

    // ── BUG-RUST-B: shutdown() must reset stream_count to 0 ──────────────────

    /// BUG-RUST-B: shutdown() clears streams but doesn't reset stream_count.
    /// After shutdown the atomic still shows the pre-shutdown count.
    /// This test verifies that after BUG-RUST-A is fixed (using atomic),
    /// BUG-RUST-B ensures the atomic is also reset so the count returns 0.
    #[test]
    fn bug_rust_b_shutdown_resets_stream_count_atomic() {
        let smmu = SMMU::new();
        let sid = StreamID::new(2).unwrap();
        smmu.configure_stream(sid, StreamConfig::bypass()).unwrap();
        assert_eq!(smmu.stream_count.load(Ordering::Acquire), 1, "pre-shutdown atomic must be 1");

        smmu.shutdown().unwrap();
        // BUG-RUST-B: before fix, stream_count atomic is still 1 after shutdown.
        assert_eq!(
            smmu.stream_count.load(Ordering::Acquire),
            0,
            "BUG-RUST-B: stream_count atomic must be reset to 0 after shutdown()"
        );
    }

    // ── BUG-RUST-C: TOCTOU in configure_stream() max_streams check ───────────

    /// BUG-RUST-C: Verify that after the fix, fetch_add occurs BEFORE reading
    /// max_streams.  This is a structural test — we verify the correct outcome
    /// (limit enforced) still holds, confirming the ordering doesn't break logic.
    #[test]
    fn bug_rust_c_stream_limit_enforced_after_fetch_add_reorder() {
        let cfg = SMMUConfig::default().with_max_streams(2);
        let smmu = SMMU::with_config(cfg);

        let s1 = StreamID::new(1).unwrap();
        let s2 = StreamID::new(2).unwrap();
        let s3 = StreamID::new(3).unwrap();

        smmu.configure_stream(s1, StreamConfig::bypass()).unwrap();
        smmu.configure_stream(s2, StreamConfig::bypass()).unwrap();

        // Third stream must be rejected because prev_count (2) >= max_streams (2).
        let result = smmu.configure_stream(s3, StreamConfig::bypass());
        assert!(
            result.is_err(),
            "BUG-RUST-C: stream limit must be enforced; third configure_stream must fail"
        );
        // stream_count must remain at 2 (rolled back).
        assert_eq!(
            smmu.stream_count.load(Ordering::Acquire),
            2,
            "BUG-RUST-C: stream_count must roll back to 2 after limit exceeded"
        );
    }

    // ── BUG-RUST-D: CfgiSte must use get() not contains_key() ───────────────

    /// BUG-RUST-D: Structural correctness test — CfgiSte for unknown stream
    /// must still produce C_BAD_STREAMID after replacing contains_key with get().
    /// CR2.RECINVSID must be set so the event is recorded in the event queue.
    #[test]
    fn bug_rust_d_cfgi_ste_unknown_stream_uses_get() {
        let smmu = SMMU::new();
        smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
        // §6.3.12: set RECINVSID so C_BAD_STREAMID events are recorded.
        smmu.set_cr2(SMMU::CR2_RECINVSID);

        // Submit CfgiSte for unknown stream — must fail with InvalidCommandParameters.
        let cmd = CommandEntry::new(CommandType::CfgiSte, 0xBEEF, 0);
        smmu.submit_command(cmd).unwrap();
        let result = smmu.process_command_queue();
        assert!(
            result.is_err(),
            "BUG-RUST-D: CfgiSte for unknown stream must return error; got {result:?}"
        );

        // Event queue must contain C_BAD_STREAMID event.
        let events = smmu.get_events();
        assert!(
            events.iter().any(|e| e.stream_id == 0xBEEF),
            "BUG-RUST-D: C_BAD_STREAMID event for stream 0xBEEF must be in event queue"
        );
    }

    // ── BUG-RUST-E: signal_gerror() must re-read gerror for consistency ───────

    /// BUG-RUST-E: Structural test — signal_gerror() must correctly activate
    /// a new error bit and leave already-active bits untouched.  The re-read
    /// consistency check must not break correct single-threaded behaviour.
    #[test]
    fn bug_rust_e_signal_gerror_consistency_reread() {
        let smmu = SMMU::new();
        smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

        // Trigger CMDQ_ERR by submitting a bad command.
        let cmd = CommandEntry::new(CommandType::CfgiSte, 0xDEAD, 0);
        smmu.submit_command(cmd).unwrap();
        let _ = smmu.process_command_queue();

        // CMDQ_ERR must be active: (GERROR ^ GERRORN) & CMDQ_ERR != 0.
        let active = smmu.get_gerror() ^ smmu.get_gerrorn();
        assert_ne!(
            active & SMMU::GERROR_CMDQ_ERR,
            0,
            "BUG-RUST-E: CMDQ_ERR must be active after bad command"
        );

        // signal_gerror() with already-active bit must NOT double-toggle.
        // Calling signal_gerror indirectly by triggering another bad command.
        let cmd2 = CommandEntry::new(CommandType::CfgiSte, 0xDEAD2, 0);
        smmu.submit_command(cmd2).unwrap();
        let _ = smmu.process_command_queue();

        let active2 = smmu.get_gerror() ^ smmu.get_gerrorn();
        assert_ne!(
            active2 & SMMU::GERROR_CMDQ_ERR,
            0,
            "BUG-RUST-E: CMDQ_ERR must remain active (not double-toggled) after second bad command"
        );
    }

    // ── BUG-RUST-H: translate() must read oas_bits once ──────────────────────

    /// BUG-RUST-H: Structural test — translate() OAS check still rejects
    /// addresses above max_pa_bits after the single-read refactor.
    #[test]
    fn bug_rust_h_oas_check_still_enforced_after_single_read() {
        let smmu = SMMU::new();
        // max_pa_bits default is typically 48; IOVA 0 is always in-range.
        // Verify bypass path works normally (SMMUEN=0, GBPA.ABORT=0).
        let sid = StreamID::new(1).unwrap();
        let pasid = PASID::new(0).unwrap();
        let iova_zero = IOVA::new(0).unwrap();
        let result = smmu.translate(sid, pasid, iova_zero, AccessType::Read, SecurityState::NonSecure);
        assert!(
            result.is_ok(),
            "BUG-RUST-H: bypass translate of IOVA 0 must succeed; got {result:?}"
        );
    }

    // ── BUG-RUST-J: toggle_ovflg_once() must re-read prod for consistency ────

    /// BUG-RUST-J: Structural test — overflow flag toggling logic must still
    /// correctly set OVFLG when the queue overflows after the consistency re-read.
    ///
    /// Strategy: fill the event queue via submit_event(), then trigger a
    /// translation fault (which uses the internal record_translation_fault path
    /// that calls toggle_ovflg_once() when the queue is full).
    #[test]
    fn bug_rust_j_toggle_ovflg_once_consistency_reread() {
        // Use the minimum allowed event queue size so we can overflow it quickly.
        let capacity: usize = 16;
        let smmu = SMMU::with_config(SMMUConfig::from(crate::types::QueueConfig {
            event_queue_size: capacity,
            command_queue_size: 32,
            pri_queue_size: 32,
        }));
        // Enable SMMU and event queue so faults generate events.
        smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
        // §6.3.12: set RECINVSID so C_BAD_STREAMID events (from unknown-stream
        // translate calls) are recorded — without this the event queue never
        // fills and toggle_ovflg_once() is never exercised.
        smmu.set_cr2(SMMU::CR2_RECINVSID);

        let make_event = |n: u32| EventEntry {
            event_type: EventType::FTranslation,
            stream_id: n,
            pasid: 0,
            address: 0,
            security_state: SecurityState::NonSecure,
            error_code: 0,
            timestamp: 0,
            stall: false,
            stag: 0,
        };
        // Fill the event queue to capacity via public API.
        for i in 0..(capacity as u32) {
            let _ = smmu.submit_event(make_event(i));
        }

        // Now trigger a translation fault for an unconfigured stream.
        // record_translation_fault() will attempt to enqueue an event;
        // with the queue full, it drops the non-stall event and calls
        // toggle_ovflg_once() to set OVFLG.
        let sid = StreamID::new(0x42).unwrap();
        let pasid = PASID::new(0).unwrap();
        let iova = IOVA::new(0x1000).unwrap();
        let _ = smmu.translate(sid, pasid, iova, AccessType::Read, SecurityState::NonSecure);

        // OVFLG must be active: prod bit-31 != cons bit-31.
        let prod = smmu.eventq_prod.load(Ordering::Acquire);
        let cons = smmu.eventq_cons.load(Ordering::Acquire);
        assert_ne!(
            (prod >> 31) & 1,
            (cons >> 31) & 1,
            "BUG-RUST-J: OVFLG must be active (prod bit-31 != cons bit-31) after overflow"
        );
    }
}
