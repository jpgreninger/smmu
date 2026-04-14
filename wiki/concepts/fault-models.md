---
title: "Fault Models"
type: concept
tags: [smmu, fault, terminate, stall, translation-fault, fault-model]
created: 2026-04-07
updated: 2026-04-07
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

## Transaction Independence

The SMMU treats every transaction as independent. The fault behavior of one transaction has no direct effect on any other transaction, even from the same stream. Whether a higher-level agent groups transactions together is outside the scope of the SMMU architecture.

## Model Implementation Notes

- A functional model must implement all four Translation-related fault types for both stages with the correct configurable behavior.
- Stall model requires buffering transactions pending `CMD_RESUME` or `CMD_STALL_TERM`. The STAG field in the event record identifies the stalled transaction.
- RAZ/WI termination (CD.S==0, CD.A==0) is a stage 1-specific behavior; stage 2 always aborts on termination.
- The model must correctly differentiate faults that are always-terminate from those that are configurable.
- Version gating: `SMMU_IDR0.STALL_MODEL` must be checked before enabling stall; the model should reflect whichever version it is implementing.

## Related Concepts

- [[concepts/event-queue]] — events are the reporting mechanism for all fault conditions
- [[concepts/two-stage-translation]] — faults arise during the translation pipeline
- [[concepts/context-descriptor]] — CD.{S, R, A} configure stage 1 fault behavior
- [[concepts/stream-table-entry]] — STE.{S2S, S2R, S1STALLD} configure stage 2 and stream-level fault behavior
- [[concepts/command-queue]] — CMD_RESUME, CMD_STALL_TERM resolve stalled transactions

## Sources That Use This Concept

- [[sources/ihi0070g-b-smmuv3-architecture-spec]] — §3.12 Fault models, recording and reporting; §3.12.1 Terminate model; §3.12.2 Stall model; §5.5 Fault configuration (A, R, S bits)
