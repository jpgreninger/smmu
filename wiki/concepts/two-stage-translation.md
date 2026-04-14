---
title: "Two-Stage Translation"
type: concept
tags: [smmu, translation, virtualization, stage1, stage2, mmu]
created: 2026-04-07
updated: 2026-04-07
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Two-Stage Translation

## Definition

The SMMU supports two independent and optionally-combined stages of address translation:

- **Stage 1:** Translates the incoming Virtual Address (VA) to an Intermediate Physical Address (IPA). Configured per-stream (via the [[concepts/context-descriptor]]) and optionally per-substream.
- **Stage 2:** Translates the IPA to a Physical Address (PA). Configured per-stream via the [[concepts/stream-table-entry]].

When both are enabled the configuration is called **nested**. When only one is enabled the other behaves as bypass (address passes through unmodified). An SMMU must support at least one stage; the absent stage is equivalent to permanent bypass.

## Precise Lookup Sequence

An incoming transaction is processed in the following logical order:

1. **Global bypass check:** If `SMMU_CR0.SMMUEN == 0`, the transaction bypasses the SMMU entirely; attributes are taken from `SMMU_GBPA` (or the transaction is aborted if `SMMU_GBPA.ABORT == 1`).
2. **STE lookup:** The StreamID indexes the Stream table to locate an STE. If the StreamID is out of range or the STE is invalid/disabled, the transaction is terminated and an event recorded.
3. **Stage 2 config:** If the STE enables stage 2 (`STE.Config[1] == 1`), the STE contains `STE.S2TTB` (stage 2 translation table base pointer) and `STE.S2VMID`.
4. **CD lookup (stage 1):** If the STE enables stage 1 (`STE.Config[0] == 1`), a CD is located using `STE.S1ContextPtr` (optionally indexed by SubstreamID). If stage 2 is also enabled, the CD is fetched from IPA space (i.e., the CD address is translated through stage 2). If stage 2 is not enabled, the CD is fetched from PA space.
5. **Stage 1 translation:** The CD's `TTB0`/`TTB1` is walked. The output is an IPA (or bypassed if `STE.S1DSS` causes bypass). Walk addresses for stage 1 are themselves subject to stage 2 translation when nested is configured.
6. **Stage 2 translation:** The IPA is walked using `STE.S2TTB`. Output is a PA.
7. **Output:** The final PA, with memory attributes, is forwarded into the system.

Any step may generate a fault; behavior on fault depends on the [[concepts/fault-models]] configuration.

## Address Sizes

- **VA input range:** Configurable up to VAS bits (49/53/56 depending on `SMMU_IDR5.VAX`); sign-extended from the top significant bit.
- **IPA range:** Capped to the OAS (Output Address Size) for VMSAv8-64; fixed at 40 bits for VMSAv8-32 LPAE.
- **PA output range:** The OAS as reported in `SMMU_IDR5.OAS`.
- Stage 1 Address Size fault: input VA exceeds the configured range.
- Stage 2 Address Size fault: IPA output from stage 1 exceeds the effective stage 2 input range.

## Relationship to PE Translation

SMMU translation table formats are identical to Armv8-A PE formats (VMSAv8-32 LPAE, VMSAv8-64, VMSAv9-128). Tables may be shared between SMMU and PE without modification. The SMMU stage 1 / stage 2 split mirrors the PE EL1 / EL2 hypervisor split: stage 1 for OS-level isolation, stage 2 for VM-level isolation.

## Model Implementation Notes

- A model must handle the case where stage 1 is bypassed but stage 2 is not: in this case the input VA is passed directly to stage 2 as the IPA; an IAS range check is still performed and can generate an F_ADDR_SIZE fault.
- Stage 2 faults during a stage 1 table walk (i.e., the walk address is in IPA space and stage 2 faults on it) are reported as stage 2 faults, not stage 1 faults. The event record differentiates these for hypervisor use.
- A stage 2 fault while fetching a CD from IPA space is also a stage 2 fault.
- TBI (Top Byte Ignore) is only meaningful when stage 1 uses a CD. When stage 1 is bypassed/disabled, TBI is always disabled.

## Related Concepts

- [[concepts/stream-table-entry]] — contains stage 2 config and pointer to stage 1 config
- [[concepts/context-descriptor]] — contains stage 1 config (TTB0/TTB1, ASID, fault behavior flags)
- [[concepts/fault-models]] — governs what happens when translation fails
- [[concepts/tlb-invalidation]] — how TLB entries tagged with ASID/VMID are invalidated
- [[concepts/httu]] — hardware update of access flag/dirty state during walks

## Sources That Use This Concept

- [[sources/ihi0070g-b-smmuv3-architecture-spec]] — §3.3 Data structures and translation procedure; §3.4 Address sizes; §3.3.3 Configuration and Translation lookup
