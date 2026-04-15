---
title: "Permission Indirections (S1PIE / S2PIE / S2POE)"
type: concept
tags: [smmu, permission, indirection, overlay, s1pie, s2pie, s2poe, smmuv3.4, armv8.9, pir, por, piindex, poindex, indirect-permission-scheme]
created: 2026-04-07
updated: 2026-04-14
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Permission Indirections (S1PIE / S2PIE / S2POE)

## Definition

Permission indirections and overlays are SMMUv3.4 features (corresponding to Armv8.9-A `FEAT_S1PIE`, `FEAT_S2PIE`, `FEAT_S2POE`) that decouple permission encoding in translation table descriptors from the actual access rights applied to a transaction. They allow a software-controlled permission remapping table to override or modify raw descriptor permissions.

## Background

Introduced in Armv8.9-A (`FEAT_S1POE`, `FEAT_S2POE`, `FEAT_S1PIE`, `FEAT_S2PIE`). The SMMU follows the same A-profile behavior for these features. **Note:** The SMMU does **not** support the stage 1 permission overlay feature present in the PE architecture.

**Feature discovery:** `SMMU_IDR3.S1PI` (S1PIE), `SMMU_IDR3.S2PI` (S2PIE/S2POE). These are optional in SMMUv3.4+.

## §3.26.1 Stage 1 Permission Indirections (S1PIE)

`PIIndex` is a 4-bit translation table descriptor field (introduced by `FEAT_S1POE` in the A-profile architecture). When S1PIE is active, stage 1 permissions are looked up from `CD.PIIP[PIIndex]` and `CD.PIIU[PIIndex]` rather than being taken directly from the descriptor.

**Control hierarchy:**

| `SMMU_IDR3.S1PI` | `STE.S1PIE` | `CD.PIE` | Stage 1 permission behavior |
|------------------|-------------|---------|---------------------------|
| 0 | RES0 | RES0 | Direct from translation tables. |
| 1 | 0 | RES0 | Direct from translation tables. |
| 1 | 1 | 0 | Direct from translation tables. |
| 1 | 1 | 1 | Indirected via CD.PIIP/CD.PIIU using PIIndex from descriptors. |

Note: `STE.S1PIE` allows a hypervisor to prevent guest configuration of permission indirections on StreamIDs where the guest directly controls CDs. In that case, the hypervisor presents an emulated SMMU without S1PIE support to the guest.

**If stage 1 Indirect Permission Scheme is enabled:**
- `CD.WXN` is RES0 and has no effect.
- Stage 1 permissions are computed as follows:
  1. Decode permissions from `CD.PIIU[PIIndex]` and `CD.PIIP[PIIndex]`.
  2. For NS-EL1, Secure, Realm-EL1, and any-EL2-E2H StreamWorlds: Apply `CD.PAN` — if PAN=1 and any Unprivileged access is granted, then Privileged read and write permissions are removed (regardless of `CD.EPAN`). This may be applied here or after step 4 (IMPLEMENTATION DEFINED).
  3. For Secure state translations: if `SMMU_S_CR0.SIF == 1` and stage 1 output is Non-secure → remove both Privileged execute and Unprivileged execute permissions.
  4. For Realm state translations: if stage 1 output is Non-secure → remove both Privileged execute and Unprivileged execute permissions. Not affected by `CD.{EPD0, EPD1, E0PD0, E0PD1}`.

## §3.26.2 Stage 2 Permission Indirections and Overlays (S2PIE / S2POE)

`PIIndex` (from `FEAT_S2PIE`) and `POIndex` (from `FEAT_S2POE`) are translation table descriptor fields for stage 2.

**Control hierarchy:**

| `SMMU_IDR3.S2PI` | `STE.S2PIE` | `STE.S2POE` | Stage 2 permission behavior |
|------------------|-------------|------------|---------------------------|
| 0 | RES0 | RES0 | Direct from translation tables. |
| 1 | 0 | 0 | Direct from translation tables. |
| 1 | 0 | 1 | **ILLEGAL — generates C_BAD_STE.** |
| 1 | 1 | 0 | Indirected via `SMMU_S2PII` using PIIndex. |
| 1 | 1 | 1 | Overlay via `STE.S2POI` using POIndex, combined with `SMMU_S2PII` using PIIndex. |

**Stage 2 permission computation order (and F_PERMISSION priority: highest first):**
1. **AssuredOnly permission check** (if `SMMU_IDR3.THE == 1`) — applied to stage 2 translation of stage 1 output address and stage 1 table walk addresses.
2. **Base and Overlay permissions:**
   - If S2POE enabled: look up Overlay permissions from `STE.S2POI[POIndex]`.
   - If S2PIE enabled: look up Base permissions from `SMMU_S2PII[PIIndex]`; otherwise Base permissions are taken directly from the descriptor.
   - Combine Base and Overlay permissions per A-profile rules.
3. Effect of `STE.S2PTW` — applied to translation of stage 1 table walk or CD fetch.
4. **Dirty state permission check** — if permission indirection is enabled and write permission is required.
5. Effects of `STE.DRE` and `STE.DCP` — for directed prefetch and CMO operations.

## PIR/POR Format (§3.26)

`CD.PIIP` and `CD.PIIU` are arrays of 16 4-bit entries (indexed by PIIndex[3:0]), each entry encoding the stage 1 permissions for a permission index value.

`STE.S2POI` is an array of 16 4-bit entries (indexed by POIndex[3:0]), each entry encoding the stage 2 overlay permissions.

`SMMU_S2PII` is a per-implementation register holding the stage 2 base permission indirection table.

The encoding of each entry follows the A-profile PIR/POR format defined in `FEAT_S1PIE`/`FEAT_S2PIE`/`FEAT_S2POE` respectively.

## Stage 2 Permission Indirection Inhibit (S2PII)

`STE.S2PII == 1` (introduced in SMMUv3.4) prevents stage 2 permission indirections from being applied, even if configured in `STE.S2PIE`. This allows a hypervisor to selectively inhibit indirections on specific streams.

## Interaction with STE.S2PII

`STE.S2PII` controls whether stage 2 permission indirections are applied. With `STE.S2PII == 1`, the `SMMU_S2PII` register is not used for that stream.

## Translation Hardening (THE / AssuredOnly)

Related feature: `SMMU_IDR3.THE == 1` (optional in SMMUv3.4, `FEAT_THE`):
- Introduces a **Protected** attribute on translation table descriptors.
- **AssuredOnly permission checks** (§3.27.2): when a descriptor has the Protected attribute, additional permission checks are performed to enforce "assured-only" access policies.
- This is step 1 in the stage 2 permission computation order.
- See `[translation-hardening.md](translation-hardening.md)`.

## Model Implementation Notes

- S1PIE requires the CD to carry `PIIP` (16×4-bit) and `PIIU` (16×4-bit) arrays; model must implement 4-bit PIIndex lookup.
- S2PIE requires `SMMU_S2PII` register; S2POE requires `STE.S2POI` array; model must apply both in the correct computation order.
- `STE.S2PIE == 0` with `STE.S2POE == 1` is ILLEGAL → C_BAD_STE.
- Version-gating on `SMMU_IDR3.S1PI` and `SMMU_IDR3.S2PI` is essential before enabling these paths.
- The stage 2 permission computation order (AssuredOnly → Base+Overlay → S2PTW → Dirty → DRE/DCP) must be preserved; F_PERMISSION priority follows this order.

## Related Concepts

- [two-stage-translation.md](two-stage-translation.md) — permission indirections modify the permission output of each translation stage
- [context-descriptor.md](context-descriptor.md) — S1PIE configuration is held in the CD
- [stream-table-entry.md](stream-table-entry.md) — S2PIE and S2POE configuration is in the STE
- [fault-models.md](fault-models.md) — F_PERMISSION faults arise from permission check failures

## Sources That Use This Concept

- [../sources/ihi0070g-b-smmuv3-architecture-spec.md](../sources/ihi0070g-b-smmuv3-architecture-spec.md) — §3.26 Permission Indirections; §3.27 Translation Hardening; §2.8 SMMUv3.4 features
