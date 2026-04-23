---
title: "Coherency and Embedded Implementations"
type: concept
tags: [smmu, coherency, embedded, httu, memory-types, cohacc, implementation-defined]
created: 2026-04-13
updated: 2026-04-14
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Coherency and Embedded Implementations

## Definition

§3.15 governs the memory access types used by the SMMU for its own structure accesses (translation tables, queues, STEs, CDs), the coherency requirements for HTTU, and client device coherency. §3.16 governs embedded implementations where SMMU structures and queues reside in IMPLEMENTATION DEFINED on-chip storage rather than system memory.

---

## §3.15 Coherency Considerations

### Memory Access Types for SMMU Structures

All SMMU-accessed structures (translation tables, STEs, CDs, command queue, event queue, PRI queue) **must** be held in Normal memory as seen by the SMMU. The SMMU uses the memory type attributes defined by system configuration to access these structures.

**SMMU_IDR0.COHACC** indicates whether the SMMU accesses translation table data and configuration structures as:
- **COHACC==0:** IO-coherent access is not supported — the SMMU does not perform coherent accesses to translation tables, configuration structures, or queues.
- **COHACC==1:** IO-coherent access is supported — the SMMU can access translation table walks, STE/CD fetches, and command/event/PRI queue entries in an IO-coherent manner. Whether any specific access uses cacheable-shareable attributes depends on the access type configured for that structure. Software still requires invalidation commands on structural change, because SMMU configuration caches are **not** required to be snooped.

Key rule: regardless of COHACC, software must issue the appropriate CMD_CFGI_* and CMD_TLBI_* commands after modifying any SMMU-managed structure. COHACC indicates the memory access type used by the SMMU when performing table walks or reading configuration; it does not eliminate the software invalidation requirement.

### Single-Copy Atomicity

SMMU structure updates require single-copy atomicity:
- **VMSAv8-64 (AArch64, 64-bit descriptors):** 64-bit single-copy atomic updates required.
- **VMSAv9-128 (128-bit descriptors):** 128-bit single-copy atomic updates required. The SMMU follows the same single-copy atomicity rules as PEs for translation table descriptor accesses (§3.15); no explicit FEAT_LSE2 requirement applies to descriptor atomicity itself.

Note: FEAT_LSE2 affects a separate concern — when present in the system, it sets the single-copy atomicity size for **configuration structure fetches** (STEs, CDs, etc.) to 128-bit (§3.21.3). This is distinct from translation table descriptor atomicity.

See §3.21.3 (config invalidation completion) for the full atomicity rules in the context of structure update procedures.

### HTTU Atomicity Requirements

When HTTU (Hardware Translation Table Update) is enabled, the SMMU must perform atomic read-modify-write operations on translation table descriptors to set Access flags and Dirty state bits. This requires:

- **Local monitors (LDREX/STREX or similar):** The SMMU must access translation tables via a **fully-coherent port** to the memory system to use local monitor exclusive sequences.
- **Armv8.1 LSE atomics:** Alternatively, the SMMU may use Large System Extension atomic instructions (e.g., `STSET`), which do not require a fully-coherent port but must still be visible across the Inner Shareability domain.

If HTTU is enabled and the SMMU does not have a fully-coherent port and LSE atomics are not supported, HTTU behavior is UNPREDICTABLE. SMMU_IDR0.HTTU encodes the supported HTTU modes (see [httu.md](httu.md)).

### SMMU Configuration Caches

SMMU implementations may cache STE, CD, translation table entries, and other configuration. These caches:
- Are **not** required to be snooped by PE cache maintenance operations.
- Must be invalidated explicitly via CMD_CFGI_* commands after software modifies configuration.
- May prefetch structures speculatively (subject to §3.21.3 bounds); see [speculative-accesses.md](speculative-accesses.md).

---

## §3.15.1 Client Device Coherency

Client device coherency is **separate** from SMMU structure coherency (COHACC) and refers to whether the client device's DMA accesses are coherent with the PE cache hierarchy.

### Client Coherency Types

**Fully-coherent clients:** The client device's DMA accesses participate in the Inner Shareability domain coherency protocol (e.g., via a cache-coherent interconnect). From the SMMU's perspective:
- Snoop requests from the coherent fabric bypass the SMMU — the SMMU does not intercept or translate snoop requests.
- The client may be a StreamID-bearing device or a NoStreamID device.
- GPC (Granule Protection Check) still applies to the physical address output.
- The DPT W bit (Device Permission Table writable) may be treated as 1 for fully-coherent clients (IMPL DEFINED). See [device-permission-table.md](device-permission-table.md).

**Non-coherent clients:** The client device's DMA accesses are Non-cacheable from the PE perspective. Standard CMO requirements apply for software managing shared buffers. CMOs initiated from a client device are supported by the SMMU (see CMO support in [../synthesis/smmu-system-implementation.md](../synthesis/smmu-system-implementation.md)).

**COHACC does not describe client coherency.** COHACC describes how the SMMU itself accesses its own structures, not how client DMA is treated.

### TLB Maintenance from Clients

TLB maintenance operations issued by client devices are **not** propagated by the SMMU to the PE TLB or vice versa. Client-initiated TLB maintenance affects only the client's own ATC (Address Translation Cache). The SMMU CMD_TLBI_* commands are the mechanism for the software-managed invalidation path. See [tlb-invalidation.md](tlb-invalidation.md).

---

## §3.16 Embedded Implementations

An embedded implementation is one where the SMMU's translation tables, queues, or both reside in IMPLEMENTATION DEFINED on-chip storage rather than in system memory. This allows the SMMU to be used in systems where there is no external DRAM or where structures must reside in a dedicated on-chip SRAM.

### Discovery

- **SMMU_IDR1.TABLES_PRESET==1:** Translation tables reside in IMPL DEFINED on-chip storage. The STRTAB_BASE, S1ContextPtr, and related pointer registers are not used to locate tables in system memory; instead, the SMMU uses its internal storage.
- **SMMU_IDR1.QUEUES_PRESET==1:** Command queue, event queue, and PRI queue reside in IMPL DEFINED on-chip storage.
- **SMMU_IDR1.REL==1:** Base register values are relative offsets rather than absolute physical addresses. Used when the SMMU's embedded storage is addressed relative to a base, supporting relocatable embedded configurations.

### Memory Access Requirements

When structures are preset (TABLES_PRESET or QUEUES_PRESET):
- The PE must access the preset storage via a **non-coherent** (Non-cacheable Normal) path if the PE needs to read or write those structures. This ensures that caching by the PE does not interfere with the SMMU's direct access to on-chip storage.
- The address regions for preset storage must **not overlap** with each other or with any other mapped region. Overlapping regions produce UNPREDICTABLE behavior.

---

## §3.16.1 Embedded Storage Field Requirements

### §3.16.1.1 Embedded Event and PRI Queue Entries

For embedded Event queue and PRI queue entries, individual fields within queue entries may be implemented as **Read-Only / Write-Ignored (RO/WI)**. Specifically:
- Software writes to event queue entries are permitted to be ignored (WI) since the SMMU writes event entries and software only reads them.
- PRI queue entries similarly may have WI fields.
- This is permitted because in an embedded implementation the SMMU writes the entries directly from on-chip logic; the field storage need not be writeable from the PE side.

### §3.16.1.2 Embedded Command Queue — Storage Reduction

For an embedded command queue, fields are readable and writable (to allow software to issue commands), but **storage is not required** for the following field categories. The SMMU may implement these as RAZ/WI:

| Category | Condition for No-Storage |
|----------|--------------------------|
| Reserved / RES0 fields | Always — these are architecturally RES0 |
| High-order StreamID bits | Bits beyond the SMMU's implemented StreamID width |
| High-order SubstreamID bits | Bits beyond the SMMU's implemented SubstreamID width |
| SSV (SubstreamID Valid) field | If the SMMU does not support SubstreamIDs |
| STAG bits beyond used width | Bits of the stall tag beyond the implementation's tag width |
| SSec (Secure SubstreamID) | If the SMMU is NS-only (no Secure EL2) |
| CMD_SYNC MSI fields | If MSI is not supported by this implementation |
| ASID[15:8] | If the SMMU implements only 8-bit ASIDs |
| VMID[15:8] | If the SMMU implements only 8-bit VMIDs |
| CMD_CFGI_STE Leaf bit | If the SMMU supports only single-level stream tables (no two-level) |
| Fields for CERROR_ILL commands | For commands that produce only CERROR_ILL (IMPL DEFINED unimplemented commands) |

**RAZ/WI rule:** Any field that is not stored must read as zero and writes must be ignored. Fields that **are** functionally required for correct operation must be fully readable and writable.

### §3.16.1.3 Embedded STE — Storage Reduction

For an embedded stream table (TABLES_PRESET), STE fields are freely readable and writable from software, but **storage is not required** for:

| Category | Condition for No-Storage |
|----------|--------------------------|
| Undefined or Reserved fields | Always |
| RES0 fields | Always |
| IGNORED fields | For configurations that are supported — if a field is architecturally IGNORED in the supported Config encodings |
| RAZ/WI fields | Always |
| Fields for unsupported features | E.g., S1ContextPtr if stage 1 translation is not supported; S2TTB if stage 2 is not supported |
| Fields for unsupported security states | E.g., Secure-only fields if only Non-secure operation is supported |

As with command queue fields, any unstored STE field must be RAZ/WI. Any field that is required for correct function in a supported configuration must be fully stored, readable, and writable.

---

## Related Concepts

- [httu.md](httu.md) — HTTU atomicity depends on coherency port availability; SMMU must have fully-coherent or LSE-capable access to translation tables
- [speculative-accesses.md](speculative-accesses.md) — Speculative prefetch of structures governed by §3.21.3; COHACC affects how prefetches are issued
- [tlb-invalidation.md](tlb-invalidation.md) — Invalidation commands required regardless of COHACC; config caches are not snooped
- [stream-table-entry.md](stream-table-entry.md) — STE field storage reduction rules for embedded implementations
- [command-queue.md](command-queue.md) — Command queue storage reduction and RAZ/WI rules for embedded implementations
- [event-queue.md](event-queue.md) — Event queue WI rules for embedded implementations
- [external-interfaces.md](external-interfaces.md) — Port coherency model defined in Ch. 14: fully-coherent port required for HTTU local monitor atomics; IO-coherent sufficient for non-HTTU accesses
- [../synthesis/smmu-system-implementation.md](../synthesis/smmu-system-implementation.md) — Full system integration requirements; CMO support; caching architecture

## Sources That Use This Concept

- [../sources/ihi0070g-b-smmuv3-architecture-spec.md](../sources/ihi0070g-b-smmuv3-architecture-spec.md) — §3.15 Coherency considerations; §3.15.1 Client device coherency; §3.15.1.1 Fully-coherent clients; §3.16 Embedded implementations; §3.16.1.1–3 Embedded storage field requirements
