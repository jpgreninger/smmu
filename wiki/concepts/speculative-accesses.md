---
title: "Speculative Accesses"
type: concept
tags: [smmu, speculative, httu, translation-request, atos, implementation-defined]
created: 2026-04-13
updated: 2026-04-13
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Speculative Accesses

## Definition

§3.14 defines the SMMU's behavior when an incoming transaction or Translation Request is marked as speculative. Speculative marking is IMPLEMENTATION DEFINED in how it is signaled from client device to SMMU; the architectural behavior on receipt is fully specified.

## Speculative Client Transactions

### Write transactions

A write transaction marked as speculative is **always** terminated with an abort. No event is recorded to software. This is unconditional regardless of translation outcome.

### Read transactions

A speculative read transaction has the following behavior:

1. **Translation succeeds without fault:** The read proceeds into the system and returns data. If HTTU is enabled, Access flags in relevant translation table descriptors are updated exactly as for a non-speculative read.
2. **Any fault or configuration error occurs:** The transaction is terminated with an abort. **No event is recorded to software** for any speculative transaction, regardless of fault type. The determination of whether a fault occurs is identical to non-speculative reads, including Access flag faults.

Key point: fault detection logic is the same for speculative and non-speculative reads; only the response (abort-without-event vs. normal fault handling) differs.

### HTTU and Speculative Reads

When HTTU is enabled and a speculative read translates successfully:
- Access flags in relevant TT descriptors are updated per Armv8.1-A rules.
- Stage 2 translation table flags are updated for both speculative stage 1 accesses and for writes of stage 1 descriptors caused by setting Access flags.
- This exactly matches the SMMU HTTU rules for non-speculative reads.

## Speculative Translation Requests

An implementation may support speculatively-issued Translation Requests from a client device. An IMPLEMENTATION DEFINED mechanism must differentiate speculative from non-speculative Translation Requests. PCIe ATS Translation Requests are **always non-speculative**.

### Read Translation Requests (speculative)

- A successful response is returned if translation succeeds.
- If HTTU Access flag management is enabled, AF is updated.

### Write Translation Requests (speculative)

Write permission is granted in the response **only if** all translation table descriptors for the address are marked writable-dirty (not writable-clean):

- **Writable-dirty descriptors:** Grant write in response. If AF management is enabled, AF is updated. If dirty state management is enabled, speculative Write Translation Requests do **not** mark any writable-clean descriptor as writable-dirty (even if HTTU dirty state management is otherwise enabled for non-speculative writes). If a descriptor is writable-clean, write access is denied in the response.
- **Nested (stage 1 + stage 2) HTTU:** An update of a stage 1 descriptor to set AF or Dirty may cause the stage 2 descriptors related to that stage 1 descriptor to be marked Dirty as required — this applies to both read and write speculative Translation Requests.

The Translation Request response encodes:
- Whether the denial was due to a page fault / missing translation (i.e., stop retrying).
- Whether a valid translation existed but was denied because the descriptor is writable-clean (i.e., a non-speculative write request may succeed after software action).

## Speculative Accesses to SMMU Structures

For speculative accesses of SMMU structures and cached translations, the following sections govern behavior:
- §3.21.1: Translation tables and TLB invalidation completion behavior — speculative walks and interactions with in-progress invalidations.
- §3.21.3: Configuration structures and configuration invalidation completion — prefetch of STEs/CDs and interaction with CMD_CFGI_*.

An implementation is permitted to speculatively fetch any reachable configuration structure (STE, CD, L1STD, L1CD) at any time, subject to the constraints that it must not read outside configured table bounds and must not cache structures under wrong types.

## Model Implementation Notes

- A functional model may treat all transactions as non-speculative and remain correct. Speculative behavior is IMPLEMENTATION DEFINED on the marking side.
- A performance model must implement the speculative read path (no event on fault) to avoid overcounting fault events.
- Speculative Write Translation Request logic — no dirty-state promotion — is critical for correctness of HTTU models when ATS is in use.
- The no-event-on-abort rule for speculative transactions means the event queue must not be written on these paths.

## Related Concepts

- [concepts/httu.md](concepts/httu.md) — Access flag and dirty state updates apply identically to successful speculative reads
- [concepts/fault-models.md](concepts/fault-models.md) — Speculative faults terminate silently; no event recorded, no stall
- [concepts/pcie-ats-pri.md](concepts/pcie-ats-pri.md) — ATS Translation Requests are always non-speculative; PRI and ATS interactions
- [concepts/tlb-invalidation.md](concepts/tlb-invalidation.md) — §3.21.1 governs interaction of speculative walks with in-progress invalidations
- [concepts/stream-table-entry.md](concepts/stream-table-entry.md) — Speculative STE prefetch rules from §3.21.3

## Sources That Use This Concept

- [sources/ihi0070g-b-smmuv3-architecture-spec.md](sources/ihi0070g-b-smmuv3-architecture-spec.md) — §3.14 Speculative accesses; §3.21.1 Translation tables and TLB invalidation completion; §3.21.3 Configuration structures
