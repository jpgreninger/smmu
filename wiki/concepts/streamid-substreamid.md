---
title: "StreamID and SubstreamID"
type: concept
tags: [smmu, streamid, substreamid, pasid, stream, identification]
created: 2026-04-07
updated: 2026-04-07
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# StreamID and SubstreamID

## Definition

**StreamID** is the sideband identifier that accompanies every client device transaction into the SMMU. It identifies the originating device (or logical source) and is used to index the Stream table to locate the [concepts/stream-table-entry.md](concepts/stream-table-entry.md).

**SubstreamID** is an optional secondary identifier that selects among multiple stage 1 translation contexts ([concepts/context-descriptor.md](concepts/context-descriptor.md)) for a given stream. It is equivalent to a PCIe PASID (Process Address Space ID) and maps to the SubstreamID 1:1 in PCIe systems.

## StreamID Properties

- Size: 0 to 32 bits, IMPLEMENTATION DEFINED. Discovered via `SMMU_IDR1.SIDSIZE`.
- Namespace: per-SMMU. Devices behind different SMMUs with the same StreamID are distinct.
- When Secure state is supported, StreamID is qualified by `SEC_SID` (Secure/Non-secure flag), which selects between the Secure and Non-secure Stream tables. The term "StreamID" in the spec implicitly includes this disambiguation.
- For PCIe: Arm expects `StreamID[15:0] == RequesterID[15:0]`. When multiple PCIe hierarchies share one SMMU, upper bits of StreamID distinguish Root Complexes (PCIe domains). Implementations for PCIe must support at least 16-bit StreamID.
- A device may emit traffic with more than one StreamID (representing multiple logical data streams).
- Arm recommends a dense namespace starting at 0.

## SubstreamID Properties

- Size: 0 to 20 bits, IMPLEMENTATION DEFINED. Discovered via `SMMU_IDR1.SSIDSIZE`.
- Maximum of 20 bits matches maximum PCIe PASID size.
- The transaction flag `SSV` (SubstreamID Valid) indicates whether a SubstreamID is supplied; this may differ on a per-transaction basis.
- Only meaningful when stage 1 is enabled for the stream (`STE.Config` enables stage 1).
- A stage 2-only implementation does not accept SubstreamID input.
- An SMMU with stage 1 is not required to support substreams.

## StreamID to STE Lookup Procedure

1. Range-check the StreamID against the configured table size. If out of range → C_BAD_STREAMID event, transaction aborted.
2. For 2-level tables: index the L1 table with `StreamID[n:x]` to get the L1STD; check validity. Index the L2 table with `StreamID[x-1:0]`.
3. For linear tables: directly index by StreamID.
4. Fetch the STE.

## SubstreamID to CD Lookup Procedure

1. If SubstreamID is supplied (`SSV == 1`) and stage 1 is enabled:
   - Use SubstreamID to index into the CD table (flat or 2-level, per `STE.S1Fmt`).
   - Range-check SubstreamID against `STE.S1CDMax`. If out of range → C_BAD_SUBSTREAMID.
2. If SubstreamID is not supplied (`SSV == 0`) and substreams are configured:
   - Behavior governed by `STE.S1DSS` (see [concepts/stream-table-entry.md](concepts/stream-table-entry.md)).

## PCIe Mapping

| SMMU concept    | PCIe equivalent    | Notes |
|-----------------|--------------------|-------|
| StreamID[15:0]  | RequesterID (BDF)  | Bus:Device:Function |
| SubstreamID     | PASID              | 1:1 mapping; 20-bit max |
| SEC_SID=Realm   | T-bit in IDE TLP prefix (T=1) | RME DA systems |
| SEC_SID=NS      | Absence of T-bit or T=0 | |

## Model Implementation Notes

- StreamID is the primary lookup key; models must implement the full range-check and two-level table traversal.
- SEC_SID must be modeled as part of StreamID disambiguation. A physical StreamID value of N refers to two distinct streams (Secure and Non-secure) when Secure state is supported.
- SubstreamID enables multiple process address spaces to share a single device's STE (same stage 2 VM isolation) while maintaining per-process stage 1 translation. This is the key mechanism for multi-process DMA isolation.

## Related Concepts

- [concepts/stream-table-entry.md](concepts/stream-table-entry.md) — STE located by StreamID
- [concepts/context-descriptor.md](concepts/context-descriptor.md) — CD selected by SubstreamID
- [concepts/security-states.md](concepts/security-states.md) — SEC_SID qualifies StreamID for Secure/NS/Realm disambiguation
- [concepts/pcie-ats-pri.md](concepts/pcie-ats-pri.md) — PASID/SubstreamID mapping in PCIe ATS context

## Sources That Use This Concept

- [sources/ihi0070g-b-smmuv3-architecture-spec.md](sources/ihi0070g-b-smmuv3-architecture-spec.md) — §3.2 Stream numbering; §3.3.1 Stream table lookup; §3.3.2 StreamIDs to Context Descriptors
