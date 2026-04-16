---
title: "SMMU System and Implementation Considerations"
type: synthesis
tags: [smmu, implementation, caching, system-integration, pcie, amba, cmo, mpam, far-atomics]
created: 2026-04-13
updated: 2026-04-15
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# SMMU System and Implementation Considerations

This page synthesizes Chapter 16 of the SMMUv3 specification: guidance for implementers and system integrators on caching architectures, bus integration, software requirements, and feature-specific behaviors.

---

## Caching Architecture (§16.2)

### Implementation Freedom

An SMMU is not required to implement any caching, but performance requirements will typically demand it. Caches may be:
- Separate per structure type (STE cache, CD cache, VMS cache, S1 TLB, S2 TLB, walk caches).
- Combined (e.g., STE+CD combined, S1+S2 combined TLB, all-in-one STE+CD+TLB).

The invalidation semantics of a combined cache entry are the **union** of the semantics of its constituent structures. An entry is invalidated if any part of it would have been invalidated by an equivalent operation on the discrete structures.

### Lookup Order for Combined Caches

For a (StreamID, SubstreamID, VA) input:
1. Resolve StreamID → STE fields (ASID, VMID, StreamWorld).
2. Use SubstreamID → CD (if stage 1 enabled).
3. Look up TLB with (VA, ASID, VMID, EL/StreamWorld).

### Per-Structure Cache Properties

| Cache | Index | Invalidated by |
|---|---|---|
| STE | StreamID | StreamID, StreamID-span, or all |
| L1STD (pointer) | StreamID (upper bits) | Same as STE |
| CD | StreamID + SubstreamID | StreamID+SubstreamID, StreamID, or all |
| L1CD (pointer) | StreamID+SubstreamID | Same as CD |
| VMS | VMID or StreamID | VMID or StreamID |
| S1 TLB | VA + ASID + VMID + EL | VA+ASID+VMID+EL, ASID+VMID+EL, VMID+EL, EL |
| S2 TLB | IPA + VMID + EL | IPA+VMID, VMID, all |

Walk caches (intermediate translation table descriptors) are invalidated under the same conditions as PE translation walk caches.

### Data Dependencies Between Structures (§16.2.2)

Structures form a chain: **L1STD → STE → L1CD → CD → TT (S1) / TT (S2)**

An STE invalidation also invalidates all CDs that were cached through that STE, because STE fields affect CD interpretation (StreamWorld, S1STALLD, etc.). A change to any STE field requires an STE invalidation command.

CD fields that may be cached in TLB entries: `{ASID, ASET, HA, HD, AFFD, HADx, MAIR, AMAIR}`. Changes to these fields require TLB invalidation in addition to CD invalidation.

Some configuration register fields are permitted to be cached in TLB entries (noted in register field descriptions). Changes require TLB invalidation.

---

## Programming Implications of Address Sizing (§16.3)

- SMMU performs sign-extension checks on input addresses (matching PE behavior). If a device cannot convey all 64 address bits, the SMMU cannot perform these checks.
- Effective TBI (Top Byte Ignore) behavior occurs when address bits are truncated ≤56 bits before reaching the SMMU.
- Software or the device must validate upper address bits if required and the interconnect truncates them.

---

## System Integration Requirements (§16.4)

- The SMMU must be in the **same Shareability domain** as any agent that uses DVM with it.
  - Pre-Armv8.4: DVM broadcast over Inner Shareable domain only.
  - Armv8.4+: Some systems broadcast over Outer Shareable.
- Arm **does not recommend** SMMUs connected in series. Two-stage translation must use a single SMMU that supports both stages, not two single-stage SMMUs in series.
- **PCIe integration:** Must support at least the full 16-bit PCI RequesterID range as StreamIDs. `StreamID[15:0] == RequesterID[15:0]`. Larger StreamIDs may concatenate domain/segment bits above bit 15.
- When PASIDs are used with PCIe, Arm recommends SMMU SubstreamID capacity ≥ PCIe endpoint PASID bits for end-to-end capability detection.
- **Stall and PCIe:** PCIe endpoint streams **must not** use the Stall fault model. PCIe traffic must always make forward progress. A stalled PCIe transaction risks timeout or deadlock.

### RME DA Integration (§16.4.1)

- A device interface that can operate as Trusted or Untrusted (SEC_SID = Non-secure or Realm) presents the same StreamID to the SMMU in both modes.
- TDISP-compliant PCIe devices may issue MSIs via MSI capability (T=0 → SEC_SID=Non-secure) or via protected MMIO region (T=1 → SEC_SID=Realm); the DeviceID presented to GIC ITS is the same in both cases.

---

## Software Requirements (§16.5)

Software drivers must:
- Not assume both stage 1 and stage 2 are present.
- Support systems without broadcast TLB invalidation (fall back to software invalidation commands).
- Discover StreamID and SubstreamID sizes from IDR registers.
- Probe `SMMU_IDR1` for `PRESET` configuration; only allocate memory for non-preset pointers.
- Discover maximum table sizes from IDR registers rather than assuming fixed sizes.
- Not assume which Security state(s) it is interacting with.
- Present system-specific StreamIDs via firmware descriptions (StreamIDs are system-specific, not device-specific).
- Ensure DMA memory descriptors have the Accessed flag set (and not read-only if writes are expected) when HTTU is not in use, to avoid AF faults.

---

## IMPLEMENTATION DEFINED Features (§16.6)

### Cache Locking (§16.6.1)

An implementation may support TLB/configuration cache locking via IMPLEMENTATION DEFINED registers. Rules:
- `TLB_invalidate_all` does not invalidate locked entries.
- Invalidate-by-VA or Invalidate-by-ASID: implementation choice whether locked entries matching the operation are invalidated.
- `SMMU_S_INIT.INV_ALL` **does** invalidate locked entries.
- CMD_CFGI_* behavior on locked configuration cache entries is IMPLEMENTATION DEFINED.

---

## Interconnect-Specific Features (§16.7)

### Unsupported Client Transactions (§16.7.1)

An implementation may define its own input alignment restrictions (e.g., transactions crossing 4 KB boundary rejected on AMBA downstream). Such transactions are aborted and `F_UUT` recorded.

AMBA 4 unsupported transactions:
- Far Atomic operations where not supported by downstream interconnect.

### Cache Maintenance Operations (§16.7.2)

**SMMUv3.0:** No CMO support; implementation-defined handling.
**SMMUv3.1+:**

- Address-based CMOs (Clean, Invalidate, CleanInvalidate, CleanToPersistence, Destructive Hint) are supported.
- Non-address-based CMOs are silently terminated.
- When bypass: CMOs pass through (Secure: still subject to SIF check).
- When one+ stages translated: subject to `STE.DRE` control and permission model.

**STE.DRE control** (§16.7.2.1):
| Input | `DRE == 0` | `DRE == 1` |
|---|---|---|
| Invalidate | Transformed → CleanInvalidate | Eligible as Invalidate |
| Destructive Hint (DH) | Transformed → No-op | Eligible as DH |

**Permission requirements** (§16.7.2.2):
| CMO type | Required permissions |
|---|---|
| Clean, CleanInvalidate, CleanToPersistence | Read or Execute (per INSTCFG) at appropriate privilege |
| Invalidate | Both Read/Exec AND Write (else downgraded to CleanInvalidate if only Read/Exec) |
| Destructive Hint | Both Read/Exec AND Write without HTTU dirty; else treated as No-op |

CMO faults are recorded as reads (`RnW == 1`). Stall behavior for CMOs is the same as for ordinary reads.

**Shareability for CMOs (§16.7.2.3):** CMOs have no input memory type. Input shareability is normalized per §13.1.3 defaults if not supplied. Output shareability determined the same way as ordinary transactions.

### AMBA Exclusives (§16.7.3)

ACE-Lite does not permit Exclusive accesses to IS/OS domains. If an NS/SS Exclusive access is translated to IS/OS, Arm recommends transforming it to a non-Exclusive. The non-Exclusive response is treated as an Exclusive Fail by the upstream device.

### Downstream Aborts (§16.7.4)

Client-originated translated transactions that are aborted in the memory system are **not** recorded by the SMMU; the abort is returned to the client device. SMMU-originated transaction aborts are recorded:

| Access | Event recorded |
|---|---|
| STE fetch | `F_STE_FETCH` |
| CD fetch | `F_CD_FETCH` |
| VMS fetch | `F_VMS_FETCH` |
| Translation table walk | `F_WALK_EABT` |
| Command queue read | `GERROR.CMDQ_ERR` + `CERROR_ABT` |
| Event queue access | `GERROR.EVENTQ_ABT_ERR` |
| PRI queue access | `GERROR.PRIQ_ABT_ERR` |

### SMMU and AMBA Attribute Differences (§16.7.5)

#### §16.7.5.1.1 AMBA → Armv8 Input Conversion

Incoming AMBA attributes are converted to SMMU/Armv8 architectural attributes:

| AMBA attribute | Armv8 attribute | Notes |
|---|---|---|
| Device-Sys non-bufferable | Device-nGnRnE | |
| Device-Sys bufferable | Device-nGnRE | |
| Normal-Non-cacheable-Sys (bufferable or non-bufferable) | Normal-iNC-oNC-OSH | |
| Normal-Non-cacheable {NSH,ISH,OSH} (bufferable or non-bufferable) | Normal-iNC-oNC-OSH **or** Normal-iWB-oNC-{NSH,ISH,OSH} | IMPLEMENTATION DEFINED; if iWB-oNC: RA=WA=1, Non-transient |
| Normal-WriteThrough-{NSH,ISH,OSH} | Normal-iNC-oNC-OSH **or** Normal-iWT-oWT-{NSH,ISH,OSH} | IMPLEMENTATION DEFINED; if iWT-oWT: RA/WA from input, Non-transient |
| Normal-WriteBack-{NSH,ISH,OSH} | Normal-iWB-oWB-{NSH,ISH,OSH} | RA/WA from input, always Non-transient |

An ACE-Sys input Shareability domain is treated as OSH for the purposes of attribute combining and overriding.

#### §16.7.5.2.1 Armv8 → AMBA Output Conversion

SMMU/Armv8 architectural attributes are converted to AMBA attributes on output:

| Armv8 attribute | AMBA attribute | Notes |
|---|---|---|
| Device-nGnRnE | Device-Sys non-bufferable | |
| Device-(n)G(n)RE | Device-Sys bufferable | |
| Normal-iNC-oNC-OSH | Normal-Non-cacheable-Sys bufferable | Architecturally, iNC-oNC-{NSH,ISH} is not possible |
| Normal-iNC-oWT-{NSH,ISH,OSH} | Normal-Non-cacheable-Sys bufferable | (1) |
| Normal-iNC-oWB-{NSH,ISH,OSH} | Normal-Non-cacheable-Sys bufferable | (1) |
| Normal-iWT-oNC-{NSH,ISH,OSH} | Normal-Non-cacheable-Sys bufferable | (1) |
| Normal-iWT-oWT-{NSH,ISH,OSH} | Normal-Non-cacheable-Sys bufferable | (1) |
| Normal-iWT-oWB-{NSH,ISH,OSH} | Normal-Non-cacheable-Sys bufferable | (1) |
| Normal-iWB-oNC-{NSH,ISH,OSH} | Normal-Non-cacheable-Sys bufferable | (1) |
| Normal-iWB-oWT-{NSH,ISH,OSH} | Normal-Non-cacheable-Sys bufferable | (1) |
| Normal-iWB-oWB-{NSH,ISH,OSH} | Normal-WriteBack-{NSH,ISH,OSH} | |

(1) Per §16.7.5.3: In systems requiring interoperation with Arm Cortex PE IP, any Normal memory attribute that is not iWB-oWB is transformed to iNC-oNC-OSH, output as ACE-NC-Sys. For other PE IP, these transformations are IMPLEMENTATION DEFINED.

When a cacheable type is output, AMBA RA and WA attributes are generated directly from the Arm RA/WA portion. The output AxDOMAIN is made consistent with AxCACHE: Device or Non-cacheable types always use ACE-Sys (highest domain).

Arm strongly recommends supporting all architected access types for compatibility with generic driver software.

### Far Atomic Operations (§16.7.6)

Far Atomics are treated as both a data read and a write for permission-checking purposes. The `INST` attribute override from `STE.INSTCFG` is ignored for Far Atomics (always Data).

---

## SMMU and PCIe/AMBA Transaction Equivalents (§16.8)

Table 16.6 maps SMMU transaction types to their PCIe, AXI/ACE-Lite (AMBA), DTI, and LTI equivalents:

| SMMU Transaction | PCIe Equivalent | AXI/ACE-Lite Signal | AXI/ACE-Lite Opcode | DTI | LTI (LATRANS) |
|---|---|---|---|---|---|
| Ordinary read request | Memory read request | `ARSNOOP` | ReadNoSnoop, ReadOnce | `DTI_TBU_TRANS_REQ.PERM == R` | R |
| RCI | Not applicable | `ARSNOOP` | ReadOnceCleanInvalid | `PERM == R` | R-CMO |
| DR (Destructive Read) | Not applicable | `ARSNOOP` | ReadOnceMakeInvalid | `PERM == R` | R-DCMO |
| Speculative transaction | Not applicable | Not applicable | Not applicable | Not applicable | Not applicable |
| Far Atomic operations | FetchAdd, Swap, CAS | `AWATOP` | AtomicStore, AtomicLoad, AtomicSwap, AtomicCompare | `PERM == RW` | RW |
| Ordinary write transaction | Memory write request | `AWSNOOP` | WriteNoSnoop, WriteUniquePtl, WriteNoSnoopFull, WriteUniqueFull, WriteZero | `PERM == W` | W |
| W-DCP | Memory write with TLP Processing Hint (non-zero Steering Tag) | `AWSNOOP` | WriteUniquePtlStash, WriteUniqueFullStash | `PERM == W` | W-DCP |
| NW-DCP | Zero-length Write with TLP Processing Hint (non-zero ST) | `AWSNOOP` | StashOnceShared, StashOnceUnique | `PERM == SPEC` | DCP |
| DH (Destructive Hint) | Not applicable | `AWSNOOP` | InvalidateHint | `PERM == SPEC` | DHCMO |
| Clean / CleanInvalidate | Not applicable | `ARSNOOP` | CleanShared, CleanInvalid, CleanSharedPersist | `PERM == R` | CMO |
| Invalidate | Not applicable | `ARSNOOP` | MakeInvalid | `PERM == R` | DCMO |
| Ordinary translation request | Not applicable | Not applicable | Not applicable | `DTI_TBU_TRANS_REQ.PERM` depends on request type | Not applicable |
| Ordinary speculative translation request | Not applicable | Not applicable | Not applicable | `PERM == SPEC` | Not applicable |
| ATS Translation Request | ATS Translation Request | Not applicable | Not applicable | `DTI_ATS_TRANS_REQ.nW` depends on request type | Not applicable |
| ATS PRI | ATS PRI | Not applicable | Not applicable | `DTI_ATS_PAGE_REQ.{READ,WRITE}` depends on request type | Not applicable |

## Relationship to Other Wiki Pages

- [../concepts/tlb-invalidation.md](../concepts/tlb-invalidation.md) — Invalidation command details; combined cache invalidation semantics
- [../concepts/smmu-initialization.md](../concepts/smmu-initialization.md) — CR0/CR0ACK sequence; initialization prerequisites
- [../concepts/attribute-transformation.md](../concepts/attribute-transformation.md) — Detailed attribute override and combine rules (Chapter 13)
- [../concepts/fault-models.md](../concepts/fault-models.md) — CMO fault recording; F_UUT; stall behavior
- [../concepts/pcie-ats-pri.md](../concepts/pcie-ats-pri.md) — PCIe-specific integration requirements; stall prohibition; TDISP
- [../concepts/security-states.md](../concepts/security-states.md) — RME DA StreamID/DeviceID integration rules
- [../concepts/ras.md](../concepts/ras.md) — RAS/SFM system-level requirements; error node integration; SFM entry conditions (Ch. 12)
- [smmu-fault-model.md](smmu-fault-model.md) — Full fault detection and reporting
- [smmu-queue-mechanics.md](smmu-queue-mechanics.md) — Queue implementation constraints
