---
title: "Command Formats (Chapter 4)"
type: concept
tags: [smmu, commands, tlbi, cfgi, ats, dpti, sync, resume, stall, opcode, encoding]
created: 2026-04-15
updated: 2026-04-15
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Command Formats (Chapter 4)

## Overview

Chapter 4 of the SMMUv3 specification defines all command encodings submitted to the Command queue. Commands are 16-byte entries (two 64-bit words). The first byte of the first word (`[7:0]`) is the **opcode**. All remaining fields are opcode-specific except for the common fields defined in §4.1.6.

Commands are grouped into seven functional categories:

| Category | Opcodes | Purpose |
|---|---|---|
| Prefetch | 0x01, 0x02 | Hint the SMMU to prefetch configuration or TLB entries |
| Config invalidation | 0x03–0x08 | Invalidate STE, CD, VMS, L1STD caches |
| TLB invalidation | 0x10–0x51 | Invalidate TLB entries by scope |
| ATS/PRI | 0x40, 0x41 | Outgoing ATS invalidation, PRI response |
| DPT maintenance | 0x70, 0x73 | Device Permission Table cache invalidation |
| Fault response/sync | 0x44, 0x45, 0x46 | Stall resume, stall terminate, synchronization |
| Implementation Defined | 0x80–0x8F | IMPLEMENTATION DEFINED |

Commands with unsupported opcodes raise `CERROR_ILL`. Command encodings with reserved fields set non-zero may also raise `CERROR_ILL` or be unpredictable.

---

## §4.1.6 Common Command Fields

Two fields appear in nearly all commands:

### SSec (Security state selector)

| SSec | When issued from NS queue | When issued from Secure queue |
|---|---|---|
| 0b0 | Non-secure stream table / Non-secure operation | Non-secure stream table |
| 0b1 | CERROR_ILL | Secure stream table |

- When issued from a Realm command queue, SSec is ignored; the command always applies to Realm StreamIDs.
- When issued from a Root command queue (where applicable), the semantics are Root-specific.

### SSV / SubstreamID

- `SSV == 1`: the SubstreamID field is valid; command targets that specific SubstreamID.
- `SSV == 0`: SubstreamID is IGNORED; command applies to all SubstreamIDs of the StreamID.

### Common address fields

VA fields use bits `[63:12]` of a 64-bit word (bits `[11:0]` are always treated as zero). IPA/PA fields similarly use `[55:12]`.

### Out-of-range parameters (§4.1.7)

When a StreamID, SubstreamID, ASID, or VMID parameter falls outside the implemented range, the behavior is CONSTRAINED UNPREDICTABLE: the command may have no effect or may operate on an UNKNOWN value within the implemented range.

---

## §4.2 Prefetch Commands

### CMD_PREFETCH_CONFIG (opcode 0x01)

```
[7:0] = 0x01
[31:8] = StreamID
[35:32] = SSec
```

Hints the SMMU to prefetch and cache the STE (and optionally associated CDs) for `StreamID`. Consumption of this command does not guarantee prefetch completion. Prefetch is not required to be performed and must not affect correctness.

### CMD_PREFETCH_ADDR (opcode 0x02)

```
[7:0] = 0x02
[31:8] = StreamID
...
[127:72] = Address (VA or IPA, bits [63:12])
```

Hints the SMMU to speculatively translate `Address` for `StreamID`. Has the same non-binding semantics as `CMD_PREFETCH_CONFIG`; the hint is IMPLEMENTATION DEFINED whether used for TLB pre-loading or ignored.

The address span prefetched is given by the optional Stride and Size fields:
- Range = `(NUM+1) × 2^SCALE × Translation_Granule_Size` (when TG is non-zero)
- When `TG == 0b00`, no range hint is given; the SMMU prefetches at least the indicated page.

---

## §4.3 Configuration Invalidation Commands

After any STE, CD, L1STD, L1CD, or VMS structure is modified, the appropriate invalidation command must be issued before the change takes effect in the SMMU. Consumption of a config invalidation command does **not** guarantee completion; a subsequent `CMD_SYNC` does.

### CMD_CFGI_STE (opcode 0x03)

Parameters: `StreamID`, `SSec`, `Leaf`

- Invalidates the STE for `StreamID`.
- `Leaf == 0`: also invalidates all cached L1STD intermediate descriptors that locate the STE; all CDs and L1CDs cached for this StreamID; all VMS data cached using this StreamID.
- `Leaf == 1`: only the STE is required to be invalidated (intermediate L1ST descriptors **not** required). Faster for linear stream tables.
- Arm recommends `Leaf == 1` unless `Leaf == 0` behavior is explicitly required.
- When issued from the Realm command queue, always applies to Realm StreamIDs.

### CMD_CFGI_STE_RANGE (opcode 0x04)

Parameters: `StreamID`, `SSec`, `Range` (0–31)

Invalidates an aligned range of `2^(Range+1)` STEs. Lower `Range+1` bits of `StreamID` are IGNORED (range is always aligned). Also invalidates all L1STDs, CDs, L1CDs, and VMS data for StreamIDs in the range.

`CMD_CFGI_STE_RANGE` with `Range == 31` is the encoding for **CMD_CFGI_ALL**.

### CMD_CFGI_CD (opcode 0x05)

Parameters: `StreamID`, `SSec`, `SubstreamID`, `Leaf`

Invalidates one CD (identified by `SubstreamID` as an index into the CD table for `StreamID`). A single-CD configuration is treated as a one-entry table; use `SubstreamID == 0`.

- `Leaf == 0`: also invalidates intermediate L1CD descriptors that locate this CD.
- `Leaf == 1`: only the CD is required to be invalidated.
- If multiple STEs share a CD table, this command must be issued for **each** StreamID that could have cached the CD.
- Raises `CERROR_ILL` when stage 1 is not implemented.

### CMD_CFGI_CD_ALL (opcode 0x06)

Parameters: `StreamID`, `SSec`

Invalidates **all** CDs and L1CDs cached for `StreamID`. Used when decommissioning a device stream. A separate TLB invalidation command must also be issued for all ASIDs used.

### CMD_CFGI_VMS_PIDM (opcode 0x07)

Parameters: `SSec`, `VMID`

Invalidates cached `VMS.PARTID_MAP` information stored in a cache that is **not** invalidated by `CMD_CFGI_STE*` (i.e., a VMID-indexed PARTID_MAP cache).

Raises `CERROR_ILL` if `SMMU_IDR3.MPAM == 0`, or MPAM/VMS is not supported for the indicated Security state.

Usage procedure for PARTID_MAP change:
1. `CMD_CFGI_VMS_PIDM(SSec, VMID)`
2. `CMD_SYNC`
3. `CMD_CFGI_STE_RANGE` (or `CMD_CFGI_STE`) for all StreamIDs using this VMS
4. `CMD_SYNC`

### CMD_CFGI_ALL (opcode 0x04 / Range==31)

Parameters: `SSec`

Invalidates:
- All cached STEs, L1STDs, CDs, L1CDs for all StreamIDs associated with the Security state given by SSec.
- All VMS information associated with that Security state, including VMID-indexed caches.

Note: Arm recommends following `CMD_CFGI_ALL` with TLB invalidation commands to avoid a race where prefetch uses stale config.

### §4.3.7 Hypervisor translation of guest OS invalidations

| Guest S1 command | Hypervisor action |
|---|---|
| `CMD_CFGI_STE` | Re-shadow STE, then `CMD_CFGI_STE` with mapped host StreamID |
| `CMD_CFGI_STE_RANGE` | Re-shadow STEs, then `CMD_CFGI_STE` or `CMD_CFGI_STE_RANGE` |
| `CMD_CFGI_CD` | `CMD_CFGI_CD` |
| `CMD_CFGI_CD_ALL` | `CMD_CFGI_CD_ALL` |
| `CMD_CFGI_ALL` | `CMD_CFGI_ALL` or per-guest-StreamID `CMD_CFGI_STE` loop |

### §4.3.8 Config invalidation rules

- Stalled transactions are **unaffected** by config invalidation; handle via `CMD_RESUME` or `CMD_STALL_TERM`.
- A transaction in progress when an invalidation is consumed is not required to be re-translated; however, it will not see a partial structure update.
- A subsequent `CMD_SYNC` ensures all in-progress translations using invalidated config are globally observable before it completes.

---

## §4.4 TLB Invalidation Commands

TLB commands mirror Armv8-A TLBI instruction scopes. They affect only the SMMU TLB and do not broadcast. Range-based invalidation and the level hint (TTL/TG) require `SMMU_IDR3.RIL == 1` (mandatory in SMMUv3.2+).

### §4.4.1.1 Common range-based fields (for address-based TLBIs)

| Field | Bits | Meaning |
|---|---|---|
| SCALE | [25:20] | Range multiplier exponent |
| NUM | [16:12] | Range multiplier mantissa |
| TG | [75:74] | Translation granule (0b00=any/no-range, 0b01=4K, 0b10=16K, 0b11=64K) |
| TTL | [73:72] | Translation table level hint |
| TTL128 | [71] | TTL applies to 128-bit descriptor format |
| Leaf | varies | 1=invalidate last-level only; 0=also invalidate table descriptors |

Range formula: `Range = (NUM+1) × 2^SCALE × Translation_Granule_Size`

`TG == 0b00` means no range invalidation and no TTL hint. `NUM == SCALE == 0, TG != 0` is Reserved (CERROR_ILL).

### §4.4.2 Stage 1 TLBI commands

| Command | Opcode | Scope | Parameters |
|---|---|---|---|
| `CMD_TLBI_NH_ALL` | 0x10 | All NS-EL1 entries for VMID (or Secure ALLE1/VMALLE1) | VMID |
| `CMD_TLBI_NH_ASID` | 0x11 | NS-EL1 non-global by ASID+VMID (ASIDE1) | VMID, ASID |
| `CMD_TLBI_NH_VAA` | 0x12 | NS-EL1 all ASIDs by VA+VMID (VAA{L}E1) | VMID, Addr, Leaf |
| `CMD_TLBI_NH_VA` | 0x13 | NS-EL1 by VMID+ASID+VA (VA{L}E1) | VMID, ASID, Addr, Leaf |
| `CMD_TLBI_EL3_ALL` | 0x18 | All EL3 stage 1 entries (ALLE3) | — |
| `CMD_TLBI_EL3_VA` | 0x1C | EL3 by VA (VA{L}E3) | Addr, Leaf |
| `CMD_TLBI_EL2_ALL` | 0x20 | All NS-EL2/Hyp entries (ALLE2) | — |
| `CMD_TLBI_EL2_VA` | 0x28 | NS-EL2 by VA; depends on SMMU_CR2.E2H | ASID, Addr, Leaf |
| `CMD_TLBI_EL2_VAA` | 0x29 | NS-EL2 by VA all ASIDs; depends on E2H | Addr, Leaf |
| `CMD_TLBI_EL2_ASID` | 0x21 | NS-EL2-E2H non-global by ASID (ASIDE1 in E2H mode) | ASID |
| `CMD_TLBI_S_EL2_ALL` | — | Secure S-EL2 equivalent of EL2_ALL | — |
| `CMD_TLBI_S_EL2_VA` | — | Secure S-EL2 equivalent of EL2_VA | ASID, Addr, Leaf |
| `CMD_TLBI_S_EL2_VAA` | — | Secure S-EL2 equivalent of EL2_VAA | Addr, Leaf |
| `CMD_TLBI_S_EL2_ASID` | 0x51 | Secure S-EL2 equivalent of EL2_ASID | ASID |

Notes:
- `EL3_ALL`/`EL3_VA` raise `CERROR_ILL` if `SMMU_IDR0.RME_IMPL == 1` (EL3 StreamWorld not supported with RME) or if issued on the Non-secure queue.
- `EL2_*` commands raise `CERROR_ILL` if `SMMU_IDR0.Hyp == 0`.
- `S_EL2_*` commands raise `CERROR_ILL` on the Non-secure queue or if Secure EL2 is not supported.
- `NH_*` commands: when Secure stage 2 is not supported, VMID is RES0 for Secure queue commands.

### §4.4.3 Stage 2 TLBI commands

| Command | Scope | Parameters |
|---|---|---|
| `CMD_TLBI_S2_IPA` | Stage 2 by VMID+IPA (IPAS2{L}E1) | VMID, Addr, Leaf |
| `CMD_TLBI_S12_VMALL` | All stages for VMID (VMALLS12E1) | VMID |
| `CMD_TLBI_S_S2_IPA` | Secure stage 2 by VMID+IPA+NS flag | VMID, Addr, Leaf, NS |
| `CMD_TLBI_S_S12_VMALL` | All Secure-regime stages for VMID | VMID |

`CMD_TLBI_S2_IPA` is **not** required to invalidate combined stage 1+2 TLB entries; pair it with `CMD_TLBI_NH_ALL` or `CMD_TLBI_NH_VAA` for nested configs.

### §4.4.4 Common TLB invalidation

| Command | Opcode | Scope |
|---|---|---|
| `CMD_TLBI_NSNH_ALL` | — | All NS stages all scopes (ALLE1 for Non-secure); from Realm queue = Realm ALLE1 |
| `CMD_TLBI_SNH_ALL` | — | All Secure stages all scopes (Secure ALLE1); CERROR_ILL on NS queue |

---

## §4.5 ATS and PRI Commands

ATS/PRI commands issue outgoing requests to a connected Root Complex. Ignored silently if ATS is not fully supported by the system.

### CMD_ATC_INV (opcode 0x40)

Parameters: `StreamID`, `SubstreamID`, `SSV`, `Global`, `Address`, `Size`

Sends an ATS Invalidation Request to `StreamID`. Invalidates all ATC entries within:
```
Address[aligned] .. Address + 4096 × 2^Size
```

- `Size == 52` means invalidate all.
- `Global == 1` (only with `SSV == 1`): sets the PCIe Global Invalidate flag in the Invalidation Request.
- Completion guaranteed only by a subsequent `CMD_SYNC` (analogous to TLB invalidation).
- Typical sequence for updating a translation:
  1. Update translation table entry
  2. `CMD_TLBI_NH_VA(VMID, ASID, VA, Leaf=1)`
  3. `CMD_SYNC`
  4. `CMD_ATC_INV(SID, SSID, SSV, Global=0, VA)`
  5. `CMD_SYNC` (wait for completion)

### CMD_PRI_RESP (opcode 0x41)

Parameters: `StreamID`, `SubstreamID`, `SSV`, `PRGIndex`, `Resp`

Responds to a page request group from a PRI-capable endpoint:

| Resp | Meaning |
|---|---|
| 0b00 | ResponseFailure: Permanent error |
| 0b01 | InvalidRequest: Page-in unsuccessful |
| 0b10 | Success: All pages paged in |
| 0b11 | Reserved (CERROR_ILL) |

Raises `CERROR_ILL` if `SMMU_IDR0.PRI == 0`.

---

## §4.6 DPT Maintenance Commands

DPT (Device Permission Table) commands invalidate cached DPT lookup results. Require `SMMU_IDR3.DPT == 1`.

### CMD_DPTI_ALL (opcode 0x70)

Removes all cached DPT information for the target Security state (Realm, Non-secure, or Non-secure from Secure queue with `SAMS==0`).

### CMD_DPTI_PA (opcode 0x73)

Parameters: `Address`, `SIZE`, `Leaf`

Removes cached DPT information for the aligned region of length `SIZE` starting at `Address`:

| SIZE | Region |
|---|---|
| 0b0000 | 4 KB |
| 0b0001 | 16 KB |
| 0b0010 | 64 KB |
| 0b0011 | 2 MB |
| 0b0100 | 32 MB |
| 0b0101 | 512 MB |
| 0b0110 | 1 GB |
| 0b0111 | 16 GB |
| 0b1000 | 64 GB |
| 0b1001 | 512 GB |

`Leaf == 0`: invalidate all levels of the DPT walk. `Leaf == 1`: final level only.

A DPT TLB entry is only guaranteed to be invalidated if `SIZE >= region size of the entry`.

---

## §4.7 Fault Response and Synchronization Commands

### CMD_RESUME (opcode 0x44)

Parameters: `StreamID`, `SSec`, `STAG`, `Action (Ac)`, `Abort (Ab)`

Resumes or terminates a stalled transaction identified by `StreamID` and `STAG` (opaque token from the stall event record).

| Ac | Ab | Result |
|---|---|---|
| 1 (Retry) | — | Transaction is retried from scratch |
| 0 (Terminate) | 0 | Terminated with RAZ/WI |
| 0 (Terminate) | 1 | Terminated with abort/bus error |

- If `SMMU_IDR0.TERM_MODEL == 1`, `Ab` is IGNORED; always terminates with abort.
- STAG value must be exactly as provided in the corresponding fault event record.
- If STAG does not correspond to any stalled transaction, this command is a no-op.
- Consumption of `CMD_RESUME(Retry)` does not guarantee the transaction has been retried; it guarantees it will be retried in finite time.
- `CMD_RESUME(Terminate)` guarantees termination in finite time unless the transaction completes on early retry first.
- Raises `CERROR_ILL` when the Stall model is not supported.
- Issuing to the Realm command queue results in `CERROR_ILL`.

### CMD_STALL_TERM (opcode 0x45)

Parameters: `StreamID`, `SSec`

Marks **all** stalled transactions from `StreamID` for termination. Must be issued only after:
1. The STE is updated to terminate all new incoming transactions (Config set to abort, no new stalls).
2. Configuration cache invalidation and `CMD_SYNC` for the STE update have completed.

Stream shutdown sequence:
1. Stop the device from issuing new transactions.
2. Set `STE[i].Config = 0b000`, keep `STE[i].V = 1`.
3. `CMD_CFGI_STE(i, ...)` + `CMD_SYNC` (ensures stall events from old config are visible).
4. `CMD_STALL_TERM(i, ...)` — marks remaining stalled transactions for termination.
5. Wait for outstanding device transactions to complete (IMPLEMENTATION DEFINED mechanism).
6. Optional `CMD_SYNC` to ensure visibility of terminated-abort events.
7. Discard all event records for StreamID `i`.

Raises `CERROR_ILL` when the Stall model is not supported. Issuing to the Realm command queue results in `CERROR_ILL`.

### CMD_SYNC (opcode 0x46)

Parameters: `CompISignal (CS)`, `MSIAddress`, `MSIData`, `MSIWriteAttributes (MSIAttr, MSH)`

The synchronization barrier command. When consumed, it ensures all prior commands in the same Command queue have completed.

**CompISignal encodings:**

| CS | Signal |
|---|---|
| 0b00 | SIG_NONE: No completion signal |
| 0b01 | SIG_IRQ: Raise interrupt (MSI write if configured, wired if supported) |
| 0b10 | SIG_SEV: Send PE WFE wakeup event |
| 0b11 | Reserved (CERROR_ILL) |

**MSI configuration:**
- `MSIAttr`: Write memory type (encoded as `STE.MemAttr`)
- `MSH`: Shareability for MSI write (0b00=NSH, 0b10=OSH, 0b11=ISH)
- `MSIAddress[55:2]`: Physical address for MSI write (zero = no write even if SIG_IRQ)
- `MSI_NS` (Realm queue only): 0 = Realm PA space, 1 = Non-secure PA space

**Guarantees when CMD_SYNC completes:**

| Prior command type | Guarantee |
|---|---|
| `CMD_TLBI_*`, `CMD_ATC_INV` | All targeted TLB/ATC entries invalidated; all affected transactions observable to their Shareability domain |
| `CMD_CFGI_*` | All targeted config cache entries invalidated; affected in-progress translations observable |
| `CMD_PREFETCH_*` | Prefetch table walks affected by any TLBI/CFGI in the same sync interval |
| `CMD_PRI_RESP` | No additional guarantee (SMMU cannot guarantee endpoint visibility) |
| `CMD_RESUME`, `CMD_STALL_TERM` | No additional guarantee (already complete when consumed) |
| Prior `CMD_SYNC` | Prior CMD_SYNC guarantees have been met; MSI writes visible or abort reported |

**Additional CMD_SYNC event-visibility guarantee:** Any fault event record for a client transaction terminated before the CMD_SYNC that could have been observed by the client is guaranteed to have either been made visible in the Event queue or permanently discarded (if the queue is not writable).

**ATC timeout behavior:** If a prior `CMD_ATC_INV` times out, the `CMD_SYNC` raises `CERROR_ATC_INV_SYNC`. In this case, the CMD_SYNC is not consumed (`SMMU_CMDQ_CONS.RD` remains on the CMD_SYNC); no completion signal is sent; and the completion guarantees are not met.

---

## §4.8 Command Consumption Summary

| Command type | Consumption means |
|---|---|
| TLB and ATS invalidation (`CMD_TLBI_*`, `CMD_ATC_INV`) | Nothing (no completion guarantee) |
| Config invalidation (`CMD_CFGI_*`) | Nothing |
| Prefetch (`CMD_PREFETCH_*`) | Nothing |
| PRI responses (`CMD_PRI_RESP`) | Nothing |
| Stall commands (`CMD_RESUME`, `CMD_STALL_TERM`) | Individual completion guarantees have been met |
| Synchronization (`CMD_SYNC`) | Completion guarantees of CMD_SYNC have been met |

---

## Related Concepts

- [command-queue.md](command-queue.md) — Queue mechanics, circular buffer implementation, ECMDQ, error handling
- [tlb-invalidation.md](tlb-invalidation.md) — TLB tagging, ASID/VMID parameters, BBM levels, 7-step update procedures
- [stream-table-entry.md](stream-table-entry.md) — STE Config field drives which TLBIs and CFGIs are needed
- [context-descriptor.md](context-descriptor.md) — CD ASID/VMID fields are targets of CMD_TLBI_NH_ASID/CMD_TLBI_NH_VA
- [fault-models.md](fault-models.md) — Stall model; CMD_RESUME and CMD_STALL_TERM are the stall response interface
- [pcie-ats-pri.md](pcie-ats-pri.md) — CMD_ATC_INV and CMD_PRI_RESP are the ATS/PRI software interface
- [device-permission-table.md](device-permission-table.md) — CMD_DPTI_ALL and CMD_DPTI_PA invalidate DPT caches
- [../synthesis/smmu-queue-mechanics.md](../synthesis/smmu-queue-mechanics.md) — Queue producer/consumer mechanics; ECMDQ; CMD_SYNC ordering

## Sources That Use This Concept

- [../sources/ihi0070g-b-smmuv3-architecture-spec.md](../sources/ihi0070g-b-smmuv3-architecture-spec.md) — Chapter 4 Commands; §4.1–§4.8
