---
title: "Hardware Translation Table Update (HTTU)"
type: concept
tags: [smmu, httu, access-flag, dirty-state, translation-table, hardware-update, dbm, dps, ips, haft, ats-pri]
created: 2026-04-07
updated: 2026-04-14
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Hardware Translation Table Update (HTTU)

## Definition

Hardware Translation Table Update (HTTU) is the SMMU feature that automatically updates the **Access flag** (AF) and/or **Dirty state** (DBM/dirty bit) in translation table descriptors when a translation is used, without requiring a software fault-and-update cycle. This mirrors the `FEAT_HAFDBS` feature in Armv8-A PEs.

HTTU capability is reported in `SMMU_IDR0.HTTU`:

| `SMMU_IDR0.HTTU` | Capability |
|-----------------|------------|
| 0b00            | No HTTU support |
| 0b01            | Access flag hardware update only |
| 0b10            | Access flag + Dirty state hardware update |
| 0b11            | Access flag + Dirty state hardware update, **and** Access flag update for Table descriptors (HAFT; SMMUv3.4) |

## §3.13.1 Software Update of Flags

When HTTU is not supported or not enabled, software is responsible for maintaining the Access flag and dirty state. The recommended software procedure:

### Access Flag Software Update
1. A read or write that fetches a descriptor with `AF == 0` causes an **F_ACCESS** fault (when `AFFD == 0`).
2. Software (exception handler or SMMU driver) sets `AF = 1` in the translation table descriptor.
3. **No TLB invalidation is required** when only setting AF to 1 (agents are not permitted to cache descriptors with `AF == 0` in TLBs).
4. Software issues `CMD_RESUME` to retry the stalled transaction.

Note: `AFFD == 1` in the CD/STE suppresses F_ACCESS faults for `AF == 0` descriptors; the translation is used as if `AF == 1`. This is only relevant when HTTU is not used.

### Dirty State Software Update (without HTTU)
Dirty state is maintained by write-protecting clean pages:
- A **writable-dirty** descriptor has full write permission (`AP[2] == 0` for stage 1, `S2AP[1:0] == 0b1x` for stage 2).
- A **writable-clean** descriptor is temporarily non-writable (write-protected) to trigger a Permission fault on first write. A software-defined bit (or a convention) distinguishes this from a genuinely read-only page.
- A write to a writable-clean page causes an **F_PERMISSION** fault. The fault handler marks the page as dirty (sets `AP[2] = 0` or `S2AP[1] = 1`) and retries.
- A write to a genuinely non-writable page is an error.

Priority rule: **F_ACCESS takes priority over F_PERMISSION.** A write to a writable-clean page with `AF == 0` and `AFFD == 0` causes F_ACCESS first; only after `AF == 1` is set does an F_PERMISSION fault occur on the next attempt.

When HTTU is used with shared translation tables, all agents must use the **DBM flag convention** and perform **atomic updates** (see §3.13.3.1).

## §3.13.2 Access Flag Hardware Update

When HTTU is supported and enabled for a stream, a translation that fetches a descriptor with `AF == 0` (which would without HTTU cause F_ACCESS) **atomically sets `AF = 1`** in the descriptor held in memory, coherently if appropriate. The translation continues without a fault.

**This includes** stage 2 translation for the fetch of an L1CD or CD.

- The SMMU **never clears** AF.
- If a descriptor access causes a Permission fault, it is **UNKNOWN** whether AF is updated to 1.
- If dirty state update occurs, the final descriptor also has `AF == 1`.

When HTTU is disabled or not supported: `AF == 0` and `AFFD == 0` → F_ACCESS.

## §3.13.3.1 Dirty State Hardware Update — Direct Permission Scheme (DPS)

The **Direct Permission Scheme** uses a new flag at bit[51] of Block and Page descriptors: the **Dirty Bit Modifier (DBM)**. DBM differentiates a non-writable page from a writable-clean page.

DBM only applies to stages of translation using the Direct Permission Scheme. The SMMU uses DBM as follows:

| Descriptor state | DBM | Permissions | Write access behavior |
|-----------------|-----|-------------|----------------------|
| Read-only (non-writable) | `0` | `AP[2:1] == 0b1x` (S1) or `S2AP[1:0] == 0b0x` (S2) | **Permission fault.** DBM==0 means the page has no write intent. Software fault handler invokes error-handling. In stall mode: `CMD_RESUME(Terminate)`. |
| Writable-clean | `1` | `AP[2:1] == 0b1x` (S1) or `S2AP[1:0] == 0b0x` (S2) | Without HTTU: Permission fault. Software sets `AP[2] = 0` or `S2AP[1] = 1` and retries. With HTTU: **SMMU atomically sets `AP[2] = 0` (S1) or `S2AP[1] = 1` (S2)** and allows the write to proceed. No fault. |
| Writable-dirty | `—` | `AP[2] == 0` (S1) or `S2AP[1:0] == 0b1x` (S2) | Write permitted. No fault with or without HTTU. |

Specifically: a writable-clean non-writable descriptor with `DBM == 1` — when a write occurs and HTTU dirty state is enabled — the SMMU atomically sets `AP[2] = 0` at stage 1, or `S2AP[1] = 1` at stage 2, in the descriptor held in memory, then allows the write to proceed.

**SMMU invariants:**
- The SMMU **never sets or clears DBM**.
- The SMMU **never clears `S2AP[1]`**.
- The SMMU **never sets `AP[2]`** — a descriptor is only made writable by the SMMU when `DBM == 1`.
- The SMMU **never sets `S2AP[1] = 1`** for the stage 2 translation used to fetch an L1CD or CD.

Note: Although APTable hierarchical permission removal can restrict write access, DBM-based HTTU only applies to pages made non-writable due to page/block AP/S2AP. If APTable removes write access, `DBM` does not override it.

## §3.13.3.2 Dirty State Hardware Update — Indirect Permission Scheme (IPS)

When the **Indirect Permission Scheme** is used:
- For **stage 1** Base permissions: `CD.HD` exclusively defines whether dirty state is managed by hardware (`CD.HD == 1`) or software (`CD.HD == 0`).
- For **stage 2** Base permissions: `STE.S2HD` exclusively defines whether dirty state is managed by hardware (`STE.S2HD == 1`) or software (`STE.S2HD == 0`).

When IPS is in use, there is no DBM field in Block or Page descriptors.

## §3.13.4 HTTU Behavior Summary

SMMU HTTU operation behaves identically to the Armv8.9-A Hardware Updates to Access Flag and dirty state (FEAT_HAFDBS), with the following SMMU-specific rules:

1. **ATOS visibility:** A descriptor update caused by a completed ATOS translation is made visible to the required Shareability domain (as specified by the translation table walk attributes) by completion of a `CMD_SYNC` submitted **after** the ATOS translation began.

2. **Incoming transaction visibility:** A descriptor update caused by a completed incoming transaction is made visible to the required Shareability domain by completion of a `CMD_SYNC` submitted **after** the completion of the incoming transaction.

3. **TLB invalidation completion visibility:** Completion of a TLB invalidation operation makes descriptor updates visible if those updates were caused by transactions that are themselves completed by the TLB invalidation completion. Both broadcast and explicit `CMD_TLBI_*` invalidations have this property.

4. **Speculative stage 2 dirty update exception:** If stage 2 hardware update of dirty state is enabled, the SMMU is **permitted** to speculatively update the dirty state of a stage 2 descriptor used for a **stage 1 translation table walk**, even if stage 1 HTTU is disabled. (Note: In the A-profile architecture, this is only permitted if stage 1 HTTU is enabled — the SMMU relaxes this restriction.)

## §3.13.5 HTTU with Two Stages of Translation

When both stage 1 and stage 2 are enabled, multiple descriptors may be updated in a single access:
- The **stage 1 descriptor** (leaf) for the translation output.
- The **stage 2 descriptors** mapping each step of the stage 1 walk (the walk IPA addresses).
- The **stage 2 descriptor** mapping the final IPA output from stage 1.

**Critical constraint:** Because a stage 1 descriptor HTTU update is a **write** to the stage 1 translation table (which lives at an IPA), the stage 2 mapping for the stage 1 table address must **allow writes** for the HTTU update to succeed. If the stage 2 mapping is read-only, the HTTU write for stage 1 fails.

See Figure 3.9 in the spec (image-only) for an example procedure of nested translation walk with HTTU enabled.

## §3.13.6 Access Flag in Table Descriptors (HAFT)

An SMMU supporting HTTU may also support hardware update of the Access flag in **Table** (non-leaf) descriptors. This is indicated by `SMMU_IDR0.HTTU` and controlled per-stage:

| Stage | Control |
|-------|---------|
| Stage 1 | `CD.HAFT` |
| Stage 2 | `STE.S2HAFT` |

- If HTTU (Access flag update) is disabled for a stage, HAFT is also disabled for that stage.
- If `HAFT` is enabled, any Table entry with Access flag clear is **not permitted** to be cached in a TLB.
- Hardware updates of Access flag in Table descriptors are made observable by `CMD_SYNC` in the same manner as leaf descriptor AF updates (§3.13.4).
- Hardware updates of Access flag in Table descriptors are triggered by ATS Translation Requests in the same manner as leaf descriptor AF updates (§3.13.7).

## §3.13.7 ATS, PRI and Translation Table Flag Update

When ATS and PRI are used with dynamically paged memory, the Access flag and dirty state must be maintained. HTTU is performed **at the time of the ATS Translation Request (TR)**.

### §3.13.7.1 Hardware Flag Update for ATS & PRI

When an ATS TR is received, the SMMU assumes the device will subsequently access the page. The following HTTU rules apply:

**Access flag:** If the page is otherwise valid and an ATS response will be returned, `AF` is set to 1 in the same way as for a direct transaction.

**Dirty state (NW == 0, write request):**
- If hardware dirty state update is enabled (`CD.HD == 1` or `STE.S2HD == 1`), and the ATS TR is for write access (`NW == 0`), and the page is **writable-clean**: the SMMU **marks the page as writable-dirty before returning the ATS response**. The modification to page data by the device is not visible before the page state is visible as writable-dirty.
- If HTTU is only enabled for Access (not dirty state): an ATS write request to a writable-clean or read-only page results in an ATS Translation Completion with `W == 0` (write access denied).

**Write request to read-only nested stage 1:** If the ATS TR is for a write and the stage 1 translation is read-only (not writable), the dirty state is not updated in either the stage 1 descriptor or the stage 2 descriptor mapping the stage 1 output address (even if that stage 2 descriptor is writable-clean).

**Split-stage ATS (STE.EATS == 0b10):** When HTTU is enabled for both stage 1 and stage 2:
- The ATS TR performs HTTU at stage 1, and updates stage 2 descriptors used to fetch and update the stage 1 descriptors.
- For the stage 2 descriptor for the final IPA:
  - The AF is **permitted** to be speculatively set to 1 by the ATS TR.
  - If write permission is granted, a writable-clean stage 2 descriptor for the final IPA is **permitted** to be marked writable-dirty by the ATS TR.
  - A subsequent Translated access to the IPA continues normal HTTU behavior regardless of whether the ATS TR already performed the update.

### §3.13.7.2 ATS & PRI Behavior Without HTTU

When HTTU is **not** enabled for the Access flag:
- An ATS TR to a page with `AF == 0` and `AFFD == 0` is **denied** — the SMMU returns an ATS response with `R == W == 0` (no access).
- The client device may raise an error or issue a **PRI page request**, if configured. Software can manually set `AF = 1` on receipt of the PRI request in anticipation of the device access.

When HTTU is **not** enabled for dirty state:
- An ATS TR to a read-only page does not grant write access (`W == 0`). Read access may be granted if AF conditions are met.
- The client device may raise an error or issue a PRI page request for write access. On receipt, software could assume data will be written shortly and mark the page writable-dirty before responding.
- An ATS TR to a writable page may grant write access (`W == 1`). Software must consider writable pages as **potentially dirty**.

**Note on speculative PRI:** PCIe PRI requests can be issued speculatively by an endpoint, implying speculatively marking a page dirty. Arm recommends that general-purpose systems support HTTU when PRI is used, so dirty state is only marked when an actual ATS write request occurs.

## §3.13.8 HTTU for CMO and Destructive Reads

**HTTU Dirty state update is NOT performed** for the following operations:
- Invalidate Cache Maintenance Operations (CMOs)
- Destructive Reads (DR)
- Destructive Hints (DH)

When these operations are performed to a **writable-clean** translation table descriptor, the descriptor is **not** updated to writable-dirty. If the required Read or Execute permissions are available but the descriptor is not writable-dirty, the operations are **downgraded** as described in §16.7.2.2 (CMO permissions) and §3.22.2 (Destructive Read permissions).

**HTTU of the Access flag is unaffected** — AF update still occurs if required. When HTTU is enabled, a stage 2 descriptor for a stage 1 table may still have its dirty state updated for these operations (the stage 2 update for fetching the stage 1 table still occurs).

**Execute permission note:** For determining execute permission, a writable-clean descriptor is considered writable when HTTU is enabled (consistent with Armv8-A). This applies even when the descriptor is not updated to writable-dirty for Invalidate CMO, DR, or DH operations.

## Access Flag Update

When the Access flag (AF) in a translation table descriptor is 0 and HTTU is enabled (`SMMU_IDR0.HTTU >= 0b01`):
- Without HTTU: an F_ACCESS fault occurs when AF=0 is encountered during a walk.
- With HTTU: the SMMU atomically sets AF=1 in the descriptor (via a hardware read-modify-write to memory) and continues the translation without a fault.

HTTU for the Access flag is enabled per-CD via `CD.HA = 1` (stage 1) and per-STE via `STE.S2HA = 1` (stage 2).

## Dirty State (DBM) Update

When HTTU for dirty state is supported (`SMMU_IDR0.HTTU == 0b10`):
- A write to a page whose Dirty Bit Modifier (DBM/AP) indicates the page is "clean" and hardware dirty-state tracking is enabled: the SMMU atomically marks the page as "dirty" by updating the descriptor.
- This avoids a fault-and-fixup cycle for the first write to a clean page.
- Enabled per-CD via `CD.HD = 1` (stage 1) and per-STE via `STE.S2HD = 1` (stage 2).

## Related Concepts

- [two-stage-translation.md](two-stage-translation.md) — HTTU interacts differently at stage 1 and stage 2
- [fault-models.md](fault-models.md) — F_ACCESS is the fault raised when AF=0 and HTTU is not enabled
- [tlb-invalidation.md](tlb-invalidation.md) — TLB invalidation required after software updates AF/dirty
- [destructive-reads.md](destructive-reads.md) — §3.13.8: HTTU dirty state update is NOT performed for CMO, DR, or DH transactions; AF update still occurs

## Sources That Use This Concept

- [../sources/ihi0070g-b-smmuv3-architecture-spec.md](../sources/ihi0070g-b-smmuv3-architecture-spec.md) — §3.13 Translation tables and Access flag/Dirty state; §3.13.1–3.13.8; §2.2 SMMUv3.0 features (HTTU); §2.8 SMMUv3.4 features (HAFT)
