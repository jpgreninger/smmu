---
title: "Device Permission Table (DPT)"
type: concept
tags: [smmu, dpt, rme, realm, device-permission, pa-space, smmuv3.4, dpt-tlb, device-access-fault, dpt-lookup-fault]
created: 2026-04-07
updated: 2026-04-14
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Device Permission Table (DPT)

## Definition

The Device Permission Table (DPT) is an in-memory structure (added in SMMU-for-RME DA, indicated by `SMMU_R_IDR3.DPT == 1`) that gates device DMA access at a PA-space granularity. It associates physical address ranges with permitted device access rights, specifically controlling which PA space (Realm vs Non-secure) a device is permitted to access.

The DPT is distinct from the Granule Protection Table (GPT) ([granule-protection-check.md](granule-protection-check.md)). GPT controls physical memory ownership at the granule level for all agents (PEs and devices); DPT is an additional per-device PA-space gate specifically for the SMMU's Realm-state ATS flow.

DPT support is optional (`SMMU_R_IDR3.DPT`). Support is strongly recommended for RME DA systems.

## Overview and Scope

DPT is used **only** for StreamIDs configured with `StreamWorld == EL1`. Use with other StreamWorlds generates `C_BAD_STE`.

DPT is independently optional for Non-secure and Realm states:
- Non-secure DPT: `SMMU_IDR3.DPT == 1`
- Realm DPT: `SMMU_R_IDR3.DPT == 1`

DPT configuration is **not required** for StreamIDs where ATS is disabled or configured for Split-stage operation.

The DPT describes each configured granule of physical address space as one of:
- **No access** — access to the granule is not permitted.
- **VMID-specific access** — accessible for specific VMIDs.
- **Any-VMID access** — accessible regardless of VMID.

## DPT Configuration

| Register | Purpose |
|----------|---------|
| `SMMU_(R_)DPT_BASE` | Base PA of the level 0 DPT table. `RA` field: read-allocate hint. |
| `SMMU_(R_)DPT_BASE_CFG` | `DPTPS` (DPT protected space PA size), `DPTGS` (granule size), `L0DPTSZ` (L0 block size), `{TABLE_SH/OC/IC}` |
| `SMMU_(R_)DPT_CFG_FAR` | Fault address register for DPT lookup errors. `FAULT` bit: sticky, cleared by writing 0. |
| `SMMU_(R_)CR0.DPT_WALK_EN` | Enables DPT walking. |

## §3.24.1 DPT Check

A successful DPT lookup resolves to: No Access, or Access Control (AC), W, and VMID fields.

The DPT check fails as a **Device Access fault** (reported as F_TRANSL_FORBIDDEN) in the following cases:
- Input PA exceeds `SMMU_(R_)DPT_BASE_CFG.DPTPS` range → No Access.
- DPT descriptor indicates No Access (Level 0 No Access entry, or `A[1:0]` encoding indicating No Access in Level 1).
- `W == 0` in the DPT entry and the incoming transaction is a **write access**.
- `VMID` match required and `STE.S2VMID` does not match the DPT VMID.

**VMID matching table (STE.DPT_VMATCH vs DPT AC):**

| `STE.DPT_VMATCH` | AC=0b00 (VMID required) | AC=0b01 (VMID required) | AC=0b10 (no VMID check) |
|------------------|------------------------|------------------------|------------------------|
| `0b00` | Yes (match required) | Yes (match required) | No check |
| `0b01` | Yes (match required) | No check | No check |
| `0b10` | No check | No check | No check |

Note: For Realm STEs, `DPT_VMATCH` is always `0b00` — VMID is always checked when `AC` is `0b00` or `0b01`.

**Output PA space determination:**
- Non-secure DPT: output PA space is always Non-secure.
- Realm DPT: `AC == 0b01` or `0b10` → Non-secure PA space; otherwise → Realm PA space.

Note: `DPT_VMATCH` does not affect the output PA space.

Note: Write-only permission cannot be expressed in DPT; write-only is treated as read+write.

Note: For fully-coherent clients, write permission cannot be separately enforced in some coherency protocol implementations; in this case, the `W` bit is ignored (treated as 1) for fully-coherent translated transactions.

## §3.24.3 DPT Format and Lookup Process

The DPT has **two levels** (Level 0 and Level 1). All descriptors are 8 bytes, little-endian. All tables are aligned to their size.

DPT lookups use memory attributes from `SMMU_(R_)CR1.{TABLE_IC, TABLE_OC, TABLE_SH}` and the `RA` hint from `SMMU_(R_)DPT_BASE.RA`.

DPT lookups use MPAM `STE.{PARTID, PMG}` values for the StreamID being checked. DPT lookups are performed as if PBHA is disabled.

**Input PA interpretation:**

| PA bits | Usage |
|---------|-------|
| `[oas-1:dptps]` | Device Access fault if non-zero |
| `[dptps-1:l0dptsz]` | Level 0 (L0DPT) index |
| `[l0dptsz-1:dptgs+1]` | Level 1 (L1DPT) index |
| `[dptgs]` | Level 1 page descriptor index (upper/lower granule selector) |
| `[dptgs-1:0]` | Ignored |

**Walk algorithm:**
1. `SMMU_(R_)DPT_BASE` points to the base of the level 0 table.
2. Use PA bits `[dptps-1:l0dptsz]` as the L0 index. Each entry is 8 bytes.
3. If L0 entry is No Access, Block, or Invalid → walk complete.
4. If L0 entry is Table → contains a pointer to an L1 table.
5. Use PA bits `[l0dptsz-1:dptgs+1]` as the L1 index.
6. Decode L1 entry. PA bit `[dptgs]` may select upper or lower granule depending on `A[1:0]`.

### §3.24.3.1 DPT Descriptor Formats

**Level 0 No Access entry** (`bits[1:0] == 0b00`):
- Indicates region of size L0DPTSZ is not accessible.
- Not permitted to be cached in a DPT TLB.

**Level 0 Block entry** (`bits[1:0] == 0b01`):
- `AC[1:0]`: Access control (see table above). `0b11` = Reserved/invalid.
- `W`: Write permission (`0` = no write, `1` = write permitted).
- `VMID[15:0]`: VMID to check (VMID[15:8] = RES0 if `SMMU_IDR0.VMID16 == 0`).
- Permitted to be cached as a contiguous region of DPTGS-sized granules.

**Level 0 Table entry** (`bits[1:0] == 0b11`):
- `Address[55:12]`: Next-level L1 table base (aligned by hardware).
- Bits above OAS are RES0.
- Permitted to be cached in a DPT TLB.

**Level 1 entry:**
- Represents attributes for two adjacent granules (upper: `AC1/W1/VMID1`; lower: `AC0/W0/VMID0`), or a contiguous region when `Contig != 0`.
- `A[1:0]` field:
  - `0b00`: No Access to both granules.
  - `0b01`: No Access to upper; lower controlled by AC0/W0/VMID0.
  - `0b10`: Upper controlled by AC1/W1/VMID1; No Access to lower.
  - `0b11`, Contig=0: Both granules independently controlled.
  - `0b11`, Contig≠0: Contiguous region controlled by AC0/W0/VMID0 only; AC1/W1/VMID1 are RES0.
- `Contig` encodings: `0b0000`=no contig, `0b0001`=64KB, `0b0010`=2MB, `0b0011`=32MB, `0b0100`=512MB, `0b0101`=1GB, `0b0110`=16GB, `0b0111`=64GB, otherwise Reserved.
- `Contig` encodings selecting a region size > L0DPTSZ are Reserved.

Any descriptor with a RES0 bit set to 1, or a field configured to a Reserved encoding, is **invalid**.

## §3.24.2 DPT Caching Behavior

For `STE.EATS == 0b11` streams, DPT TLB entries may be created:
1. When an ATS Translation Completion grants any permissions (R or W): TLB entry based on final or all enabled translation stages.
2. When a DPT walk succeeds (no fault, no No Access indication).

DPT TLB entries are indexed by `SEC_SID` and the final output PA. Entries cover up to the effective translation region size (from translation level or Contig hint). Entries distinguish read-only vs read+write permission and output PA space.

**DPT TLB and Device Access faults:**
- A DPT TLB entry created from a **DPT walk** result may generate a Device Access fault.
- A DPT TLB entry created from a **translation table walk** (ATS TR result) may **not** generate a Device Access fault — a fresh DPT walk must be performed.
- Where an existing TLB entry grants access, Device Access fault is not generated.

**Note:** CMD_TLBI_* and broadcast TLBIs for stage 1/2 are **not** required to invalidate DPT TLB entries (unless implementation combines GPT and DPT in TLBs, in which case TLBI by PA removes DPT entries too).

## §3.24.4 DPT Lookup Errors

Two classes of fault:
1. **Device Access fault** — DPT lookup succeeded but access is not permitted → F_TRANSL_FORBIDDEN.
2. **DPT lookup fault** — DPT lookup itself failed → F_TRANSL_FORBIDDEN + report in `SMMU_(R_)DPT_CFG_FAR`.

**DPT lookup fault priority table:**

| Priority | Reason | Reported as | Level |
|----------|--------|-------------|-------|
| 1 | `DPT_WALK_EN == 0` | DPT_DISABLED | 0 |
| 2 | Invalid DPT register config | DPT_WALK_FAULT | 0 |
| 3 | GPC fault on L0 fetch | DPT_GPC_FAULT | 0 |
| 4 | External abort on L0 fetch | DPT_EABT | 0 |
| 5 | Invalid L0 descriptor | DPT_WALK_FAULT | 0 |
| 6 | GPC fault on L1 fetch | DPT_GPC_FAULT | 1 |
| 7 | External abort on L1 fetch | DPT_EABT | 1 |
| 8 | Invalid L1 descriptor | DPT_WALK_FAULT | 1 |

When the SMMU encounters a DPT lookup fault and `SMMU_(R_)DPT_CFG_FAR.FAULT == 0`: records information and sets FAULT=1. If FAULT already 1: not updated (first-fault semantics).

`SMMU_(R_)GERROR.DPT_ERR` is activated when a fault is recorded in DPT_CFG_FAR. If DPT_ERR is observable → DPT_CFG_FAR information is also observable.

**Invalid DPT register configuration conditions:**
- DPTPS = Reserved (`0b111`) or > `SMMU_IDR5.OAS`.
- DPTGS = Invalid or Reserved.
- L0DPTSZ = Invalid or Reserved.
- L0DPTSZ address size > OAS or DPTPS.

**DPT_GPC_FAULT** also records the GPC fault in `SMMU_ROOT_GPT_CFG_FAR` or `SMMU_ROOT_GPF_FAR` with REASON=TRANSLATION, FAULTCODE=GPF_WALK_EABT.

## §3.24.5 DPT Maintenance Operations

`CMD_DPTI_ALL` and `CMD_DPTI_PA` remove cached DPT information:
- Consumption provides no guarantee.
- Consumption of subsequent `CMD_SYNC` on the same Command queue guarantees: invalidation performed, all Events/faults for invalidated entries reported, all client transactions using invalidated entries completed.
- DPT maintenance does not affect TLBs or STE/CD caches.

Guidance:
- `CMD_DPTI_ALL`: sufficient to invalidate all DPT information.
- L0 Table descriptor: `CMD_DPTI_PA(Leaf=0, addr in L0DPTSZ region)`.
- L0 Table + all L1 entries: `CMD_DPTI_PA(Leaf=0, SIZE=L0DPTSZ-aligned region)`.
- L0 Block or L1 contiguous entries: `CMD_DPTI_PA(Leaf=1, SIZE matching region)`.
- Single L1 non-contiguous granule: `CMD_DPTI_PA(Leaf=1, granule addr)`.

## §3.24.6 Software Guidance

The DPT is generally used to partition physical address space between different EL1 contexts, so guidance applies to stage 2 translation configuration (or stage 1 for EL2-E2H StreamWorld future use cases).

### §3.24.6.1 Access Permissions

DPT should be configured to represent the **most-permissive** access permissions for the final enabled translation stage. Grant access when:
- `AF == 1`, or
- `AF == 0` and HTTU (Access flag hardware update) is enabled.

Grant write access when:
- Descriptor grants write access (including writable-dirty), or
- Descriptor is writable-clean and HTTU is enabled.

### §3.24.6.2 Invalid to Valid Transition

1. Configure DPT to grant access for the granule.
2. Perform appropriate cache maintenance and barriers.
3. Configure the final enabled translation stage to grant access.

Note: TLB maintenance is **not** required for Invalid→Valid transitions.

### §3.24.6.3 Valid to Invalid Transition

1. Mark descriptor in final enabled translation stage as **Invalid**.
2. Issue appropriate TLBI + CMD_SYNC. (New ATS translation requests will now fail; but existing Translated transactions may still succeed.)
3. Issue `CMD_ATC_INV` + CMD_SYNC. (Device should not issue new ATS Translated transactions, except write-backs from fully-coherent devices.)
4. For fully-coherent device: issue appropriate CMOs for the granule. (May result in device write-backs.)
5. Mark DPT configuration as Invalid. (Rogue ATS Translated transactions may succeed or fail.)
6. Issue `CMD_DPTI_*` + CMD_SYNC. (Rogue ATS Translated transactions will now fail.)

### §3.24.6.4 Clearing DPT Lookup Errors

1. Write 0 to `SMMU_(R_)DPT_CFG_FAR.FAULT` (clears the whole register).
2. Acknowledge `SMMU_(R_)GERROR.DPT_ERR` by writing `SMMU_(R_)GERRORN.DPT_ERR` to the same value.
3. Re-read `SMMU_(R_)DPT_CFG_FAR.FAULT` to check if a new fault occurred between steps 1 and 2.

## §3.24.7 Split-Stage ATS vs Full ATS with DPT

**Use Full ATS + DPT when:** direct device access to physical address space is required (e.g., fully-coherent cache, PCIe peer-to-peer routing without going through the Root Port). Full ATS is required for correct address-based routing in these cases.

**Prefer Split-stage ATS when:** direct device access to PA is not required. Split-stage ATS is simpler to configure with comparable protection and performance.

**Security comparison:**
- Both configurations enforce GPCs and limit device access to the guest's PA footprint.
- Split-stage ATS allows SMMU to enforce read/write/execute permissions on Translated transactions.
- Full ATS + DPT cannot distinguish read/write/execute permission levels.

**Performance comparison:**
- Full ATS + DPT requires maintaining DPT entries and issuing `CMD_DPTI_*` operations.
- Split-stage ATS avoids DPT maintenance overhead.
- SMMU lookup performance is comparable; DPT walks are typically shallower than translation table walks.

## Model Implementation Notes

- DPT is an optional feature (RME DA); a model must check `SMMU_(R_)IDR3.DPT == 1` before implementing DPT walking.
- DPT checks apply at the Translated transaction input; use cached DPT TLB entry or perform a fresh walk.
- `DPT_CFG_FAR` has first-fault semantics (only written when FAULT == 0).
- VMID match logic must implement the 3×3 `DPT_VMATCH` × `AC` table.
- CONSTRAINED UNPREDICTABLE: overlapping contiguous Contig regions with different attributes may use any configured attributes.

## Related Concepts

- [granule-protection-check.md](granule-protection-check.md) — GPT provides physical memory ownership; DPT gates device ATS access
- [pcie-ats-pri.md](pcie-ats-pri.md) — DPT applies to ATS Translated transactions (EATS == 0b11)
- [security-states.md](security-states.md) — DPT is primarily relevant to Realm-state streams
- [command-queue.md](command-queue.md) — CMD_DPTI_ALL and CMD_DPTI_PA for DPT cache maintenance

## Sources That Use This Concept

- [../sources/ihi0070g-b-smmuv3-architecture-spec.md](../sources/ihi0070g-b-smmuv3-architecture-spec.md) — §3.24 Device Permission Table; §3.24.1–3.24.7; §4.6 DPT maintenance commands; §2.7 SMMU for RME DA features
