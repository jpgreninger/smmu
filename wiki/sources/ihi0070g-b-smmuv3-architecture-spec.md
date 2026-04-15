---
title: "Arm SMMUv3 Architecture Specification (IHI 0070 G.b)"
type: source
tags: [smmu, arm, iommu, virtualization, security, rme, pcie, architecture-spec]
created: 2026-04-07
updated: 2026-04-07
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Arm SMMUv3 Architecture Specification (IHI 0070 G.b)

**Type:** architecture-spec
**Author(s):** Arm Limited
**Date:** 2025-04-30 (version G.b)
**Document number:** ARM IHI 0070 G.b
**Original file:** `raw/IHI0070G_b-System_Memory_Management_Unit_Architecture_Specification.md`

## Summary

This is the definitive architecture specification for the Arm System Memory Management Unit version 3 (SMMUv3), covering all feature versions from SMMUv3.0 through SMMUv3.4 plus the SMMU-for-RME and SMMU-for-RME DA extensions. The document is non-confidential and governs compliant SMMU implementations.

The SMMU translates DMA addresses from I/O client devices before they enter the system interconnect — performing an analogous role to the PE MMU but for device-initiated traffic. It supports two stages of translation (VA→IPA via stage 1, IPA→PA via stage 2), multiple security states (Non-secure, Secure, Realm), and integration with PCIe ATS/PRI. Configuration is memory-based (scalable to thousands of streams), unlike SMMUv1/v2 which used register-based context banks.

This specification is the primary reference for functional and performance model implementation. It defines exact data structure formats, translation procedures, fault semantics, command/event queue mechanics, register layouts, and security state behavior. All model implementations should be verified against the content of this vault.

## Key Claims

- The SMMU translates DMA traffic only (not traffic from system to device). Client device traffic is active; PE-to-device traffic is managed by PE MMUs.
- SMMUv3 is not backward-compatible with SMMUv2 due to the switch to memory-based configuration.
- An SMMU must support at least one stage of translation. Either or both stages may be absent, in which case the absent stage behaves as permanent bypass.
- The SMMU_AIDR[7:0] register encodes the architecture version: 0x00=SMMUv3.0, 0x01=SMMUv3.1, 0x02=SMMUv3.2, 0x03=SMMUv3.3, 0x04=SMMUv3.4.
- An SMMUv3.x implementation may include any arbitrary subset of SMMUv3.(x+1) features, but no features of SMMUv3.(x+2) or later.
- Translation table formats are identical to Armv8-A PE formats; SMMU translation tables are shareable with PEs.
- All behavior marked CONSTRAINED UNPREDICTABLE, UNPREDICTABLE, IMPLEMENTATION DEFINED, or IMPLEMENTATION SPECIFIC must be explicitly handled in model implementations.

## Key Entities

- [../entities/arm-limited.md](../entities/arm-limited.md) — publisher and architect of the specification

## Key Concepts

- [../concepts/two-stage-translation.md](../concepts/two-stage-translation.md) — VA→IPA (stage 1) and IPA→PA (stage 2); independently enableable
- [../concepts/stream-table-entry.md](../concepts/stream-table-entry.md) — per-device configuration structure; indexed by StreamID
- [../concepts/context-descriptor.md](../concepts/context-descriptor.md) — stage 1 translation configuration; indexed by SubstreamID
- [../concepts/streamid-substreamid.md](../concepts/streamid-substreamid.md) — device/process identification for SMMU lookup
- [../concepts/command-queue.md](../concepts/command-queue.md) — software-to-SMMU command interface (circular buffer)
- [../concepts/event-queue.md](../concepts/event-queue.md) — SMMU-to-software fault/event reporting interface (circular buffer)
- [../concepts/fault-models.md](../concepts/fault-models.md) — Terminate and Stall models for translation-related faults
- [../concepts/security-states.md](../concepts/security-states.md) — Non-secure, Secure, Realm, and Root security state support
- [../concepts/granule-protection-check.md](../concepts/granule-protection-check.md) — RME-era physical address space check against GPT
- [../concepts/pcie-ats-pri.md](../concepts/pcie-ats-pri.md) — PCIe Address Translation Services and Page Request Interface
- [../concepts/virtual-machine-structure.md](../concepts/virtual-machine-structure.md) — per-VM configuration structure (VMS), added SMMUv3.2
- [../concepts/httu.md](../concepts/httu.md) — Hardware Translation Table Update (access flag and dirty state)
- [../concepts/tlb-invalidation.md](../concepts/tlb-invalidation.md) — commands and broadcast mechanisms for TLB maintenance
- [../concepts/smmu-initialization.md](../concepts/smmu-initialization.md) — reset, enable sequence, and configuration prerequisites
- [../concepts/device-permission-table.md](../concepts/device-permission-table.md) — DPT, a PA-space gating table for RME DA (SMMUv3.4+)
- [../concepts/translation-hardening.md](../concepts/translation-hardening.md) — THE/AssuredOnly permission checks (SMMUv3.4)
- [../concepts/permission-indirections.md](../concepts/permission-indirections.md) — S1PIE/S2PIE/S2POE stage permission remapping (SMMUv3.4)

## Tensions & Open Questions

- Many behaviors are CONSTRAINED UNPREDICTABLE or IMPLEMENTATION DEFINED; models must choose one legal behavior and document the choice.
- The spec permits event merging (STE.MEV); a functional model must decide whether to implement merging and to what degree.
- Performance model accuracy depends on IMPLEMENTATION SPECIFIC choices (TLB sizing, queue scheduling, cache policies) that the architecture does not constrain.
- SMMUv3.0 vs SMMUv3.1+ behavior differs in several address size fault paths; models targeting a specific version must track version-gated behavior carefully.
- The spec is written from a hardware perspective; some software-visible behaviors (e.g., PROD/CONS index inconsistency outcomes) are CONSTRAINED UNPREDICTABLE and a model must pick one.

## Related Pages

- [../synthesis/smmu-translation-pipeline.md](../synthesis/smmu-translation-pipeline.md)
- [../synthesis/smmu-queue-mechanics.md](../synthesis/smmu-queue-mechanics.md)
- [../synthesis/smmu-fault-model.md](../synthesis/smmu-fault-model.md)
- [../synthesis/smmu-security-states.md](../synthesis/smmu-security-states.md)
- [../synthesis/smmu-pcie-ats-integration.md](../synthesis/smmu-pcie-ats-integration.md)
- [../synthesis/smmu-version-feature-map.md](../synthesis/smmu-version-feature-map.md)
- [../synthesis/smmu-register-map.md](../synthesis/smmu-register-map.md)
- [../synthesis/smmu-system-implementation.md](../synthesis/smmu-system-implementation.md)
- [../concepts/atos.md](../concepts/atos.md)
- [../concepts/performance-monitors.md](../concepts/performance-monitors.md)
- [../concepts/debug-trace.md](../concepts/debug-trace.md)
- [../concepts/ras.md](../concepts/ras.md)
- [../concepts/attribute-transformation.md](../concepts/attribute-transformation.md)
- [../concepts/mpam.md](../concepts/mpam.md)
- [../concepts/mec.md](../concepts/mec.md)
- [../concepts/translation-hardening.md](../concepts/translation-hardening.md)
