---
title: "Granule Protection Check (GPC)"
type: concept
tags: [smmu, gpc, gpt, rme, realm, security, physical-address, gpf, gpt-lookup-error, nostreamid, speculative]
created: 2026-04-07
updated: 2026-04-14
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Granule Protection Check (GPC)

## Definition

A Granule Protection Check (GPC) is a check performed by the SMMU on the output physical address of a transaction against the Granule Protection Table (GPT). The GPT is an in-memory structure that maps physical address granules to a PA space (Realm, Secure, Non-secure). A GPC fault occurs when:

- The GPT lookup could not be completed (access fault), OR
- The lookup succeeded but the access PA space does not match the granule's assigned PA space.

A **GPF (Granule Protection Fault)** is the specific fault type when the lookup succeeds but the check fails. The broader term **GPC fault** covers both access failures and GPF.

GPCs are part of the RME (Realm Management Extension) and require `SMMU_ROOT_IDR0.ROOT_IMPL == 1`.

## Overview

Granule Protection Checks (GPC) are enabled only when `SMMU_ROOT_CR0.GPCEN == 1`. All GPC behavior described below applies only when GPC is enabled.

The GPT format, invalidation, and synchronization mechanisms are the same as in the Armv8 A-profile `FEAT_RME` architecture. The SMMU performs these checks for non-PE requesters.

GPT is configured via Root-state registers:
- `SMMU_ROOT_GPT_BASE` — base physical address of the GPT.
- `SMMU_ROOT_GPT_BASE_CFG` — configuration (PPS, SH, IRGN, ORGN).

## §3.25.1 Client-Originated Accesses

All accesses to physical addresses (except GPT fetch addresses) are subject to GPC.

- A client-originated access experiencing a GPC fault is signaled to the device as an **External abort**.
- A client-originated access experiencing a GPC fault on the output address is **not reported in the Event queue**.

### §3.25.1.1 GPC for NoStreamID Devices

GPC applies to accesses from client devices that are **not associated with a StreamID** (NoStreamID devices):
- NoStreamID devices access PA space directly; they are not associated with any stage 1 or stage 2 translation configuration.
- GPC fault reporting for NoStreamID accesses is the same as for regular client-originated accesses.
- NoStreamID devices are not associated with a SEC_SID value.
- Transactions from NoStreamID devices include both a physical address and a PA space.
- An access from a NoStreamID device with a physical address that exceeds the OAS (`SMMU_IDR5.OAS`) is terminated with an abort; no Event record or fault is recorded.
- The SMMU does not perform architectural transformations or overrides on NoStreamID accesses, but may apply protocol-specific normalization on transaction attributes.

### §3.25.1.2 Speculative and Hint Accesses

GPC faults encountered during speculative translation requests, translation of transactions marked as speculative, prefetch commands, or for NW-DCP or DH transactions:

- **No event record is generated** in any case.
- If `SMMU_IDR0.RME_IMPL == 0`: it is CONSTRAINED UNPREDICTABLE whether the fault is reported. If reported, it is reported in `SMMU_ROOT_GPF_FAR` or `SMMU_ROOT_GPT_CFG_FAR` (if not already containing an active fault).
- If `SMMU_IDR0.RME_IMPL == 1`: the GPC fault is **not reported**.

For speculative Translation Requests:
- If `SMMU_IDR0.RME_IMPL == 0`: CONSTRAINED UNPREDICTABLE whether GPC on the output address is applied at translation time or only when a transaction using the translation is issued.
- If `SMMU_IDR0.RME_IMPL == 1`: GPC on the output address is applied **at the time of the translation**.

## §3.25.2 Interactions with PCIe ATS

- All PCIe client transactions are subject to GPC. `SMMU_CR0.ATSCHK` has no effect on GPC.
- If an SMMU-originated access experiences a GPC fault while servicing an ATS Translation Request, the SMMU responds with **Completer Abort**.
- If an ATS Translation Request completes with Success and `R == W == 0`, the address is not valid and is not subject to GPC.
- If `SMMU_IDR0.RME_IMPL == 1`: the SMMU performs GPC on the output address before sending the Translation Completion. The SMMU returns a translation region size such that GPC passes for accesses anywhere in the region.
- If `SMMU_IDR0.RME_IMPL == 0`: the SMMU is permitted but not required to perform GPC on the ATS TR output. If the output fails GPC, the SMMU responds with Completer Abort.
- ATS Translated transactions are subject to GPC. An ATS Translated transaction failing GPC is terminated with abort.

## §3.25.3 SMMU-Originated Accesses

An SMMU-originated access experiencing a GPC fault is reported as if it experienced an **External abort**.

GPC fault reporting for structure-fetch events (`F_STE_FETCH`, `F_CD_FETCH`, `F_VMS_FETCH`, `F_WALK_EABT`) is not affected by `CD.{A, R, S}` nor `STE.{S2S, S2R}` bits.

When `SMMU_IDR0.RME_IMPL == 1`:
- Each of F_STE_FETCH, F_CD_FETCH, F_VMS_FETCH, F_WALK_EABT event records has a `GPCF` field at bit 80.
- `GPCF = 1` indicates the event arose from a GPC fault.
- `GPCF = 0` indicates the event arose for another reason.
- Example: If the SMMU experiences a GPC fault accessing an STE, it is reported as F_STE_FETCH with `GPCF = 1`, and the client transaction is aborted.
- Example: A GPC fault accessing the Non-secure Event queue is reported via `SMMU_GERROR.EVENTQ_ABT_ERR`.

## §3.25.4 Reporting of GPC Faults

GPC faults fall into three categories:

**1. Granule Protection Fault (GPF)** — reported in `SMMU_ROOT_GPF_FAR`:
- Access to a non-Non-secure PA space with a physical address exceeding `SMMU_ROOT_GPT_BASE_CFG.PPS`.
- Access to a location forbidden by the GPT configuration.

**2. GPT Lookup Error** — reported in `SMMU_ROOT_GPT_CFG_FAR`:
- Reserved values in `SMMU_ROOT_GPT_BASE_CFG`.
- `SMMU_ROOT_GPT_BASE_CFG.PPS` configured to exceed `SMMU_IDR5.OAS`.
- Invalid `{SH, IRGN, ORGN}` combination.
- `SMMU_ROOT_GPT_BASE.ADDR` exceeds `SMMU_ROOT_GPT_BASE_CFG.PPS`.
- GPT Table Entry output address exceeds `SMMU_ROOT_GPT_BASE_CFG.PPS`.
- Invalid GPT entry used by the SMMU.
- External abort fetching a GPT entry.
- RAS error fetching a GPT entry (reported as external abort).

**3. RAS errors** — reported via RAS registers.

Two edge-triggered wired interrupts exist for GPC:
| Source | Trigger |
|--------|---------|
| `GPF_FAR` | An error becomes active in `SMMU_ROOT_GPF_FAR`. |
| `GPT_CFG_FAR` | An error becomes active in `SMMU_ROOT_GPT_CFG_FAR`. |

## §3.25.5 SMMU Behavior If a GPC Fault Is Active

**When a GPF is reported in `SMMU_ROOT_GPF_FAR`:**
- If no prior GPF in `SMMU_ROOT_GPF_FAR`: syndrome information is recorded.
- Other accesses not experiencing a GPF or GPT lookup error continue as specified.
- The GPF remains active until software writes 0 to `SMMU_ROOT_GPF_FAR.FAULT`.

**When a GPT lookup error is reported in `SMMU_ROOT_GPT_CFG_FAR`:**
- If no prior GPT lookup error: syndrome is recorded.
- Other accesses continue.
- The error remains until software writes 0 to `SMMU_ROOT_GPT_CFG_FAR.FAULT`.

Note: Multiple faults/errors may occur before software clears the registers. First-occurrence semantics: a register is only updated when `FAULT == 0`.

## §3.25.6 Observability of GPC Faults

If termination of a client transaction due to GPC fault is **observable to the client**:
- If `SMMU_ROOT_GPF_FAR` or `SMMU_ROOT_GPT_CFG_FAR` already had an active fault → register is not updated.
- If no active fault → syndrome information becomes observable in the appropriate register.

If an interrupt indicating a GPC fault is observable: syndrome is observable in the appropriate FAR register.

If a client transaction's GPC-fault termination has been observable: completion of a subsequent `CMD_SYNC` guarantees observability of any related events in the Event queue (or guarantees the event will not become observable if the queue was unwritable).

**BGPTM / RGPTM — broadcast/register-based GPT maintenance visibility:**

For `SMMU_ROOT_IDR0.BGPTM == 1`:
- After completion of broadcast `TLBI *PA*` + DSB, a subsequent `CMD_SYNC` guarantees no Events for invalidated GPT entries will appear in the Event queue.
- After broadcast `TLBI *PA*`, completion of a DSB guarantees any errors in `SMMU_ROOT_GPF_FAR` / `SMMU_ROOT_GPT_CFG_FAR` relating to invalidated entries are already observable.

For `SMMU_ROOT_IDR0.RGPTM == 1`:
- After completion of register-based TLBI by PA (indicated by `SMMU_ROOT_TLBI_CTRL.RUN`), a subsequent `CMD_SYNC` guarantees no Events for invalidated entries appear in the Event queue.
- Completion of register-based TLBI by PA guarantees errors in FAR registers for invalidated entries are already observable.

**F_STE_FETCH/F_CD_FETCH/F_VMS_FETCH/F_WALK_EABT with `GPCF == 1` observability:**
- If the appropriate FAR already had an active fault → register not updated.
- If no active fault → syndrome observable in the register.
- If `SMMU_(*_)GERROR` update arising from GPC is observable → same FAR update rules apply.
- If a fault is observable in `SMMU_(*_)GERROR`, it will also appear in `SMMU_(*_)GERROR` in finite time.

## DPT vs GPT

The Device Permission Table (DPT) ([[concepts/device-permission-table]]) is a separate RME DA feature that gates device DMA access at per-device PA-space granularity. GPT operates at the physical memory side for all agents. In ATS with DPT (`STE.EATS == 0b11`), DPT check is applied first; GPC is then applied to the output PA.

## Model Implementation Notes

- A functional model must implement GPT lookups as part of the translation pipeline output stage. GPC is the final check before a transaction PA is forwarded.
- GPT lookups may be cached; invalidation via `CMD_TLBI *PA*` or register-based TLBI is required.
- GPC faults always terminate the transaction — no stall model applies to GPC faults.
- The two FAR registers are first-occurrence sticky; model must implement first-fault semantics (only update when FAULT == 0).
- NoStreamID devices bypass STE/CD lookup but are still subject to GPC.
- Speculative transactions must not generate event records for GPC faults (when `RME_IMPL == 1`).

## Related Concepts

- [[concepts/security-states]] — GPCs apply to Realm and all states where RME is implemented
- [[concepts/two-stage-translation]] — GPC is applied to the PA output of translation
- [[concepts/device-permission-table]] — DPT is a complementary per-device PA-space check
- [[concepts/pcie-ats-pri]] — Translated transactions from Realm streams subject to GPC

## Sources That Use This Concept

- [[sources/ihi0070g-b-smmuv3-architecture-spec]] — §3.25 Granule Protection Checks; §3.10.3 Support for Realm state; §2.6 SMMU for RME features; §6.3.110 SMMU_ROOT_IDR0
