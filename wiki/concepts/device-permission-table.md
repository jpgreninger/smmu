---
title: "Device Permission Table (DPT)"
type: concept
tags: [smmu, dpt, rme, realm, device-permission, pa-space, smmuv3.4]
created: 2026-04-07
updated: 2026-04-07
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Device Permission Table (DPT)

## Definition

The Device Permission Table (DPT) is an in-memory structure (added in SMMU-for-RME DA, indicated by `SMMU_R_IDR3.DPT == 1`) that gates device DMA access at a PA-space granularity. It associates physical address ranges with permitted device access rights, specifically controlling which PA space (Realm vs Non-secure) a device is permitted to access.

The DPT is distinct from the Granule Protection Table (GPT) ([[concepts/granule-protection-check]]). GPT controls physical memory ownership at the granule level for all agents (PEs and devices); DPT is an additional per-device PA-space gate specifically for the SMMU's Realm-state ATS flow.

DPT support is optional (`SMMU_R_IDR3.DPT`). Support is strongly recommended for RME DA systems.

## DPT Configuration

Configured via:
- `SMMU_R_DPT_BASE` — base address of the DPT.
- `SMMU_R_DPT_BASE_CFG` — format/size configuration.
- `SMMU_R_DPT_CFG_FAR` — fault address register for DPT lookup errors.

DPT walking is enabled by `SMMU_R_CR0.DPT_WALK_EN`.

Non-secure and Realm equivalent registers (`SMMU_DPT_BASE`, `SMMU_DPT_CFG_FAR`) also exist for Non-secure DPT configurations.

## When DPT Checks Are Applied

DPT checks are applied to ATS Translated transactions when `STE.EATS == 0b11` (Use DPT):
- If `DPT_WALK_EN == 0` and the check cannot be resolved from existing DPT TLB cache: DPT_DISABLED fault, F_TRANSL_FORBIDDEN recorded, transaction aborted.
- If `DPT_WALK_EN == 1`: the SMMU walks the DPT.
  - If DPT check fails with Device Access fault: transaction aborted, F_TRANSL_FORBIDDEN recorded.
  - If DPT check fails with DPT lookup fault: transaction aborted, F_TRANSL_FORBIDDEN recorded, fault reported in `SMMU_R_DPT_CFG_FAR`.

## DPT vs Full ATS vs Split-Stage ATS

For Realm streams, configuring `STE.EATS`:
- `0b01` (Full ATS): SMMU performs full translation for ATS requests; Translated transactions bypass stage translation but are checked against GPT.
- `0b10` (Split-stage ATS): stage 2 applied to ATS Translated transactions (IPA→PA). Requires `ATSCHK == 1`.
- `0b11` (Use DPT): same as Full ATS, plus DPT check gates the PA space for Translated transactions.

See §3.24.7 for guidance on choosing between Split-stage ATS and Full ATS + DPT.

## DPT Caching Behavior

The SMMU may cache DPT lookups:
- Cached DPT entries are invalidated via `CMD_DPTI_ALL` (invalidate entire DPT cache) or `CMD_DPTI_PA(address)` (invalidate by PA).
- `CMD_SYNC` after `CMD_DPTI_*` ensures invalidation completion.

## DPT Lookup Errors

MBZ (Must Be Zero) fields in DPT descriptors: if a DPT descriptor has a non-zero MBZ field, the descriptor is Invalid. This produces a DPT lookup fault.

## Software Guidance

From §3.24.6: Software should configure DPT entries to permit device access only to the expected PA space for each device. Incorrect DPT configuration can result in F_TRANSL_FORBIDDEN faults blocking legitimate DMA.

## Model Implementation Notes

- DPT is a relatively new feature (RME DA); a model targeting SMMUv3.4 with RME DA must implement DPT walking when `SMMU_R_IDR3.DPT == 1` and `DPT_WALK_EN == 1`.
- DPT interactions with ATS are checked at the Translated transaction input: the check uses either cached DPT results or a fresh walk, depending on implementation.
- `CMD_DPTI_ALL` / `CMD_DPTI_PA` are the only DPT maintenance commands; they do not affect TLBs or STE/CD caches.

## Related Concepts

- [[concepts/granule-protection-check]] — GPT provides physical memory ownership; DPT gates device ATS access
- [[concepts/pcie-ats-pri]] — DPT applies to ATS Translated transactions (EATS == 0b11)
- [[concepts/security-states]] — DPT is primarily relevant to Realm-state streams
- [[concepts/command-queue]] — CMD_DPTI_ALL and CMD_DPTI_PA for DPT cache maintenance

## Sources That Use This Concept

- [[sources/ihi0070g-b-smmuv3-architecture-spec]] — §3.24 Device Permission Table; §3.24.1–3.24.7; §4.6 DPT maintenance commands; §2.7 SMMU for RME DA features
