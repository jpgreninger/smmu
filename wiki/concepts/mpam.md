---
title: "Memory System Resource Partitioning and Monitoring (MPAM)"
type: concept
tags: [smmu, mpam, partid, pmg, partitioning, monitoring, smmuv3.2, qos]
created: 2026-04-13
updated: 2026-04-13
sources: [../sources/ihi0070g-b-smmuv3-architecture-spec.md]
---

# Memory System Resource Partitioning and Monitoring (MPAM)

## Definition

MPAM (Chapter 17) is an optional SMMUv3.2+ feature (`SMMU_IDR3.MPAM == 1`) that adds two per-transaction identifiers — **Partition ID (PARTID)** and **Performance Monitoring Group (PMG)** — which allow the memory system to partition and monitor resources (caches, bandwidth, etc.) per-stream. The SMMU is responsible for assigning PARTID and PMG to all client and SMMU-originated transactions; pass-through or modification of device-provided PARTID/PMG values is not supported.

## Discovery

- `SMMU_IDR3.MPAM == 1` indicates MPAM support is present.
- `SMMU_(*_)MPAMIDR` registers report maximum PARTID and PMG values per Security state.
- MPAM is supported for a given Security state when `SMMU_IDR3.MPAM == 1` and either `SMMU_(*_)MPAMIDR.PARTID_MAX` or `SMMU_(*_)MPAMIDR.PMG_MAX` is non-zero.
- MPAM may not be supported in all Security states even when globally present.

## PARTID Spaces

PARTID and PMG values are **qualified by a PARTID space**:

| System | Identifier | Spaces |
|---|---|---|
| Without RME | `MPAM_NS` | 0 = Secure, 1 = Non-secure |
| With RME | `MPAM_SP` | 0b00 = Secure, 0b01 = Non-secure, 0b10 = Root, 0b11 = Realm |

Non-secure transactions always use Non-secure PARTID space. Secure transactions use Secure space by default; if `SMMU_S_MPAMIDR.HAS_MPAM_NS == 1`, Secure transactions may use Non-secure space per `MPAM_NS` bit in STE/CD/register fields. Realm transactions use Realm space by default; may use Non-secure space per `SMMU_R_GMPAM.MPAM_NS`.

## Assignment: Client Transactions (§17.2)

PARTID and PMG are determined in order:

1. **Global bypass** (`SMMU_(*_)CR0.SMMUEN == 0`): From `SMMU_(S_)GBPMPAM.GBP_{PMG,PARTID}`.
2. **STE bypass** (`STE.Config == 0b100`): `PARTID = STE.PARTID`, `PMG = STE.PMG`.
3. **Stage 2 only** (`STE.Config == 0b110`): `PARTID = STE.PARTID`, `PMG = STE.PMG`.
4. **Stage 1 only** (`STE.Config == 0b101`):
   - `STE.S1MPAM == 0`: from STE.
   - `STE.S1MPAM == 1`: `PARTID = CD.PARTID`, `PMG = CD.PMG`.
5. **Nested (stage 1 + stage 2)** (`STE.Config == 0b111`):
   - `STE.S1MPAM == 0`: from STE.
   - `STE.S1MPAM == 1`: `PARTID = VMS.PARTID_MAP[CD.PARTID]`, `PMG = CD.PMG`.

In nested configurations with `STE.S1MPAM == 1`, `CD.PARTID` is a **virtual PARTID** that is remapped to a physical PARTID via the `VMS.PARTID_MAP` table (see [virtual-machine-structure.md](virtual-machine-structure.md)). This enables hypervisor-controlled physical PARTID assignment while the guest controls virtual PARTIDs.

`STE.S1MPAM == 0` provides backward compatibility for software unaware of MPAM.

## Assignment: PCIe ATS Transactions (§17.3)

- **ATS Translation Request:** Same PARTID/PMG as an equivalent Untranslated transaction.
- **ATS Translated transaction:**
  - When `SMMU_(R_)CR0.ATSCHK == 0` (Non-secure only): From `SMMU_GBPMPAM.GBP_{PARTID,PMG}`.
  - When `ATSCHK == 1`: Determined by STE/CD as for ordinary transactions; PASID dependency on `SMMU_IDR3.PASIDTT`.

## Assignment: SMMU-Originated Transactions (§17.4)

| Access type | Source of PARTID/PMG |
|---|---|
| L1STD, STE, queues, MSIs, VMS | `SMMU_(*_)GMPAM.SO_{PARTID,PMG}` |
| L1CD, CD | `STE.{PARTID,PMG}` |
| Stage 2 translation table descriptors | Same as client transaction for that stream |
| Stage 1 translation table descriptors | Same as client transaction (STE/CD/VMS chain as appropriate) |

GPT accesses (GPC) inherit MPAM attributes from the access that triggered them.

## PARTID_MAP and VMS Interaction

In nested configurations with `STE.S1MPAM == 1`, the SMMU must resolve `CD.PARTID` through the `VMS.PARTID_MAP` structure before beginning the translation table walk (to enable PARTID-based TLB partitioning). This requires fetching the VMS before starting stage 1 table walks.

CD and VMS do not have their own `MPAM_NS` bits; they inherit PARTID space selection from the STE that led to them.

## Internal Resource Partitioning (§17.6)

SMMU-internal resources (TLBs, caches, queues) may be partitioned by the PARTID of client transactions. The control interface for this is IMPLEMENTATION DEFINED. Arm recommends using the MPAM Memory Partitioning and Monitoring Register (MMR) interface [MPAM spec]. Base addresses are IMPLEMENTATION DEFINED.

## PMCG MPAM Support (§17.5)

PMCGs that support MSIs may independently carry MPAM attributes for their MSI writes. Configured via `SMMU_PMCG_GMPAM`. Maximum supported PARTID/PMG values reported in `SMMU_PMCG_{S_}MPAMIDR`. The PARTID space for an MSI is determined by `SMMU_PMCG_SCR.{NSMSI, MSI_MPAM_NS}`.

## Tensions & Open Questions

- `SMMU_IDR3.PASIDTT` controls whether PASID is used for MPAM determination on ATS Translated transactions; behavior when `PASIDTT == 0` is IMPLEMENTATION DEFINED.
- MPAM for NoStreamID devices uses an IMPLEMENTATION DEFINED choice between device-provided values and zero.
- Not all Security states in an SMMU are required to support MPAM even when the feature is present.

## Related Concepts

- [virtual-machine-structure.md](virtual-machine-structure.md) — VMS.PARTID_MAP enables hypervisor-controlled virtual-to-physical PARTID translation
- [stream-table-entry.md](stream-table-entry.md) — STE.{PARTID, PMG, S1MPAM, MPAM_NS} fields
- [context-descriptor.md](context-descriptor.md) — CD.{PARTID, PMG} fields for stage 1 MPAM
- [security-states.md](security-states.md) — PARTID spaces are Security-state qualified; Realm and Root spaces added with RME
- [pcie-ats-pri.md](pcie-ats-pri.md) — ATS Translated transactions have special PARTID/PMG rules
- [performance-monitors.md](performance-monitors.md) — PMCGs can filter events by PARTID/PMG; independently support MPAM for MSIs

## Sources That Use This Concept

- [../sources/ihi0070g-b-smmuv3-architecture-spec.md](../sources/ihi0070g-b-smmuv3-architecture-spec.md) — Chapter 17 Memory System Resource Partitioning and Monitoring; §17.1–17.7
