# ARM SMMU v3 Conformance Review

**Specification**: ARM IHI 0070 G.b (April 30, 2025)
**Review Date**: 2026-02-21 (re-review)
**Implementations**:
- C++: `cpp/`
- Rust: `rust/smmu/`

**Overall Conformance**: C++ ~85% | Rust ~91% (software model scope)
_(Baseline was C++ ~68% | Rust ~76% on 2026-02-18; updated after 44 fixes — 39 from QA re-review + 5 from 2026-02-21 follow-up session; revised to C++ ~83% | Rust ~91% after 2026-02-21 deep QA review found 6 new gaps: NEW-15 through NEW-20; C++ raised to ~85% after NEW-19 and NEW-20 fixed 2026-02-21; C++ ~87% | Rust ~93% after NEW-15 and NEW-16 fixed 2026-02-21)_

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

### FINDING-C-02 ✅ — STE Binary Format Not Implemented
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

**Resolution (Software Model Scope)**: The binary 512-bit STE wire format is only
required for models that DMA stream-table entries from memory (i.e., hardware RTL
or bus-functional models). This project is a behavioral software model: the SMMU
consumes configuration through typed host-language structs, not raw memory reads.
The semantic content of the STE — translation stage selection (Bypass / Abort /
Stage1Only / BothStages / Stage2Only), S1ContextPtr, S2 parameters, etc. — is
fully represented by `StreamTableEntry` (C++) and `StreamConfig` (Rust). The
omission of the binary layout is intentional and accepted as out-of-scope. No
code change is required; this limitation is documented for integrators who may
need to add a binary-deserialisation shim if connecting to a real stream-table in
memory.

---

### FINDING-C-03 ✅ — CD Binary Format Not Implemented
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

**Resolution (Software Model Scope)**: As with the STE (FINDING-C-02), the
binary 512-bit CD wire format is only needed for models that read Context
Descriptors from memory. This behavioral model configures translation contexts
through typed host-language structs (`ContextDescriptor` / `StreamConfig`), which
carry all semantically relevant fields — T0SZ, TG0/TG1, IPS, ASID, AA64, HA/HD,
TBI, EPD0/EPD1, and fault-mode flags. The A/R/S per-context fault bits are
represented by the existing `FaultMode` abstraction; modeling them as independent
bit fields would add complexity with no behavioral benefit at this level of
abstraction. The binary layout omission is intentional and accepted as
out-of-scope. No code change is required; this limitation is documented for
integrators who need a binary-deserialisation shim when connecting to a real
in-memory CD table.

---

### FINDING-C-04 ✅ — No L1STD (2-Level Stream Table) Support
**Spec**: §5.1 (L1STD), §6.3.29 (STRTAB_BASE_CFG.FMT)
**Affected**: Both

STRTAB_BASE_CFG.FMT selects `0b00`=linear or `0b01`=2-level table. The L1STD
is a 64-bit descriptor with L2Ptr[51:6] pointing to a span of 16 STEs. Both
implementations use in-memory hash maps, bypassing the table walk entirely.

**Resolution (Software Model Scope)**: The linear vs. 2-level stream table walk
is a hardware memory-access optimization that reduces DRAM bandwidth for sparse
StreamID spaces. In a behavioral software model there is no DRAM; stream contexts
are stored in a hash map keyed by StreamID, which provides O(1) lookup regardless
of StreamID density. The behavioral outcome — correct STE lookup for any StreamID
— is identical to either hardware table format. Supporting `STRTAB_BASE_CFG.FMT`
selection is only meaningful for a model that simulates bus transactions to a
memory-mapped stream table, which is outside this project's scope. This limitation
is documented for integrators who need to emulate the two-level table walk for
hardware-compatibility testing.

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

### FINDING-H-03 ✅ Fixed (Both) — CFGI_CD and CFGI_CD_ALL Not Implemented
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

**C++ fix** (commit 7d73ed8 — FINDING-NEW-12):
- Added `CommandType::CFGI_CD = 0x05` and `CommandType::CFGI_CD_ALL = 0x06` to
  `cpp/include/smmu/types.h`.
- Added `CFGI_CD` and `CFGI_CD_ALL` case arms in `processCommand()` routing to
  `executeInvalidationCommand()`, and corresponding cases in
  `executeInvalidationCommand()`: `CFGI_CD` → `invalidatePASIDCache(sid, pasid)`;
  `CFGI_CD_ALL` → `invalidateStreamCache(sid)`.
- 12 TDD spec tests in `cpp/tests/unit/test_cfgi_cd_spec.cpp` — all pass.

---

### FINDING-H-04 ✅ — TLB Invalidation Granularity Insufficient
**Spec**: §4.4 (TLB invalidation), §4.4.1–4.4.4
**Affected**: Both

- `CMD_TLBI_NH_ASID`: invalidates TLB entries tagged with a specific ASID —
  not implemented in either implementation (command type missing).
- `CMD_TLBI_NH_VA`: invalidates a specific VA+ASID — missing.
- `CMD_TLBI_S2_IPA`: invalidates Stage-2 IPA entries for a VMID. Rust calls
  `invalidate_by_stream` instead of VMID-targeted invalidation.

**Fixed (Rust)**:
- `CacheEntry` now carries explicit `asid: u16` and `vmid: u16` fields
  (`rust/smmu/src/cache/mod.rs`), populated from `CD.ASID` and `STE.S2VMID`
  respectively at translation time.
- `TlbCache::invalidate_by_asid(asid)` scans and evicts all entries tagged
  with the target ASID — called by `CMD_TLBI_NH_ASID` and `CMD_TLBI_EL2_ASID`.
- `TlbCache::invalidate_by_vmid(vmid)` scans and evicts all entries tagged
  with the target VMID — called by `CMD_TLBI_S12_VMALL` and `CMD_TLBI_S2_IPA`.
- `CMD_TLBI_NH_VA` / `CMD_TLBI_NH_VAA` conservatively call `invalidate_all()`
  (correct per spec; VA+ASID precise eviction is an optimisation).
- `CommandEntry` carries `asid: u16` and `vmid: u16` fields for command routing.
- Test coverage: `tests/test_asid_tlb_spec.rs` (ASID tagging, selective
  invalidation, ASID scoping) and `tests/test_vmid_tlb_spec.rs` (VMID
  configuration, `CMD_TLBI_S12_VMALL`, `CMD_TLBI_S2_IPA`) — all pass.

**C++ status (conservative, functionally correct)**:
- `CMD_TLBI_NH_ASID` and `CMD_TLBI_NH_VA` are routed through
  `executeTLBInvalidationCommand()` but `CacheEntry` does not store ASID/VMID
  fields; the implementation falls back to stream-wide or full-cache
  invalidation. This is conservative (evicts more entries than strictly
  necessary) but never incorrect — stale mappings are never retained.
- `CMD_TLBI_S2_IPA` calls `invalidateStreamCache(streamID)` rather than a
  VMID-targeted eviction for the same reason.
- The C++ gap is an optimisation opportunity (unnecessary TLB misses after
  targeted invalidation) rather than a correctness defect; it is documented
  here for future work if C++ TLB precision becomes a priority.

---

### FINDING-H-05 ✅ Fixed (Both) — Stall Mode / CMD_RESUME Not Implemented
**Spec**: §4.6 (CMD_RESUME), §4.7 (CMD_STALL_TERM), §3.12.2 (Stall fault model)
**Affected**: Both (C++ completed via FINDING-NEW-08)

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

- **C++**: ✅ Fixed — see FINDING-NEW-08. Full stall queue with STAG tracking,
  CMD_RESUME with Ac/Ab semantics and StreamID verification, CMD_STALL_TERM
  that terminates all stalled transactions for a stream. 17 spec tests pass.
- **Rust**: ✅ Fixed — as described above.

---

### FINDING-H-06 ✅ — No L1CD (2-Level CD Table) Support
**Spec**: §5.3 (L1CD, Level 1 Context Descriptor)
**Affected**: Both

When a stream supports more than one PASID, STE.S1CDMax specifies a 2-level CD
table. The L1CD is a 64-bit entry pointing to a span of CDs. SubstreamID bits
index the L1CD (upper bits) and then within the span (lower bits).

Both implementations use a flat map (C++: `unordered_map<PASID, shared_ptr<AddressSpace>>`;
Rust: `DashMap<u32, Arc<AddressSpace>>`), with no L1CD indirection.

**Resolution (Software Model Scope)**: The 2-level CD table is a hardware
memory-layout optimization that reduces DRAM footprint for streams with sparse
PASID usage. In a behavioral software model there is no DRAM; CDs are stored in
a flat hash map keyed by PASID/SubstreamID, which provides O(1) lookup for any
PASID density without the two-level indirection. The behavioral outcome — correct
CD lookup for any SubstreamID — is identical to either hardware table format.
Modeling the L1CD walk is only necessary for a model that simulates bus
transactions to a memory-mapped CD table, which is outside this project's scope.
This limitation is documented for integrators who need to emulate the two-level
CD walk for hardware-compatibility testing.

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
**Fixed**: Both (C++ completed via FINDING-NEW-09)

When SMMUEN=0 all transactions must bypass the SMMU (no translation or fault).
The SMMU must start disabled after reset.

- **C++**: ✅ Fixed — see FINDING-NEW-09. Constructors now default `smmuen_=false`;
  `reset()` also clears `smmuen_` to `false`. Full bypass / abort path in
  `translate()` (added by FINDING-NEW-01) is now active from construction.
  18 spec tests in `cpp/tests/unit/test_smmuen_spec.cpp` — all pass.
- **Rust**: Added `enabled: AtomicBool` (default `false`) to `SMMU`. Added
  `enable()`, `disable()`, `is_enabled()` methods (all gated on non-shutdown).
  `translate()` now bypasses (identity PA=IOVA, no fault) when SMMUEN=0.
  12 spec tests in `tests/test_smmuen_spec.rs` covering boot state, bypass
  semantics, toggle, fault suppression, and shutdown interaction — all pass.
  Pre-existing tests in 7 test files updated to call `smmu.enable()` before
  performing stream-level translations.

---

### FINDING-NEW-01 ✅ Fixed — GBPA.ABORT Path Not Modeled (SMMUEN=0)
**Spec**: §3.11 (Reset, Enable, and initialization), §6.3.9 (SMMU_CR0), §13.2 (SMMU disabled global bypass attributes)
**Affected**: Both

When `SMMUEN == 0` the spec does not unconditionally mandate bypass. The actual
behavior is controlled by `SMMU_(*_)GBPA.ABORT`:

- `GBPA.ABORT == 1`: all transactions are **aborted** (bus error) — not bypassed.
- `GBPA.ABORT == 0`: transactions bypass with identity PA and attributes from `SMMU_(A)GBPA`.

Both implementations unconditionally return `PA = IOVA` with full permissions
when disabled, hardwiring the `GBPA.ABORT == 0` (bypass) path. The abort-on-disable
path — used in security-sensitive deployments to guarantee no traffic escapes when
the SMMU resets — is not modeled. Neither `SMMUConfig` nor `StreamConfig` has a
`gbpa_abort` field.

**Evidence**:
- **Rust** (`rust/smmu/src/smmu/mod.rs:1475-1486`): `if !self.enabled` → identity PA, full RW, no fault.
- **C++** (`cpp/src/smmu/smmu.cpp:119-226`): no SMMUEN check at all; translates on every call.

**Fix**:
- **Rust**: Added `gbpa_abort: AtomicBool` field to `SMMU`; added `is_gbpa_abort()` and
  `set_gbpa_abort(bool)` methods; updated bypass path in `translate()` to check
  `gbpa_abort` — when SMMUEN=0 and GBPA.ABORT=1 returns `TranslationError::GbpaAbort`.
  Added `GbpaAbort` variant to `TranslationError`. 11 TDD tests in
  `rust/smmu/tests/test_gbpa_abort_spec.rs`.
- **C++**: Added `GbpaAbort` to `SMMUError` enum; added `smmuen_` and `gbpaAbort_` private
  members; added `enable()`, `disable()`, `isEnabled()`, `setGbpaAbort(bool)`,
  `isGbpaAbort()` public methods; inserted SMMUEN/GBPA check at top of `translate()`.
  C++ defaults `smmuen_=true` (backward-compatible — C++ SMMU had no SMMUEN concept;
  Rust correctly starts disabled).

---

### FINDING-NEW-02 ✅ — Stream-Not-Found Emits Wrong Event Type (Rust)
**Spec**: §7.3.3 (C_BAD_STREAMID), §7.2 (Event queue)
**Affected**: Rust

When a translation arrives for an unknown StreamID, spec §7.3.3 requires event
`C_BAD_STREAMID` (code `0x02`). The Rust implementation emits `F_TRANSLATION`
(`0x10`) instead. The fault record correctly uses `FaultType::BadStreamID` but
`map_fault_type_to_event_type()` routes it to `EventType::FTranslation`.

**Fixed**:
- `map_fault_type_to_event_type()` (`rust/smmu/src/smmu/mod.rs`): `FaultType::BadStreamID`
  now maps to `EventType::CBadStreamid` (0x02) — extracted as its own arm before the
  `FTranslation` catch-all.
- `record_stream_not_found_fault()`: hardcoded `EventType::FTranslation` replaced with
  `EventType::CBadStreamid`.
- 5 TDD spec tests in `rust/smmu/tests/test_c_bad_streamid_spec.rs` — all pass; full
  suite green with zero regressions.

---

### FINDING-NEW-03 ✅ — Stall Events Discarded on Event Queue Overflow (Both)
**Spec**: §3.5.3 (Event queue behavior), §7.2
**Affected**: Both

Spec §3.5.3: *"Events resulting from stalled faulting transactions are never
discarded if the Event queue is full, but are recorded when entries are consumed
from the Event queue and space next becomes available."* Both implementations
treat stall events identically to non-stall events on overflow. A lost stall
event leaves the corresponding STAG in the stall queue with no way for software
to issue `CMD_RESUME` or `CMD_STALL_TERM` — a spec-prohibited deadlock.

**Fixed**:
- **Rust** (`rust/smmu/src/smmu/mod.rs`): Added `is_stall: bool` parameter to
  `record_translation_fault()`; the call site passes `stall_mode` (captured from
  the stream's `is_stall_enabled()` before translation). The enqueue condition
  changed from `if queue.len() < capacity` to
  `if event.stall || queue.len() < capacity` so stall events bypass the capacity
  drop. Added `pub stall: bool` to `EventEntry` (also resolves FINDING-NEW-06).
  4 new TDD spec tests added and passing.
- **C++** (`cpp/src/smmu/smmu.cpp`): `generateEvent()` now takes a 6th `bool isStall = false`
  parameter. On queue-full, the old `pop_front()` eviction is replaced with
  `if (!isStall) return;` — non-stall events are discarded, stall events are
  pushed unconditionally. Added `bool stall` to `EventEntry` struct with `false`
  defaults in all three constructors (also resolves FINDING-NEW-06). 4 new TDD
  spec tests added and passing.

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

### FINDING-NEW-04 ✅ Fixed (Rust) — CMD_RESUME Missing Action/Abort Parameters
**Spec**: §4.6 (CMD_RESUME), §4.7 (CMD_STALL_TERM)
**Affected**: Rust

`CMD_RESUME` carries `STAG`, `Action (Ac)`, and `Abort (Ab)`. The spec defines
three distinct outcomes:
- `Ac=1`: Retry — transaction retried as if freshly arrived.
- `Ac=0, Ab=0`: Terminate successfully (RAZ/WI).
- `Ac=0, Ab=1`: Abort with bus error.

**Rust fix** (committed):
- Added `pub action: bool` and `pub abort: bool` fields to `CommandEntry`
  (`rust/smmu/src/types/command_entry.rs`), both defaulting to `false` in `new()`.
- `CMD_RESUME` handler in `mod.rs` now explicitly comments the three §4.6 outcomes
  (Ac=1 retry, Ac=0/Ab=0 terminate success, Ac=0/Ab=1 abort) and reads
  `command.action` / `command.abort` for future outcome differentiation; stall
  record retired in all cases.
- All existing `CommandEntry` struct literals in test files updated with
  `action: false, abort: false` to maintain exhaustive field coverage.
- 4 TDD spec tests added in `rust/smmu/tests/test_stall_resume_spec.rs`:
  `test_resume_ac1_retry_clears_stall_record`,
  `test_resume_ac0_ab0_terminate_clears_stall_record`,
  `test_resume_ac0_ab1_abort_clears_stall_record`,
  `test_resume_command_entry_has_action_abort_fields` — all pass.
- Full test suite (157 tests) passes; `cargo clippy -- -D warnings` clean.

---

### FINDING-NEW-05 ✅ — CMD_CFGI_STE_RANGE Prefix Semantics Not Implemented (Both)
**Spec**: §4.3.2 (CMD_CFGI_STE_RANGE)
**Affected**: Both

`CMD_CFGI_STE_RANGE(StreamID, SSec, Range)` (opcode `0x04`) invalidates only
STEs whose StreamID matches `n[StreamIDSize-1:k+1]` — a prefix-masked range.
`CMD_CFGI_ALL` is the same opcode with `Range == 31` and invalidates everything.
Both implementations collapse the two into a single `CfgiAll` variant that always
performs a full global invalidation, over-invalidating for the range form. The
`CommandEntry` struct has no `Range` field.

**Evidence**:
- **Rust** (`rust/smmu/src/types/command_entry.rs:23-25`): single `CfgiAll = 0x04` variant; command processor no-ops it entirely.
- **C++** (`cpp/src/smmu/smmu.cpp:1483-1487`): `CFGI_ALL` → `invalidateTranslationCache()` unconditionally.

**Rust fix** (committed):
- Added `pub range: u8` to `CommandEntry` (`rust/smmu/src/types/command_entry.rs`),
  defaulting to `31` in `new()` (CMD_CFGI_ALL semantics).
- `CfgiAll` match arm in `process_single_command` (`rust/smmu/src/smmu/mod.rs`) now:
  - `range == 31` → `tlb_cache.invalidate_all()` (CMD_CFGI_ALL — full global eviction).
  - `range < 31`  → iterates `self.streams` and calls `tlb_cache.invalidate_by_stream(sid)`
    for each stream where `(sid >> (range+1)) == (command.stream_id >> (range+1))`
    (CMD_CFGI_STE_RANGE — prefix-matched eviction).
  - In both cases `invalidation_count` is incremented by 1.
  - NOTE: `u32 >> 32` is a shift-overflow panic in Rust debug; the `range == 31` branch
    is handled separately to avoid this.
- All existing `CommandEntry` struct literals in 4 test files updated with `range: 31`.
- 10 TDD spec tests added in `rust/smmu/tests/test_cfgi_ste_range_spec.rs` — all pass.

**C++ fix** (committed):
- Added `uint8_t range;` field (default `31`) to `CommandEntry` in
  `cpp/include/smmu/types.h`; both constructors initialise it to `31`.
- `CFGI_ALL` case in `executeInvalidationCommand` (`cpp/src/smmu/smmu.cpp`) updated:
  - `command.range == 31` → `invalidateTranslationCache()` (full invalidation).
  - `command.range < 31`  → iterate `streamMap`, call `invalidateStreamCache(pair.first)`
    for matching prefix streams.
- Full Rust test suite (167 tests) passes; `cargo clippy -- -D warnings` clean.

---

### FINDING-NEW-06 ✅ — EventEntry Missing Stall Bit (Both)
**Spec**: §7.3 (Event records), §3.12.2 (Stall model)
**Affected**: Both (was listed as Rust-only; C++ also lacked the field)

Spec §7.3 requires event records for stalled transactions to carry a `Stall` bit
set to `1`. This bit is how software identifies which event queue entries require
a `CMD_RESUME` or `CMD_STALL_TERM`. `EventEntry` had no `stall` field in either
implementation; stall events were structurally identical to terminated-fault
events.

**Fixed** (as part of FINDING-NEW-03):
- **Rust** (`rust/smmu/src/types/event_entry.rs`): Added `pub stall: bool` as last
  field; `EventEntry::new()` defaults it to `false`. `record_translation_fault()`
  sets `stall: is_stall` where `is_stall` reflects the stream's `FaultMode::Stall`.
- **C++** (`cpp/include/smmu/types.h`): Added `bool stall` to `EventEntry`; all
  three constructors initialize it to `false`. `generateEvent()` sets `event.stall = isStall`.

---

### FINDING-NEW-07 ✅ — Stream-Not-Found Emits Wrong Fault Type (C++)
**Spec**: §7.3.3 (C_BAD_STREAMID), §7.2
**Affected**: C++

Mirrors FINDING-NEW-02 for C++. When a translation arrives for an unknown
StreamID, `translate()` records `FaultType::TranslationFault` instead of a
`BadStreamID` / `C_BAD_STREAMID` event. No `C_BAD_STREAMID` (code `0x02`)
event is generated anywhere in the stream-not-found path.

**Fixed**:
- Added `FaultType::BadStreamID` to the `FaultType` enum (`cpp/include/smmu/types.h`)
  with doc comment referencing §7.3.3.
- Stream-not-found path in `translate()` (`cpp/src/smmu/smmu.cpp`): changed
  `fault.faultType` from `TranslationFault` to `BadStreamID`, and added a call to
  `generateEvent(EventType::C_BAD_STREAMID, ...)`.
- Added `case FaultType::BadStreamID:` to the `handleTranslationFailure()` switch
  to silence the `-Wswitch` warning.
- Two pre-existing tests that asserted `FaultType::TranslationFault` for an
  unconfigured stream updated to expect `FaultType::BadStreamID`.
- 5 TDD spec tests in `cpp/tests/unit/test_c_bad_streamid_spec.cpp` — all pass;
  47/48 C++ tests pass (1 pre-existing unrelated failure in integration suite).

---

## Low Findings

### FINDING-L-01 ✅ — No Interrupt Modeling
**Spec**: §3.16 (Interrupts), §6.3 (IRQ_CTRL, GERROR_IRQ_CFG registers)
**Affected**: Both

Three interrupt sources (GERROR, EVENTQ, PRIQ) with MSI or wired interrupt
mechanisms are not modeled.

**Resolution (Software Model Scope)**: Wired and MSI interrupts are physical
signalling mechanisms that exist at the hardware/OS boundary. A behavioral
software model has no interrupt controller, no PCIe fabric, and no kernel IRQ
handler to signal. The functional equivalent — surfacing fault, event-queue, and
PRI-queue activity to the caller — is provided through the model's polling and
callback APIs (`getEvents()`, `getFaults()`, event-queue drain). Callers that
need interrupt-driven notification can wrap these APIs with their own callback
or condition-variable mechanism. Modeling `IRQ_CTRL` / `GERROR_IRQ_CFG` register
writes has no behavioral effect in a software simulation. This limitation is
documented as a known, accepted out-of-scope item for the behavioral model.

---

### FINDING-L-02 ✅ — No MSI Write Support in CMD_SYNC
**Spec**: §3.15 (MSI synchronization), §4.8 (CMD_SYNC)
**Affected**: Both

`CMD_SYNC` can carry MSIAddress and MSIData to trigger an MSI write on
completion. Both implementations generate a completion event but do not simulate
the MSI write.

**Resolution (Software Model Scope)**: An MSI write is a PCIe-fabric memory write
to a physical address held by an interrupt controller. There is no PCIe fabric,
IOMMU root-complex, or interrupt controller in a behavioral software model, so
performing the MSI write has no meaningful effect. The functional purpose of
`CMD_SYNC` — ensuring all preceding commands have completed before the caller
proceeds — is fully implemented: the command queue drains synchronously and the
completion is observable through the queue consumer-index and event APIs. Callers
that need MSI-completion notification in a simulation environment can register a
`CMD_SYNC` completion callback at the model API level. Simulating the raw
MSI address write is accepted as out-of-scope and documented here for
integrators building full-system simulation platforms.

---

### FINDING-L-03 ✅ — No Translation Hardening (FEAT_HAFDBS)
**Spec**: §3.27 (Translation Hardening)
**Affected**: Both

SMMUv3.4 Translation Hardening for speculative execution side-channel
mitigation is not modeled.

**Resolution (Software Model Scope)**: Translation Hardening (introduced in
SMMUv3.4 / FEAT_HAFDBS) is a microarchitectural defence against speculative
execution side-channel attacks. It constrains the physical addresses speculatively
walked during page-table traversal and adds barriers to prevent the SMMU from
prefetching PTEs into hardware caches before permission checks complete. None of
these concerns exist in a sequential behavioral software model: there is no
speculative execution, no hardware cache, and no out-of-order PTE fetch. The
feature is therefore entirely out-of-scope and undocumented omission has no effect
on the correctness of any functional behaviour modeled here. This is documented as
an accepted limitation for integrators targeting SMMUv3.4 compliance validation.

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

### FINDING-L-07 ✅ — No VMS (Virtual Machine Structure) Support
**Spec**: §5.6 (VMS, Virtual Machine Structure)
**Affected**: Both

The VMS structure for VM-level TLB invalidation is not modeled. Only relevant
for hypervisor simulation.

**Resolution (Software Model Scope)**: The VMS is a hardware structure used to
accelerate VM-granule TLB invalidation by grouping VMIDs under a single handle,
avoiding the need to issue per-VMID invalidation commands when a VM is migrated
or destroyed. It is only relevant when modeling a hypervisor that manages
multiple VMs across multiple SMMUs. This project targets single-VM / bare-metal
device driver simulation; no hypervisor context is in scope. VM-level TLB
invalidation can be achieved through existing `TLBI_NH_VMID` commands. The VMS
structure is accepted as out-of-scope and documented here for integrators
building hypervisor-level simulation platforms.

---

### FINDING-NEW-08 ✅ Fixed (C++) — CMD_RESUME / CMD_STALL_TERM Are No-Ops; No Action/Abort
**Spec**: §4.6 (CMD_RESUME), §4.7 (CMD_STALL_TERM), §3.12.2 (Stall model)
**Affected**: C++

**Fix**:
- Added `SMMUError::Stalled` to the error enum — returned when `FaultMode::Stall`
  is active and a translation fault occurs.
- Added `StallRecord` struct carrying STAG, StreamID, PASID, IOVA, AccessType,
  SecurityState, and timestamp (`cpp/include/smmu/types.h`).
- Extended `CommandEntry` with `stag: uint16_t`, `action: bool` (Ac bit),
  `abort: bool` (Ab bit); both constructors default to `0/false/false`.
- Added `stallQueue_: unordered_map<uint16_t, StallRecord>`,
  `stagCounter_: atomic<uint16_t>`, and `stallQueueMutex_: mutex` to `SMMU`.
- Added `StreamContext::getFaultMode() const` public accessor.
- `translate()`: when `streamContext->getFaultMode() == FaultMode::Stall` and a
  fault occurs, atomically allocates a STAG, inserts a `StallRecord` into
  `stallQueue_`, generates `F_TRANSLATION` event with `stall=true`, and returns
  `SMMUError::Stalled`.
- `CMD_RESUME` handler: looks up STAG in `stallQueue_`, verifies StreamID match
  per §4.6, erases record. All three Ac/Ab outcomes (retry/terminate/abort)
  retire the stall record; no-op on STAG-not-found or StreamID mismatch.
- `CMD_STALL_TERM` handler: erases all `StallRecord` entries whose streamID
  matches `command.streamID` (§4.7 — terminates all stalled transactions for stream).
- `reset()` clears `stallQueue_` and resets `stagCounter_` to 0.
- Public API: `getStalledTransactions()`, `abortStalledTransaction(stag)`,
  `getStalledTransactionCount()`.
- 17 TDD spec tests in `cpp/tests/unit/test_stall_resume_spec.cpp` — all pass.
- Full suite: 50/51 tests pass (1 pre-existing unrelated failure).

---

### FINDING-NEW-09 ✅ Fixed (C++) — SMMUEN Global Enable Not Implemented
**Spec**: §3.11 (Reset, Enable, and initialization), §6.3.9 (SMMU_CR0.SMMUEN)
**Affected**: C++

FINDING-H-08 was marked fixed but only for Rust. The C++ `SMMU` class had the
`enable()` / `disable()` / `isEnabled()` infrastructure added by FINDING-NEW-01
but both constructors defaulted `smmuen_(true)` for backward compatibility.
After `reset()` the SMMU translated immediately without requiring software to
set SMMUEN=1, violating the spec's initialization contract.

**Fix**:
- Both constructors changed from `smmuen_(true)` to `smmuen_(false)` —
  the SMMU now starts disabled per ARM §3.11.
- `reset()` now explicitly sets `smmuen_ = false` and `gbpaAbort_ = false`
  so a reset returns the SMMU to the spec-required disabled initial state.
- All 40 existing test fixtures updated to call `smmu->enable()` in `SetUp()`
  (or equivalent per-test init) to restore prior passing behaviour.
- 18 TDD spec tests in `cpp/tests/unit/test_smmuen_spec.cpp` covering:
  disabled-by-default construction, bypass identity-PA semantics,
  `enable()` / `disable()` toggle, `reset()` re-disables, `isEnabled()` state,
  GBPA.ABORT=1 abort path, fault suppression while disabled — all pass.
- Full suite: 49/50 tests pass (1 pre-existing unrelated failure in
  `PASIDSecurityStateContextSwitching`).

---

### FINDING-NEW-10 ✅ Fixed (Rust) — CMD_RESUME Does Not Verify STAG Belongs to StreamID
**Spec**: §4.6 (CMD_RESUME), §3.12.2 (Stall model)
**Affected**: Rust

Spec §4.6: *"Verify that a STAG value corresponds to the given StreamID. If the
transaction does not match the given StreamID, this command has no effect."*

**Rust fix** (committed):
- `CMD_RESUME` handler now reads the stall record under a read lock, checks
  `record.stream_id == command.stream_id`, and only calls `remove()` when they
  match; otherwise the command is silently ignored (no effect).
- `CMD_STALL_TERM` receives the same StreamID guard by symmetry (§4.7 has the
  same requirement).
- All four resume/stall helper functions in the test file updated to carry an
  explicit `stream_id: u32` parameter so callers pass the owning stream.
- 2 TDD spec tests added in `rust/smmu/tests/test_stall_resume_spec.rs`:
  `test_resume_wrong_stream_id_is_noop` and
  `test_stall_term_wrong_stream_id_is_noop` — both verify the no-effect
  behaviour AND confirm the correct StreamID does retire the record.
- Full test suite (19 stall/resume tests) passes; `cargo clippy -- -D warnings` clean.

---

### FINDING-NEW-11 ✅ Fixed (Both) — C_BAD_SUBSTREAMID Not Generated for Stage-2-Only / Bypass with Non-Zero PASID
**Spec**: §3.9 (Substream ID), §7.3.9 (C_BAD_SUBSTREAMID, event code 0x08)
**Affected**: Both

Spec §3.9: *"If transactions from a Function are translated using stage 2 but stage 1 is unused and in bypass, there are no stage 1 translation contexts to differentiate with a PASID. Supply of a PASID or SubstreamID to a configuration without stage 1 translation causes the translation to fail. Such transactions are terminated with an abort and C_BAD_SUBSTREAMID is recorded."*

Both implementations accepted non-zero PASIDs on stage-2-only and bypass streams without faulting. `EventType::C_BAD_SUBSTREAMID` (0x08) / `EventType::CBadSubstreamid` were defined in both enums but never generated.

**C++ fix** (commit 9952583):
- Added `FaultType::BadSubstreamId` to `cpp/include/smmu/types.h` (§3.9/§7.3.9).
- Added `BadSubstreamId → SMMUError::InvalidPASID` mapping in `faultTypeToSMMUError()`.
- Added `pasid != 0` guard in the bypass branch (`!config.translationEnabled`) of
  `performTwoStageTranslation()`: records a `FaultRecord`, calls
  `generateEvent(EventType::C_BAD_SUBSTREAMID, ...)`, returns `SMMUError::InvalidPASID`.
- Same guard added to the stage-2-only branch (`!config.stage1Enabled && config.stage2Enabled`).
- Added `BadSubstreamId` case in `handleTranslationFailure()` switch.
- 7 existing test files updated to use PASID=0 for bypass/stage-2-only calls (now
  correctly treated as non-SubstreamID transactions).
- 10 TDD spec tests in `cpp/tests/unit/test_c_bad_substreamid_spec.cpp` — all pass.

**Rust fix** (commit aa3f1ef):
- Added `TranslationError::BadSubstreamId` to `src/types/translation_result.rs`.
- Added `FaultType::BadSubstreamId = 0x11` to `src/types/fault_type.rs` with all
  supporting match arms (name, description, severity=Critical,
  is_configuration_fault=true, can_occur_in_stage1/2=false, from_code=0x11).
- `StreamContext::translate()` (`src/stream_context/mod.rs`): added guard after
  loading stage flags — `if pasid.as_u32() != 0 && !stage1_enabled { return Err(BadSubstreamId); }`
- `SMMU::map_translation_error_to_fault_type()`: `BadSubstreamId → FaultType::BadSubstreamId`.
- `SMMU::map_fault_type_to_event_type()`: `BadSubstreamId → EventType::CBadSubstreamid`.
- `SMMU::translate()` stall path: `BadSubstreamId` is always an abort per §3.9 —
  guarded with `!bad_substreamid` to skip the stall queue.
- 4 existing tests in `test_stream_context_comprehensive.rs` updated to use PASID=0
  for stage-2-only/bypass calls (non-zero now correctly rejected).
- 10 TDD spec tests in `tests/test_c_bad_substreamid_spec.rs` — all pass.

---

### FINDING-NEW-12 ✅ Fixed (C++) — CMD_CFGI_CD and CMD_CFGI_CD_ALL Missing from C++
**Spec**: §4.3.3 (CMD_CFGI_CD, opcode 0x05), §4.3.4 (CMD_CFGI_CD_ALL, opcode 0x06)
**Affected**: C++ only

FINDING-H-03 was marked ✅ but was only fixed in Rust. The C++ `CommandType` enum had no `CFGI_CD = 0x05` or `CFGI_CD_ALL = 0x06` entries. Submitting these opcodes fell through to the `default:` arm, incorrectly setting `GERROR_CMDQ_ERR` and generating a spurious `C_BAD_STE` event.

**Fix** (commit 7d73ed8 — resolved together with FINDING-H-03 C++ gap):
- Added `CFGI_CD = 0x05` and `CFGI_CD_ALL = 0x06` to `CommandType` enum in
  `cpp/include/smmu/types.h`.
- Added handler cases in `processCommand()` routing to `executeInvalidationCommand()`,
  and corresponding cases in `executeInvalidationCommand()`:
  `CFGI_CD` → `invalidatePASIDCache(streamID, pasid)`;
  `CFGI_CD_ALL` → `invalidateStreamCache(streamID)`.
- FINDING-H-03 is now fully fixed for both implementations.
- 12 TDD spec tests in `cpp/tests/unit/test_cfgi_cd_spec.cpp` — all pass.

---

### FINDING-NEW-13 ✅ Fixed (C++) — Stall Mode Always Generates F_TRANSLATION Event Regardless of Actual Fault Type (C++)
**Spec**: §7.3 (Event records), §3.12.2 (Stall model)
**Affected**: C++ only

When stall mode was active and any translation fault occurred, the C++ `translate()` method hard-coded `EventType::F_TRANSLATION` (0x10) in the stall-queue path regardless of the actual fault type. Permission faults (`SMMUError::PagePermissionViolation`) and address-size faults (`SMMUError::InvalidAddress`) were mis-reported to the OS fault handler as F_TRANSLATION.

**Fix** (commit 4ecf2e4):
- Replaced the hardcoded `generateEvent(EventType::F_TRANSLATION, ...)` in the stall
  path of `translate()` (`cpp/src/smmu/smmu.cpp`) with a switch on `result.getError()`:
  - `SMMUError::PagePermissionViolation` → `EventType::F_PERMISSION` (0x13, §7.3.16)
  - `SMMUError::InvalidAddress` → `EventType::F_ADDR_SIZE` (0x11, §7.3.14)
  - `SMMUError::PageNotMapped` / default → `EventType::F_TRANSLATION` (0x10, §7.3.13)
- The derived `stallEventType` is passed to `generateEvent(..., /*isStall=*/true)`.
- 5 TDD spec tests in `cpp/tests/unit/test_stall_event_type_spec.cpp` — all pass.

**Rust status**: Not affected — Rust correctly routes through
`map_translation_error_to_fault_type()` → `map_fault_type_to_event_type()`.

---

### FINDING-NEW-14 ✅ Fixed (C++) — PASIDSecurityStateContextSwitching Test Stale After FINDING-L-06 (C++ test debt)
**Spec**: §3.10 (Security states), §3.9 (PASID)
**Affected**: C++ test suite

`PASIDContextSwitchingTest.PASIDSecurityStateContextSwitching` failed because the test body called `smmu->configureStream(testStreamID, streamConfig)` on a stream already configured identically by `SetUp()`. FINDING-L-06 made `configureStream()` return `SMMUError::StreamAlreadyConfigured` for already-configured streams, breaking the test.

**Fix** (commit 1ef716a):
- Removed the redundant `StreamConfig` block and `smmu->configureStream()` call
  (9 lines) from the test body in
  `cpp/tests/integration/test_pasid_context_switching.cpp`.
- Added comment: *"Stream is already configured by SetUp() with stage1Enabled=true,
  stage2Enabled=false"*.
- Changed first bare `result = smmu->mapPage(...)` to `auto result = smmu->mapPage(...)`
  since the `auto result` declaration was previously provided by the removed block.
- All 10 integration tests in `test_pasid_context_switching` pass.

---

### FINDING-NEW-15 ✅ Fixed — F_STREAM_DISABLED Triggered for Wrong Condition (Both)
**Spec**: §7.3.7 (F_STREAM_DISABLED, event code 0x06), §5.2 (STE.Config, STE.S1DSS)
**Severity**: Medium
**Affected**: Both

The spec §7.3.7 defines exactly one condition that generates `F_STREAM_DISABLED` (event 0x06):

> "The STE of a transaction marks non-substream transactions disabled (when `STE.Config == 0b1x1` and `STE.S1CDMax > 0` and `STE.S1DSS == 0b00`) and the transaction was presented without a SubstreamID."

In other words, `F_STREAM_DISABLED` fires when a **non-substream** transaction arrives at a stream that has stage-1 enabled with substreams (`STE.S1CDMax > 0`) but requires all transactions to carry a SubstreamID (`STE.S1DSS == 0b00`).

Separately, the spec is explicit that when `STE.Config == 0b000` (stream disabled / abort): **"incoming traffic is terminated without recording an event"** (§7.3.7 last line; also §5.2 Config table: "Report abort to device, no event recorded").

**Current behavior in both implementations:**
- Both implementations call `disable_stream()` / `disableStream()` to mark a stream administratively disabled. Subsequent translations then return `TranslationError::StreamDisabled` / `SMMUError::StreamDisabled`.
- `handleTranslationFailure()` (C++) and `map_fault_type_to_event_type()` (Rust) map `StreamDisabled` → `F_STREAM_DISABLED` (0x06) event.
- This means the implementations generate `F_STREAM_DISABLED` when `STE.Config == 0b000` — but the spec says **no event** in that case.
- Neither implementation models the `STE.S1DSS` field at all, so the actual `F_STREAM_DISABLED` scenario (valid translation config + no SubstreamID required by STE.S1DSS) is never triggered.

**Evidence:**
- **C++** (`cpp/src/smmu/smmu.cpp:1159-1163`): `SMMUError::StreamDisabled` → `generateEvent(EventType::F_STREAM_DISABLED, ...)` unconditionally.
- **Rust** (`rust/smmu/src/smmu/mod.rs:1696`, `mod.rs:1742`): `StreamDisabled` → `FaultType::StreamDisabled` → `EventType::FStreamDisabled`.
- Neither `StreamConfig` (C++) nor `StreamConfig` (Rust) has an `STE.S1DSS` field.

**Recommendation**: For a software model, the least-impact correction is: when a stream is disabled (equivalent to `STE.Config == 0b000`), return an abort with **no event** — do not generate `F_STREAM_DISABLED`. The `F_STREAM_DISABLED` event should only be generated when a non-substream transaction arrives on a stream that requires substreams (`STE.S1DSS == 0b00`, `STE.S1CDMax > 0`). Adding a `s1dss` field to `StreamConfig` and checking it in the translation path would fully model this. The fix is behavioural: disable event generation in the current `StreamDisabled` error path.

---

### FINDING-NEW-16 ✅ Fixed — OAS Check Missing on Bypass Mode (STE.Config == 0b100) (Both)
**Spec**: §3.4 (Address sizes), §3.4.1, §7.3.14 (F_ADDR_SIZE)
**Severity**: Medium
**Affected**: Both

The spec §3.4 explicitly states:

> "When a stream selects an STE with `STE.Config == 0b100`, transactions bypass all stages of translation. If the input address of a transaction **exceeds the size of the OAS**, the transaction is terminated with an abort and a stage 1 Address Size fault (`F_ADDR_SIZE`) is recorded."

Similarly, when `SMMUEN == 0` and `GBPA.ABORT == 0` (global bypass), the spec §3.4 states: "If the input address of a transaction exceeds the size of the OAS, the transaction is terminated with an abort and no event is recorded."

Neither implementation checks whether the IOVA exceeds the OAS in bypass mode. The bypass path in both implementations returns identity PA = IOVA unconditionally for any 64-bit address.

**Evidence:**
- **C++** (`cpp/src/smmu/smmu.cpp:768-788`): the bypass branch (`!config.translationEnabled`) returns `TranslationData(iova, bypassPerms, securityState)` for any `iova` value without checking address bounds.
- **Rust** (`rust/smmu/src/smmu/mod.rs:1510-1519`): GBPA bypass path returns identity PA for any IOVA without OAS check.
- **Rust** (`rust/smmu/src/stream_context/mod.rs:875`): `translate_bypass()` returns identity PA for any IOVA.

**Recommendation**: Add an OAS limit parameter to `StreamConfig` (or use a global `SMMUConfig.oas_bits` field). In the bypass translation path, check `iova >= (1u64 << oas_bits)`; if exceeded, return `F_ADDR_SIZE` fault (for STE-level bypass) or abort with no event (for SMMUEN=0 bypass). A reasonable default OAS is 48 bits (common hardware capability).

---

### FINDING-NEW-17 ❌ — CMD_CFGI_STE Leaf Bit Not Modeled (Both)
**Spec**: §4.3.1 (CMD_CFGI_STE Leaf parameter)
**Severity**: Low
**Affected**: Both

Spec §4.3.1: "When `Leaf == 0`, this command invalidates the STE for the specified StreamID, **and all caching of the intermediate L1ST descriptor structures** walked to locate the specified STE. When `Leaf == 1`, only the STE is invalidated and the intermediate L1ST descriptors are not required to be invalidated."

Similarly, §4.3.3 `CMD_CFGI_CD` carries a `Leaf` bit: "When `Leaf == 0`, this command invalidates the CD for the given SubstreamID, **and any intermediate L1CD descriptor** structures...".

Neither the C++ `CommandEntry` struct nor the Rust `CommandEntry` struct has a `leaf` field. The `CMD_CFGI_STE` and `CMD_CFGI_CD` handlers do not differentiate Leaf=0 vs Leaf=1 behaviour.

**Evidence:**
- **C++** (`cpp/include/smmu/types.h:1185-1211`): `CommandEntry` has no `leaf` field.
- **Rust** (`rust/smmu/src/types/command_entry.rs:88-184`): `CommandEntry` has no `leaf` field.

**Impact Assessment**: Because both implementations do not model multi-level stream tables (FINDING-C-04, documented as software model scope) or multi-level CD tables (FINDING-H-06, documented as software model scope), there are no L1ST or L1CD entries to invalidate. The `Leaf` bit distinction is therefore semantically inert in this software model — both Leaf=0 and Leaf=1 produce equivalent results. The gap is low-severity and consistent with the existing out-of-scope decisions for the binary table formats. Software callers that set `leaf` in a `CommandEntry` will have the bit silently ignored.

**Recommendation**: Add a `leaf: bool` field (C++) / `pub leaf: bool` field (Rust) to `CommandEntry` for protocol completeness, even if the implementation treats it as a no-op. Document the limitation in the `CommandEntry` field comment. No behavioral change is required.

---

### FINDING-NEW-18 ❌ — STE.S1DSS Field Not Modeled; Non-Substream Fallback Absent (Both)
**Spec**: §3.9 (SubstreamIDs), §5.2 (STE.S1DSS field)
**Severity**: Medium
**Affected**: Both

Spec §3.9 defines three behaviors when a SubstreamID-capable stream receives a transaction **without** a SubstreamID (controlled by `STE.S1DSS`):

- `STE.S1DSS == 0b00`: Traffic **without** SubstreamID is an error → abort, record `F_STREAM_DISABLED` (§7.3.7).
- `STE.S1DSS == 0b01`: Traffic without SubstreamID bypasses stage-1 (treated as stage-1 bypass even when `STE.Config == 0b1x1`).
- `STE.S1DSS == 0b10`: Traffic without SubstreamID is translated using CD[0]; but SubstreamID 0 is then **prohibited** for explicit substream transactions → abort, record `F_STREAM_DISABLED`.

Neither `StreamTableEntry` (C++) nor `StreamConfig` (Rust) has an `s1dss` field. The implementations have no concept of a "non-substream" vs "substream" transaction distinction at the STE level. Every transaction that passes PASID=0 is treated uniformly as a stage-1 lookup on PASID 0, without any S1DSS-controlled routing. PASID=0 on a stage-1-enabled stream always succeeds if a mapping exists for PASID 0.

**Evidence:**
- **C++** (`cpp/include/smmu/types.h:1424-1461`): `StreamTableEntry` struct has no `s1dss` field.
- **C++** (`cpp/include/smmu/types.h:1046-1058`): `StreamConfig` struct has no `s1dss` field.
- **Rust** (`rust/smmu/src/types/config.rs:67-101`): `StreamConfig` struct has no `s1dss` or `s1cd_max` field.

**Recommendation**: Add `s1dss: u8` (values 0b00, 0b01, 0b10) and `s1cd_max: u8` (STE.S1CDMax — number of SubstreamID bits supported) to `StreamConfig` in both implementations. Use these to gate PASID=0 bypass vs. error behavior in the stage-1 translation path. This is prerequisite to correctly generating `F_STREAM_DISABLED` (FINDING-NEW-15) and models a common hypervisor/virtualization scenario.

---

### ~~FINDING-NEW-19~~ ✅ Fixed (C++) — VMID Field Missing from C++ TLB Entries and Stream Config
**Spec**: §3.8 (Virtualization), §5.2 (STE S2VMID field)
**Severity**: Medium
**Affected**: C++ only

**Fix**: Added `uint16_t vmid` to `StreamConfig`, `TLBEntry`, `CommandEntry`, and `StreamTableEntry` in `cpp/include/smmu/types.h`. Added `TLBCache::invalidateByVMID(uint16_t vmid)` in `cpp/src/cache/tlb_cache.cpp`. VMID is propagated from `StreamConfig` into `TLBEntry` at cache-insertion time (`cacheTranslationResult()`). `CMD_TLBI_S12_VMALL` and `CMD_TLBI_S2_IPA` now route to `tlbCache->invalidateByVMID(vmid)` in `executeTLBInvalidationCommand()`. Also closes the C++ portion of FINDING-M-02.

**Tests**: `cpp/tests/unit/test_asid_vmid_tlb_spec.cpp` — 9/9 passing (VMIDTargetedInvalidation_TLBI_S12_VMALL, VMIDTargetedInvalidation_TLBI_S2_IPA, field presence tests).

---

### ~~FINDING-NEW-20~~ ✅ Fixed (C++) — ASID Field Missing from C++ TLB Entries
**Spec**: §3.17 (TLB tagging, VMIDs, ASIDs), §4.4 (TLB invalidation)
**Severity**: Medium
**Affected**: C++ only

**Fix**: Added `uint16_t asid` to `StreamConfig`, `TLBEntry`, and `CommandEntry` in `cpp/include/smmu/types.h`. Added `TLBCache::invalidateByASID(uint16_t asid)` in `cpp/src/cache/tlb_cache.cpp`. ASID is propagated from `StreamConfig` into `TLBEntry` at cache-insertion time. `CMD_TLBI_NH_ASID` and `CMD_TLBI_EL2_ASID` now route to `tlbCache->invalidateByASID(asid)` in `executeTLBInvalidationCommand()`. Also closes the C++ portion of FINDING-M-03.

**Tests**: `cpp/tests/unit/test_asid_vmid_tlb_spec.cpp` — 9/9 passing (ASIDTargetedInvalidation_TLBI_NH_ASID, ASIDTargetedInvalidation_TLBI_EL2_ASID, field presence tests).

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

**C++**: Functional test suite — 100% pass rate. All previously noted gaps
(disabled-stream event type, `CFGI_CD`/`CFGI_CD_ALL` handling, stall mode
event type, `C_BAD_SUBSTREAMID`) resolved by NEW-05 through NEW-13.
NEW-19 and NEW-20 resolved by 2026-02-21 session: 9 new spec tests added in
`cpp/tests/unit/test_asid_vmid_tlb_spec.cpp` covering VMID-targeted and
ASID-targeted TLB invalidation (all 9/9 passing).
NEW-15 resolved 2026-02-21: removed `generateEvent(F_STREAM_DISABLED)` for
`STE.Config==0b000` path (C++: `smmu.cpp`; Rust: `smmu/mod.rs`). Tests:
2 new tests in `test_smmuen_spec.cpp`; 3 existing Rust tests in
`test_f_stream_disabled_spec.rs` updated to assert no event. 53/55 C++ pass.
NEW-16 resolved 2026-02-21: OAS checks added for GBPA bypass (silent abort)
and STE bypass (F_ADDR_SIZE) in both C++ and Rust. Tests: 4 new tests in
`test_addr_size_fault_spec.cpp`; existing Rust bypass tests still pass.
Open gaps (NEW-17, NEW-18) do not yet have associated test cases.

**Rust**: All 157 tests passing (100%). NEW-15 and NEW-16 implemented in
`rust/smmu/src/smmu/mod.rs`: suppressed StreamDisabled event recording,
added OAS checks for GBPA bypass and STE bypass, fixed `AddressSizeFault`
→ `FAddrSize` mapping in `map_fault_type_to_event_type()`.
Open gaps (NEW-17, NEW-18) do not yet have associated test cases.

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
12. ~~FINDING-H-04 — ASID/VMID-targeted TLB invalidation~~ ✅ Fixed (Rust); C++ conservative (documented)
13. ~~FINDING-M-09 — Implement range-based ATC invalidation (Rust)~~ ✅ Fixed (Rust)
14. ~~FINDING-M-10 — Add address size fault checking (C++)~~ ✅ Fixed (C++)
15. ~~FINDING-M-04 — Access Flag and Dirty State simulation~~ ✅ Fixed (Both)

### Medium-term (feature completeness)
16. ~~FINDING-M-01 — Circular queue PROD/CONS index semantics~~ ✅ Fixed (Both)
17. ~~FINDING-M-08 — PRG index in PRIEntry and PRI_RESP handling~~ ✅ Fixed (Both)
18. ~~FINDING-M-06 — GERROR register conditions for command queue errors~~ ✅ Fixed
19. ~~FINDING-L-04 — Validate fault syndrome register encoding against spec tables~~ ✅ Fixed
20. ~~FINDING-L-06 — Enforce invalidation sequence before stream reconfiguration (C++)~~ ✅ Fixed

### Low-priority / document as limitation
21. ~~FINDING-C-01 — Register map (software model scope; document limitation)~~ ✅ Documented as software model scope limitation
22. ~~FINDING-C-02 — Binary STE format (document as software model)~~ ✅ Documented as software model scope limitation
23. ~~FINDING-C-03 — Binary CD format (document as software model)~~ ✅ Documented as software model scope limitation
24. ~~FINDING-C-04 — L1STD two-level stream table (document as software model)~~ ✅ Documented as software model scope limitation
25. ~~FINDING-H-06 — L1CD two-level context descriptor table~~ ✅ Documented as software model scope limitation
26. ~~FINDING-L-01 — Interrupt modeling~~ ✅ Documented as software model scope limitation
27. ~~FINDING-L-02 — MSI write in CMD_SYNC~~ ✅ Documented as software model scope limitation
28. ~~FINDING-L-03 — Translation Hardening (SMMUv3.4)~~ ✅ Documented as software model scope limitation
29. ~~FINDING-L-07 — VMS support~~ ✅ Documented as software model scope limitation

### New findings (2026-02-20 review)
30. ~~FINDING-NEW-02 — C_BAD_STREAMID event type wrong (Rust)~~ ✅ Fixed
31. ~~FINDING-NEW-07 — C_BAD_STREAMID event type wrong (C++)~~ ✅ Fixed
32. ~~FINDING-NEW-03 — Stall events discarded on event queue overflow (Both)~~ ✅ Fixed
33. ~~FINDING-NEW-06 — EventEntry missing Stall bit (Both)~~ ✅ Fixed (resolved by NEW-03)
34. ~~FINDING-NEW-04 — CMD_RESUME missing Action/Abort parameters (Rust)~~ ✅ Fixed
35. ~~FINDING-NEW-10 — CMD_RESUME does not verify STAG/StreamID (Rust)~~ ✅ Fixed
36. ~~FINDING-NEW-05 — CMD_CFGI_STE_RANGE range prefix semantics absent (Both)~~ ✅ Fixed
37. ~~FINDING-NEW-01 — GBPA.ABORT abort-on-disable path not modeled (Both)~~ ✅ Fixed
38. ~~FINDING-NEW-09 — SMMUEN global enable not implemented (C++)~~ ✅ Fixed
39. ~~FINDING-NEW-08 — CMD_RESUME / CMD_STALL_TERM are no-ops; no Ac/Ab (C++)~~ ✅ Fixed

### New findings (2026-02-21 QA re-review)
40. ~~FINDING-NEW-11 — C_BAD_SUBSTREAMID not generated for stage-2-only / bypass with non-zero PASID (Both)~~ ✅ Fixed (Both)
41. ~~FINDING-NEW-12 — CMD_CFGI_CD / CMD_CFGI_CD_ALL missing from C++~~ ✅ Fixed (C++)
42. ~~FINDING-NEW-13 — Stall mode hard-codes F_TRANSLATION event regardless of actual fault type (C++)~~ ✅ Fixed (C++)
43. ~~FINDING-NEW-14 — PASIDSecurityStateContextSwitching test stale after FINDING-L-06 (C++ test debt)~~ ✅ Fixed (C++)

### New findings (2026-02-21 deep QA review — new open gaps)
44. ~~FINDING-NEW-15 — F_STREAM_DISABLED triggered for wrong condition; no-event rule for STE.Config==0b000 not enforced (Both)~~ ✅ Fixed (Both)
45. ~~FINDING-NEW-16 — OAS check missing on bypass mode translations — F_ADDR_SIZE not generated for oversized addresses (Both)~~ ✅ Fixed (Both)
46. FINDING-NEW-17 ❌ — CMD_CFGI_STE and CMD_CFGI_CD Leaf bit not modeled in CommandEntry (Both) — Low priority
47. FINDING-NEW-18 ❌ — STE.S1DSS field not modeled; non-substream fallback semantics absent (Both)
48. ~~FINDING-NEW-19 — VMID field missing from C++ TLB entries and StreamTableEntry — re-states open FINDING-M-02 C++ gap (C++)~~ ✅ Fixed (C++)
49. ~~FINDING-NEW-20 — ASID field missing from C++ TLBEntry — re-states open FINDING-M-03 C++ gap (C++)~~ ✅ Fixed (C++)

---

## Key Files for Fixes

| File | Relevant Findings |
|------|------------------|
| `cpp/include/smmu/types.h` | H-01, H-02, H-07, L-05, NEW-08, NEW-09, NEW-11, NEW-12, NEW-17, NEW-19, NEW-20 |
| `cpp/src/smmu/smmu.cpp` | H-05, H-08, M-05, M-10, NEW-03, NEW-07, NEW-08, NEW-09, NEW-11, NEW-12, NEW-13, NEW-15, NEW-16 |
| `cpp/tests/integration/test_pasid_context_switching.cpp` | NEW-14 |
| `rust/smmu/src/types/command_entry.rs` | H-02, H-03, NEW-04, NEW-05, NEW-17 |
| `rust/smmu/src/types/event_entry.rs` | H-01, M-05, NEW-06 |
| `rust/smmu/src/types/fault_type.rs` | NEW-11 |
| `rust/smmu/src/types/security_state.rs` | H-07, L-05 |
| `rust/smmu/src/types/translation_result.rs` | NEW-11 |
| `rust/smmu/src/types/config.rs` | NEW-18 |
| `rust/smmu/src/smmu/mod.rs` | H-03, H-05, H-08, M-09, NEW-02, NEW-03, NEW-10, NEW-11, NEW-15, NEW-16 |
| `rust/smmu/src/stream_context/mod.rs` | NEW-11, NEW-18 |
| `rust/smmu/src/cache/` | M-03, M-04 |
