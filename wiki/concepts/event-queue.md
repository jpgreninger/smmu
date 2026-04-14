---
title: "Event Queue"
type: concept
tags: [smmu, event-queue, circular-buffer, faults, events, software-interface]
created: 2026-04-07
updated: 2026-04-07
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Event Queue

## Definition

The Event queue is the SMMU-to-software interface for reporting faults, errors, and other asynchronous events related to incoming transaction processing. It is a memory-based circular buffer where the SMMU is the producer and software is the consumer. It is the output counterpart to the [[concepts/command-queue]].

There is one Event queue per Security state:
- Non-secure: `SMMU_EVENTQ_*` registers.
- Secure: `SMMU_S_EVENTQ_*` (when `SMMU_S_IDR1.SECURE_IMPL == 1`).
- Realm: `SMMU_R_EVENTQ_*`.

Events from a stream are written to the Event queue associated with the stream's Security state.

## Circular Buffer Mechanics

Same mirrored circular buffer mechanics as the [[concepts/command-queue]], with roles reversed:
- SMMU updates `PROD.WR` after writing a new event record.
- Software updates `CONS.RD` after consuming an event record.
- Empty/full semantics are identical (wrap bit differentiates empty from full).

## Event Queue Visibility Semantics

- The SMMU writes event data to memory, then updates `PROD.WR` to publish the entry. An event is not considered visible until the PROD index covers the entry.
- Software must not assume a new event is present without first reading PROD.
- Interrupt ordering: the SMMU updates PROD no later than when it asserts the queue interrupt. However, software must not assume new entries are present on interrupt arrival without reading PROD — a prior interrupt handler may have already consumed all entries.

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

## Event Merging

Implementations may merge duplicate event records to reduce queue fill rate:
- Merging is only permitted when all fields are identical (except those explicitly excluded per §7.3) and `Stall == 0` (stall events are never merged).
- If an implementation supports merging, it must implement `STE.MEV` to enable/disable merging per stream.
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

Configuration errors (C_*) always terminate with abort. Translation-related faults (F_TRANSLATION, F_ADDR_SIZE, F_ACCESS, F_PERMISSION) may stall or terminate depending on [[concepts/fault-models]] configuration.

## Model Implementation Notes

- A functional model must implement the full commit/visibility semantics: stall events must be held until the queue is writable; non-stall events may be dropped on overflow.
- The model must track the PROD wrap bit and update it atomically with the PROD index.
- Event record ordering: all output queues (Event, PRI) are appended sequentially; there is one global Event queue per security state (not per-stream).
- Priority ordering of events for a single transaction must be preserved (e.g., C_BAD_STREAMID takes priority over C_BAD_STE); only the highest-priority event for a given transaction is recorded.

## Related Concepts

- [[concepts/command-queue]] — input counterpart (software-to-SMMU)
- [[concepts/fault-models]] — fault models determine whether events carry Stall==1 or Stall==0
- [[concepts/two-stage-translation]] — translation faults are the primary event source

## Sources That Use This Concept

- [[sources/ihi0070g-b-smmuv3-architecture-spec]] — §3.5 Command and Event queues; §3.5.3 Event queue behavior; §3.5.4 Definition of event record write "Commit"; §3.5.5 Event merging; §7.2 Event queue recorded faults; §7.3 Event records
