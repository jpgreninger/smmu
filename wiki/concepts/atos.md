---
title: "Address Translation Operations (ATOS)"
type: concept
tags: [smmu, atos, vatos, address-translation, debug, software-lookup]
created: 2026-04-13
updated: 2026-04-13
sources: [../sources/ihi0070g-b-smmuv3-architecture-spec.md]
---

# Address Translation Operations (ATOS)

## Definition

Address Translation Operations (ATOS) is an optional software-accessible facility (§3.9 / Chapter 9) that lets privileged software perform an explicit address lookup through the SMMU — determining what output address and fault status a transaction would receive given a specified input address, stream, substream, and access properties. Presence is indicated by `SMMU_IDR0.ATOS`.

## Purpose

ATOS allows software to:
- Debug and validate SMMU translation configuration without issuing actual device transactions.
- Verify what output address a device transaction would receive.
- Determine whether a translation fault would occur and why.

ATOS requests are **not** affected by fault configuration bits in the STE or CD. They do **not** record fault events and do **not** stall.

## Scope of Lookup

An ATOS lookup can be:
- **Full end-to-end:** applies all configured stages for the stream.
- **Partial:** only a subset of the configured stages (e.g., stage 1 only).

The result is either:
- The **output address** (physical address after all configured stages).
- A **fault status code** if translation cannot complete.

ATOS respects the translation configuration of the specified stream but bypasses all fault-recording and stall machinery.

## Register Interface

| Register group | Coverage |
|---|---|
| `SMMU_GATOS_SID`, `SMMU_GATOS_ADDR`, `SMMU_GATOS_PAR` | Non-secure ATOS |
| `SMMU_S_GATOS_SID`, `SMMU_S_GATOS_ADDR`, `SMMU_S_GATOS_PAR` | Secure ATOS (when `SMMU_S_IDR1.SECURE_IMPL == 1`) |
| `SMMU_VATOS_SID`, `SMMU_VATOS_ADDR`, `SMMU_VATOS_PAR` | VATOS page (when `SMMU_IDR0.VATOS == 1`) |
| `SMMU_S_VATOS_SID`, `SMMU_S_VATOS_ADDR`, `SMMU_S_VATOS_PAR` | Secure VATOS (when both VATOS and Secure stage 2 supported) |

Each register group can perform one lookup at a time. All implemented groups may perform independent lookups simultaneously.

## VATOS (Virtual ATOS)

An optional extension reported by `SMMU_IDR0.VATOS`. The VATOS page can be mapped to a chosen software entity (e.g., a hypervisor or OS driver) to perform limited **stage 1-only** ATOS lookups directly:

- Supports stage 1-only lookups for stage 1+2 nested configurations and for stage 1-only configurations.
- VATOS is restricted to the **NS-EL1 StreamWorld**; it cannot perform lookups for other StreamWorlds.
- Arm expects VATOS to be used with stage 1 + stage 2 nested configurations.
- The VATOS register group resides in a distinct VATOS page (see Chapter 6 register map).
- When both VATOS and Secure stage 2 are supported, a fourth register group `SMMU_S_VATOS_*` is present in a distinct S_VATOS page.

## ATOS and Attribute Transformation

- The `PRIV`/`INST` attributes of an ATOS lookup are taken from the `ATOS_ADDR` register fields (`PnU` and `InD`), not from STE.PRIVCFG/INSTCFG.
- STE attribute override fields (`NSCFG`, `MTCFG`, `SHCFG`, `ALLOCCFG`) are **ignored** for ATOS (see §13.1 Table 13.5).
- `STE.S2FWB` does affect the attribute returned by ATOS (§13.1.6).
- Output attributes are normalized for consistency per §13.1.7.

## ATS and ATOS

There are no ATOS-specific changes from the introduction of F_PERMISSION fields `DirtyBit` and `AssuredOnly` in SMMUv3.4. An ATOS operation that fails with a Permission fault reports this using existing `ATOS_PAR` encodings regardless of which sub-cause the fault would have had.

## Model Implementation Notes

- A functional model must implement independent ATOS lookup paths that bypass fault-recording and stall state machines.
- Each GATOS/VATOS register group is independent; concurrent lookups on different groups are permitted.
- ATOS lookups should route through the same translation logic as ordinary transactions, but skip the fault model dispatch.

## Related Concepts

- [two-stage-translation.md](two-stage-translation.md) — ATOS performs end-to-end or partial stage lookups
- [stream-table-entry.md](stream-table-entry.md) — STE locates the stream configuration for an ATOS lookup
- [context-descriptor.md](context-descriptor.md) — CD provides stage 1 configuration during ATOS
- [fault-models.md](fault-models.md) — ATOS does not trigger fault events or stall; reports status via PAR
- [security-states.md](security-states.md) — Separate GATOS / S_GATOS register groups per security state
- [smmu-initialization.md](smmu-initialization.md) — ATOS availability gated on `SMMU_IDR0.ATOS`

## Sources That Use This Concept

- [../sources/ihi0070g-b-smmuv3-architecture-spec.md](../sources/ihi0070g-b-smmuv3-architecture-spec.md) — Chapter 9 Address Translation Operations; §3.9 ATOS facility; `SMMU_IDR0.ATOS` and `SMMU_IDR0.VATOS` field descriptions; §13.1 attribute override tables
