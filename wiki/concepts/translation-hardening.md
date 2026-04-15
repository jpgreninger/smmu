---
title: "Translation Hardening (THE)"
type: concept
tags: [smmu, smmuv3.4, security, permission, the, assuredonly, protected, feat_the]
created: 2026-04-13
updated: 2026-04-13
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Translation Hardening (THE)

## Definition

Translation Hardening Extension (THE) is an optional SMMUv3.4 feature (`SMMU_IDR3.THE == 1`, `FEAT_THE`) that adds two security mechanisms to translation: a **Protected attribute** at stage 1 and **AssuredOnly permission checks** at stage 2. Together they support the Arm Confidential Computing Architecture by ensuring that only trusted software can access certain memory regions.

## Stage 1: Protected Attribute (§3.27.1)

When `SMMU_IDR3.THE == 1`, the SMMU honors the Protected (PnCH) attribute in stage 1 translation table descriptors, mirroring PE behavior:

- In **VMSAv9-128** descriptors, the Protected attribute is always present.
- In **VMSAv8-64** descriptors, the Protected attribute is only present when `CD.PnCH == 1`. When `CD.PnCH == 1`, the Contiguous bit is absent from those descriptors (bit 52 is reinterpreted).
- The SMMU does not support Read-Check-Write (RCW) operations; RCW checks are not applied.
- Enabling `CD.PnCH` without `STE.AssuredOnly` has limited utility unless stage 1 translation tables are shared between PE and SMMU contexts.

## Stage 2: AssuredOnly Permission Checks (§3.27.2)

When `SMMU_IDR3.THE == 1`, the SMMU applies AssuredOnly permission checks at stage 2:

- Enabled per-stream via `STE.AssuredOnly == 1` (analogous to `VTCR_EL2.AssuredOnly` on PE).
- When enabled, a transaction accessing memory **not** marked AssuredOnly at stage 2 triggers a stage 2 **F_PERMISSION** fault with `AssuredOnly == 1` in the event record.
- **CD fetch dependency:** If a CD (or its L1CD pointer) is fetched from memory that is **not** marked AssuredOnly at stage 2, accesses translated through that CD's `TTB0`/`TTB1` fields do **not** carry the Assured Translation property — even if the translation table descriptors themselves would grant it.
- If both the CD and its L1CD are fetched from AssuredOnly memory, translations through those TTBs carry the Assured Translation property per the A-profile architecture definition.
- When stage 1 is bypassed (via `STE.Config` or `STE.S1DSS`), any access to a region marked AssuredOnly at stage 2 generates a Permission fault — matching PE behavior when stage 1 is disabled.

## ATS Interactions

- **ATS Translation Request:** AssuredOnly check applies normally. If it fails, the ATS Translation Completion is returned with Success and `R == W == 0` (no-access response).
- **ATS Translated Transaction with `STE.EATS == 0b10` (Split-stage ATS):** AssuredOnly is ignored.

## Discovery

| Register field | Meaning |
|---|---|
| `SMMU_IDR3.THE == 1` | THE feature implemented |
| `STE.AssuredOnly` | Enable AssuredOnly checks for this stream |
| `CD.PnCH` | Enable Protected attribute interpretation in stage 1 VMSAv8-64 descriptors |
| F_PERMISSION `AssuredOnly` bit | Set to 1 when fault caused specifically by AssuredOnly check |

## Related Concepts

- [permission-indirections.md](permission-indirections.md) — S1PIE/S2PIE/S2POE permission remapping (also SMMUv3.4); THE is a distinct feature
- [two-stage-translation.md](two-stage-translation.md) — AssuredOnly checks operate within the stage 2 permission check phase
- [context-descriptor.md](context-descriptor.md) — `CD.PnCH` field enables Protected attribute
- [stream-table-entry.md](stream-table-entry.md) — `STE.AssuredOnly` field enables AssuredOnly checks
- [fault-models.md](fault-models.md) — F_PERMISSION with `AssuredOnly == 1` is the fault raised on check failure
- [pcie-ats-pri.md](pcie-ats-pri.md) — ATS Translation Request and Translated transaction interactions

## Sources That Use This Concept

- [../sources/ihi0070g-b-smmuv3-architecture-spec.md](../sources/ihi0070g-b-smmuv3-architecture-spec.md) — §3.27 Translation Hardening; §3.27.1 Protected attribute; §3.27.2 AssuredOnly permission checks; §2.8 SMMUv3.4 features; `SMMU_IDR3.THE` register field description
