---
title: "Attribute Transformation"
type: concept
tags: [smmu, attributes, memory-type, shareability, cacheability, ns, instcfg, privcfg, mtcfg, shcfg]
created: 2026-04-13
updated: 2026-04-13
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Attribute Transformation

## Definition

Chapter 13 of the SMMUv3 specification defines how memory access attributes are determined for transactions passing through the SMMU. The SMMU supports the same attribute set as Armv8-A PEs. For each transaction, the final output attributes are determined by combining the incoming attributes, SMMU global override registers, STE override fields, and translation table descriptor attributes.

## Attribute Set

The SMMU carries the following per-transaction attributes (mirroring Armv8-A):

| Attribute | Meaning |
|---|---|
| MT (Memory Type) | Normal-WB, Normal-WT, Normal-NC, Device-GRE, Device-nGRE, Device-nGnRE, Device-nGnRnE |
| SH (Shareability) | NSH (Non-Shareable), ISH (Inner Shareable), OSH (Outer Shareable) |
| RA/WA/TR | Read-Allocate, Write-Allocate, Transient hints (for Normal-WB/WT only) |
| INST | Instruction / Data |
| PRIV | Privileged / Unprivileged |
| NS | Non-secure / Secure output physical address space |

R/W, INST, and PRIV are used only for **permission checking** against translation table descriptors. MT, SH, RA/WA, and TR are used by the memory system for caching behavior.

## Four Transaction Paths

| Path | Condition | Attribute source |
|---|---|---|
| Global bypass | `SMMU_(S_)CR0.SMMUEN == 0` | `SMMU_(S_)GBPA` override fields; incoming if "Use incoming" |
| STE bypass | `STE.Config == 0b100` or stage skipped | STE override fields apply (MTCFG, SHCFG, etc.) |
| Normal translation | STE configures one or more stages | Input → STE overrides → translation table descriptor attrs → combine_attrs() |
| PCIe ATS Translated | Device caches pre-translation, presents translated address | NS from STE.NSCFG; MT/SH/ALLOC: ignored (see §13.1 Table 13.5) |

## Default Input Attributes (§13.1.3)

When the upstream interconnect does not supply an attribute, the SMMU constructs a default:

| Attribute | Default |
|---|---|
| MT | Normal iWB-oWB |
| SH | NSH |
| RA/WA | Allocate |
| TR | Non-transient |
| INST | Data |
| PRIV | Unprivileged |
| NS | Non-secure |

## STE Override Fields

STE fields that override input attributes:

| Field | Overrides | Notes |
|---|---|---|
| `STE.INSTCFG` | INST attribute for reads | Ignored for writes (always Data); ATOS: from `ATOS_ADDR.InD` |
| `STE.PRIVCFG` | PRIV attribute | ATOS: from `ATOS_ADDR.PnU` |
| `STE.NSCFG` | NS (output PA space) | Applies to all untranslated txns and ATS TRs |
| `STE.MTCFG`/`MemAttr` | Memory type | For PCIe: IMPLEMENTATION DEFINED |
| `STE.SHCFG` | Shareability | For PCIe: IMPLEMENTATION DEFINED; ignored for ATS Translated |
| `STE.ALLOCCFG` | RA/WA/TR hints | For PCIe: IMPLEMENTATION DEFINED |

Whether override fields take effect is also gated by `SMMU_IDR1.ATTR_TYPES_OVR` and `SMMU_IDR1.ATTR_PERMS_OVR`.

For SMMUv3.4 and later, INST and PRIV are always output as **Data, Privileged** regardless of INSTCFG/PRIVCFG (§13.1.2).

## Combine Operation (§13.1.5)

When two stages of translation are both enabled, stage 2 combines its attributes with stage 1's output. The rule takes the **stronger** of each attribute pair:

| Dimension | Weakest → Strongest |
|---|---|
| Memory type | Normal-WB → Normal-WT → Device-GRE → Device-nGRE → Device-nGnRE → Device-nGnRnE |
| Shareability | NSH → ISH → OSH |
| Allocate hints | Allocate → No-allocate |
| Transient | Non-transient → Transient |

Only MT, SH, RA/WA, TR are subject to combine; permission attributes (R/W, INST, PRIV, NS) are checked per-stage and not combined.

## Stage 2 FWB Override (§13.1.6)

If `SMMU_IDR3.FWB == 1` and `STE.S2FWB == 1`, stage 2 can force any Normal memory type to Normal-WB shareable, overriding stage 1 attribute output (Armv8.4 `FEAT_FWB`). This also affects ATOS output attributes.

## Consistency Enforcement (§13.1.7)

The SMMU ensures no inconsistent attribute combinations reach the memory system:
- Device and Normal-NC memory types are always output as Outer Shareable.
- Non-cacheable memory has no RA/WA/TR attributes regardless of configuration.
- A cacheable type with RA==0 and WA==0 implies TR is non-transient.

## SMMU-Originated Accesses

All SMMU-originated transactions (structure fetches, translation table walks, queue accesses, MSI writes) are output as **Data, Privileged** with attributes configured by SMMU registers (e.g., `SMMU_(*_)GMPAM` for MPAM, register/structure fields for memory type).

## Secure State Impact (§13.1.2)

- When `SMMU_S_IDR1.SECURE_IMPL == 1`, the output NS attribute is meaningful and distinguishes Secure vs Non-secure PA space.
- `SMMU_S_CR0.SIF` (Secure Instruction Fetch) is checked against the NS attribute at stage 1 for Secure streams.

## Related Concepts

- [[concepts/two-stage-translation]] — Two-stage combine operation; stage 2 can override stage 1 attributes via FWB
- [[concepts/stream-table-entry]] — STE contains MTCFG, SHCFG, ALLOCCFG, INSTCFG, PRIVCFG, NSCFG override fields
- [[concepts/context-descriptor]] — CD contains MAIR/AMAIR for interpreting translation table descriptor attribute indices
- [[concepts/security-states]] — NS output attribute determines Secure vs Non-secure PA space; SIF check
- [[concepts/atos]] — ATOS ignores most STE attribute overrides; uses ATOS_ADDR fields for INST/PRIV
- [[concepts/pcie-ats-pri]] — ATS Translated transactions: NS from NSCFG; MT/SH/ALLOC ignored

## Sources That Use This Concept

- [[sources/ihi0070g-b-smmuv3-architecture-spec]] — Chapter 13 Attribute Transformation; §13.1–13.7; Tables 13.4, 13.5; §13.1.5 combine examples; §13.1.6 FWB
