---
title: "Address Translation Operations (ATOS)"
type: concept
tags: [smmu, atos, vatos, address-translation, debug, software-lookup]
created: 2026-04-13
updated: 2026-04-16
sources: [ihi0070g-b-smmuv3-architecture-spec]
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

## ATOS_PAR: Result Register (§9.1.4)

`ATOS_PAR` (one per register group: `SMMU_GATOS_PAR`, `SMMU_S_GATOS_PAR`, `SMMU_VATOS_PAR`, `SMMU_S_VATOS_PAR`) holds the translation result. Its contents are only valid after the corresponding `ATOS_CTRL.RUN` bit has been set to 1 by software and cleared to 0 by the SMMU.

### Success (ATOS_PAR.FAULT == 0)

The translation succeeded. Fields include:
- **ADDR:** The translated output address (physical address).
- **ATTR / SH:** Memory type and shareability determined from the translation (post-combine per §13.1.5). Device types always return `SH = OSH`. STE attribute overrides (MTCFG/SHCFG/ALLOCCFG) are **not** included — attributes may reflect the TLB-cached subset of architectural TTD/MAIR attributes.

### Fault (ATOS_PAR.FAULT == 1)

Translation failed. Additional fields:

- **FAULTCODE:** Identifies the type of fault (see table below).
- **REASON:** Identifies which stage and sub-cause experienced the fault:
  - `0b00` — Stage 1 or general (no stage 2 involvement)
  - `0b01` — Stage 2 fault on CD fetch (IPA is CD address)
  - `0b10` — Stage 2 fault on stage 1 descriptor fetch (IPA is descriptor address)
  - `0b11` — Stage 2 fault on input IPA (stage 1 output address)
- **FADDR:** Contains the IPA that caused a stage 2 fault (0 when REASON = 0b00 or when FAULTCODE is INV_REQ/INV_STAGE).

### REASON and FADDR Validity Table (§9.1.4)

| ATOS_ADDR.TYPE | FAULTCODE class | REASON | FADDR |
|---|---|---|---|
| Any | INV_REQ, INV_STAGE | 0b00 | 0 |
| 0b01 (S1) on S1-only stream | F_WALK_EABT, F_CD_FETCH, TRF, MISC | 0b00 | 0 |
| 0b01 (S1) on S1+S2 stream | F_WALK_EABT (actual bus abort at S1) | 0b00 | 0 |
| | F_CD_FETCH (actual bus abort at CD) | 0b00 | 0 |
| | F_CD_FETCH (synthetic, S2 fault on CD fetch) | 0b00 | 0 |
| | F_WALK_EABT (synthetic, S2 fault on S1 descriptor) | 0b00 | 0 |
| | TRF, MISC (S1 or config fault) | 0b00 | 0 |
| 0b10 (S2 only) | F_ADDR_SIZE (S2) | 0b11 | 0 |
| | F_ADDR_SIZE (S1) | 0b00 | 0 |
| | F_TRANSLATION, F_ACCESS, F_PERMISSION, F_WALK_EABT, MISC | 0b11 | 0 |
| 0b11 (S1+S2) | TRF (S1 fault) | 0b00 | 0 |
| | TRF (S2 fault on CD fetch) | 0b01 | IPA (CD address) |
| | TRF (S2 fault on S1 descriptor fetch) | 0b10 | IPA (descriptor address) |
| | TRF (S2 fault on IPA = S1 output) | 0b11 | IPA (S1 output address) |
| | F_WALK_EABT (S1 descriptor fetch abort) | 0b00 | 0 |
| | F_WALK_EABT (S2 descriptor fetch abort) | 0b01/0b10/0b11 | 0 |
| | F_CD_FETCH, MISC | 0b00 | 0 |

Notes:
- TRF = Translation Related Faults: F_TRANSLATION, F_ADDR_SIZE, F_ACCESS, F_PERMISSION.
- MISC = all other faults except F_WALK_EABT, F_CD_FETCH, INV_REQ, INV_STAGE. Specifically: INTERNAL_ERR, C_BAD_STREAMID, F_STE_FETCH, F_VMS_FETCH, C_BAD_STE, F_STREAM_DISABLED, C_BAD_SUBSTREAMID, C_BAD_CD, F_TLB_CONFLICT, F_CFG_CONFLICT.

### §9.1.5 ATOS_PAR.FAULTCODE Encodings

| Code | Name | Meaning |
|---|---|---|
| 0xFF | INV_REQ | Malformed ATOS request (invalid TYPE or field combination) |
| 0xFE | INV_STAGE | Requested stage not present in STE configuration |
| 0xFD | INTERNAL_ERR | Miscellaneous termination (RAS error, SMMUEN cleared during translation) |
| 0x02 | C_BAD_STREAMID | ATOS_SID.STREAMID out of range of configured Stream table |
| 0x03 | F_STE_FETCH | External abort on STE fetch |
| 0x04 | C_BAD_STE | Selected STE invalid or ILLEGAL |
| 0x06 | F_STREAM_DISABLED | Non-substream request with S1DSS==0b00 on substream-enabled config, or SubstreamID==0 with S1DSS==0b10 |
| 0x08 | C_BAD_SUBSTREAMID | ATOS_SID.SSID out of range of configured CD table |
| 0x09 | F_CD_FETCH | External abort on CD fetch (or stage 2 fault during CD fetch for VATOS/S1-only requests) |
| 0x0A | C_BAD_CD | Selected CD invalid or ILLEGAL |
| 0x0B | F_WALK_EABT | External abort on translation table walk (or stage 2 fault during S1 walk for S1-only requests) |
| 0x10 | F_TRANSLATION | Translation fault |
| 0x11 | F_ADDR_SIZE | Address Size fault |
| 0x12 | F_ACCESS | Access flag fault |
| 0x13 | F_PERMISSION | Permission fault |
| 0x20 | F_TLB_CONFLICT | Translation caused TLB conflict condition |
| 0x21 | F_CFG_CONFLICT | Translation caused configuration cache conflict |
| 0x25 | F_VMS_FETCH | External abort on VMS fetch |

All other values are Reserved. Priority order follows the Event queue priority order (§7.3.22), with INV_REQ highest, followed by INV_STAGE, then C_BAD_STREAMID, F_STE_FETCH, C_BAD_STE, and so on.

### GPC Interaction with ATOS (§9.1.4)

When RME is implemented (`SMMU_IDR0.RME_IMPL == 1`), configuration structure fetches and translation table walks arising from ATOS are subject to **Granule Protection Checks (GPC)**. This applies in the same manner as for any other SMMU-originated access (§3.25.3).

If a configuration structure fetch or translation table walk fails with a GPC fault during ATOS:
1. The fault is reported in `SMMU_ROOT_GPF_FAR` or `SMMU_ROOT_GPT_CFG_FAR` (if no active fault already exists in that register).
2. The ATOS_PAR register reports the fault as though the failed access experienced an **external abort** (i.e., as F_STE_FETCH, F_CD_FETCH, or F_WALK_EABT as appropriate).

The SMMU does **not** perform a GPC check on the **final output address** of a successful ATOS translation — GPC is applied only to structure fetches and table walks, not to the translated PA result.

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
- [attribute-transformation.md](attribute-transformation.md) — §13.1 output attribute determination applies to ATOS results; STE attribute overrides ignored except STE.S2FWB; ATOS_ADDR fields supply INST/PRIV instead of STE.INSTCFG/PRIVCFG
- [../synthesis/smmu-register-map.md](../synthesis/smmu-register-map.md) — GATOS/S_GATOS/VATOS register group locations in the memory map

## Sources That Use This Concept

- [../sources/ihi0070g-b-smmuv3-architecture-spec.md](../sources/ihi0070g-b-smmuv3-architecture-spec.md) — Chapter 9 Address Translation Operations; §9.1.4 ATOS_PAR encoding (REASON/FADDR validity table); §9.1.5 FAULTCODE encodings; §3.9 ATOS facility; GPC interaction §3.25.3; `SMMU_IDR0.ATOS` and `SMMU_IDR0.VATOS` field descriptions; §13.1 attribute override tables
