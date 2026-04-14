---
title: "Context Descriptor (CD)"
type: concept
tags: [smmu, cd, context-descriptor, stage1, translation, data-structure]
created: 2026-04-07
updated: 2026-04-07
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Context Descriptor (CD)

## Definition

A Context Descriptor (CD) is the stage 1 translation configuration structure for an SMMU stream or substream. It is pointed to by the [[concepts/stream-table-entry]] via `STE.S1ContextPtr` and optionally indexed by SubstreamID. Each CD contains:

- Stage 1 translation table base pointers (`CD.TTB0`, `CD.TTB1`).
- ASID for TLB tagging.
- Translation table format, granule, address size, and input range configuration.
- Fault behavior flags (`CD.S`, `CD.R`, `CD.A`) controlling the [[concepts/fault-models]] for stage 1.
- TBI (Top Byte Ignore) configuration.
- NSCFG fields controlling output NS attribute for stage 1 translation table walks.
- AA64 flag selecting VMSAv8-64 vs VMSAv8-32 LPAE format.

## CD Location

The CD address derived from `STE.S1ContextPtr` is:
- A **PA** when only stage 1 is active.
- An **IPA** (subject to stage 2 translation) when nested (stage 1 + stage 2) is configured.

For substream configurations, `STE.S1ContextPtr` may point to:
- A flat array of CDs indexed by SubstreamID.
- An L1CD table (indexed by upper SubstreamID bits), where each L1CD entry points to an L2 CD array (indexed by lower SubstreamID bits).

## Stage 1 Fault Behavior Flags

The three fault flag bits in the CD control behavior for Translation-related faults (F_TRANSLATION, F_ADDR_SIZE, F_ACCESS, F_PERMISSION) at stage 1:

| CD.S | CD.A | Behavior on stage 1 Translation-related fault |
|------|------|------------------------------------------------|
| 0    | 0    | Terminate with RAZ/WI (read returns zero, write ignored) |
| 0    | 1    | Terminate with abort |
| 1    | —    | Stall (if `STE.S1STALLD == 0` and stall model supported) |

`CD.R` enables recording of fault events. When `CD.R == 0`, fault events may not be recorded.

## Translation Table Base Pointers

- `CD.TTB0` — used for VA[55] == 0 (lower address range), or all addresses when `STE.Config` uses stage 1 only with VMSAv8-32 LPAE.
- `CD.TTB1` — used for VA[55] == 1 (upper address range); only valid for VMSAv8-64 and VMSAv9-128.
- `CD.T0SZ` / `CD.T1SZ` — determine the input VA range for each table (number of ignored most-significant bits).
- `CD.TG0` / `CD.TG1` — translation granule for TTB0 / TTB1 (4 KB, 16 KB, 64 KB).
- `CD.IPS` — effective IPA output size, capped to OAS. Ignored for VMSAv8-32 LPAE (fixed 40 bits).
- `CD.AA64` — if 1, VMSAv8-64 (or VMSAv9-128 with D128). If 0, VMSAv8-32 LPAE.

## ASID

`CD.ASID` tags TLB entries created from this CD's translations. Used during TLB lookup and for targeted TLB invalidation (`CMD_TLBI_NH_ASID`, etc.). The ASID namespace is per-security-state. ASID width is up to 16 bits if `SMMU_IDR0.ASID16 == 1`, otherwise 8 bits.

## CD ILLEGAL Conditions

A CD is ILLEGAL (and generates C_BAD_CD) if, for example:
- `CD.TTB0` or `CD.TTB1` contains an address outside the effective output address size range.
- Conflicting or reserved field values are set.
The full list is in §5.4.2 Validity of CD.

## Model Implementation Notes

- The CD is fetched from memory (PA or IPA) during the configuration lookup phase. A functional model must implement the full fetch + validity check before beginning the stage 1 table walk.
- For nested configurations, the CD fetch address is an IPA, meaning a stage 2 walk occurs before the stage 1 walk begins. A stage 2 fault during CD fetch is reported as a stage 2 fault with a record indicating the CD was being fetched.
- `CD.ASID` is critical for TLB tagging; a model must associate every inserted TLB entry with the ASID from the CD used to generate it.
- `CD.TBI` affects address range checking: when enabled, VA[63:56] are ignored and the effective sign-extension is from VA[55].

## Related Concepts

- [[concepts/stream-table-entry]] — STE contains `S1ContextPtr` pointing to CD
- [[concepts/two-stage-translation]] — CD governs stage 1 of the translation
- [[concepts/fault-models]] — CD.{S, R, A} flags configure stage 1 fault behavior
- [[concepts/streamid-substreamid]] — SubstreamID selects the CD within a CD table
- [[concepts/tlb-invalidation]] — ASID from CD used to tag and invalidate TLB entries

## Sources That Use This Concept

- [[sources/ihi0070g-b-smmuv3-architecture-spec]] — §3.3.2 StreamIDs to Context Descriptors; §5.4 CD data structure format; §5.5 Fault configuration bits; §3.4.1 Input address size
