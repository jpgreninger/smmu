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

### FINDING-C-01 ❌ — No Hardware Register Map
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

**Recommendation**: For a simulation model, add a `register_map` module with at
minimum IDR0, CR0, STRTAB_BASE, CMDQ_BASE/PROD/CONS, and EVENTQ_BASE/PROD/CONS.

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
All 43 C++ tests and all Rust test suites pass. Fixed commit: **TBD**.

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

### FINDING-H-03 ❌ — CFGI_CD and CFGI_CD_ALL Not Implemented
**Spec**: §4.3.3 (CMD_CFGI_CD), §4.3.4 (CMD_CFGI_CD_ALL)
**Affected**: Both

`CMD_CFGI_CD(StreamID, SSec, SubstreamID, Leaf)` invalidates a single CD entry.
`CMD_CFGI_CD_ALL(StreamID, SSec)` invalidates all CDs for a stream. Neither
command type exists in either implementation.

**Recommendation**: Add `CfgiCd` and `CfgiCdAll` to `CommandType` and implement
handlers that invalidate PASID-level (CD) configuration caches for the
specified stream.

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

### FINDING-H-05 ❌ — Stall Mode / CMD_RESUME Not Implemented
**Spec**: §4.6 (CMD_RESUME), §3.12.2 (Stall fault model)
**Affected**: Both

The Stall model requires the SMMU to halt transaction processing and wait for
`CMD_RESUME(StreamID, SSec, STAG, Action, Abort)`. The STAG field identifies
the stalled transaction group.

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

### FINDING-H-07 ❌ — Security State Bit Encoding Inconsistency
**Spec**: §3.10 (Security states), §3.10.1 (StreamID Security state SEC_SID)
**Affected**: Both

The ARM specification encodes SEC_SID as: `0b00`=NonSecure, `0b01`=Secure,
`0b10`=Realm, `0b11`=Root.

- **Rust**: `SecurityState` uses `Secure=0b00, NonSecure=0b01, Realm=0b10` —
  the Secure and NonSecure bit values are inverted relative to the hardware.
- **C++**: No bit values assigned to `SecurityState` enum at all; no
  binary-compatible serialisation is possible.

**Recommendation**: Align both implementations to `NonSecure=0b00`,
`Secure=0b01`, `Realm=0b10`, `Root=0b11`.

---

### FINDING-H-08 ❌ — No SMMU Global Enable/Disable (SMMU_CR0.SMMUEN)
**Spec**: §6.3.9 (SMMU_CR0), bit 0 SMMUEN
**Affected**: Both

When SMMUEN=0 all transactions must bypass the SMMU (no translation or fault).
The SMMU must start disabled after reset.

- **C++**: `translate()` performs translation immediately after construction.
  `SMMU::reset()` does not model SMMUEN.
- **Rust**: `SMMU::initialize()` is documented as a no-op. `is_shutdown()` is
  not equivalent to the SMMUEN enable control.

**Recommendation**: Add a global `enabled` atomic boolean defaulting to `false`.
Add `enable()` / `disable()` methods. All `translate()` calls must bypass when
`enabled == false`.

---

## Medium Findings

### FINDING-M-01 ❌ — Circular Queue PROD/CONS Semantics Not Implemented
**Spec**: §3.5 (Command and Event queues), §3.5.1 (SMMU circular queues)
**Affected**: Both

Queues must use circular buffer semantics with Producer/Consumer index registers
including a WRAP bit. The queue is empty when PROD == CONS.

Both use `std::deque` (C++) / `VecDeque` (Rust) with no PROD/CONS index pair.

**Recommendation**: Add explicit `prod_index: u32` and `cons_index: u32`
(including wrap bit) to enable register-equivalent queue state queries.

---

### FINDING-M-02 ❌ — No VMID in Two-Stage Translation
**Spec**: §3.8 (Virtualization), §5.2 (STE S2VMID field)
**Affected**: Both

Stage-2 translation requires a VMID (STE Word 2, bits 63:48) to tag TLB
entries. TLB invalidation uses VMID for targeted invalidation.

Neither implementation includes a VMID field in stream table configuration or
TLB cache entries.

**Recommendation**: Add VMID to stream table entry config. Tag TLB entries with
VMID. Support `CMD_TLBI_S12_VMALL` VMID-targeted invalidation.

---

### FINDING-M-03 ❌ — ASID Not Tracked in TLB Entries
**Spec**: §3.17 (TLB tagging, VMIDs, ASIDs), §4.4 (TLB invalidation)
**Affected**: Both

Stage-1 TLB entries must be tagged with the ASID from the Context Descriptor
(CD.ASID, Word 1[31:16]). `CMD_TLBI_NH_ASID` invalidates by ASID.

- **C++**: `TLBEntry` struct has no `asid` field.
- **Rust**: `CacheKey::new(stream_id, pasid, iova, security_state)` — no ASID
  parameter.

**Recommendation**: Add ASID field to TLB cache entries and implement
ASID-targeted invalidation.

---

### FINDING-M-04 ❌ — No Access Flag / Dirty State Management
**Spec**: §3.13 (Translation tables and AF/Dirty state), §3.13.2–3.13.5
**Affected**: Both

CD.HA (bit 43) enables hardware Access Flag management. CD.HD (bit 42) enables
hardware Dirty State management.

Neither implementation tracks AF or Dirty bits in page entries or context
descriptors.

**Recommendation**: Add `access_flag` and `dirty` booleans to page entries.
Simulate AF set on first access when HA=1. Simulate dirty set on write when
HD=1.

---

### FINDING-M-05 ❌ — No F_STREAM_DISABLED Event Generation
**Spec**: §7.3.7 (F_STREAM_DISABLED)
**Affected**: Both

When STE.Config indicates a disabled/abort stream, transactions must generate an
`F_STREAM_DISABLED` fault record. Both implementations return a generic
`TranslationFault` or `BadSTE` type instead.

**Recommendation**: Add `StreamDisabled` to the event type enum. When
`translate()` encounters a disabled stream, record `F_STREAM_DISABLED`.

---

### FINDING-M-06 ❌ — No GERROR Register Modeling
**Spec**: §6.3.17 (SMMU_GERROR), §7.5 (Global error recording)
**Affected**: Both

SMMU_GERROR bits indicate global error conditions (SFE, MSI_ABT_ERR,
PRIQ_ABT_ERR, EVENTQ_ABT_ERR, CMDQ_ERR, CMDQ_ABT_ERR). Neither implementation
sets CMDQ_ERR when a command error occurs.

**Recommendation**: Add a GERROR status register abstraction. Set CMDQ_ERR when
a command error occurs. Fire interrupt notifications when configured.

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

### FINDING-M-08 ❌ — No PRG Index Tracking
**Spec**: §8 (Page request queue), §8.3 (PRG Response Message codes)
**Affected**: Both

`CMD_PRI_RESP` requires a matching PRGIndex to complete a page request response
cycle. Neither `PRIEntry` nor `CommandEntry` has a `prg_index` field.

**Recommendation**: Add `prg_index: u16` to `PRIEntry` and `CommandEntry` (for
`PriResp`). Implement matching logic in PRI queue processing.

---

### FINDING-M-09 ❌ — AtcInv Does Full Flush Instead of Range
**Spec**: §4.5.1 (CMD_ATC_INV)
**Affected**: Rust

`CMD_ATC_INV(StreamID, SubstreamID, SSV, Global, Address, Size)` must invalidate
ATC entries for a specific address range. The Rust implementation calls
`tlb_cache.invalidate_all()` with a TODO comment acknowledging the gap.

**Relevant file**: `rust/smmu/src/smmu/mod.rs`, `process_single_command`,
`CommandType::AtcInv` arm.

**Recommendation**: Implement range-based TLB invalidation using the
`start_address` and `end_address` fields of `CommandEntry`. Expose
`invalidate_range(stream_id, start, end)` on `TlbCache`.

---

### FINDING-M-10 ❌ — No Address Size Fault Checking
**Spec**: §3.4 (Address sizes), §3.4.1 (Input address size)
**Affected**: C++

The SMMU must raise `AddressSizeFault` when the input address exceeds the
address size configured by TCR.T0SZ. `MAX_VIRTUAL_ADDRESS` is set to 52-bit
but no validation against the context descriptor's `inputAddressSize` is
performed in `AddressSpace::translatePage`.

**Recommendation**: Add address size validation in `AddressSpace::translatePage`
that checks the IOVA against the configured T0SZ and raises `AddressSizeFault`
if exceeded.

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

### FINDING-L-04 ❌ — Fault Syndrome Register Encoding Not Validated
**Spec**: §7.3 (Event records — fault syndrome)
**Affected**: C++

The `FaultSyndrome` struct and `encodeFaultSyndromeRegister` function
(`cpp/include/smmu/types.h`) have not been cross-validated against the
per-event syndrome bit layouts defined in §7.3.x tables.

**Recommendation**: Cross-validate `encodeFaultSyndromeRegister` output against
Table 7-x in §7.3 for each fault type.

---

### FINDING-L-05 ❌ — No Root Security State
**Spec**: §3.10 (Security states), SMMUv3.3 Root Control Page
**Affected**: Both

SMMUv3.3 adds a fourth security state: Root (`0b11`). Neither implementation
includes it.

- **C++**: `SecurityState` has `NonSecure, Secure, Realm`.
- **Rust**: `SecurityState` has `Secure=0b00, NonSecure=0b01, Realm=0b10`.

**Recommendation**: Add `Root = 0b11` to `SecurityState` in both
implementations.

---

### FINDING-L-06 ❌ — C++ Allows Stream Reconfiguration Without Invalidation
**Spec**: §3.11 (Reset, Enable and initialization)
**Affected**: C++ only

The specification requires a `CFGI_STE` + `CMD_SYNC` sequence before changing
stream configuration to maintain TLB and configuration cache consistency.
`SMMU::configureStream` allows updating an existing stream directly without
requiring this sequence.

The Rust implementation is more conservative — it returns
`SMMUError::StreamAlreadyExists` if the stream is already configured.

**Recommendation**: Either reject reconfiguration of existing streams (follow
Rust's approach) or require a `CMD_CFGI_STE` + `CMD_SYNC` sequence first.

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
1. FINDING-H-02 — Correct command opcode values to ARM hex constants
2. FINDING-H-01 — Add missing event types (F_STREAM_DISABLED, C_BAD_SUBSTREAMID, etc.)
3. FINDING-L-05 — Add `Root = 0b11` security state to both implementations
4. FINDING-H-07 — Fix security state bit encoding (Secure/NonSecure inverted in Rust)
5. FINDING-M-05 — Generate F_STREAM_DISABLED instead of generic fault

### Short-term (behavioural conformance)
6. FINDING-H-08 — Add SMMUEN global enable/disable
7. FINDING-M-03 — Add ASID to TLB entries; implement ASID-targeted invalidation
8. FINDING-M-02 — Add VMID to STE config and TLB entries
9. FINDING-H-05 — Implement CMD_RESUME stall model with STAG tracking
10. FINDING-H-03 — Add CFGI_CD and CFGI_CD_ALL command types
11. FINDING-M-09 — Implement range-based ATC invalidation (Rust)
12. FINDING-M-10 — Add address size fault checking (C++)

### Medium-term (feature completeness)
13. FINDING-M-04 — Access Flag and Dirty State simulation
14. FINDING-M-01 — Circular queue PROD/CONS index semantics
15. FINDING-M-08 — PRG index in PRIEntry and PRI_RESP handling
16. FINDING-M-06 — GERROR register conditions for command queue errors
17. FINDING-L-04 — Validate fault syndrome register encoding against spec tables
18. FINDING-L-06 — Enforce invalidation sequence before stream reconfiguration (C++)

### Low-priority / document as limitation
19. FINDING-C-01 — Register map (software model scope; document limitation)
20. FINDING-C-02/C-03 — Binary STE/CD format (document as software model)
21. FINDING-C-04 / FINDING-H-06 — L1STD / L1CD two-level tables
22. FINDING-L-01 — Interrupt modeling
23. FINDING-L-02 — MSI write in CMD_SYNC
24. FINDING-L-03 — Translation Hardening (SMMUv3.4)
25. FINDING-L-07 — VMS support

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
