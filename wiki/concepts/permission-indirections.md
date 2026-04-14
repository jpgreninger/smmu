---
title: "Permission Indirections (S1PIE / S2PIE / S2POE)"
type: concept
tags: [smmu, permission, indirection, overlay, s1pie, s2pie, s2poe, smmuv3.4, armv8.9]
created: 2026-04-07
updated: 2026-04-07
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Permission Indirections (S1PIE / S2PIE / S2POE)

## Definition

Permission indirections and overlays are SMMUv3.4 features (corresponding to Armv8.9-A `FEAT_S1PIE`, `FEAT_S2PIE`, `FEAT_S2POE`) that decouple permission encoding in translation table descriptors from the actual access rights applied to a transaction. They allow a software-controlled permission remapping table to override or modify raw descriptor permissions.

## Stage 1 Permission Indirections (S1PIE)

- **Feature:** `SMMU_IDR3.S1PI == 1` (optional in SMMUv3.4).
- When enabled, the 4-bit permission index in stage 1 translation table descriptors indexes a Permission Indirection Register (PIR) to determine the actual access rights, rather than directly encoding read/write/execute permissions.
- Allows a VM to remap permissions for a set of pages by updating the PIR without modifying the translation table entries.
- Configured via `CD` (stage 1 configuration fields for S1PIE).
- See §3.26.1.

## Stage 2 Permission Indirections (S2PIE)

- **Feature:** `SMMU_IDR3.S2PI == 1` (optional in SMMUv3.4).
- Same concept at stage 2: permission index in stage 2 translation table descriptors is indirected through a stage 2 PIR.
- Configured via STE (stage 2 configuration fields for S2PIE).
- See §3.26.2.

## Stage 2 Permission Overlays (S2POE)

- **Feature:** `SMMU_IDR3.S2PO == 1` (optional in SMMUv3.4).
- Permission Overlays allow stage 2 to further restrict (overlay) the permissions determined by stage 1. The overlay narrows permissions; it cannot grant additional permissions.
- Uses a separate Permission Overlay Register (POR) indexed by the stage 1 output.
- See §3.26.2.

## Interaction with STE.S2PII

`STE.S2PII` is a register field introduced in SMMUv3.4 that controls whether stage 2 permission indirections are applied. Requires `SMMU_S2PII` register family.

## Translation Hardening (THE / AssuredOnly)

Related feature: `SMMU_IDR3.THE == 1` (optional in SMMUv3.4, `FEAT_THE`):
- Introduces a **Protected** attribute on translation table descriptors.
- **AssuredOnly permission checks** (§3.27.2): when a descriptor has the Protected attribute, additional permission checks are performed to enforce "assured-only" access policies.
- See §3.27 Translation Hardening.

## Model Implementation Notes

- S1PIE/S2PIE require additional per-CD and per-STE register state (the PIR table).
- A model implementing SMMUv3.4 must check the relevant `SMMU_IDR3` bits before enabling these features.
- S2POE adds a permission narrowing step after stage 1 completes and before stage 2 permission checks; the model must apply these in the correct order.
- These features significantly change the permission-check logic compared to SMMUv3.3 and earlier. Version-gating in the model is essential.

## Related Concepts

- [[concepts/two-stage-translation]] — permission indirections modify the permission output of each translation stage
- [[concepts/context-descriptor]] — S1PIE configuration is held in the CD
- [[concepts/stream-table-entry]] — S2PIE and S2POE configuration is in the STE
- [[concepts/fault-models]] — F_PERMISSION faults arise from permission check failures

## Sources That Use This Concept

- [[sources/ihi0070g-b-smmuv3-architecture-spec]] — §3.26 Permission Indirections; §3.27 Translation Hardening; §2.8 SMMUv3.4 features
