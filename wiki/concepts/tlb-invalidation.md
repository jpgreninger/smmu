---
title: "TLB Invalidation"
type: concept
tags: [smmu, tlb, invalidation, commands, broadcast, asid, vmid, bbml, tlbinxs, aset, vmid-wildcard]
created: 2026-04-07
updated: 2026-04-13
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# TLB Invalidation

## Definition

TLB (Translation Lookaside Buffer) invalidation is the process of removing cached translation entries from the SMMU's internal TLBs. Because the SMMU caches translations from translation table walks, software must invalidate the TLB whenever it modifies translation tables or changes configuration. Failure to invalidate results in stale translations being used.

## §3.17 TLB Entry Tagging

Every SMMU TLB entry is tagged by the StreamWorld (the combination of Security state, EL, and translation regime) and address type. The full tagging table:

| StreamWorld | Address Type | ASID | ASET | VMID |
|-------------|-------------|------|------|------|
| NS-EL1 | VA (stage 1) | Yes (if nG==1) | Yes | Yes (if S2P) |
| NS-EL1 | IPA (stage 2) | No | No | Yes (if S2P) |
| Realm-EL1 | VA (stage 1) | Yes (if nG==1) | Yes | Yes (if S2P) |
| Realm-EL1 | IPA (stage 2) | No | No | Yes (if S2P) |
| Any-EL2 (no E2H) | VA | No | No | No |
| Any-EL2-E2H | VA | Yes (if nG==1) | Yes | No |
| Secure-EL1 | VA (stage 1) | Yes (if nG==1) | Yes | Yes (if S2P and SEL2) |
| Secure-EL1 | IPA (stage 2) | No | No | Yes (if S2P and SEL2) |
| EL3 | VA | No | No | No |

Key fields:
- **ASID** — from `CD.ASID`; tags stage 1 TLB entries per-process/substream. Recorded only when nG==1 (non-global descriptor) in the applicable StreamWorld.
- **ASET** — Address Space Entry Tag; recorded for stage 1 entries in the applicable StreamWorlds. Governs whether PE broadcast maintenance (VALEX/ASIDEX) is required to invalidate a given entry.
- **VMID** — from `STE.S2VMID`; tags entries involving stage 2 (S2P==1). Secure VMIDs form a distinct namespace from Non-secure VMIDs when SEL2==1.
- **Security state / StreamWorld** — Non-secure, Secure, Realm, and Root entries are in entirely distinct namespaces; no cross-state invalidation occurs from TLB entries.

### §3.17.1 Global Flag

- A descriptor with `nG==0` creates a **global** TLB entry. The ASID is **not recorded** in global entries.
- A global entry matches **any** ASID lookup within the same StreamWorld; it is not process-specific.
- Exception: a Secure stage 1 fetch from Non-secure memory is **always treated as non-global** regardless of the `nG` bit value.

### ASET Semantics

ASET (Address Space Entry Tag) in the TLB entry reflects whether the stage 1 page table entry was fetched with a CD whose ASET field == 0 or == 1:
- **ASET==0 (shared with PE processes):** PE broadcast TLB maintenance operations including VALEX (VA with last-level hint) and ASIDEX broadcasts **must** invalidate matching SMMU TLB entries. The SMMU is required to participate in these broadcasts.
- **ASET==1 (non-shared / private):** PE VALEX/ASIDEX broadcasts are **not required** to invalidate these entries. However, ALL-invalidation commands (ALLE1, ALLE2, ALLE3) must still invalidate ASET==1 entries.
- Global entries (nG==0): An entry with ASET==0 does not match lookups against ASET==1 configs and vice versa.

### ASID Rollover Procedure

When software needs to recycle the ASID namespace while keeping some shared ASIDs valid:
1. Refresh the ASID namespace, leaving all shared ASIDs (ASET==0) untouched.
2. Or: swap the ASID in the CD by making both old and new ASIDs active simultaneously.
3. Then: issue `CMD_CFGI_CD` + `CMD_SYNC` to flush the CD cache.
4. Then: issue `CMD_TLBI_NH_ASID` (old ASID) + `CMD_SYNC` to remove old-ASID entries.

For the "both active" step, the SMMU may hold entries tagged with either the old or new ASID; the CMD_TLBI step removes any residual old-ASID entries before the old ASID is reused.

## Command-Based Invalidation

TLB invalidation is primarily performed by issuing commands to the [concepts/command-queue.md](concepts/command-queue.md). Key command families:

### Stage 1 TLB Invalidation

| Command | Scope |
|---------|-------|
| `CMD_TLBI_NH_ALL` | All Non-Hyp (EL0/1) stage 1 TLB entries |
| `CMD_TLBI_NH_ASID(ASID, VMID)` | All Non-Hyp entries with given ASID (in context of VMID) |
| `CMD_TLBI_NH_VAA(VA, VMID, Range)` | Non-Hyp entries for given VA range, any ASID |
| `CMD_TLBI_NH_VA(VA, ASID, VMID, Range)` | Non-Hyp entries for given VA + ASID |
| `CMD_TLBI_EL2_ALL` | All EL2/hypervisor stage 1 TLB entries |
| `CMD_TLBI_EL2_ASID(ASID)` | EL2 entries with given ASID |
| `CMD_TLBI_EL2_VAA(VA, Range)` | EL2 entries for given VA range |
| `CMD_TLBI_EL2_VA(VA, ASID, Range)` | EL2 entries for VA + ASID |
| `CMD_TLBI_S12_VMALL(VMID)` | All stage 1 + stage 2 entries for given VMID |

### Stage 2 TLB Invalidation

| Command | Scope |
|---------|-------|
| `CMD_TLBI_S2_IPA(VMID, IPA, Range)` | Stage 2 entries for given IPA and VMID |
| `CMD_TLBI_NSNH_ALL` | All Non-secure Non-Hyp entries (stage 1 + 2) |

### EL3 / Root TLB Invalidation

| Command | Scope |
|---------|-------|
| `CMD_TLBI_EL3_ALL` | All EL3 stage 1 TLB entries |
| `CMD_TLBI_EL3_VA(VA, Range)` | EL3 entries for given VA |

### Common Fields in TLB Commands

- `VMID` — virtual machine scope for stage 1 and 2 invalidation.
- `ASID` — process scope for stage 1 invalidation (only entries with matching ASID).
- `VA` / `IPA` — address scope; combined with `Range` for range-based invalidation (SMMUv3.2+).
- `Range` — level hint and range encoding for range-based invalidation (SMMUv3.2+, `SMMU_IDR3.BBML`).

## Range-Based TLB Invalidation (SMMUv3.2+)

SMMUv3.2 introduced range-based invalidation commands and the Level Hint (`Leaf`) field, reducing the number of commands needed to invalidate large mappings. The `Range` parameter encodes the base address and size of the region to invalidate. This feature requires `SMMU_IDR3.BBML != 0b00`.

## §3.17.2 Broadcast TLB Maintenance (AArch64 / EL3)

When `SMMU_IDR0.BTM == 1`, the SMMU participates in broadcast TLB maintenance from Armv8-A PEs. Broadcast operations propagate to the SMMU and are matched against TLB entries using the StreamWorld, ASID, VMID, and VA tags.

Mapping from PE broadcast to SMMU StreamWorld:
- **Secure StreamWorld:** PE Secure EL1 TLBI ops + `CMD_TLBI_NH_*` on the Secure command queue.
- **EL3 StreamWorld:** PE EL3 TLBI ops + `CMD_TLBI_EL3_*`.

An RME SMMU is **not required** to process EL3 broadcast TLB maintenance.

### §3.17.2.1 Secure EL2 (SEL2) Rules

When Secure EL2 is present (`STE.SEL2`):
- **SEL2==0 (Secure EL2 disabled):** Invalidation commands and PE broadcasts do not use a VMID — Secure translations have no VMID context.
- **SEL2==1 (Secure EL2 enabled):** Use VMID==0 for PEs that do not have `SCR_EL3.EEL2` set. For PEs with `EEL2==1`, use the supplied VMID.
- If `SEL2==1` but `EEL2==0` on the PE: SMMU entries are not required to be affected by broadcasts with VMID.
- If `SEL2==1` and `EEL2==1`: use the supplied VMID in the invalidation command or broadcast.

### §3.17.3 Broadcast TLB Maintenance (AArch32)

For AArch32 PEs:
- PE TLBI operations MVA{L}, MVAA{L}, ASID, ALL translate to SMMU Secure queue `CMD_TLBI_NH_*` commands.
- The SMMU interprets AArch32 broadcast operations against Non-secure and Secure TLB entries as appropriate.

### §3.17.4 Mixed ASID/VMID Sizes

The SMMU may implement 8-bit or 16-bit ASIDs and VMIDs. Interactions when a broadcast carries a wider or narrower ID:
- **16-bit SMMU:** Matches broadcasts directly on the full 16-bit value.
- **8-bit SMMU:** Compares only the low 8 bits of the broadcast ASID/VMID. A match is **required** if the top 8 bits are zero; a match is **permitted** (IMPL DEFINED) if the top 8 bits are non-zero.
- A PE with an 8-bit ASID/VMID zero-extends to 16 bits when issuing broadcasts; the high 8 bits are zero, so an 8-bit SMMU will always match.

### §3.17.5 E2H (EL2 Host Extensions)

When `CR2.E2H==1` (EL2 Host Extensions active), the EL2 translation regime gains ASID support:
- EL2-E2H TLB entries record an ASID (from the EL2 TTBR).
- PE broadcast EL2-E2H includes an ASID qualifier.
- **Cross-invalidation rule:** Any-EL2 (E2H==0) entries and any-EL2-E2H (E2H==1) entries are **not required** to invalidate each other on respective broadcasts; they occupy separate sub-namespaces.
- A change to `CR2.E2H` requires invalidation of **all** EL2 and EL2-E2H entries (use ALLE2 or equivalent before toggling E2H).
- `CMD_TLBI_EL2_VAA` / `CMD_TLBI_EL2_VA` behavior depends on the value of `CR2.E2H` at the time the command is processed.

### §3.17.6 VMID Wildcards (SMMUv3.2+)

`SMMU_CR0.VMW` configures VMID wildcard matching on TLB invalidation:
- The field specifies how many LSBs of the VMID are **wildcarded** during invalidation matching — entries with any value in those bits are matched.
- **Lookups** still use the full VMID (no aliasing occurs; a client with VMID X is not confused with VMID Y simply because wildcarding is configured).
- Wildcarding affects both broadcast maintenance and explicit `CMD_TLBI_*` commands.
- `SMMU_S_CR0.VMW` provides the equivalent for the Secure command queue.
- Discovery: `SMMU_IDR0.VMID_WILDCARD==1`.

### §3.17.7 GPT Broadcast (RME)

For SMMU implementations with RME and `SMMU_IDR0.BGPTM==1`:
- The SMMU participates in PE EL3 `TLBI *PA* to OSH` (GPT broadcast) operations.
- This is independent of `BTM` (standard TLB maintenance) and `PTM` (PE-side TLB maintenance).
- GPT broadcast ensures that SMMU-cached translations that reference Granule Protection Table (GPT) information are invalidated when the GPT is updated.

### §3.17.8 TLBInXS (SMMUv3.4 / FEAT_XS)

When `SMMU_IDR3.BTM_XS==1` and `SMMU_CR0.BTM==1`:
- MAIR encodings `0b01100000`, `0b01000000`, `0b10100000` remain **Reserved** in the SMMU context (same as without XS).
- XS attribute bit is treated as **0** for all SMMU-cached translations; the SMMU does not cache the XS bit.
- Stage 2 bit[11] is `RES0`; XS==0 for stage 2 translations.
- With `PTM==0`: `DSB nXS` waits for TLBInXS completions; a `DSB` (non-nXS) waits for **both** TLBI and TLBInXS completions.

---

## §3.20 TLB and Configuration Cache Conflicts

### §3.20.1 TLB Conflict

A **TLB conflict** occurs when the SMMU has two or more TLB entries that both match a given lookup (overlapping translations for the same address/ASID/VMID context). This is a software programming error.

- **If detected:** The SMMU must **abort** the triggering transaction and **attempt** to record an `F_TLB_CONFLICT` event in the event queue. IMPL DEFINED fields within the event entry.
- **If undetected:** Behavior is **UNPREDICTABLE**, but the following invariants are preserved:
  - The SMMU cannot **grant access beyond configured permissions** — it may only restrict or terminate.
  - **Security state invariants are preserved** — a conflict in one VMID/StreamWorld cannot cause access to be granted in another VMID/StreamWorld.
  - **Cross-stream isolation is preserved** — one stream's conflict must not cause another stream's transaction to be terminated.

### §3.20.2 Configuration Cache Conflict

A **configuration cache conflict** occurs when the SMMU's STE cache contains overlapping entries for the same StreamID (e.g., due to incorrect configuration of the `CONT` field in the stream table, which causes a span of STEs to be cached as a single entry).

- **If detected:** The SMMU must **abort** the triggering transaction and **attempt** to record an `F_CFG_CONFLICT` event. IMPL DEFINED fields within the event entry.
- **If undetected:** Behavior is **UNPREDICTABLE**, but the STE cannot be treated as belonging to a **different Security state** than it does — the Security state invariant is preserved even under undetected conflict.

---

## §3.21 Structure Access Rules and Update Procedures

### §3.21.1 TLB Invalidation Completion

An invalidation operation is **complete** when all of the following hold:
1. All targeted TLB entries have been invalidated.
2. Any HTTU updates triggered by the now-invalidated entries are **globally visible** in memory.
3. No access using an old (invalidated) translation is visible in the system.
4. All affected table walks have completed.
5. ATOS results cannot be based on an old (invalidated) translation.

**Completion guarantee by operation type:**
- **Broadcast TLB maintenance (PE-issued, BTM==1):** Complete after a **Shareable DSB** (`DSB ISH` or `DSB OSH`).
- **Command-queue-based invalidation:** Complete after a subsequent `CMD_SYNC` completes.

### §3.21.1.1 BBML Level 0 (pre-SMMUv3.2 / Armv8.4)

`SMMU_IDR3.BBML==0b00` — break-before-make (BBM) is required before changing translation table entries in the following scenarios:
- Memory type change.
- Cacheability change.
- Output address change.
- Block/page size change (e.g., block → page, or changing block level).
- Global → non-global transition.

BBM procedure: invalidate the old entry → `CMD_SYNC` → write the new entry.

### §3.21.1.2 BBML Level 1 (SMMUv3.2+)

`SMMU_IDR3.BBML==0b01` — adds the `nT` (not-Translation) hint bit to block descriptors:
- The `nT` bit **must** be set (nT==1) when changing block/page size without performing BBM. Setting nT==1 signals to the SMMU that TLB conflict detection should not be performed for this entry.
- `F_TLB_CONFLICT` **may** occur if a size change is made without BBM and without setting nT.
- Setting `nT==1` itself does **not** fault.
- **Contiguous bit:** Changing the Contiguous bit without BBM or nT does **not** cause a TLB conflict (Level 1 does not require BBM for Contiguous bit changes).

### §3.21.1.3 BBML Level 2 (SMMUv3.2+)

`SMMU_IDR3.BBML==0b10` — maximum relaxation:
- The `nT` bit is **ignored** by the SMMU implementation.
- Block/page **size changes** are permitted without BBM; the implementation automatically resolves multi-match.
- `F_TLB_CONFLICT` is **never** reported at Level 2.
- Contiguous bit rules are the same as Level 1 (no BBM required for Contiguous changes).

### §3.21.2 Queue Access Rules

Software and SMMU access to queues must follow strict ordering rules:

**Command queue (software writes, SMMU reads):**
1. Software writes commands into the queue buffer.
2. Software issues a **DSB** after writing all commands, before updating PROD.
3. Software writes the `PROD` pointer to notify the SMMU of new commands.
4. The SMMU may cache commands from the queue. This cache is invalidated on error or when the SMMU is disabled.
5. Software **must not** alter commands that the SMMU has already consumed (i.e., where CONS has passed them).
6. The `PROD` pointer of the command queue must only be written **consistently** — CONSTRAINED UNPREDICTABLE if written inconsistently. Consequences of inconsistent write: SMMU may execute UNPREDICTABLE commands or may stop processing the queue.

**Event and PRI queues (SMMU writes, software reads):**
1. The `CONS` pointer of output queues must only be written by software **consistently** — i.e., advancing monotonically after reading entries.
2. Software must not write `CONS` to a position it has not yet read.

### §3.21.3 Configuration Invalidation Completion

Structures are **reachable** if they are accessible via valid pointer chains:
- `STRTAB_BASE` → STE (linear) or L1STD → STE (two-level).
- `STE.S1ContextPtr` → CD (single) or L1CD → CD (multi-level).

**Speculative prefetch:** An SMMU implementation is permitted to **speculatively fetch** any reachable configuration structure (STE, CD, L1STD, L1CD) at any time, subject to:
- Must not fetch outside configured table bounds (STRTAB_BASE_CFG, S1ContextPtr size fields).
- Must not cache a structure under the wrong type (e.g., cannot cache a CD as if it were an STE).

**Completion definition:** A configuration invalidation is complete when:
1. All targeted cached structure entries have been invalidated.
2. All affected table walks that used those entries have completed.
3. All client transactions that used the old entries are **globally visible**.

**Single-copy atomicity rules for structure writes:**
- With `FEAT_LSE2` in the system: 128-bit single-copy atomicity is guaranteed for aligned 128-bit writes. Use for VMSAv9-128 descriptors and 128-bit CD/STE fields.
- Without `FEAT_LSE2`: 64-bit single-copy atomicity is the minimum guaranteed. Structure updates to 128-bit structures must use appropriate locking or split-update procedures.

### §3.21.3.1 Configuration Update Procedures

**7-step procedure for bringing a structure from invalid to valid (active):**
1. Start with V==0 (Validity bit clear) in the target structure.
2. Write all non-V fields of the structure.
3. Issue a **DSB** to ensure the writes are globally visible.
4. Issue `CMD_CFGI_*` targeting this structure + `CMD_SYNC`.
5. Write V==1 to the target structure.
6. Issue a **DSB**.
7. Issue `CMD_CFGI_*` targeting this structure + optional `CMD_SYNC` (required if software must wait for the new configuration to take effect).

**4-step procedure for marking a structure invalid:**
1. Write V==0 to the target structure.
2. Issue a **DSB**.
3. Issue `CMD_CFGI_*` targeting this structure.
4. Issue `CMD_SYNC` and wait for completion.

After the 4-step procedure, all in-flight uses of the old structure are guaranteed complete and no new transactions will use it.

## Invalidation Completion and CMD_SYNC

TLB invalidation commands are asynchronous. To guarantee completion:
- Issue `CMD_SYNC` after the TLB invalidation commands.
- After the `CMD_SYNC` completes, all effects of prior commands (including TLB invalidations) are guaranteed complete.

See §3.21.1 Translation tables and TLB invalidation completion behavior for the precise ordering rules required when updating translation table entries.

## Configuration Cache Invalidation

Separate from TLB invalidation, the SMMU may cache copies of STEs, CDs, and VMSs. These must be invalidated whenever configuration structures are modified:
- `CMD_CFGI_STE(StreamID)` — invalidate STE cache for a specific StreamID.
- `CMD_CFGI_STE_RANGE` — range of StreamIDs.
- `CMD_CFGI_CD(StreamID, SubstreamID)` — invalidate CD cache.
- `CMD_CFGI_CD_ALL(StreamID)` — all CDs for a stream.
- `CMD_CFGI_VMS_PIDM(VMID)` — invalidate VMS cache.
- `CMD_CFGI_ALL` — invalidate all configuration caches.

Configuration and TLB invalidation completion rules differ; see §3.21.3.

## Model Implementation Notes

- A performance model must track TLB hit/miss rates; TLB tag fields (ASID, VMID, VA/IPA, security state, global flag) must match the command parameters for selective invalidation.
- A functional model may implement a simple "invalidate all" for correctness, but must correctly interpret the scoped commands for accurate behavior.
- `CMD_CFGI_*` and `CMD_TLBI_*` are often issued together when updating mappings; the model must handle both.
- Range-based invalidation (SMMUv3.2+) requires correct Level Hint parsing.
- Broadcast TLB maintenance from the PE requires the model to process external TLB operation signals, not just Command queue commands.

## Related Concepts

- [concepts/command-queue.md](concepts/command-queue.md) — all invalidation commands are issued via the Command queue; §3.21.2 queue access rules
- [concepts/two-stage-translation.md](concepts/two-stage-translation.md) — TLB entries cache the results of translation walks
- [concepts/stream-table-entry.md](concepts/stream-table-entry.md) — configuration cache invalidation uses STE-scoped commands; §3.21.3 update procedures
- [concepts/context-descriptor.md](concepts/context-descriptor.md) — ASID from CD tags TLB entries; ASET from CD; CD cache has its own invalidation
- [concepts/httu.md](concepts/httu.md) — access flag/dirty state updates interact with TLB invalidation rules; HTTU completion is part of §3.21.1
- [concepts/speculative-accesses.md](concepts/speculative-accesses.md) — speculative prefetch of structures governed by §3.21.3 completion rules
- [concepts/coherency-and-embedded-implementations.md](concepts/coherency-and-embedded-implementations.md) — coherency requirements for SMMU walks; COHACC; config caches not snooped
- [concepts/granule-protection-check.md](concepts/granule-protection-check.md) — GPT broadcast maintenance §3.17.7
- [concepts/security-states.md](concepts/security-states.md) — StreamWorld determines ASID/VMID/ASET tagging and which broadcast namespace applies

## Sources That Use This Concept

- [sources/ihi0070g-b-smmuv3-architecture-spec.md](sources/ihi0070g-b-smmuv3-architecture-spec.md) — §3.17 TLB tagging table, ASET semantics, broadcast maintenance, E2H, VMID wildcards, GPT broadcast, TLBInXS; §3.20.1–2 TLB/config conflict detection; §3.21.1 Translation tables and TLB invalidation completion; §3.21.1.1–3 BBML levels; §3.21.2 Queue access rules; §3.21.3 Config invalidation completion and structure update procedures; §4.3–4.4 invalidation command reference
