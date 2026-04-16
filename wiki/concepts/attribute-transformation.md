---
title: "Attribute Transformation"
type: concept
tags: [smmu, attributes, memory-type, shareability, cacheability, ns, instcfg, privcfg, mtcfg, shcfg]
created: 2026-04-13
updated: 2026-04-15
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

## Summary of Output Attribute Determination (§13.5)

The following table shows how each output attribute is determined across all translation/bypass configurations (Table 13.3 in spec):

| Attribute | Stage 1-only | Stage 1 + Stage 2 | Stage 2-only | Bypass (`STE.Config == 0b100`, `SMMUEN == 1`) | Global bypass (`SMMUEN == 0`) |
|---|---|---|---|---|---|
| **INST** | Data, if required by the memory system | ← | ← | ← | ← |
| **PRIV** | Privileged, if required by the memory system | ← | ← | ← | ← |
| **PA space (Secure stream)** | Effective `S1_TTD.NS`, `NSTable`, and `CD.NSCFGx` | Effective `S1_TTD.NS`, `NSTable`, `CD.NSCFGx` and `STE.{S2NSA, S2NSW, S2SA, S2SW}` | `STE.NSCFG` and `STE.{S2NSA, S2NSW, S2SA, S2SW}` | `STE.NSCFG` | `SMMU_S_GBPA.NSCFG` |
| **PA space (Realm stream)** | EL1: Fixed, Realm PA space. EL2/EL2-E2H: `S1_TTD.NS` | `S2_TTD.NS` | `S2_TTD.NS` | `STE.NSCFG` | Transaction aborted |
| **PA space (Non-secure stream)** | Fixed, Non-secure PA space | ← | ← | ← | ← |
| **MT** | `CD.MAIR[S1_TTD.AttrIndx]` | `Combine(CD.MAIR[S1_TTD.AttrIndx], S2_TTD.MemAttr, STE.S2FWB)` | `Combine(STE.{MemAttr, MTCFG}, S2_TTD.MemAttr, STE.S2FWB)` | `STE.{MemAttr, MTCFG}` | `SMMU_(S_)GBPA.{MemAttr, MTCFG}` |
| **RA, WA, TR** | `CD.MAIR[S1_TTD.AttrIndx]`, unless `STE.{MemAttr, MTCFG}` provides any-Device or NC input to Stage 1, in which case: `Combine(STE.ALLOCCFG, CD.MAIR[...])` | Same as Stage 1-only column | `Combine(STE.ALLOCCFG, S2_TTD.MemAttr)` | `STE.ALLOCCFG` | `SMMU_(S_)GBPA.ALLOCCFG` |
| **SH** | `S1_TTD.SH` | `Combine(S1_TTD.SH, S2_TTD.SH)` | `Combine(STE.SHCFG, S2_TTD.SH)` | `STE.SHCFG` | `SMMU_(S_)GBPA.SHCFG` |

**Notes:**
- Permission checking of `InD` and `PnU` against translation descriptor fields is not shown.
- References to `INSTCFG`, `PRIVCFG`, `NSCFG`, `MTCFG/MemAttr`, and `ALLOCCFG` refer to the effective output of these override fields (gated by `SMMU_IDR1.ATTR_PERMS_OVR` and `SMMU_IDR1.ATTR_TYPES_OVR`). The table does not show interdependencies (e.g., allocation hints are irrelevant if the MemType is any-Device).
- When Stage 2 is enabled and `STE.S2FWB == 1`, the `Combine()` operation for memory type is instead an **override** for some values of the stage 2 MemAttr field.

## PCIe and ATS Attribute Handling (§13.6)

### PCIe Memory Type Attributes (§13.6.1)

PCIe does not encode memory type attributes; each transaction takes a system-defined type when it progresses into the system. SBSA requires the base type to be **Normal-iWB-oWB cacheable shareable** (IO-coherent). When an SMMU is present with translation enabled, software configuration (STE overrides, translation table descriptors) determines the output attribute.

**No_snoop (§13.6.1.1):** In an Arm system, PCIe No_snoop corresponds to `Normal-iNC-oNC-OSH`. When No_snoop support is implemented, a No_snoop flag on a transaction downgrades any final Normal cacheable output attribute to `Normal-iNC-oNC-OSH` downstream of the SMMU. No_snoop does not affect any-Device attributes and applies after `STE.S2FWB`.

### ATS Attribute Overview (§13.6.2)

PCIe ATS Translation Completions do not carry an explicit memory type field. Whether an SMMU assigns attributes to ATS Translated transactions consistent with the Untranslated path is **IMPLEMENTATION DEFINED**. Two behaviors exist:

- **With ATS attribute support:** The SMMU encodes attributes (e.g., in upper physical-address bits of the ATS completion, termed "attribute stashing" in pre-SMMUv3.4). From SMMUv3.4, attribute stashing is **forbidden**; attribute assignment mechanism is IMPLEMENTATION DEFINED.
- **Without ATS attribute support:** ATS Translated transactions carry the fixed upstream PCIe attribute (cacheable shareable); the SMMU output may not match the Untranslated path for the same address.

The N (No_snoop) field in ATS Translation Completions is always 0; the SMMU does not provide per-page No_snoop control.

### Split-Stage ATS (STE.EATS == 0b10) (§13.6.3)

When Split-stage ATS is enabled (`STE.EATS == 0b10`, `SMMU_CR0.ATSCHK == 1`, `STE.Config == 0b111`):

- An ATS Translation Request response carries the **IPA** (stage 1 output), not the final PA.
- Subsequent ATS Translated transactions present the IPA to the SMMU, which performs **stage 2 translation** on them.
- Permissions in the ATS completion reflect combined stage 1 + stage 2 permissions.
- HTTU may be performed at the time of the Translation Request (stage 2) or deferred to the Translated transaction's stage 2 lookup.
- Attribute: if ATS attributes are supported, the response carries an attribute that—after stage 2 combination—yields the correct final output attribute.

Compared to regular `EATS == 0b01` ATS:
- `EATS == 0b01`: Translated transaction = PA, bypasses all translation.
- `EATS == 0b10`: Translated transaction = IPA, undergoes stage 2 in the SMMU.
- `EATS == 0b10` requires `SMMU_CR0.ATSCHK == 1`.

### Full ATS Skipping Stage 1 (§13.6.4)

When `STE.Config == 0b1x1`, `STE.EATS == 0b01`, `STE.S1CDMax != 0`, and `STE.S1DSS == 0b01`, non-PASID ATS Translation Requests skip stage 1:

- If stage 2 is enabled: translated stage 2-only, completion returns a PA.
- If only stage 1 is enabled: identity-mapped completion (`U==0, R==1, W==1`; address passes through 1:1). Output attribute is the fixed upstream input with STE overrides applied.

### Split-Stage ATS Skipping Stage 1 (§13.6.5)

When `STE.Config == 0b111`, `STE.EATS == 0b10`, `STE.S1CDMax > 0`, `STE.S1DSS == 0b01`: non-PASID ATS Translation Requests skip stage 1 but return an identity-mapped IPA with R/W from stage 2. Subsequent Translated transactions are translated stage 2-only.

## PCIe Permission Attribute Interpretation (§13.7)

PCIe expresses only R/W permission on base transactions. The PASID TLP prefix adds Execute (Exe) and Privileged (Priv) attributes for ATS.

**Normal (non-ATS) transactions:**
- Without PASID: treated as Data, Unprivileged.
- With PASID: INST = Execute_Requested, PRIV = Privileged_Mode_Requested.

**ATS Translation Request permission fields:**
- **Read** is implied in all Translation Requests.
- **NW (No-Write):** `NW == 0` requests write permission; `NW == 1` signals read-only intent. Write permission is granted if the page is writable-dirty, or writable-clean when HTTU is enabled (HTTU marks page dirty when `NW == 0`).
- **Exe:** Requests execute permission; only granted if the page has execute permission and Exe was requested.
- **Priv:** Marks privileged request. Permissions are computed for the effective privilege level but the Priv bit in the response echoes the request value.

| Example request | Page permissions | Response |
|---|---|---|
| NW=1, Exe=0, Priv=0 | User-RO, Priv-RW, XN=0 | R=1, W=0, Exe=0, Priv=0 |
| NW=0, Exe=0, Priv=0 | User-RW, Priv-RW, XN=0 | R=1, W=1, Exe=0, Priv=0 |
| NW=0, Exe=0, Priv=1 | User-RO, Priv-RW, XN=0 | R=1, W=1, Exe=0, Priv=1 |
| NW=0, Exe=1, Priv=0 | User-RW, Priv-RW, XN=0 | R=1, W=1, Exe=1, Priv=0 |
| NW=0, Exe=1, Priv=0 | User-X (XO), Priv-RW | R=0, W=0, Exe=0, Priv=0 (XO not ATS-compatible) |
| Any | Translation fault | R=0, W=0, Exe=0, Success (not UR/CA) |

**Execute-only (XO) pages** are not ATS-compatible unless `STE.INSTCFG == Instruction`; an ATS TR to an XO page returns no access.

### §13.7.1 INSTCFG/PRIVCFG Interaction with ATS

When `SMMU_IDR1.ATTR_PERMS_OVR == 1`, `STE.INSTCFG` and `STE.PRIVCFG` affect ATS Translation Request permission computation:

- `PRIVCFG == Privileged/Unprivileged`: overrides the effective privilege used to compute R/W/X permissions; the Priv field in the response still echoes the TR's Priv.
- `INSTCFG == Instruction`: uses the page's X permission to determine R (so XO pages grant R+Exe).
- `INSTCFG == Data`: uses R permission for both R and Exe grants.

Changes to `STE.{INSTCFG,PRIVCFG}` must be accompanied by ATC invalidation.

## Related Concepts

- [two-stage-translation.md](two-stage-translation.md) — Two-stage combine operation; stage 2 can override stage 1 attributes via FWB
- [stream-table-entry.md](stream-table-entry.md) — STE contains MTCFG, SHCFG, ALLOCCFG, INSTCFG, PRIVCFG, NSCFG override fields
- [context-descriptor.md](context-descriptor.md) — CD contains MAIR/AMAIR for interpreting translation table descriptor attribute indices
- [security-states.md](security-states.md) — NS output attribute determines Secure vs Non-secure PA space; SIF check
- [atos.md](atos.md) — ATOS ignores most STE attribute overrides; uses ATOS_ADDR fields for INST/PRIV
- [pcie-ats-pri.md](pcie-ats-pri.md) — ATS Translated transactions: NS from NSCFG; T/TE/XT bit handling; split-stage ATS
- [memory-tagging-extension.md](memory-tagging-extension.md) — FEAT_MTE_PERM reinterprets stage 2 MemAttr field when S2FWB==0/1; interacts with the attribute combine path (§3.23.1)
- [../synthesis/smmu-system-implementation.md](../synthesis/smmu-system-implementation.md) — §16.7.5 AMBA↔Armv8 attribute conversion tables

## Sources That Use This Concept

- [../sources/ihi0070g-b-smmuv3-architecture-spec.md](../sources/ihi0070g-b-smmuv3-architecture-spec.md) — Chapter 13 Attribute Transformation; §13.1–13.7; §13.6.1–13.6.5 PCIe/ATS attribute handling; §13.7–13.7.1 permission interpretation and pseudocode
