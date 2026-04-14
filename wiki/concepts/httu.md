---
title: "Hardware Translation Table Update (HTTU)"
type: concept
tags: [smmu, httu, access-flag, dirty-state, translation-table, hardware-update]
created: 2026-04-07
updated: 2026-04-07
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Hardware Translation Table Update (HTTU)

## Definition

Hardware Translation Table Update (HTTU) is the SMMU feature that automatically updates the **Access flag** (AF) and/or **Dirty state** (DBM/dirty bit) in translation table descriptors when a translation is used, without requiring a software fault-and-update cycle. This mirrors the `FEAT_HAFDBS` feature in Armv8-A PEs.

HTTU capability is reported in `SMMU_IDR0.HTTU`:

| `SMMU_IDR0.HTTU` | Capability |
|-----------------|------------|
| 0b00            | No HTTU support |
| 0b01            | Access flag hardware update only |
| 0b10            | Access flag + Dirty state hardware update |

## Access Flag Update

When the Access flag (AF) in a translation table descriptor is 0 and HTTU is enabled (`SMMU_IDR0.HTTU >= 0b01`):
- Without HTTU: an F_ACCESS fault occurs when AF=0 is encountered during a walk.
- With HTTU: the SMMU atomically sets AF=1 in the descriptor (via a hardware read-modify-write to memory) and continues the translation without a fault.

HTTU for the Access flag is enabled per-CD via `CD.HA = 1` (stage 1) or per-STE via the stage 2 equivalent field.

## Dirty State (DBM) Update

When HTTU for dirty state is supported (`SMMU_IDR0.HTTU == 0b10`):
- A write to a page whose Dirty Bit Modifier (DBM/AP) indicates the page is "clean" and hardware dirty-state tracking is enabled: the SMMU atomically marks the page as "dirty" by updating the descriptor.
- This avoids a fault-and-fixup cycle for the first write to a clean page.

Enabled per-CD via `CD.HD = 1` (stage 1).

## HTTU with Two Stages

In nested translation (`STE.Config == 0b111`):
- HTTU updates must occur in the correct PA space. Stage 1 descriptor updates are made through the stage 2 IPA→PA translation (the AF/dirty write to a stage 1 descriptor is itself a DMA-like write that must be translated through stage 2).
- Stage 2 HTTU updates target PA space directly.
- Ordering and fault behavior for HTTU in the two-stage case is detailed in §3.13.5.

## Table Descriptor Access Flag (SMMUv3.4)

SMMUv3.4 (`FEAT_HAFT`, `SMMU_IDR0.HTTU` with additional encoding) adds hardware update of the Access flag in **table** (non-leaf) descriptors, not just leaf page descriptors. This is `SMMU_IDR3.HAFT`-gated.

## HTTU for Cache Maintenance Operations and Destructive Reads

HTTU behavior may differ for Cache Maintenance Operations (CMO) and Destructive Read transactions:
- Destructive reads (e.g., PCIe DMWr) may interact with dirty state tracking differently.
- See §3.13.8 for CMO and Destructive Read interactions.

## ATS and PRI Interactions

When ATS is in use and HTTU is enabled:
- An ATS Translation Request may trigger an HTTU update of AF/dirty state in the translation table.
- If HTTU fails (e.g., write to translation table is not permitted), behavior is IMPLEMENTATION DEFINED.
- See §3.13.7.

## Software Update of Flags

When HTTU is not supported or not enabled, software must handle F_ACCESS faults:
1. Software receives F_ACCESS event from Event queue.
2. Software sets the AF bit in the relevant translation table descriptor.
3. Issues appropriate TLB invalidation.
4. Resumes the stalled transaction (if stall model) or accepts the abort.

## Model Implementation Notes

- A functional model implementing HTTU must perform atomic read-modify-write operations on translation table descriptors in memory when AF/dirty conditions are encountered.
- In a two-stage nested model, the HTTU write for a stage 1 descriptor must itself be translated through stage 2 (it is an IPA write).
- For performance models, HTTU eliminates the F_ACCESS fault/resume overhead for frequently-accessed pages; the model should track HTTU update rates as a performance metric.
- HTTU support is optional (IMPLEMENTATION DEFINED); a model targeting a specific implementation must check `SMMU_IDR0.HTTU` and disable HTTU paths accordingly.

## Related Concepts

- [[concepts/two-stage-translation]] — HTTU interacts differently at stage 1 and stage 2
- [[concepts/fault-models]] — F_ACCESS is the fault raised when AF=0 and HTTU is not enabled
- [[concepts/tlb-invalidation]] — TLB invalidation required after software updates AF/dirty

## Sources That Use This Concept

- [[sources/ihi0070g-b-smmuv3-architecture-spec]] — §3.13 Translation tables and Access flag/Dirty state; §3.13.1–3.13.8; §2.2 SMMUv3.0 features (HTTU); §2.8 SMMUv3.4 features (HAFT)
