---
title: "Virtual Machine Structure (VMS)"
type: concept
tags: [smmu, vms, virtualization, vmid, smmuv3.2, configuration, mpam, partid-map, f-vms-fetch]
created: 2026-04-07
updated: 2026-04-14
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Virtual Machine Structure (VMS)

## Definition

The Virtual Machine Structure (VMS) is an in-memory data structure introduced in SMMUv3.2 that holds per-VM configuration information, scoped by VMID. It reduces duplication of per-VM state across multiple STEs that share the same VMID (i.e., multiple devices assigned to the same VM).

VMS presence is indicated by `SMMU_IDR0.VMS == 1` (optional, SMMUv3.2+).

## §5.6 VMS Structure

**Size:** 4 KB (4096 bytes), 4 KB-aligned.

**Current defined fields:**

| Field | Bits | Description |
|-------|------|-------------|
| `PARTID_MAP` | `[511:0]` | Array of 32 × 16-bit little-endian physical PARTIDs, indexed by virtual `CD.PARTID`. Maps virtual CD PARTID values to physical PARTID values. |
| Reserved | `[32767:512]` | RES0. Reserved for future per-VM functionality. |

### PARTID_MAP Detail

The 32-entry PARTID_MAP array maps virtual PARTID values (from `CD.PARTID` for CDs located through an STE referencing this VMS) to physical PARTID values used for MPAM.

**Determining the appropriate `PARTID_MAX` for range checking:**
- Non-secure stream: `SMMU_MPAMIDR.PARTID_MAX`.
- Secure stream with `SMMU_S_MPAMIDR.HAS_MPAM_NS == 0` or `STE.MPAM_NS == 0`: `SMMU_S_MPAMIDR.PARTID_MAX`.
- Secure stream with `SMMU_S_MPAMIDR.HAS_MPAM_NS == 1` and `STE.MPAM_NS == 1`: `SMMU_MPAMIDR.PARTID_MAX`.

If an entry is configured with a value greater than the supported PARTID size, an **UNKNOWN PARTID** is used.

Note: There is no equivalent PARTID_MAP for PMG values — PMG does not need to be mapped from virtual to physical.

Note: The number 32 derives from the MPAM PE limit in the A-profile architecture.

## §5.6.1 VMS Presence and Fetching

The VMS is supported by a Security state when **all** of the following are true:
- `SMMU_IDR3.MPAM == 1`
- `SMMU_IDR0.S1P == 1`
- `SMMU_IDR0.S2P == 1`

**Additionally for Non-secure state:**
- `SMMU_MPAMIDR.PARTID_MAX != 0`

**Additionally for Secure state:**
- `SMMU_S_IDR1.SEL2 == 1`, and at least one of:
  - `SMMU_S_MPAMIDR.PARTID_MAX != 0`
  - `SMMU_MPAMIDR.PARTID_MAX != 0` and `SMMU_S_MPAMIDR.HAS_MPAM_NS == 1`

**Additionally for Realm state:** at least one of:
- `SMMU_R_MPAMIDR.PARTID_MAX != 0`
- `SMMU_MPAMIDR.PARTID_MAX != 0` and `SMMU_R_MPAMIDR.HAS_MPAM_NS == 1`

VMS is not always enabled even when supported — see `STE.VMSPtr` for when the VMS is enabled.

**VMS fetch attributes:** same memory attributes and Security state as those used to fetch STEs in the corresponding Security state (`SMMU_CR1` and `SMMU_STRTAB_BASE`).

**Speculative VMS access:** If VMS is enabled for a stream, the SMMU is permitted (but not required) to access the VMS even when processing a transaction that does not require VMS information. If such a speculative access experiences an External abort → treated as if the VMS was required, reported as **F_VMS_FETCH**.

Example: A nested configuration with `STE.S1MPAM == 1` enables VMS. A transaction that bypasses stage 1 due to `STE.S1DSS == 0b01` does not require VMS information, but the SMMU may still access it; an error would be reported as F_VMS_FETCH. Also possible for non-client (PRI queue overflow) transactions.

## §5.6.2 VMS Caching and Invalidation

`PARTID_MAP` contents may be cached as:
1. **A configuration cache entry indexed by StreamID** — invalidated by any operation affecting the StreamID (CMD_CFGI_STE scope or wider).
2. **A separate configuration cache for PARTID_MAP contents, indexed by VMID** — invalidated by CMD_CFGI_VMS_PIDM.

`PARTID_MAP` is **not** cached in a TLB.

Because PARTID_MAP may be cached by both StreamID and VMID, a change requires invalidation by **both** StreamID and VMID:

- `CMD_CFGI_STE` / `CMD_CFGI_STE_RANGE`: invalidate StreamID-indexed VMS information for matching streams.
- `CMD_CFGI_ALL`: invalidates all VMS information regardless of indexing method.
- `CMD_CFGI_VMS_PIDM(VMID)`: invalidates any separate VMID-indexed PARTID_MAP cache.

Note: `STE.VMSPtr` does not interact with `SMMU_(*_)CR0.VMW`. If STEs with different VMIDs point to the same VMS, information may be cached multiple times and invalidation requires operations for each VMID.

## VMS Location and Consistency

The VMS is pointed to from an STE via `STE.VMSPtr` (PA). If `STE.VMSPtr` points to an address out of OAS range → **C_BAD_STE**.

**Multiple STEs sharing a VMS:**
- Multiple STEs can point to the same VMS to avoid configuration duplication.
- Multiple STEs within a Security state with the same VMID **must** point to the same VMS. Using different VMS pointers for STEs sharing a VMID is UNPREDICTABLE (the SMMU may use any of the pointers).

## Model Implementation Notes

- VMS support is optional; check `SMMU_IDR3.MPAM`, `SMMU_IDR0.S1P/S2P`, and per-state MPAMIDR conditions before implementing VMS fetch logic.
- VMS adds an additional memory access in the configuration lookup path; performance model should account for this latency.
- Two cache invalidation scopes: StreamID (CMD_CFGI_STE family) and VMID (CMD_CFGI_VMS_PIDM). Both must be issued when updating PARTID_MAP.
- F_VMS_FETCH event must be generated on External abort during VMS access (even for speculative access).

## Related Concepts

- [[concepts/stream-table-entry]] — STE.VMSPtr points to the VMS
- [[concepts/two-stage-translation]] — VMS holds per-VM stage 2 configuration
- [[concepts/tlb-invalidation]] — VMS invalidation is distinct from TLB invalidation

## Sources That Use This Concept

- [[sources/ihi0070g-b-smmuv3-architecture-spec]] — §5.6 VMS data structure; §3.3 Data structures; §4.3.5 CMD_CFGI_VMS_PIDM; §2.4 SMMUv3.2 features
