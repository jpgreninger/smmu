---
title: "Stream Table Entry (STE)"
type: concept
tags: [smmu, ste, stream-table, configuration, data-structure]
created: 2026-04-07
updated: 2026-04-16
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Stream Table Entry (STE)

## Definition

A Stream Table Entry (STE) is the per-stream configuration structure in the SMMU. It is located by indexing the Stream table with the incoming transaction's [streamid-substreamid.md](streamid-substreamid.md). Each STE describes:

- Whether the stream is disabled, bypassed, or subject to stage 1 and/or stage 2 translation.
- The stage 2 translation table base pointer (`STE.S2TTB`) and VMID (`STE.S2VMID`).
- A pointer (`STE.S1ContextPtr`) to the [context-descriptor.md](context-descriptor.md) or CD table for stage 1 config.
- Security and attribute override configuration.
- Fault behavior configuration for stage 2 (`STE.S2R`, `STE.S2S`).
- ATS/PCIe integration flags (`STE.EATS`).
- StreamWorld (`STE.STRW`) determining TLB tagging regime.
- MPAM/MECID/permission indirection controls.

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

## STE Field Reference (§5.2)

All undefined fields and many defined fields are permitted to be RAZ/WI in an Embedded Implementation (EI) that stores STEs internally. EI exceptions are noted per field. Invalid or contradictory configurations are ILLEGAL — an ILLEGAL STE behaves as `STE.V == 0`.

### V, bit [0] — STE Valid

| V | Meaning |
|---|---------|
| 0 | Structure contents invalid; other fields IGNORED; device transactions terminated with abort; C_BAD_STE recorded (exception: ATS Translation Requests completed with CA status, no event). |
| 1 | Structure contents valid. |

When `ATSCHK==1`, ATS Translated transactions that select an invalid STE are terminated with abort; no event is recorded.

### Config, bits [3:1] — Stream Configuration

| Config | Traffic can pass? | Stage 1 | Stage 2 | Notes |
|--------|-------------------|---------|---------|-------|
| 0b000  | No | — | — | Transaction terminated with abort; **no event recorded**. ATS Translation Request denied with UR; no event recorded. ATS Translated (ATSCHK==1) silently aborted. |
| 0b0xx  | No | — | — | Reserved (behaves as 0b000) |
| 0b100  | Yes | Bypass | Bypass | STE.EATS effective value == 0b00. ATS Translation Requests cause F_BAD_ATS_TREQ; Translated traffic causes F_TRANSL_FORBIDDEN (ATSCHK==1). |
| 0b101  | Yes | Translate | Bypass | S1* fields valid |
| 0b110  | Yes | Bypass | Translate | S2* fields valid |
| 0b111  | Yes | Translate | Translate | S1* and S2* valid |

**ILLEGAL conditions:**
- `Config == 0b1x1` when `SMMU_IDR0.S1P == 0` (stage 1 not implemented).
- `Config == 0b11x` when `SMMU_IDR0.S2P == 0` (stage 2 not implemented).
- `Config == 0b11x` for Secure STE when `SMMU_S_IDR1.SEL2 == 0`.
- `Config == 0b11x` for Secure STE when `STE.S2AA64` selects VMSAv8-32 LPAE.

### S1Fmt, bits [5:4] — Stage 1 CD Table Format

Ignored when `STE.S1CDMax == 0` or `SMMU_IDR1.SSIDSIZE == 0` (single CD pointed to by S1ContextPtr).

| S1Fmt | Meaning |
|-------|---------|
| 0b00  | Linear array of CDs (or single CD if S1CDMax==0). Indexed by `SubstreamID[S1CDMax-1:0]`. Table aligned to its size. |
| 0b01  | 2-level table with 4 KB L2 leaf tables. L1 table of up to 16384 L1CD pointers (128 KB max); each L2 table holds 64 CDs. L1 indexed by `SubstreamID[S1CDMax-1:6]`; L2 by `SubstreamID[5:0]`. L2 tables 4 KB-aligned. |
| 0b10  | 2-level table with 64 KB L2 leaf tables. L1 table of up to 1024 L1CD pointers (8 KB max); each L2 table holds 1024 CDs. L1 indexed by `SubstreamID[S1CDMax-1:10]`; L2 by `SubstreamID[9:0]`. L2 tables 64 KB-aligned. |
| 0b11  | Reserved (behaves as 0b00) |

**Notes:** If `Config == 0b1x0` (stage 1 disabled), this field is IGNORED; supplying a SubstreamID causes termination with `C_BAD_SUBSTREAMID`. It is ILLEGAL to set `S1Fmt != 0b00` when `SMMU_IDR0.CD2L == 0`. If stage 2 is configured, `S1ContextPtr` is an IPA; in that case L1CD pointers are also IPAs.

### S1ContextPtr, bits [55:6]

Pointer to the Stage 1 Context descriptor (or L1 CD table). Bits above `SMMU_IDR5.OAS` are RES0. When `Config == 0b11x` (stage 2 enabled), this is an IPA translated through stage 2 and must be within the IAS; otherwise it is a PA and must be within the OAS.

In a Realm STE with stage 2 enabled: treated as a Realm IPA. With stage 1 only: treated as a Realm PA.

### S1CDMax, bits [63:59]

log₂ of the number of CDs pointed to by `S1ContextPtr`. Range: 0 to `SMMU_IDR1.SSIDSIZE` (values above are ILLEGAL). When 0: single CD, substreams disabled (SSV=1 transactions cause `C_BAD_SUBSTREAMID`).

### S1DSS, bits [65:64] — Default Substream

When substreams are enabled (`S1CDMax != 0`) and a transaction arrives without a SubstreamID:

| S1DSS | Meaning |
|-------|---------|
| 0b00  | Terminate: abort reported, `F_STREAM_DISABLED` recorded. For ATS Translation Requests: denied with CA, no event. |
| 0b01  | Bypass stage 1 as if `Config == 0b1x0`. Address can cause stage 1 Address Size fault. Stage 2 translation applied if enabled. No CD fetch; no `F_CD_FETCH`, `C_BAD_CD` or stage 2 Translation fault with `CLASS == CD`. |
| 0b10  | Use CD[0]; SubstreamID 0 from transactions that include a substream is terminated (`F_STREAM_DISABLED`). |
| 0b11  | Reserved (behaves as 0b00) |

IGNORED when `Config == 0b1x0`, `S1CDMax == 0`, or `SMMU_IDR1.SSIDSIZE == 0`.

### S1CIR, bits [67:66] — S1ContextPtr Inner Region attribute

| S1CIR | Meaning |
|-------|---------|
| 0b00  | Normal, Non-cacheable |
| 0b01  | Normal, Write-Back cacheable, Read-Allocate |
| 0b10  | Normal, Write-Through cacheable, Read-Allocate |
| 0b11  | Normal, Write-Back cacheable, no Read-Allocate |

Sets memory access attributes for CD (and L1CD) fetches through `S1ContextPtr`. When `Config == 0b11x` (stage 2 enabled), these attributes are combined with the stage 2 translation descriptor attributes for the page mapping the IPA.

### S1COR, bits [69:68] — S1ContextPtr Outer Region attribute

Same encoding as S1CIR, for outer region.

### S1CSH, bits [71:70] — S1ContextPtr Shareability attribute

| S1CSH | Meaning |
|-------|---------|
| 0b00  | Non-shareable |
| 0b01  | Reserved (behaves as 0b00) |
| 0b10  | Outer Shareable |
| 0b11  | Inner Shareable |

**Note:** If both S1CIR and S1COR == 0b00 (Normal Non-cacheable), Shareability is taken as Outer Shareable regardless of this field.

### S2HWU59/60/61/62, bits [72–75] — PBHA Hardware Use bits (SMMUv3.1+)

Each bit controls interpretation of stage 2 page/block descriptor bits [59–62] respectively. When set to 1, the corresponding bit has IMPLEMENTATION DEFINED hardware use. Ignored if `SMMU_IDR3.PBHA == 0`. RES0 in SMMUv3.0. RES0 if `S2AA64` selects VMSAv9-128.

**ILLEGAL interaction:** If any of `S2HWU59–62` are 1, it is ILLEGAL to set `S2POE == 1`.

### DRE, bit [76] — Destructive Read Enable (SMMUv3.1+)

| DRE | Meaning |
|-----|---------|
| 0   | Read-and-invalidate transactions downgraded to read without destructive side-effect; Invalidate CMOs downgraded to CleanInvalidate. |
| 1   | Destructive read transactions permitted (read-and-invalidate). Invalidate CMOs permitted without transformation. Both require correct permissions (§3.22.2). |

IGNORED on implementations not supporting this class. Applies to transactions through at least one stage of translation; also applies to ATS Translated transactions when `ATSCHK==1` with `EATS` selects Full ATS (see DPT conditions). RES0 in SMMUv3.0.

### CONT, bits [80:77] — Contiguous Hint

4-bit hint indicating that this STE is identical to its neighbors in a span of `2^CONT` StreamIDs starting at a StreamID for which `StreamID[CONT-1:0] == 0`. Value 0 means no contiguous hint. Allows SMMU caches to match a single cached STE for any StreamID in the span. All defined fields except CONT must be identical across the span; if not, CONSTRAINED UNPREDICTABLE (may use requested STE, use a neighboring STE in the span, or report `F_CFG_CONFLICT`). This field does not affect configuration invalidation; every STE in the span must be individually targeted by CMD_CFGI_* commands.

**2-level table note:** If CONT spans beyond the L1STD coverage, use of StreamIDs outside the L1STD Span range is CONSTRAINED UNPREDICTABLE (C_BAD_STREAMID, or any STE from the same security state).

### DCP, bit [81] — Directed Cache Prefetch (SMMUv3.1+)

| DCP | Meaning |
|-----|---------|
| 0   | Directed cache prefetch inhibited. Hint side-effects stripped; standalone hints complete successfully with no effect. |
| 1   | Directed cache prefetch permitted if permissions allow (write-side-effect requires write permission; standalone hint requires read or write or execute permission; otherwise hint completes with no effect). |

IGNORED on implementations not supporting this class. Same ATS applicability conditions as DRE. RES0 in SMMUv3.0.

### PPAR, bit [82] — PRI Page Request Auto Response PASID

| PPAR | Meaning |
|------|---------|
| 0    | Overflow auto-responses do not include a PASID TLP prefix. |
| 1    | Overflow auto-responses include a PASID TLP prefix if permitted. |

RES0 if `SMMU_IDR0.PRI == 0` or `SMMU_IDR1.SSIDSIZE == 0`. IGNORED (treated as PPAR==1) when `SMMU_IDR3.PPS == 1`.

### MEV, bit [83] — Merge Events

| MEV | Meaning |
|-----|---------|
| 0   | Physical SMMU must not coalesce fault records for this stream. |
| 1   | SMMU permitted to coalesce fault records sharing the same page granule, access type, and SubstreamID. |

Setting MEV==1 does not guarantee coalescing will occur. Setting MEV==0 prevents coalescing in physical SMMUs but a hypervisor might not honour this. Software must tolerate coalesced event records even when MEV==0. See §7.3.1 for the four event types that may always be merged regardless of MEV.

### SW_RESERVED, bits [87:84]

Reserved for software use; SMMU ignores this field. An EI must provide storage for this field.

### S1PIE, bit [88] — Stage 1 Permission Indirection Enable (SMMUv3.4)

| S1PIE | Meaning |
|-------|---------|
| 0     | CDs fetched via this STE cannot enable stage 1 permission indirection. |
| 1     | CDs fetched via this STE can enable stage 1 permission indirection. |

RES0 if `SMMU_IDR3.S1PI == 0`.

### S2FWB, bit [89] — Stage 2 Force Write-Back (SMMUv3.2+)

| S2FWB | Meaning |
|-------|---------|
| 0     | Attribute calculation per Chapter 13 Attribute Transformation. |
| 1     | Output attribute calculation and stage 2 descriptor bits [4:2] behave as `HCR_EL2.FWB == 1` in Armv8-A — stage 2 memory type output directly, overriding stage 1 memory type. |

Applies when stage 2 translation is performed (Config == 0b11x or 0b110). IGNORED when stage 2 not performed. ILLEGAL to set to 1 when `S2AA64` selects VMSAv8-32 LPAE. If `SMMU_IDR3.MTEPERM == 1`, effects on memory attribute interpretation change (see §3.23.1). RES0 prior to SMMUv3.2.

### S1MPAM, bit [90] — Stage 1 Control of MPAM (SMMUv3.2+)

| S1MPAM | Meaning |
|--------|---------|
| 0      | PARTID and PMG from `STE.PARTID` / `STE.PMG`. |
| 1      | PARTID and PMG from `CD.PARTID` / `CD.PMG` (may be translated via `VMS.PARTID_MAP` in nested configs). |

When stage 1 is not performed, IGNORED and STE.PARTID/PMG fields are used.

### S1STALLD, bit [91] — Stage 1 Stall Disable

| S1STALLD | Meaning |
|----------|---------|
| 0        | Stall fault model for stage 1 configurable in CD (CD.S). |
| 1        | Stall model disallowed for stage 1; faults always terminate. |

RES0 if `SMMU_IDR0.S1P == 0`. IGNORED if `Config == 0b1x0`. ILLEGAL to set to 1 when `SMMU_IDR0.STALL_MODEL != 0b00` (stall model not configurable). Arm recommends setting for PCIe streams or stall-unsafe topologies.

### EATS, bits [93:92] — Enable PCIe ATS

| EATS | Meaning |
|------|---------|
| 0b00 | ATS disabled. Translation Requests → UR + `F_BAD_ATS_TREQ`. Translated traffic (ATSCHK==1) → abort + `F_TRANSL_FORBIDDEN`. |
| 0b01 | Full ATS: Translation Requests serviced at all enabled stages. Translated traffic bypasses SMMU. ILLEGAL when `S2S == 1` (SMMUv3.1+; SMMUv3.0 CONSTRAINED UNPREDICTABLE when `Config != 0b11x`). |
| 0b10 | Split-stage ATS: Translation responses return stage 1 IPA to endpoint. Subsequent Translated traffic carries IPA and undergoes stage 2. ILLEGAL if `Config != 0b111`, `S2S == 1`, or `SMMU_IDR0.NS1ATS == 1`. If none of those conditions hold but `ATSCHK == 0`, the STE is not ILLEGAL — it simply behaves as 0b00 (per §5.2). |
| 0b11 | Full ATS with DPT checks (RME DA). Reserved (behaves as 0b00) if `SMMU_(R_)IDR3.DPT == 0`. ILLEGAL if `DPT == 1` and `StreamWorld != EL1`. ILLEGAL if `Config == 0b11x` and `S2S == 1`. |

RES0 for Secure STEs (effective value 0b00). IGNORED if `SMMU_IDR0.ATS == 0` or `Config[1:0] == 0b00`.

### STRW, bits [95:94] — StreamWorld Control

Selects the translation regime for this stream when stage 1 translation is used. Affects TLB entry tagging, the number of translation tables used in the CD, and the permissions model.

**For Non-secure STEs (`SMMU_IDR0.Hyp == 1`):**

| STRW | E2H | Resulting StreamWorld |
|------|-----|-----------------------|
| 0b00 | X   | NS-EL1                |
| 0b10 | 0   | NS-EL2                |
| 0b10 | 1   | NS-EL2-E2H            |
| 0bx1 | X   | Reserved, ILLEGAL     |

**For Realm STEs:**

| Config  | STRW | Mode |
|---------|------|------|
| 0b0xx   | any  | Stream disabled |
| 0b100   | any  | Stream bypass |
| 0b101   | 0b00 | Realm EL1&0 stage 1 only |
| 0b111   | 0b00 | Realm EL1&0 stage 1 and 2 |
| 0b110   | 0b00 | Realm EL1&0 stage 1 disabled, stage 2 |
| 0b101   | 0b10 | Realm EL2 (E2H=0) or Realm EL2&0 (E2H=1) |

**For Secure STEs:**

| STRW | E2H | Resulting StreamWorld | ILLEGAL when |
|------|-----|-----------------------|--------------|
| 0b00 | X   | Secure                | — |
| 0b10 | 0   | S-EL2                 | `SMMU_S_IDR1.SEL2 == 0` |
| 0b10 | 1   | S-EL2-E2H             | `SMMU_S_IDR1.SEL2 == 0` |
| 0b01 | X   | EL3                   | `SMMU_IDR0.RME_IMPL == 1` |
| 0b11 | X   | Reserved              | Always ILLEGAL |

**STRW overrides:** IGNORED (effective NS-EL1) when `Config == 0b11x` and STE is Non-secure. IGNORED (effective Secure) when `Config == 0b11x` and STE is Secure with `SEL2 == 1`. IGNORED when `Config == 0b100` or `Config == 0b1x0` (no translation). RES0 when `SMMU_IDR0.S1P == 0` or `SMMU_IDR0.Hyp == 0` (Non-secure STE).

### MemAttr, bits [99:96]

Memory attribute override value (VMSAv8-64 stage 2 MemAttr encoding). Applied when `MTCFG == 1`. Encodings 0b0100, 0b1000, 0b1100 are Reserved (behave as Device-nGnRnE). RES0 if `MTCFG == 0` or `SMMU_IDR1.ATTR_TYPES_OVR == 0`.

### MTCFG, bit [100] — Memory Type Configuration

| MTCFG | Meaning |
|-------|---------|
| 0     | Use incoming memory type |
| 1     | Replace incoming memory type with `MemAttr` |

RES0 if `SMMU_IDR1.ATTR_TYPES_OVR == 0`. IMPLEMENTATION DEFINED whether MTCFG applies to PCIe streams.

### ALLOCCFG, bits [104:101] — Allocation Hints Override

- `0b0xxx`: Use incoming RA, WA, TR hints.
- `0b1rwt`: Override: Read-Allocate=R, Write-Allocate=W, Transient=T. Both inner and outer hints set to same value. No effect on Device or Normal-NC types.

CONSTRAINED UNPREDICTABLE whether ALLOCCFG affects allocation hints when `MTCFG == 0`.

### SHCFG, bits [109:108] — Shareability Configuration

| SHCFG | Meaning |
|-------|---------|
| 0b00  | Non-shareable |
| 0b01  | Use incoming Shareability attribute |
| 0b10  | Outer Shareable |
| 0b11  | Inner Shareable |

### NSCFG, bits [111:110] — Non-secure Attribute Configuration

**For Secure STEs:**

| NSCFG | Meaning |
|-------|---------|
| 0b00  | Use incoming NS attribute |
| 0b01  | Reserved (behaves as 0b00) |
| 0b10  | Secure |
| 0b11  | Non-secure |

IGNORED for Non-secure STEs. IGNORED for Secure STEs when stage 1 translation is enabled (NS attribute determined by translation process). When stage 1 disabled: if `SMMU_IDR1.ATTR_PERMS_OVR == 0`, field is RES0.

**For Realm STEs:**

| NSCFG | Meaning |
|-------|---------|
| 0b00  | Use incoming NS attribute |
| 0b01  | Check incoming NS attribute (requires `SMMU_R_IDR3.XT == 1`; otherwise behaves as 0b00) |
| 0b10  | Override to Realm |
| 0b11  | Override to Non-secure |

### PRIVCFG, bits [113:112] — Privilege Configuration

| PRIVCFG | Meaning |
|---------|---------|
| 0b00    | Use incoming PRIV attribute |
| 0b01    | Reserved (behaves as 0b00) |
| 0b10    | Unprivileged |
| 0b11    | Privileged |

RES0 if `SMMU_IDR1.ATTR_PERMS_OVR == 0`.

### INSTCFG, bits [115:114] — Instruction/Data Configuration

| INSTCFG | Meaning |
|---------|---------|
| 0b00    | Use incoming INST attribute |
| 0b01    | Reserved (behaves as 0b00) |
| 0b10    | Data |
| 0b11    | Instruction |

Affects read transactions only; writes are always treated as Data regardless of this field.

### S2VMID, bits [143:128] — Virtual Machine Identifier

Tags TLB entries inserted by translations through this STE. Used for NS-EL1 StreamWorld (Non-secure STE) or Secure StreamWorld with `SEL2 == 1`. ILLEGAL to have `S2VMID[15:8] != 0` when `SMMU_IDR0.VMID16 == 0`.

IGNORED and no VMID tagging when:
- Stage 2 not implemented in the security state.
- `Config[1:0] == 0b00`.
- Non-secure STE StreamWorld is not NS-EL1.
- Secure STE StreamWorld is not Secure.
- Realm STE StreamWorld is not Realm-EL1.

### S2T0SZ, bits [165:160] — Stage 2 IPA Input Region Size

Equivalent to `VTCR_EL2.T0SZ`. 6-bit field for VMSAv8-64/VMSAv9-128; 4-bit for VMSAv8-32 LPAE. Valid range depends on granule and IAS. Inconsistency with `S2SL0` and `S2TG` is ILLEGAL. Out-of-range values: CONSTRAINED UNPREDICTABLE in SMMUv3.0; ILLEGAL in SMMUv3.1+.

### S2SL0, bits [167:166] — Stage 2 Walk Start Level

Equivalent to `VTCR_EL2.SL0`. Combined with `S2SL0_2` for LPA2 configurations. Must be consistent with `S2T0SZ` and `S2TG`.

### S2IR0/S2OR0, bits [169:168] / [171:170] — Stage 2 Walk Cache Attributes

Inner/Outer cacheability for stage 2 translation table accesses. Same encodings as S1CIR/S1COR. Non-cacheable (0b00 or 0b10) with HTTU enabled: IMPLEMENTATION DEFINED behavior (may be non-atomic or may generate `F_WALK_EABT`).

### S2SH0, bits [173:172] — Stage 2 Walk Shareability

Same encoding as S1CSH. If both `S2IR0` and `S2OR0 == 0b00`, Shareability taken as OSH.

### S2TG, bits [175:174] — Stage 2 Translation Granule

| S2TG  | Granule |
|-------|---------|
| 0b00  | 4 KB    |
| 0b01  | 64 KB   |
| 0b10  | 16 KB   |
| 0b11  | Reserved |

ILLEGAL to use unsupported granule size or Reserved value when stage 2 is enabled.

### S2PS, bits [178:176] — Physical Address Size

| S2PS  | PA bits | Notes |
|-------|---------|-------|
| 0b000 | 32 bits | |
| 0b001 | 36 bits | |
| 0b010 | 40 bits | |
| 0b011 | 42 bits | |
| 0b100 | 44 bits | |
| 0b101 | 48 bits | |
| 0b110 | 52 bits | Reserved in SMMUv3.0 (behaves as 0b101) |
| 0b111 | 56 bits | Reserved until SMMUv3.4 (behaves as 0b110 in SMMUv3.1–3.3) |

Effective value capped to `SMMU_IDR5.OAS`. 52-bit output requires 64 KB granule or `S2DS == 1`. 56-bit output requires VMSAv9-128.

### S2AA64, bit [179] — Stage 2 Table Format

| S2AA64 | Meaning |
|--------|---------|
| 0      | VMSAv8-32 LPAE (when `SMMU_IDR0.TTF[0]==1`) or VMSAv9-128 (when `SMMU_IDR5.D128==1`) |
| 1      | VMSAv8-64 |

ILLEGAL to select a table format not supported by the implementation.

### S2ENDI, bit [180] — Stage 2 Table Endianness

| S2ENDI | Meaning |
|--------|---------|
| 0      | Little Endian |
| 1      | Big Endian |

ILLEGAL to select unimplemented endianness per `SMMU_IDR0.TTENDIAN`.

### S2AFFD, bit [181] — Stage 2 Access Flag Fault Disable

| S2AFFD | Meaning |
|--------|---------|
| 0      | Access flag fault when `AF == 0` in descriptor (when HTTU not in use). |
| 1      | Access flag fault never occurs; `AF` treated as always 1. |

IGNORED when `S2HA == 1`.

### S2PTW, bit [182] — Protected Table Walk

| S2PTW | Meaning |
|-------|---------|
| 0     | CD fetches and stage 1 table walks allowed to any valid stage 2 address (if `SMMU_IDR3.PTWNNC==0`; otherwise Device-mapped accesses treated as Normal NC). |
| 1     | CD fetch or stage 1 table walk to Device-mapped stage 2 address is terminated; stage 2 Permission fault recorded. |

IGNORED unless `Config[1:0] == 0b11`.

### S2HA / S2HD, bits [184:183] — Stage 2 HTTU

Combined encoding `{S2HA, S2HD}`:
- `0b00`: HTTU disabled.
- `0b10`: Access flag update enabled.
- `0b01`: Reserved (behaves as 0b00).
- `0b11`: Access flag + dirty state update enabled.

ILLEGAL to set `S2HA` if `SMMU_IDR0.HTTU == 0b00`. ILLEGAL to set `S2HD` if `SMMU_IDR0.HTTU == 0b00 or 0b01`. ILLEGAL to set either for VMSAv8-32 LPAE stage 2.

### S2S, bit [185] / S2R, bit [186] — Stage 2 Stall / Record

`S2S == 1`: Use Stall fault model for stage 2 translation-related faults (if supported). `S2R == 1`: Record fault events. IGNORED when `Config == 0b10x`. See §5.5 fault configuration for `{A, R, S}` semantics.

### S2HAFT, bit [187] — Stage 2 Hardware AF Table update (SMMUv3.4)

ILLEGAL to set if `S2HA == 0`. RES0 if `SMMU_IDR0.HTTU != 0b11`. Enables hardware update of Access flag in stage 2 table descriptors.

### S2PIE, bit [188] — Stage 2 Permission Indirection Enable (SMMUv3.4)

| S2PIE | Meaning |
|-------|---------|
| 0     | Stage 2 uses Direct Permission Scheme. |
| 1     | Stage 2 uses Indirect Permission Scheme. |

RES0 if VMSAv9-128 (implicit IPS enabled). RES0 if `SMMU_IDR3.S2PI == 0` or `S2AA64` selects VMSAv8-32. Must be configured consistently for all STEs with the same `S2VMID`.

### S2POE, bit [189] — Stage 2 Permission Overlay Enable (SMMUv3.4)

| S2POE | Meaning |
|-------|---------|
| 0     | Do not apply stage 2 overlay permissions. |
| 1     | Apply stage 2 overlay permissions. |

ILLEGAL if stage 2 uses Direct Permission Scheme (`S2PIE == 0`). ILLEGAL if any of `S2HWU59–62` are 1. RES0 if `SMMU_IDR3.S2PO == 0`.

### S2POI, bits [511:448] — Stage 2 Permission Overlay Interpretations (SMMUv3.4)

A set of 16 permission overlay interpretations (4 bits each), indexed by `POIndex` from the translation table descriptor. Encoding:

| S2POI\<p\> | Meaning |
|-----------|---------|
| 0b0000    | No Access |
| 0b0010    | MRO |
| 0b0011    | MRO-TL1 |
| 0b0100    | WO |
| 0b0110    | MRO-TL0 |
| 0b0111    | MRO-TL01 |
| 0b1000    | RO |
| 0b1001    | RO+uX |
| 0b1010    | RO+pX |
| 0b1011    | RO+puX |
| 0b1100    | RW |
| 0b1101    | RW+uX |
| 0b1110    | RW+pX |
| 0b1111    | RW+puX |
| 0b0001, 0b0101 | Reserved → C_BAD_STE |

Effect on stage 2 translation of output address from stage 1 not cacheable in TLB; effect on stage 1 table walks is cacheable.

### DPT_VMATCH, bits [191:190] — VMID Matching for DPT (RME DA)

Controls VMID matching requirement for DPT checks when `EATS == 0b11`:

| DPT_VMATCH | Meaning |
|-----------|---------|
| 0b00 | `S2VMID` must match or DPT entry must have `AC==0b10` |
| 0b01 | `S2VMID` must match or DPT entry must have `AC==0b01 or 0b10` |
| 0b10 | S2VMID matching not required |
| 0b11 | Reserved (behaves as 0b00) |

For Realm STEs with `EATS == 0b11`: only 0b00 permitted; other values ILLEGAL. RES0 if `DPT == 0` or `EATS != 0b11`. Not cacheable in TLB.

### S2NSW / S2NSA, bits [192:193] — Secure Stage 2 Non-secure IPA Walk/Access NS bits

Relevant only for Secure STEs with Secure stage 2 (`SMMU_S_IDR1.SEL2 == 1`):
- `S2NSW`: NS bit used for stage 2 table walks for the Non-secure IPA space (`STE.S2TTB`).
- `S2NSA`: NS bit output for stage 2 Non-secure IPA translations.

Both RES0 for Non-Secure/Realm STEs or when `SEL2 == 0`.

### S2SL0_2, bit [194] — LPA2 Walk Start Level Bit 2

Bit [2] of `S2SL0` for LPA2 (52-bit address) configurations. RES0 if `SMMU_IDR5.DS == 0`, `S2AA64` selects VMSAv9-128 or VMSAv8-32 LPAE.

### S2DS, bit [195] — 52-bit Address Size Enable (LPA2)

Enables 52-bit input/output address sizes for 4 KB and 16 KB granules (equivalent to `VTCR_EL2.DS`). RES0 if `SMMU_IDR5.DS == 0`, VMSAv9-128, VMSAv8-32 LPAE, or `S2TG == 64 KB`.

### S2TTB, bits [247:196] — Stage 2 Translation Table Base

PA of the stage 2 root translation table. Bits [247:244] RES0 in SMMUv3.1+; bits [247:240] in SMMUv3.0. ILLEGAL if address outside `eff_S2PS` range. In a Realm STE: treated as Realm PA.

**VMSAv9-128:** bits [247:196] are address bits [55:4].

### PARTID, bits [287:272] — MPAM Partition ID (SMMUv3.2+)

PARTID assigned to all accesses related to this StreamID (when `S1MPAM == 0`). RES0 prior to SMMUv3.2. Interpreted as UNKNOWN if > `SMMU_(*_)MPAMIDR.PARTID_MAX`.

### S_S2T0SZ / S_S2SL0 / S_S2TG / S_S2TTB / S_S2SKL, various bits — Secure Stage 2 Parallel Tables

These fields configure a second, parallel stage 2 translation table used for the **Secure IPA space** when Secure stage 2 (`SMMU_S_IDR1.SEL2 == 1`) is enabled on a Secure STE:

- `S_S2T0SZ` — input size for Secure IPA space (same encoding as `S2T0SZ`).
- `S_S2SL0` — walk start level for `S_S2TTB`.
- `S_S2TG` — granule for `S_S2TTB`.
- `S_S2TTB` — base PA of Secure IPA space stage 2 table.
- `S_S2SKL` — skip level for VMSAv9-128 `S_S2TTB` (RES0 for VMSAv8-64).
- `S_S2SL0_2` — bit [2] of `S_S2SL0` for LPA2 configurations.

All RES0 for Non-secure/Realm STEs or when `SMMU_S_IDR1.SEL2 == 0`. IGNORED in a Secure STE when `Config == 0b10x`.

### S2SW / S2SA, bits [384:385] — Secure Stage 2 Secure IPA Walk/Access NS bits

Relevant only for Secure STEs with `SEL2 == 1`:
- `S2SW`: NS bit used for stage 2 table walks for the Secure IPA space (`S_S2TTB`).
- `S2SA`: NS bit output for all stage 2 Secure IPA translations. IGNORED when `S2SW == 1` (effective `S2SA` treated as 1).

### MECID, bits [319:304] — Memory Encryption Context ID (RME DA)

MECID value for all SMMU-originated and client accesses related to this Realm stream: stage 1/2 table walks, L1CD/CD fetches, and client accesses to Realm PA space. RES0 for Non-secure/Secure STEs. RES0 if `SMMU_R_IDR3.MEC == 0`. IGNORED for Realm streams with `Config[2] == 0`. Bits above `MECIDSIZE` treated as 0. May be cached in a configuration cache.

### PMG, bits [327:320] — MPAM Performance Monitoring Group (SMMUv3.2+)

PMG assigned to accesses related to this StreamID (when `S1MPAM == 0`). RES0 prior to SMMUv3.2.

### MPAM_NS, bit [328] — MPAM PARTID Space

| MPAM_NS | Meaning |
|---------|---------|
| 0       | Use PARTID space of the stream's security state |
| 1       | Use Non-secure PARTID space |

Affects PARTID space for: CD fetches, stage 1/2 table walks, client transactions. Does not affect VMS fetches. RES0 for Non-secure STEs. RES0 for Secure/Realm STEs if `SMMU_(*_)MPAMIDR.HAS_MPAM_NS == 0`.

### AssuredOnly, bit [329] — Stage 2 AssuredOnly (SMMUv3.4 / FEAT_THE)

| AssuredOnly | Meaning |
|-------------|---------|
| 0           | AssuredOnly permission checks disabled |
| 1           | AssuredOnly permission checks enabled; CDs must be from AssuredOnly memory |

RES0 if VMSAv9-128 (always enabled). RES0 if `SMMU_IDR3.THE == 0`. Cacheable in TLB.

### TL0 / TL1, bits [330:331] — Stage 2 TopLevel Checks (SMMUv3.4 / FEAT_THE)

Equivalent to `VTCR_EL2.TL0/TL1`. Enable stage 2 TopLevel 0/1 permission checks. RES0 if `SMMU_IDR3.THE == 0`. Cacheable in TLB.

### VMSPtr, bits [375:332] — VMS Pointer (SMMUv3.2+)

PA pointer to the [virtual-machine-structure.md](virtual-machine-structure.md). Active when `Config == 0b111` and `S1MPAM == 1` and `SMMU_(*_)MPAMIDR.PARTID_MAX != 0`. Address above OAS → C_BAD_STE. RES0 prior to SMMUv3.2. Address bits [11:0] and [63:55] taken as zero.

## STE.Config Encoding (Summary)

| STE.Config | Behavior |
|------------|----------|
| 0b000      | Stream disabled — transaction terminated with abort, **no event recorded** (F_STREAM_DISABLED **not** generated on 0b000; it is generated by S1DSS==0b00 termination on substream mis-match) |
| 0b100      | Stream bypass — no translation; attribute overrides from MTCFG/MemAttr/ALLOCCFG/SHCFG/NSCFG/PRIVCFG/INSTCFG |
| 0b101      | Stage 1 only (S1* fields valid) |
| 0b110      | Stage 2 only (S2* fields valid) |
| 0b111      | Stage 1 and Stage 2 (nested) — both S1* and S2* valid |

**Config == 0b100 (bypass):** S1* and S2* are IGNORED; only attribute override fields are used.

## L1STD: Level 1 Stream Table Descriptor (§5.1)

When a two-level stream table is used (`SMMU_STRTAB_BASE_CFG.FMT == 0b01`), the first level contains **Level 1 Stream Table Descriptors (L1STDs)**. Each L1STD is an **8-byte structure** that points to a second-level array of STEs.

### L1STD Format

| Bits | Field | Description |
|---|---|---|
| [4:0] | Span | Size of Level 2 array and L2Ptr validity (see table below) |
| [5] | — | Reserved, RES0 |
| [55:6] | L2Ptr | Pointer to Level 2 STE array base (bits above OAS are RES0) |
| [63:56] | — | Reserved, RES0 |

**Span encoding:**

| Span | Meaning |
|---|---|
| 0 | L2Ptr is invalid; all StreamIDs in this descriptor's range are invalid |
| 1–11 | Level 2 array contains 2^(Span−1) STEs |
| 12–31 | Reserved (behaves as 0) |

Span must be within 0 to `SMMU_STRTAB_BASE_CFG.SPLIT + 1`. The Level 2 array is **aligned to its size** by the SMMU: bits `L2Ptr[N:0]` are treated as 0, where `N = 5 + (Span − 1)`.

### L1STD Behavior

A StreamID selecting an L1STD with `Span == 0`, a Reserved/out-of-bounds Span, or a StreamID outside the Level 2 range described by Span, is **invalid** — the transaction is terminated with abort; `C_BAD_STREAMID` may be recorded (per `SMMU_CR2.RECINVSID`).

### L1STD Invalidation

- When an L1STD is changed, **non-leaf** `CMD_CFGI_STE` (Leaf=0) is the minimum required invalidation.
- Changing `Span: 0 → non-zero` (introducing new L2 array): only the L1STD needs invalidation.
- Changing `Span: non-zero → 0` (decommissioning): the L1STD **and** all cached STEs within the span must be invalidated (`CMD_CFGI_STE_RANGE` or `CMD_CFGI_ALL`).
- The L1STD is fetched using attributes from `SMMU_(*_)CR1.TABLE_*`.

---

## §5.2.1 STE General Properties

- An STE that is successfully fetched **may be cached** by the SMMU. Any modification requires `CMD_CFGI_STE`. A failed `F_STE_FETCH` does not result in a cached entry.
- When cached, an STE is uniquely identified by `(SEC_SID, StreamID)`.
- Stage 2 configuration from STEs **with the same S2VMID** is considered interchangeable by the SMMU.
- The following STE fields **may be cached in TLB entries** and require TLB invalidation (in addition to STE cache invalidation) when altered: S2TTB, S2PTW, S2VMID, S2T0SZ, S2IR0, S2OR0, S2SH0, S2SL0, S2TG, S2PS, S2AFFD, S2HA, S2HD, S2ENDI, S2AA64, S_S2TTB, S2NSW, S2NSA, S2SW, S2SA, S_S2SL0, S_S2TG, S_S2T0SZ, S2FWB, TL1, TL0, AssuredOnly, S2PIE, S2POE, S2POI, S2SKL, S_S2SKL, S2HAFT.
- All other STE fields are not cacheable in TLB entries; changes do not require TLB invalidation.
- Invalidation of an STE also implicitly invalidates cached CDs fetched through that STE.
- A StreamWorld change makes TLB entries of the prior StreamWorld unreachable; Arm recommends explicit invalidation when returning to a prior StreamWorld to avoid stale hits.

## ILLEGAL STE Conditions Summary

An STE is ILLEGAL when any of the following hold (transaction treated as C_BAD_STE):
- `V == 0` (but note: technically "invalid" not "ILLEGAL"; same behavior).
- Config reserved encoding.
- Config enabled with stage not implemented.
- Config == 0b11x for Secure STE without `SEL2`.
- Config == 0b11x for Secure STE with VMSAv8-32 LPAE.
- `STRW == 0bx1` (Non-secure) except per-table mapping.
- `EATS == 0b01` when `S2S == 1` (SMMUv3.1+).
- `EATS == 0b10` with incompatible Config/S2S/NS1ATS/ATSCHK combination.
- `EATS == 0b11` with `StreamWorld != EL1` (DPT check).
- `S1Fmt != 0b00` when `SMMU_IDR0.CD2L == 0`.
- `S1STALLD == 1` when stall model not configurable.
- `S2HA` or `S2HD` set for VMSAv8-32 LPAE stage 2.
- `S2HA` set when `SMMU_IDR0.HTTU == 0b00`.
- `S2HD` set when `SMMU_IDR0.HTTU == 0b00 or 0b01`.
- `S2HAFT == 1` when `S2HA == 0`.
- `S2PIE == 0` and `S2POE == 1` (Direct Permission Scheme with overlay).
- `S2POE == 1` and any of S2HWU59–62 set.
- `DPT_VMATCH != 0b00` for Realm STE with `EATS == 0b11`.
- VMSPtr address out of range (if VMSPtr enabled).

### §5.2.2 STE Validity Pseudocode (SteIllegal()) — Key Checks

The normative `SteIllegal()` pseudocode (§5.2.2) performs checks in priority order. Implementation notes:

**Pre-computed intermediates:**
- `strw_unused` — true when STRW field is irrelevant (stage 1 not implemented; NS+Hyp not supported; stage 2 enabled; bypass Config=0b100)
- `s2vmid_ignored` — true when S2VMID does not tag TLB entries (Config=0bxx; NS without S2P; Secure without SEL2; bypass; STRW!=EL1 when strw_unused=false; Secure+stage1only)
- `eff_idr0_stall_model` — effective stall model accounting for NSSTALLD

**Stage 1 check detail:**
- `STE.S1STALLD == 1`: ILLEGAL if NS stall model != "Stall and Terminate" (0b00), or Secure stall model != 0b00, or Realm stall model == 0b01.
- `STE.S1CDMax`: ILLEGAL if `UInt(S1CDMax) > UInt(SMMU_IDR1.SSIDSIZE)` (when SSIDSIZE != 0).
- Two-level CD table (`S1Fmt == 0b01 or 0b10`) with `SMMU_IDR0.CD2L == 0` → ILLEGAL.

**Stage 2 address-range checks:**
- `STES2TTBOutOfRange()`: checks S2TTB alignment and OAS; also checks 128-bit descriptor constraints when `SMMU_IDR5.D128 == 1`.
- `STES2TOSZInvalid()`: validates S2T0SZ against IAS, granule, DS bit, and STT (Small Translation Table) support. SMMUv3.1+ always treats out-of-range T0SZ as ILLEGAL.

**SMMUv3.0 compatibility:** Some checks that are CONSTRAINED UNPREDICTABLE in SMMUv3.0 (e.g., `EATS==0b01` with `S2S==1`, out-of-range S2T0SZ) are always ILLEGAL in SMMUv3.1+.

## Model Implementation Notes

- The STE is the root of all per-stream configuration. A functional model must fully decode and validate the STE before any translation step.
- STRW (StreamWorld) affects TLB tagging, translation table count in CD, and permission model — it must be resolved before looking up the CD.
- For Secure STEs with `SEL2 == 1`: the stage 2 translation involves *two* IPA spaces (Secure IPA and Non-secure IPA) with *two* translation table bases (`S_S2TTB` for Secure IPA, `S2TTB` for Non-secure IPA).
- CONT caching hints must be tracked carefully: the model may cache a single STE for the entire CONT span; cache invalidation requires targeting every STE in the span individually.
- Embedded Implementation (EI) RAZ/WI permissions are per-field; a model targeting EI implementations may omit storage for fields that would be RAZ/WI.

## Related Concepts

- [streamid-substreamid.md](streamid-substreamid.md) — key used to index the stream table
- [two-stage-translation.md](two-stage-translation.md) — translation pipeline STE feeds into
- [context-descriptor.md](context-descriptor.md) — stage 1 config pointed to by STE
- [fault-models.md](fault-models.md) — STE.S2S, STE.S2R, STE.S1STALLD govern stage fault behavior
- [pcie-ats-pri.md](pcie-ats-pri.md) — STE.EATS controls ATS behavior per stream
- [security-states.md](security-states.md) — Secure STEs live in a separate Secure stream table
- [virtual-machine-structure.md](virtual-machine-structure.md) — VMSPtr references the VMS structure
- [httu.md](httu.md) — STE.S2HA/S2HD/S2HAFT control stage 2 HTTU
- [destructive-reads.md](destructive-reads.md) — STE.DRE/DCP enable or inhibit these transaction classes
- [granule-protection-check.md](granule-protection-check.md) — GPC applied to Realm stream PA outputs
- [device-permission-table.md](device-permission-table.md) — STE.EATS==0b11 enables DPT checks; DPT_VMATCH controls matching
- [permission-indirections.md](permission-indirections.md) — S1PIE, S2PIE, S2POE, S2POI fields
- [translation-hardening.md](translation-hardening.md) — STE.AssuredOnly enables AssuredOnly permission checks at stage 2 (SMMUv3.4 FEAT_THE)
- [mpam.md](mpam.md) — STE.PARTID, STE.PMG, STE.S1MPAM, STE.MPAM_NS
- [mec.md](mec.md) — STE.MECID for Realm streams (RME DA)
- [attribute-transformation.md](attribute-transformation.md) — Chapter 13 governs MemAttr/MTCFG/ALLOCCFG/SHCFG combination
- [memory-tagging-extension.md](memory-tagging-extension.md) — CD.MAIR must not use reserved 0xF0 MTE encoding; STE.S2FWB interacts with FEAT_MTE_PERM MemAttr reinterpretation (§3.23, §3.23.1)

## Sources That Use This Concept

- [../sources/ihi0070g-b-smmuv3-architecture-spec.md](../sources/ihi0070g-b-smmuv3-architecture-spec.md) — §3.3.1 Stream table lookup; §3.3.2 StreamIDs to Context Descriptors; §3.3.3 Configuration and Translation lookup; §5.1 L1STD format; §5.2 STE data structure format; §5.2.1 General properties of the STE; §5.2.2 SteIllegal() pseudocode; §3.12 Fault models
