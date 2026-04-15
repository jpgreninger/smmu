---
title: "Command Queue"
type: concept
tags: [smmu, command-queue, circular-buffer, software-interface, commands, ecmdq, cmd-sync, cmd-consumption]
created: 2026-04-07
updated: 2026-04-14
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

## §4.8 Command Consumption Summary

| Command Type | What "consumed" means |
|-------------|----------------------|
| TLB invalidation: `CMD_TLBI_*`, `CMD_ATC_INV` | Consumption provides no guarantee. |
| Configuration invalidation: `CMD_CFGI_*` | Consumption provides no guarantee. |
| Prefetch: `CMD_PREFETCH_*` | Consumption provides no guarantee. |
| PRI responses: `CMD_PRI_RESP` | Consumption provides no guarantee. |
| Stall resume/termination: `CMD_RESUME`, `CMD_STALL_TERM` | Individual completion guarantees for the stall operation have been met. |
| Synchronization: `CMD_SYNC` | All completion guarantees of `CMD_SYNC` have been met (all effects of prior commands on the same queue are complete). |

Note: For all commands except `CMD_RESUME`, `CMD_STALL_TERM`, and `CMD_SYNC`, consumption of the command itself provides no ordering guarantee. Completion guarantees require a subsequent `CMD_SYNC`.

## §3.5.6 Enhanced Command Queue (ECMDQ)

SMMUs may implement multiple Command queues per security state. This is advertised in `SMMU_IDR1.ECMDQ` (Non-secure) and `SMMU_S_IDR0.ECMDQ` (Secure).

**Components:**
- Up to 256 Command queue control pages.
- Each control page contains up to 256 ECMDQ interfaces.
- Presence of Enhanced Command queues does **not** remove the `SMMU_(*_)CMDQ_*` interface.

**ECMDQ register layout per queue (16 bytes per ECMDQ):**

| Offset | Register | Size | Description |
|--------|----------|------|-------------|
| `0x00` | `SMMU_ECMDQ_BASEn` | 64-bit | Queue base address and size |
| `0x08` | `SMMU_ECMDQ_PRODn` | 32-bit | Queue producer write index + `EN` bit + `ERRACK` bit |
| `0x0C` | `SMMU_ECMDQ_CONSn` | 32-bit | Queue consumer read index + `ENACK` bit + `ERR` bit + `ERR_REASON` |

Number of control pages for Non-secure state: `SMMU_IDR6.CMDQ_CONTROL_PAGE_LOG2NUMP`. Secure: `SMMU_S_IDR6.CMDQ_CONTROL_PAGE_LOG2NUMP`.

Control page registers: `SMMU_CMDQ_CONTROL_PAGE_BASEn`, `SMMU_CMDQ_CONTROL_PAGE_CFGn`, `SMMU_CMDQ_CONTROL_PAGE_STATUSn`.

### §3.5.6.1 ECMDQ Behavior

- The SMMU accesses ECMDQ queues using attributes from `SMMU_(*_)CR1.{QUEUE_SH, QUEUE_OC, QUEUE_IC}` and MPAM attributes from `SMMU_(*_)GMPAM`.
- If any ECMDQ is enabled such that `CR1.{QUEUE_SH, QUEUE_OC, QUEUE_IC}` could be used for its accesses → these CR1 fields become **read-only**.
- Empty/full/non-empty semantics are identical to the main Command queue.
- The SMMU consumes from a queue when it is non-empty.
- `CMD_SYNC` consumed from an ECMDQ guarantees effects of all commands previously consumed on **that ECMDQ** are complete, including event reporting for affected configuration/translation entries.
- The main `SMMU_(*_)CMDQ` CMD_SYNC is **independent** of ECMDQ state — a main CMD_SYNC does not synchronize ECMDQ commands.
- The SMMU may consume from multiple queues **in parallel** (round-robin, weighted, or other schedule).
- **No total order** is guaranteed across different queues.
- If `SMMU_IDR0.SEV == 1`: SMMU triggers a WFE wake-up event when any ECMDQ becomes non-full.

### §3.5.6.2 ECMDQ Enable/Disable

**Enabled** when: `SMMU_ECMDQ_PRODn.EN == SMMU_ECMDQ_CONSn.ENACK == 1`.  
**Disabled** when: `SMMU_ECMDQ_PRODn.EN == SMMU_ECMDQ_CONSn.ENACK == 0`.

Same guarantees apply as for `SMMU_(*_)CR0.CMDQEN` / `SMMU_(*_)CR0ACK.CMDQEN` — the enabled/disabled state is reflected in ENACK after an IMPLEMENTATION DEFINED delay.

In the transition from enabled to disabled: once the SMMU sets `SMMU_ECMDQ_CONSn.ENACK = 0`, it is guaranteed that:
- Errors have been reported.
- Consumption of commands has stopped.
- `SMMU_ECMDQ_CONSn.{ERR_REASON, ERR, RD_WRAP, RD}` are stable.

Note: The SMMU updates `ENACK` even if `SMMU_ECMDQ_PRODn.ERRACK != SMMU_ECMDQ_CONSn.ERR`.

### §3.5.6.3 ECMDQ Errors

When the SMMU encounters an error while fetching or processing a command:
- **Toggles** `SMMU_ECMDQ_CONSn.ERR` (toggle protocol, not set/clear).
- Updates `SMMU_ECMDQ_CONSn.ERR_REASON` with the error reason code.
- Updates `SMMU_ECMDQ_CONSn.RD` and `SMMU_ECMDQ_CONSn.RD_WRAP` to point at the failing command.

If `SMMU_ECMDQ_PRODn.ERRACK != SMMU_ECMDQ_CONSn.ERR` → the SMMU does **not** consume commands from that ECMDQ.

**Recovery:** Disable the ECMDQ, make `ERRACK` consistent with `ERR`, then re-enable. This restores predictable behavior.

ECMDQ errors are **additionally** reported in:
- `SMMU_GERROR.CMDQP_ERR` for Non-secure state.
- `SMMU_S_GERROR.CMDQP_ERR` for Secure state.

ECMDQ errors operate **independently** of `SMMU_(*_)GERROR.CMDQ_ERR` (main Command queue errors).

When `SMMU_(*_)GERROR.CMDQP_ERR` is activated → GERROR interrupt is triggered in the same manner as other GERROR conditions.

Ordering guarantee: if `SMMU_(*_)GERROR.CMDQP_ERR` activation is observable → the `SMMU_ECMDQ_CONSn.ERR` field indicating the reason is observable.

If a CMD_SYNC MSI write (issued via a Command queue control page) experiences an External abort → reported in `SMMU_(*_)GERROR.MSI_CMDQ_ABT_ERR`.

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

- [event-queue.md](event-queue.md) — output counterpart; SMMU-to-software
- [tlb-invalidation.md](tlb-invalidation.md) — most common command type
- [fault-models.md](fault-models.md) — `CMD_RESUME` and `CMD_STALL_TERM` interact with stall model
- [smmu-initialization.md](smmu-initialization.md) — command queue setup is part of initialization sequence

## Sources That Use This Concept

- [../sources/ihi0070g-b-smmuv3-architecture-spec.md](../sources/ihi0070g-b-smmuv3-architecture-spec.md) — §3.5 Command and Event queues; §3.5.1 SMMU circular queues; §3.5.6 Enhanced Command queue; §4 Commands; §4.7.3 CMD_SYNC
