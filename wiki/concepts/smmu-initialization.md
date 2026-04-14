---
title: "SMMU Initialization"
type: concept
tags: [smmu, initialization, reset, enable, configuration, software, smmu-s-init, smmuen, cr0ack, realm, gpcen]
created: 2026-04-07
updated: 2026-04-14
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# SMMU Initialization

## Definition

SMMU initialization is the software sequence required to bring an SMMU from reset state to a fully operational translation state. The initialization sequence must be followed precisely to avoid undefined behavior; incorrect ordering can result in stale TLB entries, transaction faults, or data corruption.

## Reset State

At reset, the SMMU is in a disabled state:
- `SMMU_CR0.SMMUEN == 0`: all Non-secure traffic bypasses without translation.
- `SMMU_GBPA.ABORT` determines whether bypass transactions are passed through (with GBPA attributes) or aborted.
- The state of TLBs and configuration caches at reset is IMPLEMENTATION SPECIFIC.
- Reset-to-bypass allows legacy software without SMMU awareness to function.
- Reset-to-abort (if `GBPA.ABORT == 1`) blocks all traffic until software enables and configures the SMMU.

## Recommended Initialization Sequence (NS)

Arm recommends the following steps (§3.11):

1. **Allocate and initialize Stream table memory.** Write base address and format to `SMMU_STRTAB_BASE` and `SMMU_STRTAB_BASE_CFG`. Populate STEs for all expected streams.
2. **Allocate and initialize Command queue.** Write base and size to `SMMU_CMDQ_BASE`. Initialize `SMMU_CMDQ_PROD` and `SMMU_CMDQ_CONS` to a consistent empty state (both 0).
3. **Allocate and initialize Event queue.** Write to `SMMU_EVENTQ_BASE`. Initialize `SMMU_EVENTQ_PROD` and `SMMU_EVENTQ_CONS` to empty state.
4. **Enable queue processing.** Set `SMMU_CR0.CMDQEN = 1` (and `SMMU_CR0.EVENTQEN = 1`). Wait for `SMMU_CR0ACK.CMDQEN` to reflect the change.
5. **Invalidate all caches.** Issue `CMD_CFGI_ALL` and `CMD_TLBI_NSNH_ALL` (and `CMD_TLBI_EL2_ALL` if hypervisor mode is used). Follow with `CMD_SYNC` and wait for completion.
6. **Enable translation.** Set `SMMU_CR0.SMMUEN = 1`. Wait for `SMMU_CR0ACK.SMMUEN`.

**Pre-conditions before setting SMMUEN:**
- `SMMU_STRTAB_BASE` and `SMMU_CR1` table attributes must be configured first to avoid incoming traffic attempting a lookup through uninitialized pointers.

## §3.11 SMMU_S_INIT — Secure Initialization Shortcut

When `SMMU_S_IDR1.SECURE_IMPL == 1`:
- Secure software must fully initialize the Secure interface before handover to Non-secure software.
- Same sequence as Non-secure but using `SMMU_S_*` registers.
- **`SMMU_S_INIT.INV_ALL`** provides a shortcut — invalidates all SMMU caches and TLBs (all configuration and translation caches, all translation regimes and Security states) **without using the Command queue**.

**`SMMU_S_INIT.INV_ALL` procedure:**
1. Write `SMMU_S_INIT.INV_ALL = 1`.
2. Poll `SMMU_S_INIT.INV_ALL` until it reads as 0 → invalidation complete.

**Behavior details:**
- Write of 1 causes a global invalidation of all cache/TLB entries present before the write. When the invalidation completes, `INV_ALL` resets to 0.
- If an invalidation was already underway before the write, the existing operation is completed and then `INV_ALL` resets to 0.
- Invalidation completion is **not** required to wait for outstanding transactions to complete.
- Affects **locked** configuration and translation cache entries (if implementation supports locking).
- Also invalidates any **GPT information** cached in TLBs (GPT may be cached in TLBs).

**CONSTRAINED UNPREDICTABLE conditions for INV_ALL:**
- Writing INV_ALL=1 when any `SMMU_(*_)CR0.SMMUEN == 1`, or an update of any `SMMUEN` to 1 is in progress, or `SMMU_ROOT_CR0.ACCESSEN == 1`, or an update of ACCESSEN to 1 is in progress: CONSTRAINED UNPREDICTABLE — the write is either IGNORED or the invalidation occurs and completes normally.
- An update of `SMMUEN` to 1 while an `INV_ALL` operation is underway: CONSTRAINED UNPREDICTABLE effect on the invalidation.

**Realm/RME constraint:** If `SMMU_ROOT_IDR0.REALM_IMPL == 1`, then `SMMU_S_INIT.INV_ALL` has **no effect** if `SMMU_ROOT_CR0.GPCEN == 1`. Root firmware is responsible for writing INV_ALL **before** enabling granule protection checks.

**Non-secure access to SMMU_S_INIT:**
- `SMMU_S_INIT` is normally Secure-only. Exception: the system may provide an IMPLEMENTATION DEFINED mechanism allowing Non-secure software to access it at reset.
- Arm expects Secure software to **disable** this Non-secure access mechanism after SMMU initialization.
- Arm **strongly recommends** that `SMMU_S_INIT` be exposed for Non-secure initialization software when `SMMU_S_IDR1.SECURE_IMPL == 1` but no Secure software exists.
- Non-secure software **must not rely** on `SMMU_S_INIT` access — it is not guaranteed. Non-secure initialization must use `CMD_TLBI_EL2_ALL` and `CMD_TLBI_NSNH_ALL` commands instead.

## Realm State Initialization

When `SMMU_ROOT_IDR0.REALM_IMPL == 1`:
- The Realm SMMU interface (`SMMU_R_*` registers) is present.
- Realm initialization sequence is the same as Non-secure/Secure but uses `SMMU_R_*` registers.
- **No separate `SMMU_R_INIT`** register exists — the equivalent of `SMMU_S_INIT.INV_ALL` for the Realm state is performed by Root firmware before enabling GPCs (`SMMU_ROOT_CR0.GPCEN`).
- `SMMU_R_CR0.ATSCHK` is RES1 — ATS checking is always required for Realm streams.
- `SMMU_R_GBPA.ABORT` is RES1 — non-translated Realm traffic is always aborted (no bypass).
- Realm streams use `SMMU_R_CR0.SMMUEN` and `SMMU_R_CR0ACK.SMMUEN` for the same enable/ACK handshake.

## SMMU_S_INIT vs SMMU_S_CR0 SMMUEN/CR0ACK Ordering

`SMMU_S_INIT.INV_ALL` must only be used when `SMMU_S_CR0.SMMUEN == 0` (or in other permitted CONSTRAINED UNPREDICTABLE conditions described above). The typical Secure init flow:
1. Write `SMMU_S_INIT.INV_ALL = 1`.
2. Poll until `INV_ALL == 0`.
3. Configure structures (`SMMU_S_STRTAB_BASE`, queues, etc.).
4. Enable: `SMMU_S_CR0.SMMUEN = 1`, poll `SMMU_S_CR0ACK.SMMUEN`.

## SMMU_CR0ACK — Update Procedure

Changes to `SMMU_CR0` are reflected in `SMMU_CR0ACK` after an IMPLEMENTATION DEFINED delay. Software must poll `SMMU_CR0ACK` for the expected values before assuming the change is in effect. The same pattern applies to `SMMU_S_CR0`/`SMMU_S_CR0ACK` and `SMMU_R_CR0`/`SMMU_R_CR0ACK`.

## Queue Initialization — Consistent States

Queue indexes must be initialized to one of these consistent states before enabling:
- `PROD.WR == CONS.RD` and `PROD.WR_WRAP == CONS.RD_WRAP` → empty (normal initialization)
- `PROD.WR == CONS.RD` and `PROD.WR_WRAP != CONS.RD_WRAP` → full
- `PROD.WR > CONS.RD` and same wrap → partially full
- `PROD.WR < CONS.RD` and different wrap → partially full

Do not write inconsistent states.

## Bypass Configuration (GBPA / S_GBPA)

When translation is disabled:
- `SMMU_GBPA` configures bypass attributes for Non-secure traffic.
- `SMMU_AGBPA` provides alternative bypass attributes.
- `SMMU_S_GBPA` configures bypass attributes for Secure traffic.
- Setting `GBPA.ABORT = 1` aborts all traffic for that Security state when translation is disabled.

## TLB State at Enable

The SMMU is not required to invalidate caches when `SMMUEN` changes. Therefore:
- Software **must** invalidate before setting `SMMUEN = 1` (step 5 above).
- If the SMMU creates TLB entries during bypass (`SMMUEN == 0`), these are not visible to software and the SMMU does not require explicit invalidation of them on the 0→1 transition.

## Model Implementation Notes

- A functional model must implement the `SMMUEN` / `CR0ACK` handshake. Changes to `SMMUEN` should be treated as taking effect immediately (or after the ACK cycle, depending on model latency requirements).
- Before `SMMUEN = 1`, all translation requests should return bypass (or abort per GBPA).
- The initialization sequence defines the only safe ordering for configuration; a model should enforce these pre-conditions or document behavior when they are violated.
- Queue initialization (step 2–3) must occur before `CMDQEN = 1`; the model must not process commands with uninitialized queue pointers.

## Related Concepts

- [concepts/command-queue.md](concepts/command-queue.md) — must be initialized and enabled before issuing invalidation commands
- [concepts/event-queue.md](concepts/event-queue.md) — must be initialized before enabling translation
- [concepts/tlb-invalidation.md](concepts/tlb-invalidation.md) — invalidation commands required before SMMUEN=1
- [concepts/security-states.md](concepts/security-states.md) — each security state has independent initialization sequence
- [concepts/stream-table-entry.md](concepts/stream-table-entry.md) — stream table must be populated before enabling translation

## Sources That Use This Concept

- [sources/ihi0070g-b-smmuv3-architecture-spec.md](sources/ihi0070g-b-smmuv3-architecture-spec.md) — §3.11 Reset, Enable and initialization; §6.3.9 SMMU_CR0; §6.3.10 SMMU_CR0ACK; §6.3.62 SMMU_S_INIT
