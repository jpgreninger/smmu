---
title: "Reliability, Availability, and Serviceability (RAS)"
type: concept
tags: [smmu, ras, reliability, errors, sfm, ecc, poison, fault-handling]
created: 2026-04-13
updated: 2026-04-16
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Reliability, Availability, and Serviceability (RAS)

## Definition

Chapter 12 of the SMMUv3 specification covers optional RAS features aligned to the **Arm RAS System Architecture** [IHI0074]. RAS support allows the SMMU to detect, classify, record, and propagate hardware errors in a standardized way. Note: within this chapter, "RAS fault" refers to hardware error conditions (corrupted data, ECC failures) — distinct from SMMU translation faults.

## Error Classification (per Arm RAS Architecture)

| Class | Abbreviation | Meaning |
|---|---|---|
| Corrected Error | CE | Error detected and corrected in place |
| Deferred Error | DE | Error detected but deferred (e.g., poison propagation) |
| Uncorrected Error, Uncontainable | UC | Cannot be contained |
| Uncorrected Error, Unrecoverable | UEU | Not recoverable |
| Uncorrected Error, Recoverable | UER | Recoverable with software action |
| Uncorrected Error, Restartable | UEO | Restartable |

## Error Sources in an SMMU

SMMU activity is demand-driven by incoming client transactions. Errors can arise from:

1. **Internal state errors** — corruption of internal registers or caches.
2. **External state errors** — errors encountered while reading translation tables, configuration structures, or queue entries from memory.

If the SMMU consumes an error during translation for an external transaction, it achieves containment by deferring the error to the source — typically aborting the transaction (CA for PCIe). Silent propagation must be avoided to prevent silent data corruption (SDC).

## SMMU-Architectural Events Mapped to RAS

When RAS is supported, the SMMU must record errors via its standard Event queue and GERROR mechanism in addition to RAS registers:

| SMMU Event | Trigger |
|---|---|
| `F_WALK_EABT` | Translation table walk consumed an error |
| `F_STE_FETCH` | STE fetch consumed an error |
| `F_CD_FETCH` | CD fetch consumed an error |
| `F_VMS_FETCH` | VMS fetch consumed an error |
| `GERROR.CMDQ_ERR` + `CERROR_ABT` | Command queue fetch consumed an error |
| `GERROR.PRIQ_ABT_ERR` | PRI queue access aborted |
| `GERROR.EVENTQ_ABT_ERR` | Event queue access aborted |
| `GERROR.DPT_ERR` + `DPT_EABT` | DPT lookup consumed an error |

## Service Failure Mode (SFM) (§12.3)

If internal consistency is known or likely to have been lost (e.g., UE in internal register state), the SMMU **must** enter Service Failure Mode:

- All client transactions are terminated after SFM entry.
- The SMMU stops accessing its queues.
- PCIe requests receive a CA (Completer Abort) response where possible.
- Registers remain readable to aid diagnosis (Arm recommendation).
- Recovery requires at minimum a **system reset** (mechanism is IMPLEMENTATION DEFINED).
- SFM entry is signaled by `SMMU_GERROR.SFM_ERR` and `SMMU_S_GERROR.SFM_ERR` (both Non-secure and Secure programming interfaces, if implemented), plus IMPLEMENTATION DEFINED means (e.g., RAS Error Recovery Interrupt).

If an implementation has isolated partitions where a UE is confined to one portion, global SFM may not be required for that isolated UE.

## RAS Register Interface (§12.4)

When RAS is implemented:
- At least one group of memory-mapped **error recording registers** per the RAS error record format [IHI0074] must be provided.
- **Error Recovery Interrupts** and **Fault Handling Interrupts** must be present.
- The number of RAS error records, their association to nodes, and whether Corrected error counters are implemented are all IMPLEMENTATION DEFINED.
- Base addresses and discovery of RAS register frames are IMPLEMENTATION DEFINED (may use firmware description).
- Separate RAS interfaces may be provided per supported Security state.
- RAS interrupts for SMMUv3 features may be edge-triggered or level-sensitive per [IHI0074] (unlike core SMMU interrupts which must be edge-triggered or MSI).

## Confidential Information in RAS Records (§12.5)

On platforms with `FEAT_RME`, the RAS System Architecture requirements on **Confidential Data** apply to SMMU RAS Error Record registers. RAS error records must not leak Realm-state information to Non-secure or Secure observers.

## Key RAS Event Reporting Examples (§12.6, informative)

### Deferred error on structure fetch (e.g., poisoned read response)
- Reported as `UE=1, ER=1, PN=1, UET=0b11, SERR=21`.
- Signaled to client as CA (PCIe) or abort.

### Uncorrectable error on structure fetch
- Reported as `UE=1, ER=1, PN=0, UET=0b11, SERR=12`.

### ECC/EDC error in TLB or configuration cache
- Corrected: `CE!=0b00, ER=0, SERR IN {1,6,7,8,9}`.
- Entry invalidated and re-fetched on EDC; corrected in place on ECC.

### Error on Command queue fetch
- Reported as `UE=1, ER=0, SERR IN {12, 21}`.

## Tensions & Open Questions

- RAS support is optional; functional models that do not target hardware safety validation may omit it.
- All RAS register layouts and interrupt connections are IMPLEMENTATION DEFINED; no two implementations are guaranteed to be compatible.
- Whether the SMMU can detect data payload corruption on client transactions is also IMPLEMENTATION DEFINED.

## Related Concepts

- [fault-models.md](fault-models.md) — Translation faults (SMMU architectural faults) are distinct from RAS hardware errors, though both may use Event queue entries
- [event-queue.md](event-queue.md) — F_WALK_EABT, F_STE_FETCH, F_CD_FETCH are reported here
- [command-queue.md](command-queue.md) — `GERROR.CMDQ_ERR` / `CERROR_ABT` reported on command fetch error
- [security-states.md](security-states.md) — Separate RAS interfaces may exist per Security state; Realm confidentiality requirements apply
- [smmu-initialization.md](smmu-initialization.md) — SFM entry requires system reset to exit
- [debug-trace.md](debug-trace.md) — complementary Ch. 11 IMPLEMENTATION DEFINED debug facilities; PMCG-based diagnostic visibility; Security state isolation constraints shared with RAS
- [../synthesis/smmu-fault-model.md](../synthesis/smmu-fault-model.md) — §7.5 GERROR toggle handshake; CMDQ_ERR and DPT_ERR recovery procedures; GERROR flag table
- [../synthesis/smmu-register-map.md](../synthesis/smmu-register-map.md) — SMMU_ERR* RAS register group placement in the memory map

## Sources That Use This Concept

- [../sources/ihi0070g-b-smmuv3-architecture-spec.md](../sources/ihi0070g-b-smmuv3-architecture-spec.md) — Chapter 12 Reliability, Availability and Serviceability; §12.1–12.6; SFM specification
