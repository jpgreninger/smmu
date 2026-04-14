---
title: "Granule Protection Check (GPC)"
type: concept
tags: [smmu, gpc, gpt, rme, realm, security, physical-address]
created: 2026-04-07
updated: 2026-04-07
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Granule Protection Check (GPC)

## Definition

A Granule Protection Check (GPC) is a check performed by the SMMU on the output physical address of a transaction against the Granule Protection Table (GPT). The GPT is an in-memory structure that maps physical address granules to a PA space (Realm, Secure, Non-secure). A GPC fault occurs when:

- The GPT lookup could not be completed (access fault), OR
- The lookup succeeded but the access PA space does not match the granule's assigned PA space.

A **GPF (Granule Protection Fault)** is the specific fault type when the lookup succeeds but the check fails. The broader term **GPC fault** covers both access failures and GPF.

GPCs are part of the RME (Realm Management Extension) and require `SMMU_ROOT_IDR0.ROOT_IMPL == 1`.

## When GPCs Are Applied

GPCs apply to **all** transactions whose output PA falls within the GPT-controlled range:
- Client-originated transactions (after translation to PA).
- SMMU-originated accesses (table walks, queue accesses, MSI writes) from Realm security state.
- ATS Translated transactions (checked against the PA space determined from the translation/DPT/NSCFG).

Key rules from §3.25:
- GPC is performed on the PA output, not the input address.
- For client-originated accesses: the PA space determined by the translation (or bypass/NSCFG for bypassed streams) is checked against the GPT entry for the PA.
- For PCIe ATS: Translated transactions from Realm streams are also subject to GPC.
- SMMU-originated accesses from Realm state are subject to GPC.

## GPT Structure

The GPT is configured via Root-state registers:
- `SMMU_ROOT_GPT_BASE` — base physical address of the GPT.
- `SMMU_ROOT_GPT_BASE_CFG` — configuration (size, granule size, etc.).
- `SMMU_ROOT_GPT_BASE2` / `SMMU_ROOT_GPT_BASE_UPDATE` — additional GPT management.

The GPT maps each physical granule to a "Location" (PA space tag: Realm, Secure, Non-secure, or Shared). Accesses to a granule tagged Realm from a Non-secure or Secure transaction will GPC fault; accesses from a Realm transaction to a Non-secure-tagged granule will also GPC fault.

## GPC Fault Reporting

- GPC faults are visible to Non-secure, Realm, and Secure states (when `SMMU_IDR0.RME_IMPL == 1`).
- GPC fault address is reported in `SMMU_ROOT_GPF_FAR` and `SMMU_ROOT_GPT_CFG_FAR` for Root-level software.
- The event recorded in the Event queue includes GPC fault information.

## SMMU Behavior After GPC Fault

When a GPC fault is active (§3.25.5):
- The SMMU may complete in-flight operations that were already past the GPC check point.
- New transactions are held or aborted depending on implementation.
- The system must quiesce before the GPT can be safely updated.

## GPT Caching

The SMMU may cache GPT lookups. GPT maintenance commands are required to invalidate this cache:
- `CMD_TLBI_*` with appropriate scope for GPC invalidation (Root-controlled).
- `SMMU_ROOT_TLBI` register write for direct Root-level TLB invalidation.

## DPT vs GPT

The Device Permission Table (DPT) ([[concepts/device-permission-table]]) is a separate RME DA feature that gates DMA access at a per-device PA-space granularity, distinct from GPT which operates on the physical memory side. In configurations using ATS with DPT (`STE.EATS == 0b11`), the DPT check precedes GPC.

## Model Implementation Notes

- A functional model must implement GPT lookups as part of the translation pipeline output stage. GPC is the final check before a transaction PA is forwarded into the system.
- GPT lookups may be cached; the model must implement invalidation via the relevant TLB commands.
- GPC faults always terminate the transaction (no stall model for GPC faults).
- The GPT structure and lookup algorithm are defined in §3.25 and Root register formats (§6.3.110–6.3.121).

## Related Concepts

- [[concepts/security-states]] — GPCs apply to Realm and all states where RME is implemented
- [[concepts/two-stage-translation]] — GPC is applied to the PA output of translation
- [[concepts/device-permission-table]] — DPT is a complementary per-device PA-space check
- [[concepts/pcie-ats-pri]] — Translated transactions from Realm streams subject to GPC

## Sources That Use This Concept

- [[sources/ihi0070g-b-smmuv3-architecture-spec]] — §3.25 Granule Protection Checks; §3.10.3 Support for Realm state; §2.6 SMMU for RME features; §6.3.110 SMMU_ROOT_IDR0
