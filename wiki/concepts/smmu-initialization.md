---
title: "SMMU Initialization"
type: concept
tags: [smmu, initialization, reset, enable, configuration, software]
created: 2026-04-07
updated: 2026-04-07
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

## Secure State Initialization

When `SMMU_S_IDR1.SECURE_IMPL == 1`:
- Secure software must fully initialize the Secure interface before handover to Non-secure software.
- Same sequence as Non-secure but using `SMMU_S_*` registers.
- `SMMU_S_INIT.INV_ALL` provides a shortcut: write 1, poll until 0 to invalidate all SMMU caches without using the Command queue. Arm expects this to be used by Secure software.
- Non-secure software cannot rely on `SMMU_S_INIT` access; it may be available from reset (IMPLEMENTATION DEFINED) but may be revoked by Secure software.
- Non-secure invalidation must use `CMD_TLBI_EL2_ALL` and `CMD_TLBI_NSNH_ALL` commands.

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

- [[concepts/command-queue]] — must be initialized and enabled before issuing invalidation commands
- [[concepts/event-queue]] — must be initialized before enabling translation
- [[concepts/tlb-invalidation]] — invalidation commands required before SMMUEN=1
- [[concepts/security-states]] — each security state has independent initialization sequence
- [[concepts/stream-table-entry]] — stream table must be populated before enabling translation

## Sources That Use This Concept

- [[sources/ihi0070g-b-smmuv3-architecture-spec]] — §3.11 Reset, Enable and initialization; §6.3.9 SMMU_CR0; §6.3.10 SMMU_CR0ACK; §6.3.62 SMMU_S_INIT
