---
title: "Fault Models"
type: concept
tags: [smmu, fault, terminate, stall, translation-fault, fault-model, paging, virtual-memory, e-page-request]
created: 2026-04-07
updated: 2026-04-14
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Fault Models

## Definition

The SMMU defines two models for handling **Translation-related faults** — faults that arise during the translation process itself (as opposed to configuration errors which always terminate):

- **Terminate model:** The faulting transaction is immediately aborted. An event is recorded (if enabled and queue has space). The device receives an error response.
- **Stall model:** The faulting transaction is held (stalled) pending software intervention. An event with `Stall == 1` is recorded. Software can resolve the fault and resume the transaction via `CMD_RESUME`, or terminate it via `CMD_STALL_TERM`.

## Translation-Related Faults (Configurable)

The four fault types subject to Terminate/Stall configuration:

| Fault | Cause |
|-------|-------|
| F_TRANSLATION | No valid translation table entry (page not present) |
| F_ADDR_SIZE | Input or output address exceeds configured range |
| F_ACCESS | Access flag not set and HTTU not enabled or supported |
| F_PERMISSION | Translation table permission check failed |

**All other faults and configuration errors always terminate with abort**, regardless of fault model configuration. This includes F_WALK_EABT, F_TLB_CONFLICT, C_BAD_STE, C_BAD_CD, F_STREAM_DISABLED, etc.

Note: F_ADDR_SIZE from a transaction that bypassed stage 1 but has an out-of-range IPA always terminates. F_PERMISSION from an instruction fetch on a Secure stream bypassing to Non-secure PA with SIF==1 always terminates.

## Configuration

### Stage 1 Fault Behavior (per CD)

| CD.S | CD.A | Stage 1 Translation-related fault behavior |
|------|------|-------------------------------------------|
| 0    | 0    | Terminate with RAZ/WI (read→zero, write→ignored) |
| 0    | 1    | Terminate with abort |
| 1    | —    | Stall (if `STE.S1STALLD == 0` and stall supported) |

`CD.R` enables event recording for stage 1 faults.

**`STE.S1STALLD`:** When set to 1, disables the stall model for all stage 1 faults on this stream, even if the CD requests stall. Faults are terminated instead.

### Stage 2 Fault Behavior (per STE)

| STE.S2S | Stage 2 Translation-related fault behavior |
|---------|--------------------------------------------|
| 0       | Terminate with abort |
| 1       | Stall |

`STE.S2R` enables event recording for stage 2 faults.

### Implementation Support

`SMMU_IDR0.STALL_MODEL` indicates which models the implementation supports:

| Value | Meaning |
|-------|---------|
| 0b00  | Both Stall and Terminate models supported |
| 0b01  | Terminate model only |
| 0b10  | Stall model only |

When Secure state is implemented, `SMMU_S_IDR0.STALL_MODEL` reports physical capability; `SMMU_IDR0.STALL_MODEL` reports what Non-secure software may use. `SMMU_S_CR0.NSSTALLD == 1` prevents Non-secure software from using the stall model even if the hardware supports it.

`SMMU_IDR0.TERM_MODEL` indicates termination options: 0 = both abort and RAZ/WI; 1 = abort only (CD.A must be set to 1). A stage 2 fault that is terminated is always aborted (never RAZ/WI).

## Stall Model Details

When a transaction is stalled:
- The SMMU holds the transaction internally.
- An event record with `Stall == 1` is written to the Event queue (or buffered until queue space is available — stall events are never discarded due to overflow).
- Software reads the event, resolves the condition (e.g., maps the page), then issues `CMD_RESUME` with the STAG from the event to allow the transaction to retry, or `CMD_STALL_TERM` to abort it.
- The SMMU treats each transaction independently; stalling one does not block others.

### Stall Implementation Caveat

An implementation supporting both models may, for IMPLEMENTATION DEFINED reasons, treat a stalling configuration as terminating for specific client devices. In this case:
- Faults are terminated with abort.
- Events are recorded with `Stall == 0`.
- The implementation is not required to advertise which devices have this restriction.

Arm recommends software expect `Stall == 0` events for devices not explicitly documented as stall-safe.

## Fault Identification: Stage 1 vs Stage 2

The SMMU records which stage the fault occurred at:
- Stage 1 fault: fault in the stage 1 translation tables.
- Stage 2 fault: fault in the stage 2 translation tables, OR a stage 2 fault during a stage 1 table walk (the walk addresses are IPAs), OR a stage 2 fault while fetching a CD from IPA space.

This distinction is critical for hypervisor software: a stage 2 fault during a stage 1 walk is reported as a stage 2 fault so the hypervisor can simulate the correct PE external abort type to the guest VM.

## §3.12.3 Stall Termination — Client Device Considerations

When a stalled transaction is terminated (via `CMD_RESUME(Terminate)` or `CMD_STALL_TERM`), the transaction is **marked and guaranteed to terminate at some point in the future**, unless translations change such that an early-retry succeeds in the meantime. The SMMU does not guarantee when a terminated-stall is finally completed.

**Critical safety note:** A race condition can arise if software must reconfigure translations after terminating a stalled transaction. Example scenario:
1. A write transaction to an unmapped address causes a stall fault.
2. Software issues `CMD_RESUME(Terminate)` to terminate it.
3. Later, software creates a legitimate mapping at that address.
4. If the original write transaction retries and now succeeds (due to the new mapping), **data corruption may result** because the application expected the transaction to have been terminated.

The system or client devices **must provide a mechanism** allowing software to wait for all previously outstanding transactions to complete before changing translation configuration in a way that might allow them to proceed. This mechanism might be:
- An explicit signal from the client device indicating all outstanding transactions have completed.
- An interconnect ordering guarantee that prior transactions are visible.
- Another implementation-defined mechanism.

## §3.12.4 Virtual Memory Paging with SMMU

The SMMU architecture supports three models of usage with respect to translation-related faults that occur during translation of client device accesses:

### Model 1 — Always Error (Terminate)
A fault due to a device access is always treated as an error (e.g., a programming error) and is terminated. Software configures the Terminate model and aborts all faulting transactions.

### Model 2a — Stall + Resume (Non-PCIe Paging)
A fault due to a device access might be permanent (programming error) or temporary (due to page state in a virtual memory system). The **Stall model** is used:
- The device transaction is stalled.
- The fault is reported to software.
- After the virtual memory system resolves the cause (e.g., pages in the faulting page), software issues `CMD_RESUME` to retry the transaction.
- If the virtual memory system determines the access was invalid, software issues `CMD_RESUME(Terminate)` or `CMD_STALL_TERM`.

**Requirement:** This model can only be used with a device and interconnect capable of supporting stalls.

### Model 2b — PCIe ATS/PRI
For PCIe devices, transactions **cannot safely be stalled** (PCIe protocol does not allow holding transactions indefinitely). The PCIe specification provides two mechanisms:
- **ATS (Address Translation Service):** An endpoint ascertains whether a page can be accessed without causing an SMMU fault before accessing it. The SMMU responds with a Translation Completion.
- **PRI (Page Request Interface):** If an ATS response indicates that a fault would occur, PRI provides a mechanism for the page fault to be resolved. The endpoint issues a Page Request; software maps the page; the SMMU grants access.

See [pcie-ats-pri.md](pcie-ats-pri.md) for full ATS/PRI coverage.

## §3.12.4.1 Page-In Request Event (E_PAGE_REQUEST)

When non-PCIe devices use the Stall fault model to access paged virtual memory spaces, the stall fault record itself is the notification to software that a page miss occurred and software intervention is required.

An **optional** hint event record `E_PAGE_REQUEST` can be provided by an implementation to request that software initiates costly page-in operations early. An implementation may provide an **IMPLEMENTATION DEFINED** mechanism to convey this message from client devices.

`E_PAGE_REQUEST` properties:
- **Is a hint only** — it can be ignored or dropped by the SMMU or software with no consequence.
- **Can be issued speculatively** by a device — software must not rely on it reflecting actual access intent.
- **Requires no response** from software.

The distinction from a stall record: a stall fault record arises from a **non-speculative** transaction. A speculative transaction generates no software-visible record. `E_PAGE_REQUEST` allows a software-visible record that lets software make an early start on fetching pages from secondary storage, hiding latency before the actual (non-speculative) transaction stalls.

Note: Because writes cannot be emitted speculatively (see §3.14), stall fault records never arise from speculative transactions. The `E_PAGE_REQUEST` hint specifically exists to fill this gap.

## §3.12.5 Fault Configuration Combinations with Two Stages

When both stage 1 and stage 2 are active and Terminate/Stall are configured differently at each stage, the resulting transaction behavior depends on **which stage** the fault occurred at.

For Translation-related faults (those subject to Terminate/Stall configuration):

| Stage 1 config | Stage 2 config | Fault at | Transaction result | Event parameters | Hypervisor behavior |
|:---------------|:---------------|:---------|:-------------------|:-----------------|:--------------------|
| Terminate | Terminate | Stage 1 | Terminated | VA | Event passed to guest as stage 1-only event. |
| Terminate | Terminate | Stage 2 | Terminated | VA, IPA | Hypervisor might log IPA for debug. May pass event to guest if terminated (see note 1). |
| Terminate | Stall | Stage 1 | Terminated | VA | Event passed to guest as S1-only event. |
| Terminate | Stall | Stage 2 | Stalled | VA, IPA | Hypervisor may terminate with `CMD_RESUME(Terminate)` and log IPA; or correct S2 translation and `CMD_RESUME(Retry)`. May pass event to guest if terminated (note 1). |
| Stall | Terminate | Stage 1 | Stalled | VA | Event passed to guest as S1-only event with stall. Guest must `CMD_RESUME(Retry/Terminate)`. |
| Stall | Terminate | Stage 2 | Terminated | VA, IPA | Hypervisor might log IPA for debug. May pass event to guest if terminated (note 1). |
| Stall | Stall | Stage 1 | Stalled | VA | Event passed to guest as S1-only event with stall. Guest must `CMD_RESUME(Retry/Terminate)`. |
| Stall | Stall | Stage 2 | Stalled | VA, IPA | Hypervisor may terminate with `CMD_RESUME(Terminate)` and log IPA; or correct S2 translation and `CMD_RESUME(Retry)`. May pass event to guest if terminated (note 1). |

**Note 1 — Stage 2 fault and guest notification:** Anything terminated at stage 2 is equivalent to a stage 1 external abort from the guest's perspective. A successful stage 1 translation that outputs an IPA leading to a stage 2 fault would not ordinarily be reported through the guest's SMMU interface (the stage 1 translation succeeded; the error arises outside the stage 1 domain). Arm expects that a stage 1 translation table walk that faults at stage 2 is reported to the guest as **F_WALK_EABT** by the hypervisor.

**Note:** When both stage 1 and stage 2 are enabled, a CD or stage 1 translation table descriptor fetch might cause a stage 2 Translation-related fault, and might therefore stall the transaction. This is the same behavior as a faulting IPA for the transaction address: the stage 2 fault can be resolved and the transaction restarted.

All other fault types (configuration errors, structure faults such as F_BAD_STE, F_BAD_CD) always abort the transaction regardless of Terminate/Stall configuration.

## Transaction Independence

The SMMU treats every transaction as independent. The fault behavior of one transaction has no direct effect on any other transaction, even from the same stream. Whether a higher-level agent groups transactions together is outside the scope of the SMMU architecture.

## Model Implementation Notes

- A functional model must implement all four Translation-related fault types for both stages with the correct configurable behavior.
- Stall model requires buffering transactions pending `CMD_RESUME` or `CMD_STALL_TERM`. The STAG field in the event record identifies the stalled transaction.
- RAZ/WI termination (CD.S==0, CD.A==0) is a stage 1-specific behavior; stage 2 always aborts on termination.
- The model must correctly differentiate faults that are always-terminate from those that are configurable.
- Version gating: `SMMU_IDR0.STALL_MODEL` must be checked before enabling stall; the model should reflect whichever version it is implementing.

## Related Concepts

- [event-queue.md](event-queue.md) — events are the reporting mechanism for all fault conditions
- [two-stage-translation.md](two-stage-translation.md) — faults arise during the translation pipeline
- [context-descriptor.md](context-descriptor.md) — CD.{S, R, A} configure stage 1 fault behavior
- [stream-table-entry.md](stream-table-entry.md) — STE.{S2S, S2R, S1STALLD} configure stage 2 and stream-level fault behavior
- [command-queue.md](command-queue.md) — CMD_RESUME, CMD_STALL_TERM resolve stalled transactions
- [interrupts-and-power.md](interrupts-and-power.md) — fault events trigger interrupt sources; MSI synchronization fences govern when fault events become visible to software

## Sources That Use This Concept

- [../sources/ihi0070g-b-smmuv3-architecture-spec.md](../sources/ihi0070g-b-smmuv3-architecture-spec.md) — §3.12 Fault models, recording and reporting; §3.12.1 Terminate model; §3.12.2 Stall model; §5.5 Fault configuration (A, R, S bits)
