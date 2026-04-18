---
title: "SMMU Fault Model"
type: synthesis
tags: [smmu, fault, terminate, stall, event, model, correctness]
created: 2026-04-07
updated: 2026-04-16
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# SMMU Fault Model

Complete reference for implementing SMMU fault detection, recording, and reporting in a functional model. This page covers all fault types, their conditions, event record generation, stall/terminate behavior, and ordering guarantees.

## Fault Classification

### Always-Terminate Faults (configuration errors)

These faults always abort the transaction and record an event (if queue has space). Stall model never applies.

| Event Code | Trigger Condition |
|------------|------------------|
| F_UUT | Transaction type unsupported by this SMMU implementation |
| C_BAD_STREAMID | StreamID out of configured Stream table range; L1STD invalid or span exceeded |
| F_STE_FETCH | External abort while fetching STE or L1STD from memory |
| C_BAD_STE | STE is invalid (V=0) or ILLEGAL (illegal field combination) |
| F_BAD_ATS_TREQ | ATS Translation Request received when `STE.EATS == 0b00` |
| F_STREAM_DISABLED | S1DSS substream mismatch: S1DSS==0b00 with no SubstreamID, or S1DSS==0b10 with SubstreamID 0 (note: Config==0b000 aborts with **no event**) |
| F_TRANSL_FORBIDDEN | ATS Translated transaction forbidden (ATS disabled, Split-stage ATS protocol error, Realm→NS Instruction, DPT check fail) |
| C_BAD_SUBSTREAMID | SubstreamID out of range given STE.S1CDMax |
| F_CD_FETCH | External abort while fetching L1CD or CD |
| C_BAD_CD | CD is invalid (V=0) or ILLEGAL |
| F_WALK_EABT | External abort during translation table walk |
| F_TLB_CONFLICT | TLB conflict detected |
| F_VMS_FETCH | External abort while fetching VMS |
| GPC fault | Granule Protection Check failure (PA space mismatch or GPT access error) |

### Configurable Translation-Related Faults

These four fault types may use either the Terminate or Stall model depending on stage and configuration:

| Fault | Typical cause |
|-------|--------------|
| F_TRANSLATION | No valid TTE (page not present) |
| F_ADDR_SIZE | Address exceeds stage input/output range |
| F_ACCESS | AF=0 in TTE and HTTU not enabled or not applicable |
| F_PERMISSION | Permission bits in TTE deny access |

**Exception:** F_ADDR_SIZE from a bypassed stage 1 (IPA out-of-range) always terminates. F_PERMISSION from an instruction fetch on a Secure stream bypassing to NS PA with SIF=1 always terminates.

## Fault Behavior Configuration

### Stage 1 (configured in CD)

| CD.S | CD.A | STE.S1STALLD | Behavior |
|------|------|--------------|----------|
| 0    | 0    | X            | Terminate with RAZ/WI |
| 0    | 1    | X            | Terminate with abort |
| 1    | X    | 0            | Stall (if supported) |
| 1    | X    | 1            | Terminate with abort (stall suppressed) |

`CD.R`: enables event recording. If 0, fault events may not be recorded. (Still required for stall events by convention.)

### Stage 2 (configured in STE)

| STE.S2S | STE.S2R | Behavior |
|---------|---------|----------|
| 0       | X       | Terminate with abort |
| 1       | X       | Stall (if supported) |

### Implementation Capability Gate

Before configuring stall, check:
- `SMMU_IDR0.STALL_MODEL`: 0b00 = both; 0b01 = terminate only; 0b10 = stall only.
- `SMMU_IDR0.TERM_MODEL`: 0 = both abort and RAZ/WI; 1 = abort only.

## Event Recording Rules

### When Is an Event Written?

An event IS written to the Event queue when:
1. The condition triggering the event is detected.
2. The event queue is enabled (`SMMU_CR0.EVENTQEN == 1`).
3. The queue is not full (for non-stall events) — or space becomes available (for stall events).
4. No global error condition blocks the queue.

An event IS NOT written (may be discarded) when:
- Queue is full and the event is not a stall event.
- Queue is in error state.
- `CD.R == 0` (stage 1 recording disabled).

### Stall Event Handling

- Stall events (Stall field == 1 in the event record): never discarded due to overflow. Buffered internally until queue is writable.
- Write does not commit until queue entry is writable.
- After commit, write is guaranteed to complete (unless Event queue external abort occurs).

### Event Priority for a Single Transaction

If a transaction generates multiple potential events, only the highest-priority event is recorded. Priority order for the main path (non-ATS) per §7.3.22:

1. C_BAD_STREAMID
2. F_STE_FETCH
3. C_BAD_STE
4. F_VMS_FETCH (IMPL DEFINED position relative to items 5–6)
5. C_BAD_SUBSTREAMID
6. F_STREAM_DISABLED
7. F_CD_FETCH / stage 2 faults fetching CD
8. C_BAD_CD
9. Translation-related faults (stage 1 or stage 2)

Note: F_UUT, F_TLB_CONFLICT, and F_CFG_CONFLICT have **implementation-specific** prioritization and are not part of the fixed ordering above (§7.3.22).

For ATS Translated transactions with ATSCHK=1, the priority ordering is specified in §3.9.1.3.

### Event Record Fields

Every event record includes:
- **Type** — event code (F_TRANSLATION, C_BAD_STE, etc.).
- **Stall** — 1 if transaction is stalled (pending CMD_RESUME).
- **STAG** — stall tag; used in CMD_RESUME to identify the stalled transaction.
- **StreamID** / **SubstreamID** — identifies the transaction source.
- **SEC_SID** — security state.
- **Input address** — recorded unmodified (full 64-bit; not truncated for fault records).
- **Class** — stage (1 or 2) and context of the fault.
- Additional fault-specific fields (e.g., level of table walk for F_TRANSLATION).

## Stall Model Operation

### Stalling a Transaction

1. Translation-related fault detected, configuration requests stall (CD.S=1 or STE.S2S=1), stall model supported.
2. SMMU holds the transaction internally.
3. Event record with Stall=1 is assembled, committed when queue is writable.
4. STAG value is included in the event for later reference.

### Resuming or Terminating Stalled Transactions

Software issues:
- **`CMD_RESUME(StreamID, SSec, STAG, Action=0)`**: allow transaction to retry. SMMU re-attempts translation from the beginning.
- **`CMD_RESUME(StreamID, SSec, STAG, Action=1)`** or **`CMD_STALL_TERM(StreamID, SSec)`**: abort the stalled transaction. Transaction is terminated with abort response to the device.

After `CMD_RESUME` (retry), if the same fault recurs (e.g., page still not mapped), the transaction may stall again, generating another event.

### Independence of Transactions

Each transaction is independent. One stalled transaction does not block other transactions from the same or different streams.

## Response Ordering

- For terminated transactions: there is **no required ordering** between the abort response reaching the client device and the event record becoming visible in the Event queue.
- `CMD_SYNC` after the relevant commands enforces that events for terminated transactions ARE visible before the sync completes.
- An event may thus appear in the Event queue *before* the client device receives the abort.

## Fault Event Queue Routing

| Transaction security state | Event queue |
|---------------------------|-------------|
| Non-secure | Non-secure Event queue |
| Secure | Secure Event queue |
| Realm | Realm Event queue |

## Event Merging

Implementation may merge identical events (same type, same all fields) if:
- `STE.MEV == 1` (per-stream merging enabled), OR implementation does not support MEV.
- `Stall == 0` (stall events are never merged).

Merging reduces Event queue fill rate. Disable on a per-stream basis with `STE.MEV = 0` for debugging.

Software SMMU emulations are not required to honor STE.MEV.

## Model Implementation Checklist

- [ ] Implement all always-terminate fault conditions (Table above) — check at each pipeline step.
- [ ] Implement configurable fault behavior using CD.{S,R,A} and STE.{S2S,S2R,S1STALLD}.
- [ ] Implement stall transaction buffer: hold faulted transaction pending CMD_RESUME/CMD_STALL_TERM.
- [ ] Implement STAG generation and matching.
- [ ] Implement event record construction with all required fields.
- [ ] Implement stall-event buffering (never drop stall events on overflow).
- [ ] Implement non-stall event dropping on overflow.
- [ ] Implement event priority ordering (only highest-priority event per transaction).
- [ ] Model response/event ordering: no ordering between abort response and event visibility.
- [ ] Implement CMD_SYNC ensuring event visibility for terminated transactions.
- [ ] Implement event merging if claimed by SMMU_IDR0 (and respect STE.MEV).

## §7.5 Global Error Recording (GERROR/GERRORN Toggle Handshake)

Global errors that pertain to SMMU infrastructure (not individual transactions) are reported in `SMMU_(*_)GERROR` rather than in the memory-based Event queue.

### GERROR Error Flags

| Flag | Meaning |
|---|---|
| `CMDQ_ERR` | Command queue processing error (commands stop being consumed while active) |
| `EVENTQ_ABT_ERR` | External abort during Event queue write (Event queue delivery stops) |
| `PRIQ_ABT_ERR` | External abort during PRI queue write (PRI queue delivery stops) |
| `MSI_CMDQ_ABT_ERR` | CMD_SYNC MSI write aborted |
| `MSI_EVENTQ_ABT_ERR` | Event queue MSI write aborted |
| `MSI_PRIQ_ABT_ERR` | PRI queue MSI write aborted (Non-secure GERROR only) |
| `MSI_GERROR_ABT_ERR` | GERROR MSI write itself aborted |
| `SFM_ERR` | Service Failure Mode entered (present in both SMMU_GERROR and SMMU_S_GERROR) |
| `CMDQP_ERR` | ECMDQ (Enhanced Command Queue) processing error |
| `DPT_ERR` | DPT Lookup fault; syndrome in `SMMU_(R_)DPT_CFG_FAR` |

### Toggle Handshake Protocol

The SMMU uses a toggle-based handshake to avoid race conditions when reporting errors:

- **Activation:** When an error becomes active, the SMMU **toggles** `SMMU_(*_)GERROR[x]` (flips the bit).
- **Active condition:** An error is active when `SMMU_(*_)GERROR[x] != SMMU_(*_)GERRORN[x]`.
- **Acknowledgment:** Software acknowledges (deactivates) the error by writing `SMMU_(*_)GERRORN[x]` to the **same value** as the current `SMMU_(*_)GERROR[x]`.
- **New error suppression:** The SMMU does not toggle `GERROR[x]` if the error is already active (the new occurrence is not logged). Only after acknowledgment can a new occurrence be reported.

This design ensures that no error event is lost due to a race between the SMMU setting a flag and software clearing it.

### GERROR Interrupt Notification (§7.5.1)

A GERROR interrupt fires when any error becomes active, **except** `MSI_GERROR_ABT_ERR` (signaling an aborted GERROR MSI — sending another to the same address might abort again).

- Interrupt fires only when `SMMU_(*_)IRQ_CTRL.GERROR_IRQEN == 1`.
- MSI notification configured via `SMMU_(*_)GERROR_IRQ_CFG{0,1,2}`.
- Multiple simultaneous error activations may produce coalesced interrupts.

### Software Recovery Flows

**CMDQ_ERR recovery:**
1. Fix the cause of the command error (identified in `SMMU_(*_)CMDQ_CONS.ERR_REASON`).
2. Acknowledge by writing `SMMU_(*_)GERRORN.CMDQ_ERR` to match `GERROR.CMDQ_ERR`.
3. Command processing resumes. (Not required to write `SMMU_(*_)CMDQ_PROD` to re-trigger.)

**DPT_ERR recovery:**
1. Read the syndrome from `SMMU_(R_)DPT_CFG_FAR`.
2. Acknowledge by writing `SMMU_(R_)GERRORN.DPT_ERR` to the same value as `SMMU_(R_)GERROR.DPT_ERR`.

**General pattern:** For any GERROR flag, software reads `GERROR[x]`, determines the cause, resolves it, then writes `GERRORN[x] = GERROR[x]` to deactivate.

---

## Related Pages

- [../concepts/fault-models.md](../concepts/fault-models.md) — concept-level description
- [../concepts/granule-protection-check.md](../concepts/granule-protection-check.md) — GPCF fault category; GPT_ABT_ERR reporting; GPC-fault observability and CMD_SYNC guarantee
- [../concepts/event-queue.md](../concepts/event-queue.md) — event queue mechanics and visibility semantics
- [../concepts/command-queue.md](../concepts/command-queue.md) — CMD_RESUME, CMD_STALL_TERM, CMD_SYNC
- [../concepts/interrupts-and-power.md](../concepts/interrupts-and-power.md) — GERROR as one of 13 interrupt sources; MSI synchronization
- [../concepts/ras.md](../concepts/ras.md) — RAS error records (SMMU_ERR*) back some GERROR conditions; SFM interaction with fault reporting
- [smmu-translation-pipeline.md](smmu-translation-pipeline.md) — where faults are generated
- [smmu-queue-mechanics.md](smmu-queue-mechanics.md) — queue-level implementation
- [smmu-system-implementation.md](smmu-system-implementation.md) — system-level fault requirements: CMO fault recording, F_UUT conditions, SFM prerequisites
