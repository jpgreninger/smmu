// ARM SMMU v3 Main Controller
// Copyright (c) 2024 John Greninger

#ifndef SMMU_SMMU_H
#define SMMU_SMMU_H

#include "smmu/types.h"
#include "smmu/stream_context.h"
#include "smmu/fault_handler.h"
#include "smmu/tlb_cache.h"
#include "smmu/configuration.h"
#include <unordered_map>
#include <vector>
#include <memory>
#include <deque>
#include <cstddef>
#include <atomic>
#include <mutex>

namespace smmu {

class SMMU {
public:
    // §6.3.9 SMMU_CR0 register bit constants — ARM IHI0070G.b §6.3.9 layout.
    // Exposed publicly so that tests and software can reference them without
    // hardcoding magic numbers.
    static constexpr uint32_t CR0_SMMUEN   = (1u << 0); ///< bit 0: SMMU global enable
    static constexpr uint32_t CR0_PRIQEN   = (1u << 1); ///< bit 1: PRI queue enable (§6.3.9)
    static constexpr uint32_t CR0_EVENTQEN = (1u << 2); ///< bit 2: Event queue enable (§6.3.9)
    static constexpr uint32_t CR0_CMDQEN   = (1u << 3); ///< bit 3: Command queue enable (§6.3.9)
    static constexpr uint32_t CR0_ATSCHK   = (1u << 4); ///< bit 4: ATS CHK enable (§6.3.9)
    // CONF-GAP-12: SMMU_CR0.VMW VMID wildcard matching (§6.3.9)
    static constexpr uint32_t CR0_VMW_SHIFT = 6u;            ///< VMW field starts at bit 6 (ARM IHI0070G.b §6.3.9.1: bits[8:6])
    static constexpr uint32_t CR0_VMW_MASK  = (7u << 6);     ///< bits[8:6]: VMW field

    // §6.3.12 SMMU_CR2 register bit constants — ARM IHI0070G.b §6.3.12.
    // RECINVSID (bit 1): when 1, C_BAD_STREAMID events are recorded in the event queue.
    // When 0 (reset default), C_BAD_STREAMID events are suppressed (not recorded).
    // GERROR.CMDQ_ERR is always toggled unconditionally for CMD_CFGI_STE with bad StreamID.
    static constexpr uint32_t CR2_E2H        = (1u << 0); ///< bit 0: EL2-E2H (VHE) mode enable — §6.3.12
    static constexpr uint32_t CR2_RECINVSID  = (1u << 1); ///< bit 1: Record invalid StreamID events
    // CONF-GAP-11: CR2.PTM — broadcast TLB maintenance participation (§6.3.12)
    static constexpr uint32_t CR2_PTM        = (1u << 2); ///< bit 2: Participate in TLB maintenance
    static constexpr uint32_t CR2_REC_CFG_ATS = (1u << 3); ///< bit 3: Record config errors for ATS (§6.3.12)

    // CONF-GAP-10: SMMU_CR1 register bit constants — ARM IHI0070G.b §6.3.11.
    static constexpr uint32_t CR1_TABLE_SH = (0x3u << 10); ///< bits[11:10]: table walk shareability
    static constexpr uint32_t CR1_TABLE_OC = (0x3u << 8);  ///< bits[9:8]: table walk outer cache
    static constexpr uint32_t CR1_TABLE_IC = (0x3u << 6);  ///< bits[7:6]: table walk inner cache
    static constexpr uint32_t CR1_QUEUE_SH = (0x3u << 4);  ///< bits[5:4]: queue shareability
    static constexpr uint32_t CR1_QUEUE_OC = (0x3u << 2);  ///< bits[3:2]: queue outer cache
    static constexpr uint32_t CR1_QUEUE_IC = (0x3u << 0);  ///< bits[1:0]: queue inner cache

    // Default constructor with default configuration
    SMMU();
    
    // Constructor with custom configuration
    explicit SMMU(const SMMUConfiguration& config);
    
    ~SMMU();
    
    // Main translation API
    // NEW-12: extended with defaulted TransactionType parameter (backward compatible).
    TranslationResult translate(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType,
                                SecurityState securityState = SecurityState::NonSecure,
                                TransactionType transactionType = TransactionType::Ordinary);
    
    // Stream management
    VoidResult configureStream(StreamID streamID, const StreamConfig& config);
    VoidResult removeStream(StreamID streamID);
    Result<bool> isStreamConfigured(StreamID streamID) const; // Returns Result<bool> - error on invalid StreamID or system failure
    VoidResult enableStream(StreamID streamID);
    VoidResult disableStream(StreamID streamID);
    Result<bool> isStreamEnabled(StreamID streamID) const;   // Returns Result<bool> - error on invalid StreamID or unconfigured stream
    
    // PASID management for streams
    VoidResult createStreamPASID(StreamID streamID, PASID pasid);
    VoidResult removeStreamPASID(StreamID streamID, PASID pasid);

    // Address size configuration (ARM §3.4.1)
    // Sets the input address size (in bits) for the context descriptor of the
    // given (streamID, pasid) pair.  Valid range: 32–52.
    // IOVAs >= (1ULL << bits) will produce F_ADDR_SIZE during translation.
    VoidResult setStreamInputAddressSize(StreamID streamID, PASID pasid, uint8_t bits);
    
    // Page mapping operations
    // accessFlag=true (default): marks the page as already-accessed (AF=1).
    // accessFlag=false: simulates a fresh OS mapping before first hardware access (AF=0).
    VoidResult mapPage(StreamID streamID, PASID pasid, IOVA iova, PA pa,
                       const PagePermissions& permissions,
                       SecurityState securityState = SecurityState::NonSecure,
                       bool accessFlag = true);
    VoidResult unmapPage(StreamID streamID, PASID pasid, IOVA iova);

    // Map a stage-2 page as Device memory type (for S2PTW testing).
    // STE.S2PTW=1 will cause F_PERMISSION on two-stage translation through such pages.
    VoidResult mapStage2DevicePage(StreamID streamID, IOVA ipa, PA pa,
                                   const PagePermissions& permissions,
                                   SecurityState securityState = SecurityState::NonSecure);

    // Stage-2 address space configuration.
    // Associates a hypervisor-managed Stage-2 address space with the given stream.
    // Must be called after configureStream() for streams with stage2Enabled=true.
    // ARM IHI0070G.b §5.2: Stage-2 translation context (S2TTB / S2VMID).
    VoidResult setStreamStage2AddressSpace(StreamID streamID,
                                           std::shared_ptr<AddressSpace> stage2AS);
    
    // Event management
    Result<std::vector<FaultRecord>> getEvents();  // Returns Result - error on event queue corruption or system failure
    VoidResult clearEvents();                       // Returns VoidResult - error on event queue corruption or thread safety issues
    
    // Configuration
    VoidResult setGlobalFaultMode(FaultMode mode);  // Returns VoidResult - error on invalid mode or thread safety issues
    VoidResult enableCaching(bool enable);          // Returns VoidResult - error on cache system failure or invalid state
    
    // Configuration management
    const SMMUConfiguration& getConfiguration() const;
    VoidResult updateConfiguration(const SMMUConfiguration& config);
    VoidResult updateQueueConfiguration(const QueueConfiguration& queueConfig);
    VoidResult updateCacheConfiguration(const CacheConfiguration& cacheConfig);
    VoidResult updateAddressConfiguration(const AddressConfiguration& addressConfig);
    VoidResult updateResourceLimits(const ResourceLimits& resourceLimits);
    
    // Cache management operations (Task 5.2)
    void invalidateTranslationCache();
    void invalidateStreamCache(StreamID streamID);
    void invalidatePASIDCache(StreamID streamID, PASID pasid);
    void setStreamVMID(StreamID streamID, uint16_t vmid);  ///< Set STE.S2VMID for a stream (ARM §5.2)
    void setStreamASID(StreamID streamID, uint16_t asid);  ///< Set CD.ASID for a stream (ARM §3.17)
    
    // Task 5.3: Event and Command Processing
    // Event queue management (Task 5.3.1)
    void processEventQueue();
    Result<bool> hasEvents() const;
    // BUG-CPP-1/2 fix: getEventQueue() is a pure non-destructive snapshot.
    // It does NOT advance eventqCons.  Only processEventQueue() (which removes
    // entries) and clearEventQueue() (which resets the queue) may advance CONS
    // per ARM §3.5.1.
    std::vector<EventEntry> getEventQueue() const;
    void clearEventQueue();
    size_t getEventQueueSize() const;
    
    // Command queue processing simulation (Task 5.3.2)
    VoidResult submitCommand(const CommandEntry& command);
    void processCommandQueue();
    Result<bool> isCommandQueueFull() const;
    std::vector<CommandEntry> getCommandQueue() const;
    size_t getCommandQueueSize() const;
    void clearCommandQueue();
    
    // PRI queue for page requests (Task 5.3.3)
    void submitPageRequest(const PRIEntry& request);
    void processPRIQueue();
    std::vector<PRIEntry> getPRIQueue() const;
    void clearPRIQueue();
    size_t getPRIQueueSize() const;

    // GAP-H: ARM §3.13.6 — auto-failure responses generated on PRIQ overflow.
    // Returns the list of auto-generated FAILURE PRG_RESPONSE records that were
    // created because the PRIQ was full when the corresponding page request arrived.
    // The caller should drain this list and send the FAILURE response to the device.
    std::vector<PRIAutoFailure> getPRIAutoFailures() const;
    /// Clear the auto-failure list (call after consuming the responses).
    void clearPRIAutoFailures();
    
    // NEW-10: ARM §6.3.96 SMMU_EVENTQ_CONS.OVACKFLG — acknowledge event queue overflow.
    // Copies bit 31 of SMMU_EVENTQ_PROD (OVFLG) into bit 31 of SMMU_EVENTQ_CONS (OVACKFLG)
    // without discarding queued events or resetting indices.  This is the spec-mandated
    // handshake by which software signals that it has observed the overflow condition.
    // Call after draining the event queue to clear the active-overflow indication.
    void acknowledgeEventQueueOverflow();

    // NEW-11: ARM §7.3.2 F_UUT — Unsupported Upstream Transaction.
    // Allows a simulation harness to inject an F_UUT event for a stream.
    // The event is gated by CR0.EVENTQEN (identical to all other events):
    // when EVENTQEN=0 the event is silently dropped.
    void reportUnsupportedTransaction(StreamID streamID, PASID pasid,
                                      IOVA iova, AccessType accessType,
                                      SecurityState securityState = SecurityState::NonSecure);

    // ARM §3.5.1: Circular queue PROD/CONS register accessors (FINDING-M-01)
    uint32_t getCmdqProdIndex() const;
    uint32_t getCmdqConsIndex() const;
    uint32_t getEventqProdIndex() const;
    uint32_t getEventqConsIndex() const;
    uint32_t getPriqProdIndex() const;
    uint32_t getPriqConsIndex() const;

    bool isCmdqEmptyByIndex() const;
    bool isEventqEmptyByIndex() const;
    uint32_t getCmdqOccupiedEntries() const;
    uint32_t getEventqOccupiedEntries() const;

    uint32_t getCmdqLog2Size() const;
    uint32_t getEventqLog2Size() const;

    // Cache invalidation command handling (Task 5.3.4)
    void executeInvalidationCommand(const CommandEntry& command);
    // CONF-GAP-6: Added iova parameter for VA-targeted selective TLBI (§4.4)
    // CONF-GAP-8: Added ril/tg/num/scale for range invalidation (§4.4.1.1)
    void executeTLBInvalidationCommand(CommandType type, StreamID streamID, PASID pasid, uint16_t asid, uint16_t vmid, IOVA iova = 0, bool ril = false, uint8_t tg = 0, uint8_t num = 0, uint8_t scale = 0);
    // ARM §3.17: Broadcast TLBIs carry no stream context — this overload is used
    // exclusively by receiveBroadcastTLBI() to make that intent explicit.
    void executeTLBInvalidationCommand(CommandType type, uint16_t asid, uint16_t vmid, IOVA va);

    // CONF-GAP-11: §6.3.12 CR2.PTM — broadcast TLB maintenance participation.
    // This method models the hardware path where a PE broadcast TLB invalidation
    // propagates to attached SMMUs.  When CR2.PTM=0 the SMMU does NOT participate
    // (request is silently ignored).  When CR2.PTM=1 the SMMU executes the
    // invalidation.  Command-queue TLBI commands are NOT gated by PTM — only
    // this broadcast path is.
    void receiveBroadcastTLBI(CommandType type, uint16_t asid = 0, uint16_t vmid = 0, IOVA va = 0);
    void executeATCInvalidationCommand(StreamID streamID, PASID pasid, IOVA startAddr, IOVA endAddr, SecurityState securityState);
    
    // ARM §6.3.17: SMMU_GERROR / SMMU_GERRORN register model (FINDING-M-06)
    // getGerror() returns the active (unacknowledged) error bits: GERROR XOR GERRORN.
    // Active errors are those where GERROR[x] != GERRORN[x].
    uint32_t getGerror() const;
    // getGerrorN() returns the raw SMMU_GERRORN register value (software-written).
    uint32_t getGerrorN() const;
    // clearGerror() implements the ARM §6.3.18 SMMU_GERRORN write protocol.
    // Software writes GERRORN to acknowledge active GERROR bits.
    // Only bits that are currently active (GERROR[x] != GERRORN[x]) are toggled.
    void clearGerror(uint32_t bits);

    // ARM §6.3.9 SMMU_CR0.SMMUEN / §3.11 SMMU_GBPA.ABORT (FINDING-NEW-01, FINDING-NEW-09)
    /// Enable the SMMU globally (SMMUEN=1).  When enabled, translations go
    /// through the full stream/page-table path.
    void enable();
    /// Disable the SMMU globally (SMMUEN=0).  When disabled, transactions
    /// bypass (identity PA) or abort depending on GBPA.ABORT.
    void disable();
    /// Returns true when SMMUEN=1 (SMMU globally enabled).
    bool isEnabled() const;
    /// Set SMMU_GBPA.ABORT.  When true and SMMUEN=0, all transactions abort
    /// with SMMUError::GbpaAbort instead of bypassing.
    void setGbpaAbort(bool abort);
    /// Returns the current value of SMMU_GBPA.ABORT.
    bool isGbpaAbort() const;
    // CONF-GAP-13: Full GBPA output attribute configuration (§6.3.22)
    /// Set all SMMU_GBPA fields including output attributes.
    void setGbpaConfig(const GbpaConfig& cfg);
    /// Get the current SMMU_GBPA configuration.
    GbpaConfig getGbpaConfig() const;

    // §6.3.9 SMMU_CR0 register (CT-33)
    /// Set the SMMU_CR0 register value.  Bit 0 = SMMUEN, bit 2 = EVENTQEN, bit 3 = CMDQEN.
    void setCR0(uint32_t value);
    /// Get the current SMMU_CR0 register value.
    uint32_t getCR0() const;
    // CONF-GAP-9: SMMU_CR0ACK register — mirrors CR0 after write (§6.3.10)
    /// Get the current SMMU_CR0ACK register value (read-only hardware echo of CR0).
    uint32_t getCR0ACK() const;
    /// Set the SMMU_CR0ACK register value (software-model synchronous update).
    void setCR0ACK(uint32_t v);

    // CONF-GAP-10: SMMU_CR1 register (§6.3.11)
    /// Set the SMMU_CR1 register value.
    void setCR1(uint32_t value);
    /// Get the current SMMU_CR1 register value.
    uint32_t getCR1() const;

    // §6.3.12 SMMU_CR2 register.
    /// Set the SMMU_CR2 register value.  Bit 1 = RECINVSID.
    void setCR2(uint32_t value);
    /// Get the current SMMU_CR2 register value.
    uint32_t getCR2() const;

    // BUG-NEW-11: IDR0.STALL_MODEL configuration (§4.7.1/§4.7.2).
    // 0b00 = stall and terminate both supported (default).
    // 0b01 = terminate-only; CMD_RESUME and CMD_STALL_TERM raise CERROR_ILL.
    // Values 0b10/0b11 are reserved.
    void setStallModel(uint8_t model);
    /// Get the current STALL_MODEL value.
    uint8_t getStallModel() const;

    // §6.3.4 SMMU_STRTAB_BASE_CFG: LOG2SIZE field (CT-04)
    /// Set the number of stream table entries as 2^log2size.
    /// StreamIDs >= 2^log2size will generate C_BAD_STREAMID.
    /// Default: 32 (accept all 32-bit StreamIDs).
    void setStrtabLog2Size(uint8_t log2size);
    /// Get the current LOG2SIZE value.
    uint8_t getStrtabLog2Size() const;
    // CONF-GAP-3: 2-level stream table format (§3.3.1.2, §6.3.25)
    /// Set the stream table format (linear or 2-level).
    void setStrtabFormat(StreamTableFormat fmt);
    /// Get the current stream table format.
    StreamTableFormat getStrtabFormat() const;
    /// Set the SPLIT value (number of StreamID bits used for L2 index; default 6, range 1-15).
    void setStrtabSplit(uint8_t split);
    /// Get the current SPLIT value.
    uint8_t getStrtabSplit() const;

    // CONF-GAP-17: CMDQ_CONS.ERR field accessor (§6.3.17)
    /// Get the current CMDQ_CONS.ERR field value (CERROR_NONE=0, CERROR_ILL=1, etc.).
    uint32_t getCmdqConsErr() const;

    // CONF-GAP-18: CMD_SYNC MSI signalling registers (§4.7.3)
    /// Set/get CMD_SYNC MSI attribute register.
    void setCmdqSyncMsiAttr(uint32_t v);
    uint32_t getCmdqSyncMsiAttr() const;
    /// Set/get CMD_SYNC MSI address register (64-bit).
    void setCmdqSyncMsiAddr(uint64_t v);
    uint64_t getCmdqSyncMsiAddr() const;
    /// Set/get CMD_SYNC MSI data register.
    void setCmdqSyncMsiData(uint32_t v);
    uint32_t getCmdqSyncMsiData() const;
    /// Get the last CMD_SYNC completion signal type used.
    CmdSyncSignalType getCmdSyncLastSignalType() const;

    // ARM §3.12.2: Stall queue management (FINDING-NEW-08)
    /// Returns a snapshot of all currently stalled transactions.
    std::vector<StallRecord> getStalledTransactions() const;
    /// Forcibly abort a stalled transaction by STAG (removes from stall queue).
    /// Returns true if the STAG was found and removed; false if not found.
    bool abortStalledTransaction(uint16_t stag);
    /// Returns the number of currently stalled transactions.
    size_t getStalledTransactionCount() const;

    // CONF-GAP-24: ARM §3.12.2 / §4.6 CMD_RESUME outcome observability.
    /// Returns the ResumeOutcome recorded for the given STAG by the last CMD_RESUME
    /// command that matched it.  Removes the entry from the internal map after the
    /// query (one-shot read).  Returns ResumeOutcome::None when no outcome is
    /// recorded for that STAG (not yet resumed or already queried).
    ResumeOutcome getResumeOutcome(uint16_t stag);
    /// Clears all recorded resume outcomes (e.g. on reset or software flush).
    void clearResumeOutcomes();

    // GAP-NEW-D: ARM §6.3.1–6.3.8 IDR registers — capability bitmasks (read-only).
    uint32_t getIDR0() const;  ///< IDR0: S1P, S2P, granule support, stall, ASID16
    uint32_t getIDR1() const;  ///< IDR1: SIDSIZE[5:0]=32, SSIDSIZE[10:6]=20
    uint32_t getIDR2() const;  ///< IDR2: IAS[3:0]=5(48-bit), OAS[7:4]=5(48-bit)
    uint32_t getIDR3() const;  ///< IDR3: reserved — returns 0
    uint32_t getIDR4() const;  ///< IDR4: reserved — returns 0
    uint32_t getIDR5() const;  ///< IDR5: OAS[5:3]=5(48-bit), 4K/16K/64K granule bits
    uint32_t getAIDR() const;  ///< AIDR: architecture implementation-defined; returns 0
    uint32_t getIIDR() const;  ///< IIDR: implementer-defined; returns 0

    // GAP-NEW-A: Fault injection — structure-fetch and walk external aborts.
    // All three methods generate the corresponding spec event into the event queue,
    // gated on CR0.EVENTQEN.  Callers are responsible for ensuring the injected
    // StreamID / PASID / IOVA are meaningful for the test or simulation scenario.
    void injectSteFetchAbort(StreamID streamID,
                             SecurityState ss = SecurityState::NonSecure);
    void injectCdFetchAbort(StreamID streamID, PASID pasid,
                            SecurityState ss = SecurityState::NonSecure);
    void injectWalkEabt(StreamID streamID, PASID pasid, IOVA iova,
                        SecurityState ss = SecurityState::NonSecure);

    // GAP-NEW-E: SMMU_STATUSR / SMMU_IRQ_CTRL / SMMU_IRQ_CTRLACK registers (§6.3.45–6.3.47).
    uint32_t getStatusr() const;        ///< SMMU_STATUSR: bit 0 = DORMANT; always 0 in this model
    void     setIrqCtrl(uint32_t v);    ///< SMMU_IRQ_CTRL: write interrupt-enable bits
    uint32_t getIrqCtrlAck() const;     ///< SMMU_IRQ_CTRLACK: synchronous mirror of IRQ_CTRL

    // GAP-NEW-F: GATOS address translation wrapper (§9.1–9.9).
    // Translates (streamID, pasid, iova, accessType) and returns a 64-bit GATOS_PAR value:
    //   bit 0 = 0: translation succeeded; bits[63:12] = PA page base.
    //   bit 0 = 1: translation faulted; remaining bits are 0.
    uint64_t gatosTranslate(StreamID streamID, PASID pasid, IOVA iova,
                            AccessType accessType,
                            SecurityState securityState = SecurityState::NonSecure);

    // Statistics and debugging
    size_t getStreamCount() const;
    uint64_t getTotalTranslations() const;
    uint64_t getTotalFaults() const;
    uint64_t getTranslationCount() const;
    uint64_t getCacheHitCount() const;
    uint64_t getCacheMissCount() const;
    CacheStatistics getCacheStatistics() const;
    void resetStatistics();
    void reset();
    
private:
    // StreamID to StreamContext mapping.
    // BUG-NEW-CPP-3 fix: use shared_ptr so that translate() can take a reference-counted
    // copy of the StreamContext under the stripe lock, then release the stripe lock before
    // calling performTwoStageTranslation(). Without this, generateEvent() calls inside
    // performTwoStageTranslation() would acquire queueMutex while the stripe lock is still
    // held, creating an ABBA deadlock with processCommandQueue() which holds queueMutex and
    // then acquires stripe locks (CFGI_STE path).
    std::unordered_map<StreamID, std::shared_ptr<StreamContext>> streamMap;
    
    // Event handling
    std::shared_ptr<FaultHandler> faultHandler;
    
    // TLB Cache system (Task 5.2)
    std::unique_ptr<TLBCache> tlbCache;
    
    // SMMU Configuration
    SMMUConfiguration configuration;
    
    // Global configuration
    // BUG-R2-CPP-3 fix: std::atomic<FaultMode> eliminates the data race between
    // reset() (writer, after releasing stripe locks) and concurrent calls to
    // setGlobalFaultMode() (writer, under all stripe locks).  Both paths write
    // globalFaultMode; without atomics the concurrent writes are C++11 UB.
    std::atomic<FaultMode> globalFaultMode;
    // BUG-NEW-CPP-2 fix: std::atomic<bool> eliminates the data race between
    // translate() (reader, no lock) and enableCaching()/reset()/applyConfiguration()
    // (writers).  Acquire/release ordering ensures visibility across threads.
    std::atomic<bool> cachingEnabled;
    
    // Statistics - Thread-safe atomic counters
    mutable std::atomic<uint64_t> translationCount;
    mutable std::atomic<uint64_t> cacheHits;
    mutable std::atomic<uint64_t> cacheMisses;
    
    // Task 5.3: Event and Command Processing private members
    std::deque<EventEntry> eventQueue;
    std::deque<CommandEntry> commandQueue;
    std::deque<PRIEntry> priQueue;
    // GAP-H: §3.13.6 — auto-failure responses generated when PRIQ overflows.
    // Protected by queueMutex (same lock as priQueue).
    std::vector<PRIAutoFailure> priAutoFailures_;

    size_t maxEventQueueSize;
    size_t maxCommandQueueSize;
    size_t maxPRIQueueSize;

    // ARM §3.5.1: Circular queue PROD/CONS indices
    // LOG2SIZE values (computed from max queue size)
    uint32_t cmdqLog2Size;     // log2(commandQueue capacity)
    uint32_t eventqLog2Size;   // log2(eventQueue capacity)
    uint32_t priqLog2Size;     // log2(priQueue capacity)

    // PROD/CONS index pairs per queue (declaration order must match constructor init list)
    // BUG-NEW-CPP-2 fix: converted from plain uint32_t to std::atomic<uint32_t>.
    // const accessor methods (getCmdqProdIndex, getEventqProdIndex, etc.) read these
    // without holding queueMutex; concurrent writes (generateEvent, processCommandQueue)
    // write them under queueMutex.  The plain uint32_t creates C++11 undefined behavior
    // on the read side.  std::atomic<uint32_t> eliminates the UB and makes every read
    // well-defined.  Acquire/release ordering ensures visibility across threads.
    std::atomic<uint32_t> cmdqProd;           // CMDQ_PROD register equivalent
    std::atomic<uint32_t> cmdqCons;           // CMDQ_CONS register equivalent
    std::atomic<uint32_t> eventqProd;         // EVENTQ_PROD register equivalent
    // BUG-CPP-1/2 fix: eventqCons is no longer mutable since getEventQueue() is a
    // pure snapshot that does NOT advance CONS.  Only processEventQueue() and
    // clearEventQueue() advance/reset CONS per ARM §3.5.1.
    std::atomic<uint32_t> eventqCons; // EVENTQ_CONS register equivalent
    std::atomic<uint32_t> priqProd;           // PRIQ_PROD register equivalent
    std::atomic<uint32_t> priqCons;           // PRIQ_CONS register equivalent

    // ARM §6.3.17: SMMU_GERROR / §6.3.18: SMMU_GERRORN register pair (BUG-03/SPEC-09)
    // GERROR is the hardware register toggled by the SMMU to signal errors.
    // GERRORN is the software-writable register used to acknowledge errors.
    // Active (unacknowledged) errors: gerrorStatus XOR gerrorNStatus != 0.
    // The SMMU toggles gerrorStatus[x] only when the error is inactive
    // (gerrorStatus[x] == gerrorNStatus[x]).  ARM IHI0070G.b §6.3.19/6.3.20.
    // BUG-CPP-NEW-1 fix: converted to std::atomic<uint32_t> to eliminate the data race
    // between translate() (reader, no lock) and signalError/reset() (writers).
    std::atomic<uint32_t> gerrorStatus;   // SMMU_GERROR: hardware error flags
    std::atomic<uint32_t> gerrorNStatus;  // SMMU_GERRORN: software acknowledgement register, init 0

    // CR0 bit constants are declared public above (§6.3.9).
    // BUG-CPP-NEW-1 fix: converted to std::atomic<uint32_t>.  translate() reads cr0_
    // without holding any lock (hot path); enable()/disable()/reset()/setCR0() write it.
    // Acquire/Release ordering ensures the SMMUEN bit is visible across threads.
    std::atomic<uint32_t> cr0_;           // SMMU_CR0 register value; bit0=SMMUEN, bit1=PRIQEN, bit2=EVENTQEN, bit3=CMDQEN

    // §6.3.12 SMMU_CR2 register.  RECINVSID (bit 1) gates C_BAD_STREAMID event recording.
    // Reset value is 0 (RECINVSID=0: events suppressed) per ARM IHI0070G.b §6.3.12.
    std::atomic<uint32_t> cr2_;           // SMMU_CR2 register value; bit1=RECINVSID

    // ARM §6.3.9 SMMU_CR0.SMMUEN (FINDING-NEW-09) and §3.11 SMMU_GBPA.ABORT (FINDING-NEW-01)
    // Note: smmuen_ is derived from cr0_ bit 0, kept for backward compatibility.
    // BUG-CPP-NEW-1 fix: converted to std::atomic<bool> to eliminate the data race.
    std::atomic<bool> smmuen_;            // SMMUEN bit — false (disabled) at reset
    std::atomic<bool> gbpaAbort_;         // GBPA.ABORT bit — false (bypass) at reset
    // CONF-GAP-9: CR0ACK register — synchronous mirror of CR0 (§6.3.10)
    std::atomic<uint32_t> cr0ack_;        // SMMU_CR0ACK register; mirrors cr0_ on write
    // CONF-GAP-10: CR1 register — table/queue memory attribute fields (§6.3.11)
    std::atomic<uint32_t> cr1_;           // SMMU_CR1 register; bits[11:0] for table/queue attrs
    // CONF-GAP-13: GBPA full config (protected by queueMutex for consistency)
    GbpaConfig gbpaConfig_;               // SMMU_GBPA register fields; abort mirrors gbpaAbort_
    // CONF-GAP-3: 2-level stream table configuration (atomic uint8_t for lock-free reads)
    std::atomic<uint8_t> strtabFmt_;      // StreamTableFormat encoded as uint8_t (0=linear,1=2level)
    std::atomic<uint8_t> strtabSplit_;    // SPLIT: number of L2 index bits (default 6)
    // CONF-GAP-18: CMD_SYNC MSI signalling registers
    std::atomic<uint32_t> cmdqSyncMsiAttr_;  // CMDQ_SYNC_MSI_ATTR
    std::atomic<uint64_t> cmdqSyncMsiAddr_;  // CMDQ_SYNC_MSI_ADDR (64-bit)
    std::atomic<uint32_t> cmdqSyncMsiData_;  // CMDQ_SYNC_MSI_DATA
    std::atomic<uint8_t>  cmdSyncLastSig_;   // Last CMD_SYNC signal type (as CmdSyncSignalType)

    // §6.3.4 SMMU_STRTAB_BASE_CFG.LOG2SIZE (CT-04)
    // StreamIDs >= 2^strtabLog2Size_ generate C_BAD_STREAMID.
    // Default 32 (max SIDSIZE per ARM §6.3.4 IDR1). Per §6.3.25 effective
    // LOG2SIZE = MIN(LOG2SIZE, SIDSIZE); values > 32 are clamped to 32.
    // BUG-NEW-CPP-1 fix: std::atomic<uint8_t> eliminates the data race between
    // translate() (reader, no lock) and setStrtabLog2Size() (writer).
    std::atomic<uint8_t> strtabLog2Size_;   // 0-24; default 24

    // ARM §3.12.2: Stall queue for stalled transactions (FINDING-NEW-08)
    std::unordered_map<uint16_t, StallRecord> stallQueue_;   ///< STAG -> StallRecord map
    std::atomic<uint16_t> stagCounter_;                       ///< Monotonically incrementing STAG generator
    mutable std::mutex stallQueueMutex_;                      ///< Protects stallQueue_

    // BUG-NEW-11: IDR0.STALL_MODEL — configurable stall model (§4.7.1/§4.7.2).
    // 0b00 = stall+terminate (default); 0b01 = terminate-only (CMD_RESUME/STALL_TERM → CERROR_ILL).
    std::atomic<uint8_t> stallModel_;   ///< STALL_MODEL value; default 0b00

    // GAP-NEW-E: SMMU_STATUSR / SMMU_IRQ_CTRL / SMMU_IRQ_CTRLACK registers (§6.3.45–6.3.47).
    // STATUSR bit 0 = DORMANT; always 0 in this SW model (no true dormant state).
    // IRQ_CTRL/CTRLACK use synchronous echo: setIrqCtrl() stores v in both atomics.
    std::atomic<uint32_t> statusr_;     ///< SMMU_STATUSR; init 0
    std::atomic<uint32_t> irqCtrl_;    ///< SMMU_IRQ_CTRL; init 0
    std::atomic<uint32_t> irqCtrlAck_; ///< SMMU_IRQ_CTRLACK; mirrors irqCtrl_ on write

    // CONF-GAP-24: ARM §4.6 CMD_RESUME outcome recording.
    // Maps STAG -> ResumeOutcome so software can observe what happened to each
    // stalled transaction after CMD_RESUME is processed.
    // Protected by stallQueueMutex_ (same lock as stallQueue_ for consistency).
    std::unordered_map<uint16_t, ResumeOutcome> resumeOutcomes_;  ///< STAG -> ResumeOutcome

    // BUG-ANALYSIS-5 fix: Stall-event pending buffer (ARM IHI0070G.b §7.4).
    // When the main event queue is full and a stall event arrives, the event is
    // placed here instead of growing eventQueue beyond maxEventQueueSize.
    // ARM §7.4: "a fault record from a stalled transaction is not discarded — an
    // event is reported for the stalled transaction when the queue is next
    // writable."  Stall-pending events do NOT trigger OVFLG.  The buffer is
    // drained into eventQueue (FIFO) whenever space becomes available.
    // Protected by queueMutex (same lock as eventQueue).
    std::deque<EventEntry> stallPending_;

    // BUG-AUDIT-8 fix: Replace std::chrono::steady_clock::now() (expensive vDSO
    // syscall, ~20-50 ns) with a cheap atomic counter (~1-2 ns, no lock interaction).
    // Each call to getCurrentTimestamp() atomically increments this counter and
    // returns the new value.  Timestamps are monotonically strictly increasing
    // across events generated in sequence.  This matches the Rust implementation.
    mutable std::atomic<uint64_t> eventTimestampCounter_;

    // Thread safety protection for SMMU controller - lock striping for scalability
    static constexpr size_t NUM_STREAM_STRIPES = 16;
    mutable std::array<std::mutex, NUM_STREAM_STRIPES> streamLockStripes;

    // BUG-03 fix: Dedicated mutex protecting all queue operations (eventQueue,
    // commandQueue, priQueue). A recursive_mutex is required because
    // processPRIQueue() calls submitCommand() while already holding the lock.
    // Lock order invariant: queueMutex must never be acquired while a
    // streamLockStripe is held.
    mutable std::recursive_mutex queueMutex;

    // BUG-CPP-1 fix: Serialization mutex for processCommandQueue().
    // The CMD_SYNC handler must temporarily release queueMutex before acquiring
    // a stream stripe lock (to avoid the ABBA deadlock: translate() holds
    // stripe → queueMutex; the old code held queueMutex → stripe).  During that
    // unlock window a second thread could enter processCommandQueue(), pick up
    // commands from the queue, and advance CONS.RD concurrently — violating the
    // ARM §3.5.1 ordering guarantee that a single logical consumer processes the
    // queue in submission order.
    //
    // cmdqProcessingMutex_ is acquired at the very start of processCommandQueue()
    // and held for its entire duration.  This ensures that at most one thread is
    // in the processing loop at any given time, regardless of how many times
    // queueMutex is temporarily released internally.
    //
    // Lock order: cmdqProcessingMutex_ → queueMutex (never the reverse).
    // signalGerror() and clearGerror() do NOT acquire cmdqProcessingMutex_;
    // they only need queueMutex, which is compatible with this ordering.
    mutable std::mutex cmdqProcessingMutex_;
    
    // BUG-NEW-CPP-1 fix: CAS-loop GERROR toggle helper.
    // Atomically signals the given error bits in SMMU_GERROR, toggling only bits
    // that are currently inactive (GERROR[x] == GERRORN[x]).  Eliminates the
    // TOCTOU race in the previous load-compare-fetch_xor pattern.
    // ARM IHI0070G.b §6.3.19: "SMMU does not toggle bit[x] if already active."
    void signalGerror(uint32_t bits);
    // CONF-GAP-17: Write ERR field of CMDQ_CONS atomically (§6.3.17)
    void writeCmdqConsErr(uint32_t errCode);
    // CONF-GAP-3: Validate StreamID against 2-level table bounds
    bool validateStreamID2Level(StreamID streamID) const;

    // Helper methods
    void recordFault(const FaultRecord& fault);
    bool validateSecurityState(SecurityState requestedState, SecurityState contextState) const;
    SecurityState determineContextSecurityState(StreamID streamID, PASID pasid) const;

    // Lock striping helper for stream map access
    size_t getStreamStripe(StreamID streamID) const {
        return streamID % NUM_STREAM_STRIPES;
    }
    
    // Configuration helper methods
    void applyConfiguration();
    VoidResult validateConfigurationUpdate(const SMMUConfiguration& config) const;
    
    // ARM SMMU v3 comprehensive fault syndrome generation methods
    FaultSyndrome generateFaultSyndrome(FaultType faultType, FaultStage stage, AccessType accessType, 
                                       SecurityState securityState, uint8_t faultLevel = 0, 
                                       PrivilegeLevel privLevel = PrivilegeLevel::EL1,
                                       uint16_t contextDescIndex = 0) const;
    uint32_t encodeFaultSyndromeRegister(FaultType faultType, FaultStage stage, uint8_t level, 
                                        bool writeAccess, bool instructionFetch) const;
    FaultStage determineFaultStage(const StreamConfig& config, FaultType faultType) const;
    PrivilegeLevel determinePrivilegeLevel(AccessType accessType, SecurityState securityState) const;
    AccessClassification classifyAccess(AccessType accessType) const;
    void recordComprehensiveFault(StreamID streamID, PASID pasid, IOVA iova, FaultType faultType,
                                 AccessType accessType, SecurityState securityState, FaultStage stage,
                                 uint64_t currentTime, uint8_t faultLevel = 0, uint16_t contextDescIndex = 0);
    FaultType classifyDetailedTranslationFault(IOVA iova, uint8_t tableLevel, bool formatError = false) const;
    void recordCacheHit() const;
    void recordCacheMiss() const;
    
    // Enhanced translation helpers (Task 5.2)
    TranslationResult performTwoStageTranslation(StreamID streamID, PASID pasid, IOVA iova,
                                               AccessType accessType, SecurityState securityState, StreamContext* streamContext, uint64_t currentTime);
    bool isTranslationCacheable(const TranslationResult& result) const;
    void cacheTranslationResult(StreamID streamID, PASID pasid, IOVA iova,
                               const TranslationResult& result, uint64_t currentTime,
                               uint16_t asid, uint16_t vmid, StreamWorld strw);
    void generateCacheKey(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState, uint64_t& cacheKey) const;

    // Stage-specific translation methods (Task 5.2)
    TranslationResult performBothStagesTranslation(StreamID streamID, PASID pasid, IOVA iova,
                                                  AccessType accessType, SecurityState securityState, StreamContext* streamContext, const StreamConfig& config, uint64_t currentTime);
    TranslationResult performStage1OnlyTranslation(StreamID streamID, PASID pasid, IOVA iova,
                                                  AccessType accessType, SecurityState securityState, StreamContext* streamContext, uint64_t currentTime);
    TranslationResult performStage2OnlyTranslation(StreamID streamID, PASID pasid, IOVA iova,
                                                  AccessType accessType, SecurityState securityState, StreamContext* streamContext, uint64_t currentTime);

    // Optimization 6: Inline validateAccessPermissions for performance
    inline bool validateAccessPermissions(const PagePermissions& permissions, AccessType accessType) const {
        // ARM SMMU v3 spec: Validate access permissions against requested operation
        switch (accessType) {
            case AccessType::Read:
                return permissions.read && !permissions.privilegedOnly;
            case AccessType::Write:
                return permissions.write && !permissions.privilegedOnly;
            case AccessType::Execute:
                return permissions.execute && !permissions.privilegedOnly;
            case AccessType::ReadWrite:
                // ARM §3.24: atomic read-modify-write is write-class; requires both
                // read and write permissions.
                return permissions.read && permissions.write && !permissions.privilegedOnly;
            case AccessType::ReadExecute:
                return permissions.read && permissions.execute && !permissions.privilegedOnly;
            case AccessType::ReadPrivileged:
                return permissions.read;
            case AccessType::WritePrivileged:
                return permissions.write;
            case AccessType::ExecutePrivileged:
                return permissions.execute;
            case AccessType::ReadWritePrivileged:
                return permissions.read && permissions.write;
            case AccessType::ReadExecutePrivileged:
                // Bug B fix: privileged combined read+execute (no privilegedOnly restriction).
                return permissions.read && permissions.execute;
            default:
                return false; // Unknown access type
        }
    }

    // Enhanced error handling and fault recovery methods (Task 5.2)
    void handleTranslationFailure(StreamID streamID, PASID pasid, IOVA iova,
                                 AccessType accessType, AccessType effectiveAccessType,
                                 SecurityState securityState, TranslationResult& result, uint64_t currentTime);
    FaultType classifyTranslationFault(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType, SecurityState securityState) const;
    void handleTranslationFaultRecovery(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState);
    void handlePermissionFaultRecovery(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType, SecurityState securityState);
    void handleAddressSizeFaultRecovery(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState);
    void handleAccessFaultRecovery(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType, SecurityState securityState);
    
    // Task 5.3: Helper methods for event and command processing
    // BUG-NEW-08/09 fix: processCommand and the internal executeInvalidationCommand
    // overload accept a queueLock reference so they can temporarily release
    // queueMutex before acquiring stripe locks, preventing ABBA deadlock.
    void processCommand(const CommandEntry& command, std::unique_lock<std::recursive_mutex>& queueLock);
    void executeInvalidationCommandLocked(const CommandEntry& command, std::unique_lock<std::recursive_mutex>& queueLock);
    // CONF-GAP-20: accessType parameter populates rnw/ind/pnu wire-format fields (§7.3).
    // Defaults to AccessType::Read (rnw=false, ind=false, pnu=false) for call sites
    // that do not have the access type available (e.g. config-fault generators).
    // GAP NEW-2: isStage2 and ipa added for ARM IHI0070G.b §7.3.13 compliance.
    // When isStage2==true the event record's S2 flag is set and ipa carries the
    // intermediate physical address (stage-1 output) at which stage-2 faulted.
    void generateEvent(EventType type, StreamID streamID, PASID pasid, IOVA address,
                       SecurityState securityState = SecurityState::NonSecure, bool isStall = false,
                       uint16_t stag = 0,
                       AccessType accessType = AccessType::Read,
                       bool isStage2 = false,
                       uint64_t ipaValue = 0);
    uint64_t getCurrentTimestamp() const;

    // GAP-L: ARM §9.1.4 / §6.3.40 GATOS_PAR FAULTCODE mapping helper.
    // Maps an EventType to the corresponding FAULTCODE byte for the GATOS_PAR register.
    static uint64_t mapEventTypeToGatosFaultCode(EventType t);
};

} // namespace smmu

#endif // SMMU_SMMU_H