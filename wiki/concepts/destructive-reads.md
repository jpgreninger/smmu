---
title: "Destructive Reads and Directed Cache Prefetch"
type: concept
tags: [smmu, destructive-read, cache-prefetch, amba, axi5, smmuv3.1, ste]
created: 2026-04-13
updated: 2026-04-13
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Destructive Reads and Directed Cache Prefetch

## Definition

Section 3.22 of the SMMUv3 specification defines four special transaction classes that carry hints beyond ordinary read or write semantics. These classes are distinct from Cache Maintenance Operations and are handled differently by the SMMU.

| Class | AMBA AXI5 example | Description |
|---|---|---|
| **RCI** — Read with clean and invalidate | `ReadOnceCleanInvalid` | Read with a side effect hint to clean and invalidate addressed cache lines |
| **DR** — Destructive read | `ReadOnceMakeInvalid` | Read that intentionally invalidates addressed cache lines without writeback, even if dirty |
| **W-DCP** — Write with directed cache prefetch | `WriteUniquePtlStash`, `WriteUniqueFullStash` | Write with a hint to prefetch into part of the cache hierarchy not on the direct path to memory; no data-destructive side effects |
| **NW-DCP** — Directed cache prefetch without write data | `StashOnceShared`, `StashOnceUnique` | Cache prefetch hint only; neither read nor write data is transferred |

## Version Behavior

- **SMMUv3.0:** These transaction classes are **not supported**. They are unconditionally converted at output as specified by the interconnect architecture.
- **SMMUv3.1 and later:** These transactions are **permitted to pass unmodified** when the transaction bypasses all implemented stages of translation, specifically when:
  - `SMMU_(*_)CR0.SMMUEN == 0` for the security state of the stream (global bypass); affected by `SMMU_(*_)GBPA` overrides the same way as ordinary transactions.
  - `SMMUEN == 1` but the stream's valid STE has `STE.Config == 0b100` (stream bypass).
  - The STE has `STE.S1DSS == 0b01` and `STE.Config == 0b101`, and no SubstreamID is provided (stage 1 skipped via S1DSS).

When translation stages are applied (§3.22.1 and §3.22.2), the `STE.DRE` and `STE.DCP` fields control whether each class passes unmodified or is downgraded.

## §3.22.1 Control of Transaction Downgrade

| Input class | Required STE field | Behavior if field not set |
|---|---|---|
| RCI | None (no additional requirement) | May pass unmodified; implementation may downgrade to ordinary read |
| DR | `STE.DRE == 1` | Downgraded to non-destructive read (ordinary read or RCI) |
| W-DCP | `STE.DCP == 1` | Downgraded to ordinary write |
| NW-DCP | `STE.DCP == 1` | Downgraded to no-op (completes successfully with no memory system effect) |

A transaction is downgraded if the enabled STE control is absent. If `STE.DRE`/`STE.DCP` is set, the transaction may still be downgraded if required permissions or memory type constraints are not met (see below).

The SMMU implementation is always permitted to downgrade these transactions for any reason. Arm recommends that the common behavior is to avoid unnecessary downgrades.

## §3.22.2 Permissions Model

When one or more translation stages are applied, the permission requirements for each class are:

| Transaction type | Required permissions | Fault/downgrade behavior if not met |
|---|---|---|
| RCI | Same as ordinary read: Read or Execute (per `STE.INSTCFG` and `InD`), at appropriate privilege (per `STE.PRIVCFG` and `PnU`) | Same as ordinary read fault |
| DR | Read or Execute **AND** Write permission that does not trigger HTTU dirty state promotion | If no Write access: downgraded to read or RCI. If no Read/Execute: treated identically to ordinary read fault |
| W-DCP | Same as ordinary write: Write permission at appropriate privilege | Same as ordinary write fault |
| NW-DCP | Read, or non-dirty-promoting Write, or Execute at appropriate privilege at each enabled stage. Whether evaluated as combined or per-stage is IMPLEMENTATION SPECIFIC | Prefetch does not occur; NW-DCP does not cause an abort response |

**Fault recording:** If RCI or DR leads to a fault, it is recorded as a read (data or instruction per `STE.INSTCFG`/`InD`). W-DCP faults are recorded as writes. RCI, DR, and W-DCP may enter the stall model if configured with `CD.S` or `STE.S2S`; on retry, they are retried as the same transaction type.

## §3.22.3 Memory Types and Shareability

The output interconnect architecture may impose constraints on which memory types and Shareability are valid for output DR, RCI, W-DCP, and NW-DCP transactions. If the determined output attributes are not valid for the transaction class, the SMMU downgrades the transaction at the point of final output. This applies in **all** configurations:

- Global bypass (attributes from GBPA).
- STE bypass (`STE.Config == 0b100` or `STE.S1DSS == 0b01`).
- Translated path.

**AMBA AXI5 constraints:**
- `W-DCP` (`WriteUniquePtlStash`, `WriteUniqueFullStash`) — **not permitted** with Non-shareable or System Shareability.
- `NW-DCP` (`StashOnceShared`, `StashOnceUnique`) — **not permitted** with System Shareability.
- `RCI` and `DR` (`ReadOnceCleanInvalid`, `ReadOnceMakeInvalid`) — **not permitted** with NSH or System Shareability.

## Related Concepts

- [[concepts/attribute-transformation]] — memory type and Shareability determination; AMBA AXI5 output attribute rules (§16.7.5)
- [[concepts/stream-table-entry]] — `STE.DRE` and `STE.DCP` fields; `STE.Config`, `STE.S1DSS`, `STE.INSTCFG`, `STE.PRIVCFG`
- [[concepts/httu]] — dirty state promotion; DR requires Write permission that does NOT promote dirty state
- [[concepts/speculative-accesses]] — NW-DCP is issued speculatively; DR and RCI are not speculative
- [[synthesis/smmu-system-implementation]] — §16.7.2 Non-data transfer transactions

## Sources That Use This Concept

- [[sources/ihi0070g-b-smmuv3-architecture-spec]] — §3.22 Destructive reads and directed cache prefetch transactions; §3.22.1–3; §16.7.2; §16.7.5
