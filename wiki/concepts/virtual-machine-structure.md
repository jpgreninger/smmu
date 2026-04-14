---
title: "Virtual Machine Structure (VMS)"
type: concept
tags: [smmu, vms, virtualization, vmid, smmuv3.2, configuration]
created: 2026-04-07
updated: 2026-04-07
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Virtual Machine Structure (VMS)

## Definition

The Virtual Machine Structure (VMS) is an in-memory data structure introduced in SMMUv3.2 that holds per-VM configuration information, scoped by VMID. It reduces duplication of per-VM state across multiple STEs that share the same VMID (i.e., multiple devices assigned to the same VM).

VMS presence is indicated by `SMMU_IDR0.VMS == 1` (optional, SMMUv3.2+).

## VMS Contents

The VMS contains per-VM configuration such as:
- VMID-scoped settings that would otherwise be duplicated in every STE of a given VM.
- Configuration relevant to the stage 2 translation regime for a VM.

(Full field list is in §5.6 VMS data structure format.)

## VMS Location

The VMS is pointed to from an STE via `STE.VMSPtr`. The pointer is a PA.

If `STE.VMSPtr` points to an address out of range of the OAS, a C_BAD_STE event is generated.

## VMS Fetching

The VMS is fetched from memory when the SMMU processes a transaction for a stream whose STE has a VMS pointer. Like STEs and CDs, the VMS may be cached by the SMMU.

## VMS Caching and Invalidation

- The SMMU may cache VMS entries.
- Invalidation command: `CMD_CFGI_VMS_PIDM(SSec, VMID)` — invalidates the VMS cache entry for the specified VMID.
- Part of the standard configuration invalidation flow when per-VM configuration changes.

## Relationship to STE

A VMS is associated with a VMID, which may be shared across multiple STEs. When multiple STEs share the same VMID, they may all point to the same VMS. Updating the VMS changes behavior for all streams associated with that VMID.

## Model Implementation Notes

- VMS support is optional; a model must check `SMMU_IDR0.VMS` before implementing VMS fetch logic.
- VMS adds an additional memory access in the configuration lookup path; a performance model should account for this latency.
- VMS cache invalidation (`CMD_CFGI_VMS_PIDM`) is a separate invalidation scope from STE and CD invalidation.

## Related Concepts

- [[concepts/stream-table-entry]] — STE.VMSPtr points to the VMS
- [[concepts/two-stage-translation]] — VMS holds per-VM stage 2 configuration
- [[concepts/tlb-invalidation]] — VMS invalidation is distinct from TLB invalidation

## Sources That Use This Concept

- [[sources/ihi0070g-b-smmuv3-architecture-spec]] — §5.6 VMS data structure; §3.3 Data structures; §4.3.5 CMD_CFGI_VMS_PIDM; §2.4 SMMUv3.2 features
