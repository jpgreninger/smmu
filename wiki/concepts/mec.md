---
title: "Memory Encryption Contexts (MEC)"
type: concept
tags: [smmu, mec, encryption, realm, rme-da, mecid, feat_mec, smmuv3.4]
created: 2026-04-13
updated: 2026-04-13
sources: [../sources/ihi0070g-b-smmuv3-architecture-spec.md]
---

# Memory Encryption Contexts (MEC)

## Definition

Chapter 18 of the SMMUv3 specification introduces **Memory Encryption Contexts (MEC)** for SMMUs with **RME DA** (`SMMU_R_IDR3.MEC == 1`). MEC is the SMMU counterpart to the PE-level `FEAT_MEC` extension and provides finer-grained memory encryption context assignment within the Realm physical address space. This feature is **only applicable** to SMMUs with the Realm programming interface (RME DA).

## Purpose

`FEAT_MEC` on PEs provides per-Realm memory encryption context granularity. MEC in the SMMU ensures that SMMU- and client-originated accesses to Realm memory carry the correct **Memory Encryption Context ID (MECID)**, enabling the memory system to enforce encryption context boundaries.

## MECID Assignment

| Access type | MECID |
|---|---|
| Secure PA space | Default MECID = 0 |
| Non-secure PA space | Default MECID = 0 |
| Root PA space | Default MECID = 0 |
| Realm PA space (SMMU or client) | Determined by `SMMU_R_GMECID` and `STE.MECID` |
| NoStreamID device → Realm PA space | IMPLEMENTATION DEFINED mechanism provided by the device |

For Realm-space accesses, the MECID choice depends on the type of access as described in the `SMMU_R_GMECID` and `STE.MECID` register/field descriptions.

## AMEC Bit and Restrictions

MEC introduces an `AMEC` (Alternative MECID) bit in translation table descriptors:
- **Stage 2 Page and Block descriptors** for the Realm EL1&0 translation regime: bit[63].
- **Stage 1 Page and Block descriptors** for the Realm EL2 and EL2&0 translation regimes: bit[63].

**Current restriction:** This revision of the SMMU architecture does **not** support Alternative MECID values. If `SMMU_R_IDR3.MEC == 1` and a Realm translation requires use of a descriptor with `AMEC == 1`, it is treated as `F_TRANSLATION` at the stage of translation where `AMEC` was set.

If the `NS` field of a descriptor is 1, the `AMEC` field is RES0 and treated as 0; it does not trigger `F_TRANSLATION` in this case.

If `SMMU_R_IDR3.MEC == 0`, the `AMEC` field is RES0 and does not trigger `F_TRANSLATION`.

## SMMU Without Realm Programming Interface

If an SMMU **without** the Realm programming interface is integrated in a system that supports MEC, all client- and SMMU-originated accesses for that SMMU are treated as having the default MECID of zero.

## Discovery

| Register/Field | Meaning |
|---|---|
| `SMMU_R_IDR3.MEC` | MEC feature implemented in this SMMU |
| `SMMU_R_MECIDR` | Supported MECID width |
| `SMMU_R_GMECID` | Global MECID configuration for SMMU-originated Realm accesses |
| `STE.MECID` | Per-stream MECID for client Realm accesses |

## Related Concepts

- [security-states.md](security-states.md) — MEC applies only to the Realm security state (RME DA); Non-secure/Secure/Root use MECID=0
- [granule-protection-check.md](granule-protection-check.md) — GPC/GPT govern PA-space ownership; MEC adds encryption context within Realm PA space
- [stream-table-entry.md](stream-table-entry.md) — `STE.MECID` field provides per-stream MECID for Realm accesses
- [fault-models.md](fault-models.md) — `F_TRANSLATION` is raised on `AMEC == 1` descriptor encounter when MEC does not support Alternative MECID
- [device-permission-table.md](device-permission-table.md) — DPT gates RME DA device access; MEC adds encryption context layered on top

## Sources That Use This Concept

- [../sources/ihi0070g-b-smmuv3-architecture-spec.md](../sources/ihi0070g-b-smmuv3-architecture-spec.md) — Chapter 18 Support for Memory Encryption Contexts; `SMMU_R_IDR3.MEC`; `SMMU_R_MECIDR`; `SMMU_R_GMECID`; `STE.MECID` field descriptions
