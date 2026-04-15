---
title: "SMMU Security States"
type: synthesis
tags: [smmu, security, non-secure, secure, realm, root, rme, model]
created: 2026-04-07
updated: 2026-04-14
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# SMMU Security States

Complete reference for modeling SMMU multi-security-state behavior. Covers SEC_SID determination, per-state programming interfaces, independent operation, Secure EL2, and RME/Realm state specifics.

## Security State Architecture Summary

The SMMU supports up to four security states, each with independent:
- Stream table (separate set of STEs).
- Command queue (separate circular buffer + enable).
- Event queue (separate circular buffer + enable).
- PRI queue (separate, where applicable).
- Register page prefix: `SMMU_*` (NS), `SMMU_S_*` (Secure), `SMMU_R_*` (Realm), `SMMU_ROOT_*` (Root/control).

States present in an implementation:
- **Non-secure:** always present.
- **Secure:** present when `SMMU_S_IDR1.SECURE_IMPL == 1`.
- **Realm:** present when `SMMU_ROOT_IDR0.REALM_IMPL == 1`.
- **Root:** present when `SMMU_ROOT_IDR0.ROOT_IMPL == 1` (control-only, no stream traffic).

## SEC_SID — Security State Sideband

Every incoming transaction carries `SEC_SID`:
- Non-secure: device traffic without Secure/Realm indication.
- Secure: traffic indicated as Secure by the interconnect/device.
- Realm: traffic indicated as Realm (for RME systems).

In PCIe:
- No IDE TLP prefix, or T=0 → SEC_SID = Non-secure.
- IDE TLP prefix with T=1 → SEC_SID = Realm.

In AMBA:
- NS signal distinguishes Secure from Non-secure. For Realm: `NSE` signal distinguishes Realm from Non-secure.

A physical StreamID value of N represents up to three distinct streams (Non-secure N, Secure N, Realm N) in a full RME DA system. Each looks up from the stream table associated with its security state.

## Non-secure State

- Controlled by `SMMU_CR0.SMMUEN`.
- Transactions always output to Non-secure PA space.
- No input NS override possible.
- Non-secure STEs, CDs, translation tables are in Non-secure memory.
- Event queue: one global Non-secure Event queue (all Non-secure streams).
- Bypass: `SMMU_GBPA` / `SMMU_AGBPA`.

## Secure State

### Basic Secure State

- Controlled by `SMMU_S_CR0.SMMUEN`.
- Secure STEs, CDs, Stream table, Command/Event queues are in Secure PA space.
- Permitted PA targets: Secure or Non-secure (determined by stage 1 table descriptor bits or `STE.NSCFG`).
- Input NS attribute: provided by client device; may be overridden by `STE.NSCFG` in bypass or stage 2-only configurations.
- `SMMU_S_CR0.SIF == 1`: aborts instruction fetches from Secure streams targeting Non-secure PA / NS IPA (security fence).

### StreamWorld for Secure State

Differentiates translation regimes within Secure state (for TLB tagging):
- **Secure** StreamWorld: maps to Secure EL1 translation regime (ASID-tagged TLB entries).
- **EL3** StreamWorld: maps to EL3 translation regime (no ASID tagging).
- Arm recommends not using EL3 StreamWorld for broadcast TLB maintenance reasons.
- Only one of Secure EL1 or EL3 should use the Secure Command queue at a time.

### Secure EL2 and Secure Stage 2 (SMMUv3.2+)

Available when `SMMU_S_IDR1.SEL2 == 1`:
- Supports Secure stage 2 translation (a Secure STE may set `Config[1] = 1`).
- Two Secure IPA spaces: **Secure IPA** (stage 1 → Secure IPA) and **Secure-stream NS IPA** (stage 1 → NS IPA).
- Each IPA space has a separate stage 2 translation table:
  - `STE.S_S2TTB` for Secure IPA → Secure stage 2.
  - `STE.S2TTB` for Secure-stream NS IPA → Secure stage 2.
- PA space determination:
  - From Secure IPA: controlled by `STE.S2SW` and `STE.S2SA`.
  - From NS IPA: controlled by `STE.S2NSW` and `STE.S2NSA` (and `STE.S2SW`/`S2SA`).
- TLB entries from Secure stage 2: tagged with Secure VMID (distinct namespace from Non-secure VMID).
- Translation tables fetched for a Secure stream from NS IPA space are treated as non-global (`nG=1` forced).

### Secure Commands Affecting NS State

Some commands on the Secure Command queue may affect Non-secure state (explicitly noted in spec §4). Commands on the Non-secure Command queue never affect Secure state.

## Realm State (RME / SMMU-for-RME)

### Basic Realm Configuration

- Controlled by `SMMU_R_CR0.SMMUEN`.
- Only VMSAv8-64 or VMSAv9-128 translation tables — VMSAv8-32 LPAE is not supported.
- Realm STEs, CDs, L1STDs, L1CDs: same format as Non-secure but all pointers are Realm physical addresses.
- Permitted PA targets: Realm (default), or Non-secure (via stage 2 translation).
- `SMMU_R_CR0.ATSCHK` is RES1 (always 1 for Realm streams).

### Realm Stream PA Space Determination

| Configuration | Output PA space |
|---------------|----------------|
| Stream bypass (or S1DSS bypass) | From STE.NSCFG applied to input NS attribute |
| EL1 stage 1 only | Always Realm PA |
| EL1 stage 1 + 2 | Determined by stage 2 translation |
| EL1 stage 2 only | Determined by stage 2 translation |
| EL2 or EL2-E2H stage 1 | Determined by stage 1 translation |

### Input NS Attribute for Realm Streams

- Distinguishes Realm from Non-secure (not Secure from Non-secure, as in Secure state).
- Provided by AMBA `NSE` signal or PCIe T-bit.
- If client device does not provide an NS attribute, it defaults to Realm.
- `CD.NSCFG0` and `CD.NSCFG1` are IGNORED for Realm CDs.

### Realm Commands

All commands on the Realm Command queue:
- Apply only to Realm SEC_SID streams.
- Any StreamID parameter is interpreted as a Realm StreamID.
- `SSec == 1` → `CERROR_ILL` (illegal).

### MECID (Memory Encryption Context ID)

SMMU-for-RME DA introduces MECID tagging for Realm streams:
- `STE.MECID` — MECID value for the stream.
- Client-originated transactions from Realm streams carry the MECID from STE.MECID.
- `SMMU_R_MECIDR` and `SMMU_R_GMECID` provide MECID discovery/configuration.

### Granule Protection Checks (GPC)

All Realm stream transactions (and some Non-secure with `RME_IMPL`) are subject to GPC at the PA output. See [smmu-translation-pipeline](smmu-translation-pipeline) Step 10 and [../concepts/granule-protection-check.md](../concepts/granule-protection-check.md).

### DPT (RME DA)

Realm streams with ATS and `STE.EATS == 0b11` are subject to DPT checks. See [../concepts/device-permission-table.md](../concepts/device-permission-table.md).

## Root State

- No stream traffic — Root is a control-only interface.
- Manages GPT: `SMMU_ROOT_GPT_BASE`, `SMMU_ROOT_GPT_BASE_CFG`, `SMMU_ROOT_GPT_BASE2`, `SMMU_ROOT_GPT_BASE_UPDATE`.
- Root TLB operations: `SMMU_ROOT_TLBI`, `SMMU_ROOT_TLBI_CTRL`.
- Fault reporting: `SMMU_ROOT_GPF_FAR`, `SMMU_ROOT_GPT_CFG_FAR`.
- Access restricted to Root-world / monitor software.
- `SMMU_ROOT_IDR0`: discovery register for Root capabilities.
- `SMMU_ROOT_CR0`, `SMMU_ROOT_CR0ACK`: Root-state enable/handshake.

## Programming Interface Independence

When multiple security states are present:
- Each state's Command queue, Event queue, and error registers operate fully independently.
- Full/overflow of one queue has no effect on another state's queue.
- Each state has its own ATOS interface.
- Interrupts configured and routed independently per state.
- `SMMU_(*_)GERROR` covers only the associated state's errors.
- Translation enable/disable per state (one state bypassing does not affect others).

## Initialization Per Security State

Each state requires its own initialization sequence (see [../concepts/smmu-initialization.md](../concepts/smmu-initialization.md)):
- Secure: initialized by Secure EL1/EL3 using `SMMU_S_*` registers. `SMMU_S_INIT.INV_ALL` available.
- Realm: initialized by Realm management software.
- Non-secure: initialized by NS kernel. Cannot rely on `SMMU_S_INIT`.
- Root: configured by monitor/Root software before NS/Secure/Realm init.

## Model Implementation Checklist

- [ ] SEC_SID is a first-class input; model must route to correct stream table based on SEC_SID.
- [ ] Separate stream tables, command queues, event queues per security state.
- [ ] Secure EL2: implement dual IPA spaces and dual stage 2 translation tables.
- [ ] Realm streams: enforce VMSAv8-64/VMSAv9-128 only; NSCFG0/NSCFG1 in CD ignored.
- [ ] Realm commands: enforce SSec==1 → CERROR_ILL; all StreamIDs treated as Realm.
- [ ] GPC: apply to all Realm stream PA outputs (and all states if RME_IMPL).
- [ ] MECID: track per-stream MECID for Realm streams (RME DA).
- [ ] Root register access: only available to Root/monitor privilege level.
- [ ] Queue independence: failure in one state's queue must not affect other states.

## Related Pages

- [../concepts/security-states.md](../concepts/security-states.md) — concept-level description
- [../concepts/granule-protection-check.md](../concepts/granule-protection-check.md) — GPC for Realm
- [../concepts/device-permission-table.md](../concepts/device-permission-table.md) — DPT for Realm ATS
- [../concepts/pcie-ats-pri.md](../concepts/pcie-ats-pri.md) — PCIe T/XT/TE bits and Realm state mapping
- [smmu-translation-pipeline.md](smmu-translation-pipeline.md) — pipeline with security state gates
- [smmu-version-feature-map.md](smmu-version-feature-map.md) — per-version availability of Secure/Realm/Root state features
