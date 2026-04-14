---
title: "SMMU Version Feature Map"
type: synthesis
tags: [smmu, version, features, smmuv3, compatibility, model]
created: 2026-04-07
updated: 2026-04-07
sources: [../sources/ihi0070g-b-smmuv3-architecture-spec.md](sources/ihi0070g-b-smmuv3-architecture-spec.md)
---

# SMMU Version Feature Map

Reference table for which features are available at each SMMUv3.x version. Essential for writing version-correct functional and performance models that gate behavior behind the correct `SMMU_AIDR[7:0]` version check.

## SMMU_AIDR Version Encoding

| `SMMU_AIDR[7:0]` | SMMUv3 Version |
|-----------------|----------------|
| 0x00 | SMMUv3.0 |
| 0x01 | SMMUv3.1 |
| 0x02 | SMMUv3.2 |
| 0x03 | SMMUv3.3 |
| 0x04 | SMMUv3.4 |

An SMMUv3.x implementation may include any arbitrary subset of SMMUv3.(x+1) features. It may not include any SMMUv3.(x+2) or later features.

## SMMUv3.0 (Base)

Core features — all mandatory unless listed as optional:

| Feature | Name | Discovery | Optional? |
|---------|------|-----------|-----------|
| Memory-based configuration (Stream table, CD) | — | — | No |
| Stage 1 translation | SMMUv3.0-S1P | `SMMU_IDR0.S1P` | Yes |
| Stage 2 translation | SMMUv3.0-S2P | `SMMU_IDR0.S2P` | Yes |
| Up to 16-bit ASID | SMMUv3.0-ASID16 | `SMMU_IDR0.ASID16` | Yes |
| Up to 16-bit VMID | SMMUv3.0-VMID16 | `SMMU_IDR0.VMID16` | Yes |
| 49-bit VA (2×48) | — | `SMMU_IDR5.VAX == 0b00` | No (base) |
| 4 KB translation granule | SMMUv3.0-GRAN4K | `SMMU_IDR5.GRAN4K` | Yes |
| 16 KB granule | SMMUv3.0-GRAN16K | `SMMU_IDR5.GRAN16K` | Yes |
| 64 KB granule | SMMUv3.0-GRAN64K | `SMMU_IDR5.GRAN64K` | Yes |
| VMSAv8-32 LPAE tables | SMMUv3.0-TTFAA32 | — | Yes |
| VMSAv8-64 tables | SMMUv3.0-TTFAA64 | — | Yes |
| Secure stream support | SMMUv3.0-SECURE_IMPL | `SMMU_S_IDR1.SECURE_IMPL` | Yes |
| Broadcast TLB maintenance | SMMUv3.0-BTM | `SMMU_IDR0.BTM` | Yes |
| HTTU (Access flag only) | SMMUv3.0-HTTUA | `SMMU_IDR0.HTTU == 0b01` | Yes |
| HTTU (Access flag + Dirty) | SMMUv3.0-HTTUD | `SMMU_IDR0.HTTU == 0b10` | Yes |
| PCIe ATS | SMMUv3.0-ATS | `SMMU_IDR0.ATS` | Yes |
| PCIe PRI | SMMUv3.0-PRI | `SMMU_IDR0.PRI` | Yes |
| Hypervisor stage 1 contexts | SMMUv3.0-Hyp | `SMMU_IDR0.HYP` | Yes |
| ATOS (Address Translation Op) | SMMUv3.0-ATOS | `SMMU_IDR0.ATOS` | Yes |
| VATOS (VM-accessible ATOS) | SMMUv3.0-VATOS | `SMMU_IDR0.VATOS` | Yes |
| HAD (Hierarchical Attribute Disable) | SMMUv3.0-HAD | `SMMU_IDR3.HAD` | Yes |
| Two-level Stream table | — | `SMMU_IDR0.ST_LEVEL` | Required if >64 StreamIDs |
| PMCG (Performance Monitor) | — | — | Optional subsystem |

## SMMUv3.1 (Armv8.2-A alignment)

All SMMUv3.0 features plus:

| Feature | Name | Discovery | Notes |
|---------|------|-----------|-------|
| 52-bit VA/IPA/PA (LPA) | SMMUv3.1-LPA | `SMMU_IDR5.OAS` | Optional; fields extended |
| 53-bit VA (LVA/VAX) | SMMUv3.1-VAX | `SMMU_IDR5.VAX == 0b01` | Optional |
| Stage 2 Unprivileged Execute-never (XNX) | SMMUv3.1-XNX | `SMMU_IDR3.XNX` | Optional; **mandatory from SMMUv3.1** if S2P |
| Page-Based Hardware Attributes (PBHA) | SMMUv3.1-TTPBHA | `SMMU_IDR3.PBHA` | Optional |
| Cache stash / destructive read support | — | — | Optional |
| PMCG error status | — | — | — |

Mandatory changes in SMMUv3.1:
- `SMMU_IDR3.XNX == 1` and `SMMU_IDR3.HAD == 1` are mandatory.

## SMMUv3.2 (Armv8.4-A alignment)

All SMMUv3.1 features plus:

| Feature | Name | Discovery | Notes |
|---------|------|-----------|-------|
| MPAM (Memory Partitioning) | SMMUv3.2-MPAM | `SMMU_IDR3.MPAM` | Optional |
| Secure EL2 and Secure stage 2 | — | `SMMU_S_IDR1.SEL2` | Optional; requires both S1P and S2P |
| Stage 2 memory type/cacheability control | — | — | — |
| Small translation table support | SMMUv3.2-TTSMALL | `SMMU_IDR3.TTSmall` | Optional |
| Range-based TLB invalidation + Level Hint | SMMUv3.2-BBML1/BBML2 | `SMMU_IDR3.BBML` | Optional |
| Break-before-make free update | SMMUv3.2-BBML1/BBML2 | `SMMU_IDR3.BBML` | Optional |
| Virtual Machine Structure (VMS) | — | `SMMU_IDR0.VMS` | Optional |
| VMID Wildcards | — | `SMMU_IDR0.VMID_WILDCARD` | Optional |

## SMMUv3.3 (SMMU for RME)

All SMMUv3.2 features plus:

| Feature | Name | Discovery | Notes |
|---------|------|-----------|-------|
| Realm security state | — | `SMMU_ROOT_IDR0.REALM_IMPL` | Optional; entire RME extension |
| Granule Protection Checks (GPC/GPT) | — | `SMMU_IDR0.RME_IMPL` | Required with Realm |
| Root control register page | — | `SMMU_ROOT_IDR0.ROOT_IMPL` | Required with Realm |
| Realm Command/Event/PRI queues | — | — | Required with Realm |
| GPT maintenance (TLBI) | — | — | Root registers |

Required features for `SMMU_ROOT_IDR0.REALM_IMPL == 1` (§2.7): HYP, S1P, S2P, TTF=0b10, COHACC, RME_IMPL, BBML=0b10, ROOT_IMPL.

## SMMUv3.4 (Armv8.7/8.9-A alignment)

All SMMUv3.3 features plus:

| Feature | Name | Discovery | Optional? |
|---------|------|-----------|-----------|
| LPA2 (52-bit VA/PA with 4K/16K granule) | SMMUv3.4-LPA2 | `SMMU_IDR5.DS` | Yes |
| Enhanced PAN (PAN3) | SMMUv3.4-PAN3 | `SMMU_IDR3.EPAN` | Yes |
| Translation Hardening (THE/AssuredOnly) | SMMUv3.4-THE | `SMMU_IDR3.THE` | Yes |
| Stage 1 Permission Indirections (S1PIE) | SMMUv3.4-S1PIE | `SMMU_IDR3.S1PI` | Yes |
| Stage 2 Permission Indirections (S2PIE) | SMMUv3.4-S2PIE | `SMMU_IDR3.S2PI` | Yes |
| Stage 2 Permission Overlays (S2POE) | SMMUv3.4-S2POE | `SMMU_IDR3.S2PO` | Yes |
| 128-bit descriptors (D128) | SMMUv3.4-D128 | `SMMU_IDR5.D128` | Yes |
| Attribute Index Enhancement (AIE) | SMMUv3.4-AIE | `SMMU_IDR3.AIE` | Yes |
| Table descriptor Access flag (HAFT) | SMMUv3.4-HAFT | `SMMU_IDR0.HTTU` (ext.) | Yes |
| MTE MemAttr NoTagAccess (stage 2) | SMMUv3.4-MTE_PERM | `SMMU_IDR3.MTEPERM` | **Mandatory** |
| PASID TLP prefix on ATS Translated txns | SMMUv3.4-PASIDTT | `SMMU_IDR3.PASIDTT` | Yes |
| XS (Extra Shareability TLBInXS) | — | `SMMU_IDR3.XS` | Yes |

SMMUv3.4 also includes deprecations:
- Stashing translation info in ATS address fields (deprecated).
- InD and PnU as output attributes (deprecated).
- `SMMU_PMCG_PMAUTHSTATUS` register (deprecated).

## SMMU for RME DA (G.b additions)

Additional required and optional features for RME Delegated Assignment:

| Feature | Discovery | Notes |
|---------|-----------|-------|
| DPT (Device Permission Table) | `SMMU_R_IDR3.DPT` | Optional, strongly recommended |
| MECID (Memory Encryption Context ID) | `SMMU_R_MECIDR` | Optional |
| XT bit support (TDISP XT Extensions) | `SMMU_R_IDR3.XT` | Optional |
| TE bit in ATS Translation Completions | — | When XT supported |
| NS1ATS (NS Split-stage ATS) | `SMMU_IDR0.NS1ATS` | Required if ATS and no DPT |

## Model Versioning Strategy

A model should:
1. Accept a target version as a configuration parameter (`SMMU_AIDR` value).
2. Gate each feature behind its discovery bit check.
3. For each CONSTRAINED UNPREDICTABLE or version-differing behavior, document the chosen behavior and the version range it applies to.
4. For SMMUv3.0-specific behaviors (e.g., address truncation vs fault on out-of-range S1ContextPtr), implement the SMMUv3.1+ behavior by default and add SMMUv3.0 variants as configuration options.

## Related Pages

- [../sources/ihi0070g-b-smmuv3-architecture-spec.md](sources/ihi0070g-b-smmuv3-architecture-spec.md) — §2.2–2.8 feature tables
- [../concepts/security-states.md](concepts/security-states.md) — security state features by version
- [../concepts/permission-indirections.md](concepts/permission-indirections.md) — SMMUv3.4 features
- [../concepts/translation-hardening.md](concepts/translation-hardening.md) — THE/AssuredOnly (SMMUv3.4)
- [../concepts/httu.md](concepts/httu.md) — HTTU features across versions
- [../concepts/granule-protection-check.md](concepts/granule-protection-check.md) — SMMUv3.3+ RME feature
- [../concepts/device-permission-table.md](concepts/device-permission-table.md) — RME DA feature
- [../concepts/mpam.md](concepts/mpam.md) — MPAM (SMMUv3.2+)
- [../concepts/mec.md](concepts/mec.md) — MEC (RME DA)
- [../concepts/atos.md](concepts/atos.md) — ATOS/VATOS (optional, SMMUv3.0+)
- [../concepts/performance-monitors.md](concepts/performance-monitors.md) — PMCG (optional, all versions)
- [../synthesis/smmu-register-map.md](synthesis/smmu-register-map.md) — full register map with IDR feature bits
