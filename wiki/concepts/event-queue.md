---
title: "Event Queue"
type: concept
tags: [smmu, event-queue, circular-buffer, faults, events, software-interface, stall, mev, event-merging]
created: 2026-04-07
updated: 2026-04-14
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Event Queue

## Definition

The Event queue is the SMMU-to-software interface for reporting faults, errors, and other asynchronous events related to incoming transaction processing. It is a memory-based circular buffer where the SMMU is the producer and software is the consumer. It is the output counterpart to the [concepts/command-queue.md](concepts/command-queue.md).

There is one Event queue per Security state:
- Non-secure: `SMMU_EVENTQ_*` registers.
- Secure: `SMMU_S_EVENTQ_*` (when `SMMU_S_IDR1.SECURE_IMPL == 1`).
- Realm: `SMMU_R_EVENTQ_*`.

Events from a stream are written to the Event queue associated with the stream's Security state.

## Circular Buffer Mechanics

Same mirrored circular buffer mechanics as the [concepts/command-queue.md](concepts/command-queue.md), with roles reversed:
- SMMU updates `PROD.WR` after writing a new event record.
- Software updates `CONS.RD` after consuming an event record.
- Empty/full semantics are identical (wrap bit differentiates empty from full).

## Event Queue Visibility Semantics

- The SMMU writes event data to memory, then updates `PROD.WR` to publish the entry. An event is not considered visible until the PROD index covers the entry.
- Software must not assume a new event is present without first reading PROD.
- Interrupt ordering: the SMMU updates PROD no later than when it asserts the queue interrupt. However, software must not assume new entries are present on interrupt arrival without reading PROD — a prior interrupt handler may have already consumed all entries.

## §7.2.1 Event Writability Conditions

Events are delivered into an Event queue only when the queue is **writable**. The Event queue is writable when **all** of the following are true:

1. The queue is enabled: `SMMU_(*_)CR0.EVENTQEN == 1` for the Security state of the queue.
2. The queue is not full (see §7.4 Event queue overflow).
3. No unacknowledged `SMMU_(*_)GERROR.EVENTQ_ABT_ERR` condition exists for the queue.

### Behavior When Queue Is Not Writable

- **Non-stall events:** Silently discarded. A queue overflow condition is triggered when events are discarded because the queue is full (see §7.4).
- **Stall events:** Never discarded. A stalled transaction that cannot record its event because the queue is unwritable has one of the following behaviors:
  1. **Invalidation+CMD_SYNC path:** If the stalled transaction is affected by a configuration or translation invalidation and a subsequent `CMD_SYNC`, the transaction must be retried after the queue becomes writable (non-full, enabled, no abort). The retry either:
     - Succeeds (no event recorded for the original fault), or
     - Generates a new fault (reflecting new configuration), which attempts to write a new event to the now-writable queue.
  2. **Permitted early retry:** A transaction not affected by an invalidation/CMD_SYNC is **permitted but not required** to be retried when the queue becomes writable.
  3. **Retry while still unwritable:** If the transaction retries while the queue is still unwritable:
     - If the retry translates successfully, the original event is **permitted but not required** to be recorded.
     - If the retry faults again and stalls again: the new stall event is recorded when the queue becomes writable.
     - If the retry faults and terminates: the terminate event recording is **lost** (no record).
  4. **No retry path:** If the transaction is not retried, the original fault event record is recorded when the queue becomes writable.

### Early Retry Permit

An event is permitted (but not required) to be recorded for a stalled transaction when the stalled transaction **early-retried** and translated successfully before the SMMU attempted to write out the event record (see §3.12.2.2 Early retry of Stalled transactions). Since the transaction has completed without software intervention, there is no benefit in recording the original stall.

### Terminated Transaction Events

Events from **terminated** faulting transactions commit to being recorded when the queue is writable. When `EVENTQEN` transitions to 0:
- Committed events are written out and guaranteed visible by the time the update completes.
- All uncommitted events from terminated faulting transactions are discarded.
- If Event queue writes aborted, the condition is visible in `GERROR` by the time the `EVENTQEN` update completes.

Some events may be recorded when `SMMU_(*_)CR0.SMMUEN == 0` (where explicitly stated in event definitions). The remainder require translation to be enabled. Note: `SMMUEN == 0` does **not** imply `EVENTQEN == 0`.

## §7.2.2 Event Queue External Abort

An external abort while writing to the Event queue activates `SMMU_(*_)GERROR.EVENTQ_ABT_ERR`. Whether the interconnect can report transaction aborts is **IMPLEMENTATION DEFINED**.

When `EVENTQ_ABT_ERR` is triggered, one or more events may have been lost, including stall fault event records.

The SMMU only writes to the Event queue when the queue is enabled and writable.

### Synchronous External Abort
- Queue entry validity semantics are maintained: all entries up to `SMMU_(*_)EVENTQ_PROD` are valid, successfully-written records.
- All outstanding queue writes complete before the error is flagged in `GERROR.EVENTQ_ABT_ERR`.
- Records written at and beyond the aborting queue location are not visible to software, even if successfully written — they are lost.
- **The PROD index is not incremented** for entries that caused a synchronous abort.
- In the scenario where a write to an empty Event queue aborts synchronously, PROD is not incremented, the queue remains empty, and the queue non-empty IRQ is not triggered.
- Software can consume and process all valid entries in the Event queue normally.

### Asynchronous External Abort
- Queue validity semantics are **broken**. The PROD index may be incremented for entries that caused an asynchronous abort.
- Software must assume all Event queue entries are **invalid** upon receiving the Global Error.
- Arm **strongly recommends** that the queue be made empty — either by re-initialization or by consuming/discarding all (invalid) entries.
- An IRQ for `SMMU_(*_)GERROR.EVENTQ_ABT_ERR` may arrive in any order relative to the Event queue non-empty IRQ.

If the stall model is implemented and enabled, software must terminate all stalling transactions present in the SMMU using `CMD_STALL_TERM` or by transitioning `SMMU_(*_)CR0.SMMUEN` through 0.

## §7.2.3 Security State Queue Independence

If Secure state is implemented, the Secure Event queue receives events relating to Secure streams; the Non-secure Event queue receives events from Non-secure streams.

Event queues for different Security states are **independent**:
- Non-secure faults/errors do **not** cause Secure event records.
- Secure faults/errors do **not** cause Non-secure event records.

## Event Write Commit Semantics

Event record generation is a multi-step process:

1. A situation triggering an event occurs (e.g., translation fault).
2. An event record is assembled internally.
3. It is determined that a queue entry is writable (queue enabled, not full, no global error).
4. **Commit point** — the event write is guaranteed to complete. Before commit, the write may not happen (e.g., if queue stays full).
5. Event record is written to memory.
6. `PROD.WR` is updated to publish the record.

A **stall event** record must not commit until the queue entry is deemed writable. If the queue is full, the stall record is buffered until the queue becomes writable. Stall events are generally not discarded (unlike non-stall events, which may be discarded on overflow).

## Overflow Behavior

- Events for stalled transactions: **never discarded**. Buffered until space is available.
- Events for terminated transactions (non-stall): **may be discarded** on overflow.
- The SMMU records that overflow has occurred; see §7.4 Event queue overflow.
- A `CMD_SYNC` ensures events for terminated transactions are visible before the sync completes. It does not help with overflow that occurred before the sync.

## Response Ordering

There is no requirement for an event to be visible in the Event queue before the transaction response is returned to the client device. Specifically:
- An event for a terminated transaction may appear in the Event queue before or after the abort reaches the client device.
- `CMD_SYNC` enforces visibility of events for terminated transactions, but only up to previously-consumed commands.

## §7.3 Event Record Format

All event records are **32 bytes** in size. All event records are **little-endian**. Event records are recorded into the Event queue appropriate to the Security status of the StreamID causing the event.

### Common Fields (§7.3)

All event record types share the following common fields:

| Field | Description |
|-------|-------------|
| `StreamID` | The StreamID of the requester that issued the transaction that led to the event. |
| `RnW` | Read/Write nature of the transaction: `0` = Write, `1` = Read. For CMOs, ATS TR, and transactions, see §16.7.2, §3.9.1, §13.1.1 respectively. |
| `PnU` | Privileged/Unprivileged (post-STE override): `0` = Unprivileged, `1` = Privileged. |
| `InD` | Instruction/Data (post-STE override): `0` = Data, `1` = Instruction. |
| `InputAddr` | The 64-bit input address to the SMMU for the transaction. Includes sign-extension as described in §3.4.1. TBI does not affect `InputAddr` — bits[63:56] are included as supplied to the SMMU. May be a VA, IPA, or PA depending on context (e.g., F_TRANSLATION at stage 1 → `InputAddr` is a VA). |
| `SSV` | SubstreamID validity: `0` = no SubstreamID present (SubstreamID field unknown), `1` = SubstreamID field valid. |
| `SubstreamID` | The SubstreamID provided with the transaction (valid only when `SSV == 1`). |
| `S2` | Stage of fault: `0` = Stage 1 fault, `1` = Stage 2 fault. |
| `CLASS` | Class of operation that caused the fault: `0b00` = CD (CD fetch), `0b01` = TTD (stage 1 translation table fetch), `0b10` = IN (input address caused fault), `0b11` = Reserved. |
| `NSIPA` | Non-secure IPA. Zero unless the event is recorded on the Secure event queue and `S2 == 1`, in which case this bit equals the NS bit output from stage 1 for the faulting access. In the case of a stage 2 fault on a Secure stream, indicates whether the translation attempt used `STE.S2TTB` (NS=0) or `STE.S_S2TTB` (NS=1). |
| `GPCF` | Granule Protection Check Fault: `0` = external abort did not arise from a GPC fault; `1` = event arose from a GPC fault (see §3.25.3). |

Some events (F_STE_FETCH, F_CD_FETCH, F_VMS_FETCH, F_WALK_EABT) additionally contain a **fetch address** field: the address of the specific structure or descriptor (STE, CD, VMS, or TTD) that an aborting transaction was originally initiated to access. This address is as calculated by the stream table, CD table, VMS, or translation table walk.

Portions of event records not explicitly defined are RES0.

Arm recommends that software treats receipt of any event type that is not defined in the architecture as a non-fatal occurrence.

## §7.3.1 Event Record Merging

Events originating from a stream are **permitted** to be merged when `STE.MEV == 0` has not been set to disable merging. A merged event record is a single record written to the Event queue representing more than one occurrence of an event.

Two or more events are **permitted but not required** to be merged when:
- The events are identical (or differ only as explicitly stated in event record definitions), **and**
- If the event type has a `Stall` field: `Stall == 0`. **Events with `Stall == 1` are never merged.**
- The events are not separated by a significant amount of time.

**Note:** The merging feature is intended to rate-limit events occurring at an unusually high frequency. Arm strongly recommends that an implementation writes separate records for events that do not occur in quick succession.

### STE.MEV Control

`STE.MEV == 0` disables merging for events from a particular stream. This is useful for debug visibility where one transaction maps to one fault event record. However, `STE.MEV` can only control merging for events generated **after a valid STE is located**.

**The following four events may occur before an STE is located, and therefore may always be merged regardless of STE.MEV:**

| Event | Reason |
|-------|--------|
| `F_UUT` | Unsupported upstream transaction — occurs before STE lookup |
| `C_BAD_STREAMID` | StreamID out of range — STE not yet located |
| `F_STE_FETCH` | External abort fetching the STE itself |
| `C_BAD_STE` | STE is invalid/illegal — STE was fetched but is bad |

Hardware implementations are **required** to respect `STE.MEV == 0` (other than these four events). Arm recommends that software expects event records might be merged even when `STE.MEV == 0` (e.g., a hypervisor might override merging).

## Event Merging (Summary)

Implementations may merge duplicate event records to reduce queue fill rate:
- Merging is only permitted when all fields are identical (except those explicitly excluded per §7.3) and `Stall == 0` (stall events are never merged).
- `STE.MEV == 0` disables merging per stream (except for F_UUT, C_BAD_STREAMID, F_STE_FETCH, C_BAD_STE).
- Software SMMU emulations are not required to honor `STE.MEV`.

## Event Record Types

Key event types (§7.3):

| Code | Name | Meaning |
|------|------|---------|
| F_UUT | Unsupported Upstream Transaction | Transaction type unsupported by this SMMU |
| C_BAD_STREAMID | Bad StreamID | StreamID out of configured range |
| F_STE_FETCH | STE fetch fault | External abort while fetching STE |
| C_BAD_STE | Bad/Illegal STE | STE is invalid or ILLEGAL |
| F_BAD_ATS_TREQ | Bad ATS Translation Request | ATS request when ATS not enabled |
| F_STREAM_DISABLED | Stream disabled | STE.Config == 0b000 |
| F_TRANSL_FORBIDDEN | Translation forbidden | ATS Translated transaction not permitted |
| C_BAD_SUBSTREAMID | Bad SubstreamID | SubstreamID out of range |
| F_CD_FETCH | CD fetch fault | External abort while fetching CD/L1CD |
| C_BAD_CD | Bad/Illegal CD | CD is invalid or ILLEGAL |
| F_WALK_EABT | Walk external abort | External abort during table walk |
| F_TRANSLATION | Translation fault | No valid TTE found |
| F_ADDR_SIZE | Address size fault | Address exceeds configured range |
| F_ACCESS | Access flag fault | Access flag not set, HTTU not enabled |
| F_PERMISSION | Permission fault | Permission check failed |
| F_TLB_CONFLICT | TLB conflict | Conflicting TLB entries detected |

Configuration errors (C_*) always terminate with abort. Translation-related faults (F_TRANSLATION, F_ADDR_SIZE, F_ACCESS, F_PERMISSION) may stall or terminate depending on [concepts/fault-models.md](concepts/fault-models.md) configuration.

## Model Implementation Notes

- A functional model must implement the full commit/visibility semantics: stall events must be held until the queue is writable; non-stall events may be dropped on overflow.
- The model must track the PROD wrap bit and update it atomically with the PROD index.
- Event record ordering: all output queues (Event, PRI) are appended sequentially; there is one global Event queue per security state (not per-stream).
- Priority ordering of events for a single transaction must be preserved (e.g., C_BAD_STREAMID takes priority over C_BAD_STE); only the highest-priority event for a given transaction is recorded.

## Related Concepts

- [concepts/command-queue.md](concepts/command-queue.md) — input counterpart (software-to-SMMU)
- [concepts/fault-models.md](concepts/fault-models.md) — fault models determine whether events carry Stall==1 or Stall==0
- [concepts/two-stage-translation.md](concepts/two-stage-translation.md) — translation faults are the primary event source

## Sources That Use This Concept

- [sources/ihi0070g-b-smmuv3-architecture-spec.md](sources/ihi0070g-b-smmuv3-architecture-spec.md) — §3.5 Command and Event queues; §3.5.3 Event queue behavior; §3.5.4 Definition of event record write "Commit"; §3.5.5 Event merging; §7.2 Event queue recorded faults; §7.3 Event records
