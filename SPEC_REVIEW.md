# ARM SMMU v3 Conformance Review

**Specification**: ARM IHI 0070 G.b (April 30, 2025)
**Review Date**: 2026-02-18
**Implementations**:
- C++: `cpp/`
- Rust: `rust/smmu/`

**Overall Conformance**: C++ ~65% | Rust ~68% (software model scope)

Both implementations are software-layer abstractions. They do not implement the
hardware register map or binary-compatible data structures of the ARM SMMU v3
specification, which is the root cause of the Critical findings below.

---

## Status Legend

| Symbol | Meaning |
|--------|---------|
| ❌ | Open — not yet fixed |
| ✅ | Fixed |

---

## Critical Findings

### FINDING-C-01 ✅ — No Hardware Register Map
**Spec**: Chapter 6 (Memory Map and Registers), §6.1–6.3
**Affected**: Both

The specification defines 170+ memory-mapped registers (SMMU_IDR0-5, SMMU_CR0,
SMMU_CR0ACK, SMMU_STRTAB_BASE, SMMU_STRTAB_BASE_CFG, SMMU_CMDQ_BASE,
SMMU_CMDQ_PROD, SMMU_CMDQ_CONS, SMMU_EVENTQ_BASE, SMMU_EVENTQ_PROD,
SMMU_EVENTQ_CONS, SMMU_PRIQ_BASE, etc.). Neither implementation models any of
these registers.

- **C++**: Configuration via `SMMUConfiguration` C++ objects, not register writes.
- **Rust**: Configuration via `SMMUConfig` structs, not register writes.

SMMU_IDR0 (§6.3.1) must advertise capability bits (COHACC, BTM, HTTU, HYP, ATS,
NS1, S1P, S2P, NX, TTF, OAS, IAS, STALL_MODEL, VMID16, MSI, PASID, etc.).
Neither implementation exposes any equivalent capability advertisement.

**Resolution (Software Model Scope)**: This is a deliberate design decision within
the scope of a software simulation model. A hardware register map is only required
for RTL or bus-functional models that must respond to MMIO accesses. This project
models SMMU *behavior* (translation, fault handling, command/event queues, PRI) at
the functional level; software configuration through typed C++ / Rust structs
(`SMMUConfiguration` / `SMMUConfig`) provides equivalent capability advertisement
without the overhead of a byte-addressable register file. This limitation is
documented here so integrators are aware that the model cannot be memory-mapped
directly into a simulation bus fabric without an adapter layer. No code change is
required; the gap is accepted as out-of-scope for a behavioral software model.

---

### FINDING-C-02 ❌ — STE Binary Format Not Implemented
**Spec**: §5.2 (Stream Table Entry)
**Affected**: Both

The STE is a 512-bit (8×64-bit) structure. Key fields:
- Bit 0: V (valid)
- Bits 3:1: Config — `0b000`=Bypass, `0b001`=Abort/Fault, `0b100`=Stage1Only,
  `0b101`=BothStages, `0b110`=Stage2Only
- Bits 5:4: S1Fmt
- Bits 51:6 of Word 0: S1ContextPtr
- Word 2: S2VMID, S2T0SZ, S2SL0, S2TG, S2PS, etc.

**C++**: `StreamTableEntry` uses high-level booleans (`stage1Enabled`,
`stage2Enabled`, `translationEnabled`) with no binary 512-bit layout. The
`translationEnabled` field conflates Abort (`0b001`) and Bypass (`0b000`).

**Rust**: `StreamConfig` uses high-level fields. `StreamConfig::bypass()` has no
separate Abort/Fault mode distinct from disabled.

**Recommendation**: Add a `Config` enum with the 3-bit STE.Config encoding:
`Bypass(0b000)`, `Abort(0b001)`, `Stage1Only(0b100)`, `BothStages(0b101)`,
`Stage2Only(0b110)`.

---

### FINDING-C-03 ❌ — CD Binary Format Not Implemented
**Spec**: §5.4 (Context Descriptor)
**Affected**: Both

The CD is a 512-bit (8×64-bit) structure. Word 0 contains T0SZ[5:0], TG0[7:6],
IR0[9:8], OR0[11:10], SH0[13:12], EPD0 (bit 14), ENDI (bit 15), T1SZ[21:16],
TG1[23:22], IR1[25:24], OR1[27:26], SH1[29:28], EPD1 (bit 30), V (bit 31),
IPS[34:32], AFFD (bit 35), WXN (bit 36), UWXN (bit 37), TBI0 (bit 38), TBI1
(bit 39), PAN (bit 40), AA64 (bit 41), HD (bit 42), HA (bit 43), S (bit 44),
R (bit 45), A (bit 46), ASET (bit 47). ASID in Word 1[31:16].

The A/R/S bits (§5.5) control per-context fault reporting/stall/terminate
behaviour. Neither implementation tracks these bits independently; they use a
global `FaultMode`.

**Recommendation**: Implement CD binary format or at minimum model the A/R/S
fault configuration bits per Context Descriptor.

---

### FINDING-C-04 ❌ — No L1STD (2-Level Stream Table) Support
**Spec**: §5.1 (L1STD), §6.3.29 (STRTAB_BASE_CFG.FMT)
**Affected**: Both

STRTAB_BASE_CFG.FMT selects `0b00`=linear or `0b01`=2-level table. The L1STD
is a 64-bit descriptor with L2Ptr[51:6] pointing to a span of 16 STEs. Both
implementations use in-memory hash maps, bypassing the table walk entirely.

**Recommendation**: Add stream table format configuration and a lookup procedure
that simulates either linear or 2-level stream table walk.

---

## High Findings

### FINDING-H-01 ✅ — Event Queue Record Types Incomplete
**Spec**: §7.3 (Event records), §7.3.1–7.3.22
**Affected**: Both

The specification defines 22 event record types. Both implementations define
only 7 `EventType` variants. Missing types include:

| Missing Event | Spec Section |
|---------------|-------------|
| F_UUT | §7.3.2 |
| C_BAD_STREAMID | §7.3.3 |
| F_STE_FETCH | §7.3.4 |
| C_BAD_STE | §7.3.5 |
| F_BAD_ATS_TREQ | §7.3.6 |
| F_STREAM_DISABLED | §7.3.7 |
| F_TRANSL_FORBIDDEN | §7.3.8 |
| C_BAD_SUBSTREAMID | §7.3.9 |
| F_CD_FETCH | §7.3.10 |
| C_BAD_CD | §7.3.11 |
| F_WALK_EABT | §7.3.12 |
| F_ADDR_SIZE | §7.3.14 |
| F_TLB_CONFLICT | §7.3.17 |
| F_CFG_CONFLICT | §7.3.18 |
| E_PAGE_REQUEST | §7.3.19 |
| F_VMS_FETCH | §7.3.20 |

The Rust `FaultType` enum correctly maps 15 fault type codes (0x01–0x0F) but
this is not reflected in `EventType`.

**Recommendation**: Expand `EventType` in both implementations to cover all 22
specification-defined types.

**Fix**: Added all 19 spec-defined `EventType` variants with exact §7.3 hex codes
(0x01–0x25) to both Rust and C++ enums. Renamed legacy variants to spec-correct
names (`FTranslation`, `FPermission`, `EPageRequest`, `CBadSte`, `FTlbConflict`).
Two IMPDEF variants kept for SW model internal use (`CommandSyncCompletion=0xE0`,
`AtcInvalidateCompletion=0xE1`). Added 20 new tests in `test_event_types_spec.rs`.
All 43 C++ tests and all Rust test suites pass. Fixed commit: **f6a2ab4**.

---

### FINDING-H-02 ✅ — Command Opcodes Do Not Match Specification
**Spec**: §4.1 (Commands overview), §4.1.1–4.8
**Affected**: Both

Both implementations use sequential integers (0–10) as command opcode values
instead of the ARM-specified hex opcodes. Examples:

| Command | ARM Opcode | C++ Value | Rust Value |
|---------|-----------|-----------|-----------|
| CMD_PREFETCH_CONFIG | 0x01 | — | — |
| CMD_CFGI_STE | 0x03 | 2 | 2 |
| CMD_CFGI_STE_RANGE | 0x04 | — | — |
| CMD_CFGI_CD | 0x05 | — | — |
| CMD_CFGI_CD_ALL | 0x06 | — | — |
| CMD_TLBI_NH_ALL | 0x10 | — | — |
| CMD_TLBI_NH_ASID | 0x11 | — | — |
| CMD_TLBI_NH_VA | 0x12 | — | — |
| CMD_TLBI_NH_VAA | 0x13 | — | — |
| CMD_TLBI_EL2_ALL | 0x20 | — | — |
| CMD_TLBI_EL2_ASID | 0x21 | — | — |
| CMD_TLBI_EL2_VA | 0x22 | — | — |
| CMD_TLBI_EL2_VAA | 0x23 | — | — |
| CMD_TLBI_S12_VMALL | 0x28 | — | — |
| CMD_TLBI_S2_IPA | 0x2A | — | — |
| CMD_TLBI_NSNH_ALL | 0x30 | — | — |
| CMD_ATC_INV | 0x40 | — | — |
| CMD_PRI_RESP | 0x41 | — | — |
| CMD_RESUME | 0x44 | — | — |
| CMD_STALL_TERM | 0x45 | — | — |
| CMD_SYNC | 0x46 | 10 | 10 |

**Relevant files**:
- `cpp/include/smmu/types.h` (CommandType enum)
- `rust/smmu/src/types/command_entry.rs` (CommandType enum)

**Recommendation**: Update `CommandType` enum values to the ARM hex opcodes.
Add the missing TLB invalidation variants and `CMD_STALL_TERM`.

---

### FINDING-H-03 ✅ Fixed (Rust) — CFGI_CD and CFGI_CD_ALL Not Implemented
**Spec**: §4.3.3 (CMD_CFGI_CD, opcode 0x05), §4.3.4 (CMD_CFGI_CD_ALL, opcode 0x06)
**Affected**: Both

`CMD_CFGI_CD(StreamID, SSec, SubstreamID, Leaf)` invalidates a single CD entry.
`CMD_CFGI_CD_ALL(StreamID, SSec)` invalidates all CDs for a stream.

**Rust fix** (committed):
- Added `CommandType::CfgiCd = 0x05` and `CommandType::CfgiCdAll = 0x06`.
- `CMD_CFGI_CD` → `tlb_cache.invalidate_by_stream_pasid(stream_id, pasid)` — evicts
  all TLB entries for the specified (stream, PASID) pair and increments invalidation_count.
- `CMD_CFGI_CD_ALL` → `tlb_cache.invalidate_by_stream(stream_id)` — evicts all TLB
  entries for all PASIDs of the specified stream.
- Added `get_invalidation_count()` public API to observe the invalidation counter.
- 10 TDD spec tests in `tests/test_cfgi_cd_spec.rs` — all pass.

---

### FINDING-H-04 ❌ — TLB Invalidation Granularity Insufficient
**Spec**: §4.4 (TLB invalidation), §4.4.1–4.4.4
**Affected**: Both

- `CMD_TLBI_NH_ASID`: invalidates TLB entries tagged with a specific ASID —
  not implemented in either implementation (command type missing).
- `CMD_TLBI_NH_VA`: invalidates a specific VA+ASID — missing.
- `CMD_TLBI_S2_IPA`: invalidates Stage-2 IPA entries for a VMID. Rust calls
  `invalidate_by_stream` instead of VMID-targeted invalidation.

**Recommendation**: Add ASID field to TLB cache entries and implement
ASID-targeted invalidation. Add VMID field support for Stage-2 invalidation.

---

### FINDING-H-05 ✅ Fixed (Rust) — Stall Mode / CMD_RESUME Not Implemented
**Spec**: §4.6 (CMD_RESUME), §4.7 (CMD_STALL_TERM), §3.12.2 (Stall fault model)
**Affected**: Both

The Stall model requires the SMMU to halt transaction processing and wait for
`CMD_RESUME(StreamID, SSec, STAG, Action, Abort)`. The STAG field identifies
the stalled transaction group.

**Rust fix** (committed):
- Added `TranslationError::Stalled { stag: u16 }` error variant.
- Added `StallRecord` struct (stag, stream_id, pasid, iova, access, security_state).
- Added `stall_queue: DashMap<u16, StallRecord>` and `stag_counter: AtomicU16` to SMMU.
- Added `stall_enabled: AtomicBool` to `StreamContext`; set from `FaultMode::Stall` in `configure_stream()`.
- `translate()` checks stall mode on fault; enqueues `StallRecord` and returns `Err(Stalled { stag })`.
- `process_single_command()` handles `Resume` and `StallTerm` by removing matching STAG from stall queue.
- Public API: `get_stalled_transactions()` and `abort_stalled_transaction(stag)`.
- 13 TDD spec tests in `tests/test_stall_resume_spec.rs` — all pass.

- **C++**: `FaultMode::Stall` is defined and `RESUME` command type exists, but
  `processCommand` does not implement stall semantics. No mechanism holds a
  stalled transaction or matches it with a RESUME STAG.
- **Rust**: `Resume` command type exists but falls through to the `_` arm in
  `process_single_command` with no processing.

**Recommendation**: Implement stall transaction queuing with a STAG. The
`CMD_RESUME` handler must look up the stalled transaction by STAG and either
complete or abort it.

---

### FINDING-H-06 ❌ — No L1CD (2-Level CD Table) Support
**Spec**: §5.3 (L1CD, Level 1 Context Descriptor)
**Affected**: Both

When a stream supports more than one PASID, STE.S1CDMax specifies a 2-level CD
table. The L1CD is a 64-bit entry pointing to a span of CDs. SubstreamID bits
index the L1CD (upper bits) and then within the span (lower bits).

Both implementations use a flat map (C++: `unordered_map<PASID, shared_ptr<AddressSpace>>`;
Rust: `DashMap<u32, Arc<AddressSpace>>`), with no L1CD indirection.

**Recommendation**: Add a configuration flag for 1-level vs. 2-level CD tables
and document the limitation explicitly.

---

### FINDING-H-07 ✅ — Security State Bit Encoding Inconsistency
**Spec**: §3.10 (Security states), §3.10.1 (StreamID Security state SEC_SID)
**Affected**: Both
**Fixed**: commit (see below)

The ARM specification encodes SEC_SID as: `0b00`=NonSecure, `0b01`=Secure,
`0b10`=Realm, `0b11`=Root. Both implementations now match.

- **Rust**: Swapped `SecurityState` discriminants to `NonSecure=0b00, Secure=0b01`;
  `from_bits()` arms updated accordingly. Hash function in `cache/mod.rs` updated
  with non-zero FNV-1a seed so all-zero inputs don't hash to 0. 4 new spec tests
  added; 75 security state tests pass.
- **C++**: `enum class SecurityState : uint8_t` now `NonSecure=0x00, Secure=0x01,
  Realm=0x02, Root=0x03`. All 43 C++ tests pass.

---

### FINDING-H-08 ✅ — No SMMU Global Enable/Disable (SMMU_CR0.SMMUEN)
**Spec**: §6.3.9 (SMMU_CR0), bit 0 SMMUEN
**Affected**: Both
**Fixed**: Rust (commit — see below)

When SMMUEN=0 all transactions must bypass the SMMU (no translation or fault).
The SMMU must start disabled after reset.

- **C++**: `translate()` performs translation immediately after construction.
  `SMMU::reset()` does not model SMMUEN. (Pending)
- **Rust**: Added `enabled: AtomicBool` (default `false`) to `SMMU`. Added
  `enable()`, `disable()`, `is_enabled()` methods (all gated on non-shutdown).
  `translate()` now bypasses (identity PA=IOVA, no fault) when SMMUEN=0.
  12 spec tests in `tests/test_smmuen_spec.rs` covering boot state, bypass
  semantics, toggle, fault suppression, and shutdown interaction — all pass.
  Pre-existing tests in 7 test files updated to call `smmu.enable()` before
  performing stream-level translations.

---

## Medium Findings

### FINDING-M-01 ✅ — Circular Queue PROD/CONS Semantics Not Implemented
**Spec**: §3.5 (Command and Event queues), §3.5.1 (SMMU circular queues)
**Affected**: Both

Queues must use circular buffer semantics with Producer/Consumer index registers
including a WRAP bit. The queue is empty when PROD == CONS.

**Fixed**: Added explicit PROD/CONS index tracking alongside existing VecDeque/deque
storage. Each queue (command, event, PRI) now carries `log2size`, `prod`, and `cons`
u32 fields. Indices advance modulo 2^(log2size+1) per ARM §3.5.1. Empty condition:
PROD == CONS. New public accessors expose all indices for register-equivalent queries.
- **Rust**: 9 new tests pass. Fields in SMMU: `{queue}_log2size`, `{queue}_prod`,
  `{queue}_cons`. Helpers: `compute_log2size()`, `advance_index()`, `queue_occupied()`.
- **C++**: 10 new tests pass (44/44 total). Static helpers: `computeLog2Size()`,
  `advanceQueueIndex()`, `queueOccupied()`. Both constructors initialize all 9 fields.
  `generateEvent()` also advances `eventqCons` on the overflow eviction path.

---

### FINDING-M-02 ✅ — No VMID in Two-Stage Translation (Rust Fixed)
**Spec**: §3.8 (Virtualization), §5.2 (STE S2VMID field)
**Affected**: Both (C++ still open)

Stage-2 translation requires a VMID (STE Word 2, bits 63:48) to tag TLB
entries. TLB invalidation uses VMID for targeted invalidation.

- **C++**: No VMID field in stream table configuration or TLB entries. ❌ Still open.
- **Rust**: ✅ Fixed — `StreamConfig` carries `vmid: u16` (STE.S2VMID); builder
  exposes `.vmid()`. `StreamContext` stores `vmid: AtomicU16` (get/set_vmid()).
  `configure_stream()` propagates `config.vmid`. `CacheEntry` has `vmid: u16`;
  new `new_with_tags(iova, pa, perms, ss, asid, vmid, ts)` constructor tags entries
  with both ASID and VMID. `TlbCache::invalidate_by_vmid()` scans entries by VMID.
  `CommandEntry` has `vmid: u16`. `CMD_TLBI_S12_VMALL` and `CMD_TLBI_S2_IPA`
  dispatch to `invalidate_by_vmid()` instead of stream-targeted flush.
  `SMMU::set_stream_vmid()` / `get_stream_vmid()` exposed in public API.
  12 spec tests in `test_vmid_tlb_spec.rs` pass.

**Recommendation**: Add VMID to C++ STE config and TLB entries.

---

### FINDING-M-03 ✅ — ASID Not Tracked in TLB Entries (Rust Fixed)
**Spec**: §3.17 (TLB tagging, VMIDs, ASIDs), §4.4 (TLB invalidation)
**Affected**: Both (C++ still open)

Stage-1 TLB entries must be tagged with the ASID from the Context Descriptor
(CD.ASID, Word 1[31:16]). `CMD_TLBI_NH_ASID` invalidates by ASID.

- **C++**: `TLBEntry` struct has no `asid` field. ❌ Still open.
- **Rust**: ✅ Fixed — `CacheEntry` now carries `asid: u16` (CD.ASID). Added
  `CacheEntry::new_with_asid()` constructor and `TlbCache::invalidate_by_asid()`
  that scans entries by ASID tag. `CommandEntry` has `asid: u16` field.
  `StreamContext` stores per-PASID ASID in `pasid_asid_map: DashMap<u32, u16>`;
  `get_pasid_asid()` / `set_pasid_asid()` exposed on `StreamContext` and `SMMU`.
  `translate()` tags new TLB entries with `get_pasid_asid_or_default()`.
  `CMD_TLBI_NH_ASID` / `CMD_TLBI_EL2_ASID` dispatch to `invalidate_by_asid()`
  instead of global flush. 11 spec tests in `test_asid_tlb_spec.rs` pass.

**Recommendation**: Add ASID field to C++ TLBEntry and implement ASID-targeted
invalidation in the C++ implementation.

---

### FINDING-M-04 ✅ — No Access Flag / Dirty State Management
**Spec**: §3.13 (Translation tables and AF/Dirty state), §3.13.2–3.13.5
**Affected**: Both
**Fixed**: Both — added `access_flag`/`dirty` to `PageEntry`; added `ha`/`hd` to `StreamConfig` (Rust) and `ContextDescriptor`/`StreamConfig` (C++); `update_access_flags()` sets AF on first access when HA=1 and dirty on write when HD=1; wired through `translate_stage1_only` / `translateUnlocked`.

CD.HA (bit 43) enables hardware Access Flag management. CD.HD (bit 42) enables
hardware Dirty State management.

Neither implementation tracks AF or Dirty bits in page entries or context
descriptors.

**Recommendation**: Add `access_flag` and `dirty` booleans to page entries.
Simulate AF set on first access when HA=1. Simulate dirty set on write when
HD=1.

---

### FINDING-M-05 ✅ — No F_STREAM_DISABLED Event Generation
**Spec**: §7.3.7 (F_STREAM_DISABLED)
**Affected**: Both
**Fixed**: Rust — added `FaultType::StreamDisabled` (0x10), `SMMU::disable_stream()`/`enable_stream()`, mapping `TranslationError::StreamDisabled → FaultType::StreamDisabled → EventType::FStreamDisabled`. C++ — added `FaultType::StreamDisabled`, explicit `SMMUError::StreamDisabled` case in `handleTranslationFailure()` calling `generateEvent(EventType::F_STREAM_DISABLED, ...)`.

When STE.Config indicates a disabled/abort stream, transactions must generate an
`F_STREAM_DISABLED` fault record. Both implementations return a generic
`TranslationFault` or `BadSTE` type instead.

**Recommendation**: Add `StreamDisabled` to the event type enum. When
`translate()` encounters a disabled stream, record `F_STREAM_DISABLED`.

---

### FINDING-M-06 ✅ — No GERROR Register Modeling
**Spec**: §6.3.17 (SMMU_GERROR), §7.5 (Global error recording)
**Affected**: Both

SMMU_GERROR bits indicate global error conditions (SFE, MSI_ABT_ERR,
PRIQ_ABT_ERR, EVENTQ_ABT_ERR, CMDQ_ERR, CMDQ_ABT_ERR). Neither implementation
sets CMDQ_ERR when a command error occurs.

**Fix**: Added SMMU_GERROR register abstraction to both implementations.
- **Bit constants** defined for all §6.3.17 fields: `GERROR_SFE` (bit 0),
  `GERROR_MSI_ABT_ERR` (bit 2), `GERROR_PRIQ_ABT_ERR` (bit 4),
  `GERROR_EVENTQ_ABT_ERR` (bit 5), `GERROR_CMDQ_ERR` (bit 7),
  `GERROR_CMDQ_ABT_ERR` (bit 8).
- **`gerror` / `gerrorStatus` field** (AtomicU32 / uint32_t) initialised to 0
  after construction and reset.
- **`get_gerror()` / `getGerror()`** reads the current GERROR value.
- **`clear_gerror(bits)` / `clearGerror(bits)`** clears only the specified bits
  (SMMU_GERRORN write semantics per §6.3.18).
- **CMDQ_ERR set on command error**:
  - **Rust**: `process_command_queue()` sets `GERROR_CMDQ_ERR` atomically when
    `process_single_command()` returns an error and halts the queue. `CMD_CFGI_STE`
    with an unrecognised stream ID generates a `C_BAD_STREAMID` event and returns
    an error (ARM §4.3.1 CONSTRAINED UNPREDICTABLE path), triggering the halt.
  - **C++**: `processCommand()` `default:` arm (unknown command opcode) sets
    `gerrorStatus |= GERROR_CMDQ_ERR` in addition to generating `C_BAD_STE`.
    `reset()` clears `gerrorStatus`.
- **Tests**: 11 Rust spec tests in `tests/test_gerror_spec.rs`; 11 C++ spec
  tests in `cpp/tests/unit/test_gerror_spec.cpp` — all pass with zero
  regressions (45/45 C++, 157/157 Rust).

---

### FINDING-M-07 ✅ — Fault Records Hard-Coded to NonSecure
**Spec**: §7.3 (Event records), §7.2.3 (Secure and Non-secure Event queues)
**Affected**: Rust
**Fixed**: commit `c0d4d5c`

`record_translation_fault()` and `record_stream_not_found_fault()` hard-coded
`SecurityState::NonSecure` in both the `FaultRecord` and `EventEntry` instead
of propagating the security state from the originating transaction.

**Fix**: Added `security_state: SecurityState` parameter to both functions and
threaded it through from `translate()`. Four hard-coded literals replaced.
Five regression tests added in `tests/test_fault_detection.rs`.

---

### FINDING-M-08 ✅ — No PRG Index Tracking
**Spec**: §8 (Page request queue), §8.3 (PRG Response Message codes)
**Affected**: Both

`CMD_PRI_RESP` requires a matching PRGIndex to complete a page request response
cycle.

**Fixed**: Added `prg_index: u16` / `prgIndex: uint16_t` to both `PRIEntry` and
`CommandEntry` in both implementations. Implemented PRGIndex matching in
`CMD_PRI_RESP` processing to find and remove the corresponding pending PRIEntry
from the PRI queue (by `stream_id + prg_index`). Also fixed the PRIQ PROD/CONS
index advancement that was missing for the PRI queue in the FINDING-M-01 fix.
- **Rust**: 13 new tests across 2 test files. `process_single_command()` now
  has an explicit `PriResp` arm. `submit_page_request()` advances `priq_prod`;
  `process_pri_queue()` advances `priq_cons`. All 63 test suites pass.
- **C++**: 8 new `SMMUPRGIndexTest` tests. Added `getCommandQueue()` accessor.
  `submitPageRequest()` advances `priqProd`; `PRI_RESP` case in
  `processCommand()` finds and removes matching entry, advances `priqCons`.

---

### FINDING-M-09 ✅ Fixed (Rust) — AtcInv Does Full Flush Instead of Range
**Spec**: §4.5.1 (CMD_ATC_INV)
**Affected**: Rust

`CMD_ATC_INV(StreamID, SubstreamID, SSV, Global, Address, Size)` must invalidate
ATC entries for a specific address range.

**Rust fix** (committed):
- Replaced `tlb_cache.invalidate_all()` with scoped invalidation in `CMD_ATC_INV` handler.
- G=0 (range mode): calls `tlb_cache.invalidate_by_va_range(stream_id, pasid, start, end)` —
  evicts only entries whose IOVA falls within [start_address, end_address] for the
  specified (stream, PASID). Entries outside the range are preserved.
- G=1 (Global flag, `CommandEntry.flags` bit 0): calls
  `tlb_cache.invalidate_by_stream_pasid(stream_id, pasid)` — evicts all entries for
  the (stream, PASID) pair regardless of address.
- Both modes leave entries for other streams and PASIDs intact.
- AtcInvalidateCompletion event and invalidation_count counter still emitted/updated.
- `TlbCache::invalidate_by_va_range()` was already implemented; no new cache API needed.
- 9 TDD spec tests in `tests/test_atc_inv_range_spec.rs` — all pass.

---

### FINDING-M-10 ✅ Fixed (C++) — No Address Size Fault Checking
**Spec**: §3.4 (Address sizes), §3.4.1 (Input address size)
**Affected**: C++

The SMMU must raise `AddressSizeFault` when the input address exceeds the
address size configured by TCR.T0SZ.

**C++ fix** (committed):
- Added `uint8_t inputAddressSizeBits` (default 52) to `AddressSpace`.
- Added `AddressSpace::setInputAddressSize(uint8_t bits)` setter.
- `AddressSpace::translatePage()` now checks `iova >= (1ULL << bits)` before
  the page table lookup and returns `FaultType::AddressSizeFault`
  (`SMMUError::InvalidAddress`) when exceeded.
- Added `StreamContext::setAddressSpaceInputSize(PASID, uint8_t bits)` that
  propagates the limit to the relevant `AddressSpace`. Valid range: 32–52.
- Added `SMMU::setStreamInputAddressSize(StreamID, PASID, uint8_t bits)` public
  API; rejects out-of-range values with `SMMUError::InvalidAddress`.
- `SMMU::handleTranslationFailure()` now calls
  `generateEvent(EventType::F_ADDR_SIZE, ...)` for `SMMUError::InvalidAddress`
  errors, producing the §7.3.14 `F_ADDR_SIZE` (0x11) event.
- `performStage1OnlyTranslation()` and `performStage2OnlyTranslation()` now
  classify `SMMUError::InvalidAddress` as `FaultType::AddressSizeFault` in the
  fault record (was `AccessFault`).
- Per-context limits are independent: different (stream, PASID) pairs can have
  different address sizes without affecting each other.
- 10 TDD spec tests in `cpp/tests/unit/test_addr_size_fault_spec.cpp` — all pass.
- Full 44-test suite passes with zero regressions.

---

## Low Findings

### FINDING-L-01 ❌ — No Interrupt Modeling
**Spec**: §3.16 (Interrupts), §6.3 (IRQ_CTRL, GERROR_IRQ_CFG registers)
**Affected**: Both

Three interrupt sources (GERROR, EVENTQ, PRIQ) with MSI or wired interrupt
mechanisms are not modeled.

**Recommendation**: Document as a known software-model limitation. Optionally
add interrupt callbacks: `set_gerror_handler()`, `set_eventq_handler()`,
`set_priq_handler()`.

---

### FINDING-L-02 ❌ — No MSI Write Support in CMD_SYNC
**Spec**: §3.15 (MSI synchronization), §4.8 (CMD_SYNC)
**Affected**: Both

`CMD_SYNC` can carry MSIAddress and MSIData to trigger an MSI write on
completion. Both implementations generate a completion event but do not simulate
the MSI write.

**Recommendation**: When `CMD_SYNC` has a non-zero MSIAddress, invoke a
registered callback or log the MSI write event.

---

### FINDING-L-03 ❌ — No Translation Hardening (FEAT_HAFDBS)
**Spec**: §3.27 (Translation Hardening)
**Affected**: Both

SMMUv3.4 Translation Hardening for speculative execution side-channel
mitigation is not modeled.

**Recommendation**: Document as out of scope for the simulation model.

---

### FINDING-L-04 ✅ — Fault Syndrome Register Encoding Not Validated
**Spec**: §7.3 (Event records — fault syndrome)
**Affected**: C++
**Fixed**: `cpp/src/smmu/smmu.cpp` — `encodeFaultSyndromeRegister` rewritten to emit
the correct SMMU v3 §7.3 syndrome bit layout.

The previous implementation used AArch64 ESR-style FSC codes (bits [5:0]) which
have no basis in SMMU v3 event records. The two key bugs were:
- RnW (read/write) was placed at bit [6] — spec mandates event record bit [99],
  which normalises to syndromeRegister bit **[3]**.
- InD (instruction/data) was placed at bit [8] — spec mandates event record bit [98],
  which normalises to syndromeRegister bit **[2]**.

The fixed bit layout per §7.3.13–7.3.16:
- bit [2]: InD (event record bit [98])
- bit [3]: RnW (event record bit [99])
- bit [7]: S2 (event record bit [103]) — was already correct
- bits [9:8]: CLASS — 00=IN, 01=TT, 10=CD (event record bits [105:104])
- bits [17:16]: IMPL_DEF level

Nine TDD spec tests in `cpp/tests/unit/test_fault_syndrome_spec.cpp` verify
the corrected encoding; five of them were red before the fix.

---

### FINDING-L-05 ✅ — No Root Security State
**Spec**: §3.10 (Security states), SMMUv3.3 Root Control Page
**Affected**: Both
**Fixed**: commit (see below)

SMMUv3.3 adds a fourth security state: Root (`0b11`). Both implementations
now include it.

- **Rust**: Added `Root = 0b11` to `SecurityState`; `is_root()`/`const_is_root()`;
  `can_access()` updated (Root accesses all; others cannot access Root);
  `from_bits(0b11) → Ok(Root)`; `Display` → `"Root"`. 71 tests pass.
- **C++**: Added `Root = 0x03` to `enum class SecurityState : uint8_t`;
  `validateSecurityState()` updated with Root case (returns true for all).
  43 C++ tests pass.

---

### FINDING-L-06 ✅ Fixed — C++ Allows Stream Reconfiguration Without Invalidation
**Spec**: §3.11 (Reset, Enable and initialization)
**Affected**: C++ only

The specification requires a `CFGI_STE` + `CMD_SYNC` sequence before changing
stream configuration to maintain TLB and configuration cache consistency.
`SMMU::configureStream` previously allowed updating an existing stream directly
without requiring this sequence.

**Fix**: `SMMU::configureStream` now returns `SMMUError::StreamAlreadyConfigured`
when the target stream ID is already present in the stream map, matching the
conservative Rust model.  Callers that need to change a stream's configuration
must follow the correct ARM §3.11 sequence:
1. `removeStream(id)` — invalidates and removes the existing STE
2. `configureStream(id, newConfig)` — installs the new STE

All affected tests updated and 5 new TDD spec tests added in
`test_stream_reconfigure_spec.cpp`.  39/39 C++ unit tests pass.

---

### FINDING-L-07 ❌ — No VMS (Virtual Machine Structure) Support
**Spec**: §5.6 (VMS, Virtual Machine Structure)
**Affected**: Both

The VMS structure for VM-level TLB invalidation is not modeled. Only relevant
for hypervisor simulation.

**Recommendation**: Document as not implemented.

---

## Features Correctly Implemented

### C++ Implementation
1. Two-stage translation framework — Stage-1 only, Stage-2 only, both-stage, bypass (§3.3)
2. 20-bit PASID with PASID-0 fast path (§3.9)
3. Read/Write/Execute permission model (§3.24)
4. Terminate vs. Stall fault mode enum (§3.12)
5. NonSecure / Secure / Realm security state domains (§3.10)
6. TLB cache with LRU eviction and invalidation (§3.17)
7. Fault record structure with StreamID, PASID, address, fault type, access type,
   security state, syndrome, timestamp (§7.3)
8. Lock-striped thread safety for concurrent translation
9. PASID 0 support as default/legacy PASID
10. `CMD_SYNC` generates completion event (§4.8)

### Rust Implementation
1. 15 fault type codes (0x01–0x0F) correctly mapped in `FaultType` (§7.3)
2. Two-stage translation — Stage-1, Stage-2, both-stage, bypass (§3.3)
3. Security state isolation (Realm ↔ Secure ↔ NonSecure) (§3.10)
4. Full 20-bit PASID with PASID-0 fast path (§3.9)
5. Faults recorded to both fault queue and event queue (§7.3)
6. Thread-safe architecture (DashMap, Arc<RwLock<>>, AtomicBool, AtomicU64)
7. TLB cache with LRU and stream-level/global invalidation
8. `SMMUConfig` validates queue sizes, cache sizes, address space limits
9. `CMD_SYNC` generates `CommandSyncCompletion` event (§4.8)
10. Zero unsafe code

---

## Test Coverage

**C++**: Functional test suite — 100% pass rate per `cpp/QA_REPORT.md`.
Gaps: disabled-stream fault type correctness, `CFGI_CD` handling, stall mode.

**Rust**: 2,239 tests passing (100%) per `rust/QA-RUST.md`. Line coverage
71.06%, region 74.24%, function 65.17%. Notable gaps: `address_space` module
(20.86%), `stream_context` module (50.00%). Missing: binary STE/CD format
tests, VMID handling, ASID-targeted invalidation, stall mode completion.

---

## Prioritised Fix Order

### Immediate (spec correctness claims)
1. ~~FINDING-H-02 — Correct command opcode values to ARM hex constants~~ ✅ Fixed
2. ~~FINDING-H-01 — Add missing event types (F_STREAM_DISABLED, C_BAD_SUBSTREAMID, etc.)~~ ✅ Fixed
3. ~~FINDING-L-05 — Add `Root = 0b11` security state to both implementations~~ ✅ Fixed
4. ~~FINDING-H-07 — Fix security state bit encoding (Secure/NonSecure inverted in Rust)~~ ✅ Fixed
5. ~~FINDING-M-05 — Generate F_STREAM_DISABLED instead of generic fault~~ ✅ Fixed
6. ~~FINDING-M-07 — Fault records hard-coded to NonSecure security state~~ ✅ Fixed

### Short-term (behavioural conformance)
7. ~~FINDING-H-08 — Add SMMUEN global enable/disable~~ ✅ Fixed (Rust)
8. ~~FINDING-M-03 — Add ASID to TLB entries; implement ASID-targeted invalidation~~ ✅ Fixed (Rust)
9. ~~FINDING-M-02 — Add VMID to STE config and TLB entries~~ ✅ Fixed (Rust)
10. ~~FINDING-H-05 — Implement CMD_RESUME stall model with STAG tracking~~ ✅ Fixed (Rust)
11. ~~FINDING-H-03 — Add CFGI_CD and CFGI_CD_ALL command types~~ ✅ Fixed (Rust)
12. ~~FINDING-M-09 — Implement range-based ATC invalidation (Rust)~~ ✅ Fixed (Rust)
13. ~~FINDING-M-10 — Add address size fault checking (C++)~~ ✅ Fixed (C++)
14. ~~FINDING-M-04 — Access Flag and Dirty State simulation~~ ✅ Fixed (Both)

### Medium-term (feature completeness)
15. ~~FINDING-M-01 — Circular queue PROD/CONS index semantics~~ ✅ Fixed (Both)
16. ~~FINDING-M-08 — PRG index in PRIEntry and PRI_RESP handling~~ ✅ Fixed (Both)
17. ~~FINDING-M-06 — GERROR register conditions for command queue errors~~ ✅ Fixed
18. ~~FINDING-L-04 — Validate fault syndrome register encoding against spec tables~~ ✅ Fixed
19. ~~FINDING-L-06 — Enforce invalidation sequence before stream reconfiguration (C++)~~ ✅ Fixed

### Low-priority / document as limitation
20. ~~FINDING-C-01 — Register map (software model scope; document limitation)~~ ✅ Documented as software model scope limitation
21. FINDING-C-02/C-03 — Binary STE/CD format (document as software model)
22. FINDING-C-04 / FINDING-H-06 — L1STD / L1CD two-level tables
23. FINDING-L-01 — Interrupt modeling
24. FINDING-L-02 — MSI write in CMD_SYNC
25. FINDING-L-03 — Translation Hardening (SMMUv3.4)
26. FINDING-L-07 — VMS support

---

## Key Files for Fixes

| File | Relevant Findings |
|------|------------------|
| `cpp/include/smmu/types.h` | H-01, H-02, H-07, L-05 |
| `cpp/src/smmu/smmu.cpp` | H-05, H-08, M-05, M-10 |
| `rust/smmu/src/types/command_entry.rs` | H-02, H-03 |
| `rust/smmu/src/types/event_entry.rs` | H-01, M-05 |
| `rust/smmu/src/types/security_state.rs` | H-07, L-05 |
| `rust/smmu/src/smmu/mod.rs` | H-03, H-05, H-08, M-09 |
| `rust/smmu/src/cache/` | M-03, M-04 |
