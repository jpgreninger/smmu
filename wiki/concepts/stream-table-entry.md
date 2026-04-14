---
title: "Stream Table Entry (STE)"
type: concept
tags: [smmu, ste, stream-table, configuration, data-structure]
created: 2026-04-07
updated: 2026-04-07
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Stream Table Entry (STE)

## Definition

A Stream Table Entry (STE) is the per-stream configuration structure in the SMMU. It is located by indexing the Stream table with the incoming transaction's [[concepts/streamid-substreamid]]. Each STE describes:

- Whether the stream is disabled, bypassed, or subject to stage 1 and/or stage 2 translation.
- The stage 2 translation table base pointer (`STE.S2TTB`) and VMID (`STE.S2VMID`).
- A pointer (`STE.S1ContextPtr`) to the [[concepts/context-descriptor]] or CD table for stage 1 config.
- Security and attribute override configuration.
- Fault behavior configuration for stage 2 (`STE.S2R`, `STE.S2S`).
- ATS/PCIe integration flags (`STE.EATS`).
- Event merging control (`STE.MEV`).

## Stream Table Formats

Two stream table formats are supported:

### Linear Stream Table
A contiguous array of STEs indexed directly by StreamID. Supported by all implementations. Size is `2^n` entries (configurable up to the maximum StreamID bits supported by the hardware).

### 2-Level Stream Table
A top-level table of L1STDs (Level 1 Stream Table Descriptors), each pointing to a second-level linear array of STEs. The split point (upper/lower StreamID bit boundary) is configured by `SMMU_(*_)STRTAB_BASE_CFG.SPLIT` and can be 6, 8, or 10 bits. This format is required for implementations supporting more than 64 StreamIDs.

| SIDSIZE | SPLIT | L1 table size | L2 table size |
|---------|-------|---------------|---------------|
| 16      | 6     | 8 KB          | 4 KB          |
| 16      | 8     | 2 KB          | 16 KB         |
| 16      | 10    | 512 B         | 64 KB         |
| 24      | 6     | 2 MB          | 4 KB          |
| 24      | 8     | 512 KB        | 16 KB         |
| 24      | 10    | 128 KB        | 64 KB         |

## STE.Config Encoding

The `STE.Config` field (3 bits) determines the translation behavior for a stream:

| STE.Config | Behavior |
|------------|----------|
| 0b000      | Stream disabled — transaction terminated with abort, F_STREAM_DISABLED recorded |
| 0b100      | Stream bypass — no translation; attributes from STE overrides |
| 0b101      | Stage 1 only |
| 0b110      | Stage 2 only |
| 0b111      | Stage 1 and Stage 2 (nested) |

Other encodings are ILLEGAL or reserved.

## Stage 1 Context Pointer and CD Layout

`STE.S1ContextPtr` points to one of:
- A **single CD** — for streams with no substreams.
- A **flat array of CDs** — indexed by SubstreamID.
- A **2-level L1CD/CD table** — L1CD table indexed by upper SubstreamID bits; each L1CD points to a L2 array of CDs indexed by lower SubstreamID bits. Configured via `STE.S1Fmt` and `STE.S1CDMax`.

When both stage 1 and stage 2 are active, `S1ContextPtr` is an IPA (translated through stage 2). When only stage 1 is active, it is a PA.

## SubstreamID Default Behavior (STE.S1DSS)

When substreams are configured but a transaction arrives without a SubstreamID:

| STE.S1DSS | Behavior |
|-----------|----------|
| 0b00      | Error — transaction aborted, event recorded |
| 0b01      | Transaction treated as stage 1 bypass |
| 0b10      | Transaction uses CD[0]; SubstreamID 0 transactions are aborted |

## Stage 2 Configuration Fields

- `STE.S2TTB` — base address of stage 2 translation table (PA).
- `STE.S2VMID` — VMID tagging stage 2 TLB entries.
- `STE.S2T0SZ` — input address range for stage 2.
- `STE.S2TG` — stage 2 translation granule.
- `STE.S2PS` — output address size cap for stage 2.
- `STE.S2AA64` — translation table format (VMSAv8-64 vs VMSAv8-32 LPAE).
- `STE.S2R` — stage 2 fault record enable.
- `STE.S2S` — stage 2 stall enable.
- `STE.S1STALLD` — disables stall model at stage 1 for this stream.

## Model Implementation Notes

- The STE is the root of all per-stream configuration. A functional model must implement the full STE validity check before beginning any translation.
- Configuration errors (invalid/ILLEGAL STE) produce C_BAD_STE events and terminate the transaction — the translation pipeline must not proceed.
- A single STE can be shared across multiple client devices via multiple StreamIDs pointing to the same stage 2 translation tables (same VM).
- Multiple STEs may share CDs (same OS process across multiple devices).
- `STE.MEV` controls event merging on a per-stream basis; software SMMU emulations are not required to honor MEV.

## Related Concepts

- [[concepts/streamid-substreamid]] — key used to index the stream table
- [[concepts/two-stage-translation]] — translation pipeline STE feeds into
- [[concepts/context-descriptor]] — stage 1 config pointed to by STE
- [[concepts/fault-models]] — STE.S2S, STE.S2R, STE.S1STALLD govern stage fault behavior
- [[concepts/pcie-ats-pri]] — STE.EATS controls ATS behavior per stream
- [[concepts/security-states]] — Secure STEs live in a separate Secure stream table

## Sources That Use This Concept

- [[sources/ihi0070g-b-smmuv3-architecture-spec]] — §3.3.1 Stream table lookup; §3.3.2 StreamIDs to Context Descriptors; §5.2 STE data structure format; §3.12 Fault models
