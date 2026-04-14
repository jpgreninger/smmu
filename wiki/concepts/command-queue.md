---
title: "Command Queue"
type: concept
tags: [smmu, command-queue, circular-buffer, software-interface, commands]
created: 2026-04-07
updated: 2026-04-07
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Command Queue

## Definition

The Command queue is the software-to-SMMU interface for issuing management commands (TLB invalidations, configuration invalidations, fault resumption, synchronization). It is a memory-based circular buffer (FIFO) where software is the producer and the SMMU is the consumer. Commands are consumed in-order within a single queue.

There is one Command queue per implemented Security state:
- Non-secure: `SMMU_CMDQ_*` registers.
- Secure: `SMMU_S_CMDQ_*` registers (present when `SMMU_S_IDR1.SECURE_IMPL == 1`).
- Realm: `SMMU_R_CMDQ_*` registers.

## Circular Buffer Mechanics

The queue is `2^n` entries in size, `0 <= n <= 19`. Base address and size are configured via `SMMU_(*_)CMDQ_BASE`. Producer/consumer state is maintained in `SMMU_(*_)CMDQ_PROD` (software-written) and `SMMU_(*_)CMDQ_CONS` (SMMU-updated).

Each index register is 20 bits: a `(n-bit)` index plus a 1-bit wrap flag in the adjacent higher bit.

**Empty/full detection using wrap bits:**
- `PROD.WR == CONS.RD` and `PROD.WR_WRAP == CONS.RD_WRAP` → **empty**
- `PROD.WR == CONS.RD` and `PROD.WR_WRAP != CONS.RD_WRAP` → **full**
- Any other state where indexes or wrap bits differ → **partially full**

**Inconsistent states (CONSTRAINED UNPREDICTABLE behavior):**
- `PROD.WR > CONS.RD` and wrap bits differ
- `PROD.WR < CONS.RD` and wrap bits equal

Permitted behaviors on inconsistent state: SMMU may consume entries at UNKNOWN locations, or stop consuming, or treat queue as full.

**Index update rule:** Producers and consumers must only increment the index (except wrapping from top to bottom); the index must never be moved backward. The wrap bit must be toggled atomically with the index when a wrap occurs. Software must read the register, compute the new value (toggling wrap if required), and write both fields simultaneously.

## Queue Entry Visibility Semantics

- Software (producer) must ensure all new queue entries are observable before updating PROD.WR.
- The SMMU must not observe the PROD index update before the entries it covers are visible.
- The SMMU consumes commands from the queue in finite time after PROD is updated.

## Command Categories

| Category | Commands |
|----------|----------|
| Prefetch | `CMD_PREFETCH_CONFIG`, `CMD_PREFETCH_ADDR` |
| Config invalidation | `CMD_CFGI_STE`, `CMD_CFGI_STE_RANGE`, `CMD_CFGI_CD`, `CMD_CFGI_CD_ALL`, `CMD_CFGI_VMS_PIDM`, `CMD_CFGI_ALL` |
| TLB invalidation — Stage 1 | `CMD_TLBI_NH_ALL`, `CMD_TLBI_NH_ASID`, `CMD_TLBI_NH_VAA`, `CMD_TLBI_NH_VA`, `CMD_TLBI_EL2_ALL`, `CMD_TLBI_EL2_ASID`, `CMD_TLBI_EL2_VAA`, `CMD_TLBI_EL2_VA`, `CMD_TLBI_S12_VMALL`, etc. |
| TLB invalidation — Stage 2 | `CMD_TLBI_S2_IPA`, `CMD_TLBI_NSNH_ALL` |
| TLB invalidation — Common | `CMD_TLBI_EL3_ALL`, `CMD_TLBI_EL3_VA` |
| ATS/PRI | `CMD_ATC_INV`, `CMD_PRI_RESP` |
| DPT maintenance | `CMD_DPTI_ALL`, `CMD_DPTI_PA` |
| Fault/sync | `CMD_RESUME`, `CMD_STALL_TERM`, `CMD_SYNC` |

## CMD_SYNC — Synchronization

`CMD_SYNC` is the critical synchronization barrier. When consumed, it guarantees that all effects of previously-consumed commands on the same queue are complete, including:
- Visibility of events relating to configuration or TLB invalidation performed by prior commands.
- Completion of any `CMD_ATC_INV` operations (ATS invalidation).

`CMD_SYNC` can signal completion via:
- A completion signal write to a register.
- An MSI write to a configured address.

Note: A `CMD_SYNC` on the main `SMMU_(*_)CMDQ` does not synchronize ECMDQ queues.

## Enhanced Command Queue (ECMDQ)

SMMUs may implement multiple Command queues per security state (Enhanced Command queue interface):
- Up to 256 Command queue control pages, each with up to 256 ECMDQs.
- Discovered via `SMMU_IDR1.ECMDQ` / `SMMU_IDR6.CMDQ_CONTROL_PAGE_LOG2NUMP`.
- Each ECMDQ has its own `BASE`, `PROD`, `CONS` registers (16 bytes each within the control page).
- Commands may be consumed in parallel across queues; no guaranteed total order between queues.
- `CMD_SYNC` in an ECMDQ synchronizes only commands previously consumed on that ECMDQ.
- If `SMMU_IDR0.SEV == 1`, the SMMU triggers a WFE wake-up event when any ECMDQ becomes non-full.

## Initialization and Enabling

A queue is enabled when `SMMU_(*_)CR0.CMDQEN` is set. Before enabling:
1. Allocate queue memory and write base address to `SMMU_(*_)CMDQ_BASE`.
2. Initialize `PROD` and `CONS` to a consistent state (typically both zero, indicating empty queue).
3. Set `SMMU_(*_)CR0.CMDQEN == 1`.

## Model Implementation Notes

- A functional model must implement the full mirrored circular buffer with wrap-bit semantics. The index is `n` bits wide; the wrap bit is bit `[n]` of the PROD/CONS registers. Bits `[19:n+1]` are ignored.
- The SMMU must not consume commands before CMDQEN is set.
- Command consumption is in-order within a queue; commands across different queues (ECMDQ) have no ordering guarantee.
- `CMD_SYNC` must only complete after all prior commands on the same queue have fully taken effect (including memory-visible event queue updates for any resulting faults).
- A command error (e.g., unknown opcode, illegal parameter) halts the queue at the error point and sets the error status in `SMMU_(*_)CMDQ_CONS.ERR`. Queue processing does not resume until the error is cleared by software.

## Related Concepts

- [[concepts/event-queue]] — output counterpart; SMMU-to-software
- [[concepts/tlb-invalidation]] — most common command type
- [[concepts/fault-models]] — `CMD_RESUME` and `CMD_STALL_TERM` interact with stall model
- [[concepts/smmu-initialization]] — command queue setup is part of initialization sequence

## Sources That Use This Concept

- [[sources/ihi0070g-b-smmuv3-architecture-spec]] — §3.5 Command and Event queues; §3.5.1 SMMU circular queues; §3.5.6 Enhanced Command queue; §4 Commands; §4.7.3 CMD_SYNC
