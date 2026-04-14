---
title: "Context Descriptor (CD)"
type: concept
tags: [smmu, cd, context-descriptor, stage1, translation, data-structure]
created: 2026-04-07
updated: 2026-04-14
sources: [../sources/ihi0070g-b-smmuv3-architecture-spec]
---

# Context Descriptor (CD)

## Definition

A Context Descriptor (CD) is the stage 1 translation configuration structure for an SMMU stream or substream. It is pointed to by the [stream-table-entry.md](stream-table-entry.md) via `STE.S1ContextPtr` and optionally indexed by SubstreamID. Each CD is a 64-byte (512-bit) structure that contains:

- Stage 1 translation table base pointers (`CD.TTB0`, `CD.TTB1`).
- ASID for TLB tagging, and ASET selecting shared vs. non-shared ASID.
- Translation table format, granule, address size, and input range configuration.
- Fault behavior flags (`CD.S`, `CD.R`, `CD.A`) controlling the [fault-models.md](fault-models.md) for stage 1.
- HTTU controls (`CD.HA`, `CD.HD`, `CD.HAFT`) for stage 1 access flag and dirty state updates.
- WXN/UWXN/PAN/EPAN permission modifiers.
- TBI0/TBI1 (Top Byte Ignore) configuration.
- EPD0/EPD1 translation disable flags per table pointer.
- DS (LPA2 52-bit addresses), IPS (output address size), AA64 (table format).
- MAIR0/MAIR1 and AMAIR0/AMAIR1 memory attribute index registers.
- NSCFG0/NSCFG1 controlling NS attribute for stage 1 table walks.
- Permission indirection tables (`PIIU<p>`, `PIIP<p>`) and enable (`PIE`).
- MPAM fields (`PARTID`, `PMG`) when `STE.S1MPAM == 1`.

## CD Location

The CD address derived from `STE.S1ContextPtr` is:
- A **PA** when only stage 1 is active (no stage 2).
- An **IPA** (subject to stage 2 translation) when nested (stage 1 + stage 2) is configured.

For substream configurations, `STE.S1ContextPtr` may point to:
- A flat array of CDs indexed by SubstreamID.
- An L1CD table (indexed by upper SubstreamID bits), where each L1CD entry points to an L2 CD array (indexed by lower SubstreamID bits).

## L1CD — Level 1 Context Descriptor (§5.3)

An 8-byte structure used when `STE.S1Fmt != 0b00` (2-level CD table). Contains:
- **V, bit [0]:** Validity. If 0: L2Ptr is IGNORED; transaction terminated with abort; `C_BAD_SUBSTREAMID` recorded.
- **Bits [11:1]:** Reserved, res0.
- **L2Ptr, bits [55:12]:** Pointer to next-level CD table. Must be within IAS if stage 2 is enabled; within OAS otherwise. Bits above `SMMU_IDR5.OAS` are RES0. Address bits above/below the field are taken as zero.
- **Bits [63:56]:** Reserved, res0.

**Invalidation:** L1CD changes require CMD_CFGI_CD with Leaf==0. Changing V from 0→1 requires invalidation of L1CD only. Changing V from 1→0 requires L1CD invalidation plus all CDs in the affected SubstreamID span.

## CD Field Reference (§5.4)

Invalid or contradictory CD configurations are ILLEGAL. A transaction using an ILLEGAL CD is terminated with abort and C_BAD_CD is recorded.

### T0SZ, bits [5:0] — TTB0 VA Region Size

Equivalent to `TCR_ELx.T0SZ`. Determines the input VA size for TTB0 translation. IGNORED if EPD0 effective value is 1.

**VMSAv8-32 LPAE:** 3-bit field; bits [5:3] ignored. Valid range 0–7.

**VMSAv8-64:** 6-bit field.
- Maximum: 39 (SMMU_IDR3.STT==0), or 48 (4K/16K granule with STT==1), or 47 (64K granule with STT==1).
- Minimum: 16 normally; 12 if 64K granule or (DS==1 and 4K/16K granule).

**VMSAv9-128:** Maximum 48 (4K/16K) or 47 (64K). Minimum 16 (48-bit VA), 12 (52-bit VA), or 9/8 (56-bit VA at EL1/EL3).

Out-of-range values: CONSTRAINED UNPREDICTABLE in SMMUv3.0; ILLEGAL in SMMUv3.1+.

### TG0, bits [7:6] — TTB0 Translation Granule

| TG0  | Granule |
|------|---------|
| 0b00 | 4 KB    |
| 0b01 | 64 KB   |
| 0b10 | 16 KB   |
| 0b11 | Reserved (ILLEGAL) |

Must select a granule supported by the SMMU (`SMMU_IDR5`). ILLEGAL if unsupported. IGNORED if `EPD0` effective value is 1. IGNORED if VMSAv8-32 LPAE.

### IR0, bits [9:8] — TTB0 Inner Region Cacheability

| IR0  | Meaning |
|------|---------|
| 0b00 | Non-cacheable |
| 0b01 | Write-back Cacheable, Read-Allocate, Write-Allocate |
| 0b10 | Write-through Cacheable, Read-Allocate |
| 0b11 | Write-back Cacheable, Read-Allocate, no Write-Allocate |

IGNORED if `EPD0` effective value is 1. Non-cacheable + HTTU enabled: IMPLEMENTATION DEFINED behavior.

### OR0, bits [11:10] — TTB0 Outer Region Cacheability

Same encoding as IR0.

### SH0, bits [13:12] — TTB0 Shareability

| SH0  | Meaning |
|------|---------|
| 0b00 | Non-shareable |
| 0b01 | Reserved (behaves as 0b00) |
| 0b10 | Outer Shareable |
| 0b11 | Inner Shareable |

If both IR0 and OR0 == 0b00 (Non-cacheable), Shareability taken as OSH. IGNORED if `EPD0` effective value is 1.

### EPD0, bit [14] — TTB0 Walk Disable

| EPD0 | Meaning |
|------|---------|
| 0    | Perform translation table walks using TTB0. |
| 1    | TLB miss on TTB0-translated address → F_TRANSLATION. No walk performed. T0SZ, TG0, IR0, OR0, SH0, TTB0 are IGNORED. |

IGNORED (effective value 0) if StreamWorld == any-EL2 or EL3 (only EL1 and EL2-E2H can disable table walks).

### ENDI, bit [15] — Translation Table Endianness

| ENDI | Meaning |
|------|---------|
| 0    | Little Endian |
| 1    | Big Endian |

IGNORED if effective values of both EPD0 and EPD1 are 1. ILLEGAL if `SMMU_IDR0.TTENDIAN == 0b10` and set to 1. ILLEGAL if `SMMU_IDR0.TTENDIAN == 0b11` and set to 0.

### T1SZ, bits [21:16] — TTB1 VA Region Size

Same encoding and range rules as T0SZ. IGNORED if EPD1 effective value is 1. RES0 if StreamWorld == any-EL2 or EL3.

### TG1, bits [23:22] — TTB1 Translation Granule

| TG1  | Granule |
|------|---------|
| 0b00 | Reserved |
| 0b01 | 16 KB   |
| 0b10 | 4 KB    |
| 0b11 | 64 KB   |

**Note:** The encoding of TG1 differs from TG0 (consistent with Armv8-A). IGNORED if `EPD1` effective value is 1. RES0 if StreamWorld == any-EL2 or EL3 or if VMSAv8-32 LPAE.

### IR1 / OR1 / SH1, bits [25:24] / [27:26] / [29:28]

Same encoding as IR0/OR0/SH0 but for TTB1 access. IGNORED if EPD1 effective value is 1. RES0 if StreamWorld == any-EL2 or EL3.

### EPD1, bit [30] — TTB1 Walk Disable

Same encoding as EPD0. Affects T1SZ/TG1/IR1/OR1/SH1/TTB1.

### V, bit [31] — CD Valid

| V | Meaning |
|---|---------|
| 0 | Invalid — entire rest of structure IGNORED; transaction terminated with abort; C_BAD_CD recorded. |
| 1 | Valid. |

### IPS, bits [34:32] — Intermediate Physical Address Size

| IPS   | PA/IPA bits | Notes |
|-------|-------------|-------|
| 0b000 | 32 bits | |
| 0b001 | 36 bits | |
| 0b010 | 40 bits | |
| 0b011 | 42 bits | |
| 0b100 | 44 bits | |
| 0b101 | 48 bits | |
| 0b110 | 52 bits | Reserved in SMMUv3.0 (behaves as 0b101) |
| 0b111 | 56 bits | Reserved until SMMUv3.4 |

Effective value = MIN(IPS, `SMMU_IDR5.OAS`). Stage 1 outputs above eff_IPS cause F_ADDR_SIZE. IGNORED for VMSAv8-32 LPAE (fixed 40 bits). TTBx address outside eff_IPS range → CD ILLEGAL (C_BAD_CD).

### AFFD, bit [35] — Access Flag Fault Disable

| AFFD | Meaning |
|------|---------|
| 0    | AF==0 in descriptor → F_ACCESS fault (when HTTU not in use). |
| 1    | AF treated as always 1; access flag faults never occur. |

IGNORED if `CD.HA == 1`.

### WXN, bit [36] — Write Execute Never

| WXN | Meaning |
|-----|---------|
| 0   | Instruction access to writable pages allowed normally. |
| 1   | Instruction access to any writable page → F_PERMISSION. |

### UWXN, bit [37] — Unprivileged Write Execute Never

| UWXN | Meaning |
|------|---------|
| 0    | Instruction access as normal. |
| 1    | Privileged instruction access to user-writable page → F_PERMISSION. |

IGNORED in EL2 and EL3 StreamWorlds. RES0 if VMSAv9-128. IGNORED if VMSAv8-64 (all EL0-writable regions treated as PXN).

### TBI0, bit [38] — Top Byte Ignore for TTB0

When 1: VA[63:56] ignored for Translation fault generation due to sign-extension mismatch with VA[55]. See also §3.9.1 for ATS TBI interaction.

### TBI1, bit [39] — Top Byte Ignore for TTB1

Same as TBI0 for TTB1. RES0 if StreamWorld == any-EL2 or EL3.

### PAN, bit [40] — Privileged Access Never

When 1: privileged data read/write access to any virtual address where unprivileged access is permitted at stage 1 is disabled. IGNORED for EL2 and EL3 StreamWorlds.

### AA64, bit [41] — Translation Table Format

| AA64 | Meaning |
|------|---------|
| 0    | VMSAv8-32 LPAE (when `SMMU_IDR0.TTF[0]==1`) or VMSAv9-128 (when `SMMU_IDR5.D128==1`) |
| 1    | VMSAv8-64 |

**ILLEGAL conditions for VMSAv8-32 LPAE:** Not supported by implementation (`TTF[0]==0`); or StreamWorld == any-EL2-E2H, S-EL2, or EL3.

**ILLEGAL conditions for VMSAv8-64:** Not supported (`TTF[1]==0`); or stage 2 configured with VMSAv8-32 LPAE.

**ILLEGAL conditions for VMSAv9-128:** StreamWorld is EL2 (not E2H). `STE.S1PIE == 0`.

### HD, bit [42] — HTTU Dirty State Update for TTB0/TTB1

Combined with HA as `{HD, HA}`:
- 0b00: HTTU disabled.
- 0b01: Access flag update enabled.
- 0b10: Reserved (behaves as 0b00).
- 0b11: Access flag + dirty state update enabled.

IGNORED for VMSAv8-32 LPAE. ILLEGAL to set HA if `SMMU_IDR0.HTTU == 0b00`. ILLEGAL to set HD if `SMMU_IDR0.HTTU == 0b00 or 0b01`.

### HA, bit [43] — HTTU Access Flag Update for TTB0/TTB1

See HD field definition above.

### S / R / A, bits [44 / 45 / 46] — Stage 1 Fault Behavior

`{A, R, S}` control the fault model for stage 1 Translation-related faults:

| CD.S | CD.A | `SMMU_IDR0.TERM_MODEL` | Behavior |
|------|------|------------------------|----------|
| 0    | 0    | 0                      | Terminate with RAZ/WI |
| 0    | 1    | 0 or 1                 | Terminate with abort |
| 1    | —    | 0 (configurable)       | Stall (if `STE.S1STALLD==0` and stall supported) |

`CD.R == 1`: events may be recorded. `CD.R == 0`: events may not be recorded.

**ILLEGAL conditions:** `STE.S1STALLD==1` and `CD.S==1`. `SMMU_IDR0.TERM_MODEL==1` and `CD.A==0`. Stall model not supported (`STALL_MODEL==0b01`) and `CD.S==1`. Stall model forced (`STALL_MODEL==0b10`) and `CD.S==0`.

### ASET, bit [47] — ASID Set

| ASET | Meaning |
|------|---------|
| 0    | Shared set: ASID shared with PE address spaces. All matching broadcast TLB invalidations affect TLB entries from this CD. |
| 1    | Non-shared set: TLB entries not invalidated by some broadcast invalidations (VAAE1IS, etc.). Requires explicit SMMU CMD_TLBI commands to invalidate. |

ASET must be included in Global cached translations for StreamWorld == NS-EL1, Secure, or any-EL2-E2H. Changes to ASET require separate TLB maintenance.

### ASID, bits [63:48] — Address Space Identifier

Tags TLB entries inserted from this CD. Must tag all cached translations for NS-EL1, Secure, any-EL2-E2H StreamWorlds. IGNORED for EL2 and EL3 StreamWorlds. ILLEGAL to have `ASID[15:8] != 0` when `SMMU_IDR0.ASID16 == 0` (8-bit ASID implementation).

### NSCFG0, bit [64] — TTB0 Walk NS Attribute

| NSCFG0 | Meaning |
|--------|---------|
| 0      | Starting-level descriptor of TTB0 fetched using NS==0 |
| 1      | Starting-level descriptor of TTB0 fetched using NS==1 |

Used only when CD is reached from a Secure STE; IGNORED otherwise.

### HAD0, bit [65] — Hierarchical Attribute Disable for TTB0

(When `SMMU_IDR5.D128 == 0`:)

| HAD0 | Meaning |
|------|---------|
| 0    | Hierarchical attributes enabled (APTable/PXNTable/XNTable honored). |
| 1    | Hierarchical attributes disabled; APTable/PXNTable/XNTable bits IGNORED in table descriptors walked through TTB0. |

RES0 if `SMMU_IDR3.HAD == 0`. Supported for both VMSAv8-32 LPAE and VMSAv8-64.

(When `SMMU_IDR5.D128 == 1`: this bit is `DisCH0` — disables Contiguous bit at initial walk level for VMSAv9-128.)

### E0PD0, bit [66] — E0PD TTB0 Disable Unprivileged Access

| E0PD0 | Meaning |
|-------|---------|
| 0     | No fault from this mechanism. |
| 1     | Unprivileged access to any address translated by TTB0 → F_TRANSLATION. |

RES0 if `SMMU_IDR3.E0PD == 0`, VMSAv8-32 LPAE, or StreamWorld == EL3 or any-EL2. Only applies to VMSAv8-64 with EL1, Secure, or EL2-E2H StreamWorld.

### HAFT, bit [67] — Hardware AF Table Descriptor Update (SMMUv3.4)

Enables hardware update of Access flag in stage 1 table descriptors. ILLEGAL if `CD.HA == 0` (results in C_BAD_CD). RES0 if `SMMU_IDR0.HTTU != 0b11`.

### TTB0, bits [119:68] — Translation Table Base 0

PA (or IPA when stage 2 is enabled) of the root TTB0 translation table.
- SMMUv3.1+: bits [51:4] of the address (bits [119:116] RES0), or bits [55:4] if VMSAv9-128.
- SMMUv3.0: bits [47:4] (bits [119:112] RES0).

ILLEGAL if address outside eff_IPS range. ILLEGAL if outside 48-bit range when `DS==0` and VMSAv8-64 and granule < 64 KB. IGNORED if `EPD0` effective value is 1.

### SKL0, bits [127:126] — VMSAv9-128 TTB0 Skip Level

(When `SMMU_IDR5.D128 == 1` and AA64 selects VMSAv9-128.) Skip Level for initial TTB0 lookup (0–3). IGNORED if EPD0 is 1.

(Otherwise: HWU062/HWU061 — PBHA hardware use bits for TTB0 descriptor bits [62:61].)

### HWU059, bit [124] / HWU060, bit [125] — PBHA Bits for TTB0

When `SMMU_IDR3.PBHA==1` and `HAD0==1`: control IMPLEMENTATION DEFINED hardware use of descriptor bits [59] and [60] for TTB0. IGNORED when `PBHA==0` or `HAD0==0`. RES0 in SMMUv3.0. RES0 if VMSAv9-128.

### NSCFG1, bit [128] — TTB1 Walk NS Attribute

Same as NSCFG0 but for TTB1. RES0 if StreamWorld == any-EL2 or EL3.

### HAD1, bit [129] — Hierarchical Attribute Disable for TTB1

(When `SMMU_IDR5.D128 == 0`:) Same as HAD0 but for TTB1 table walks. RES0 if `SMMU_IDR3.HAD==1` and StreamWorld == any-EL2 or EL3.

(When `SMMU_IDR5.D128 == 1`: this bit is `DisCH1` — disables Contiguous bit at initial TTB1 walk level for VMSAv9-128. RES0 if StreamWorld is EL3 or any-EL2.)

### E0PD1, bit [130] — E0PD TTB1 Disable Unprivileged Access

Same as E0PD0 but for TTB1-translated addresses. RES0 if `SMMU_IDR3.E0PD == 0`, VMSAv8-32 LPAE, or StreamWorld == EL3 or any-EL2.

### AIE, bit [131] — Attribute Index Extension (SMMUv3.4)

| AIE | Meaning |
|-----|---------|
| 0   | MAIR indexed by AttrIndx[2:0] (standard 8-entry). |
| 1   | MAIR indexed by AttrIndx[3:0] (16-entry extension). |

RES0 if `SMMU_IDR3.AIE == 0`.

### TTB1, bits [183:132] — Translation Table Base 1

Same format as TTB0 but for the upper VA range (VA[55]==1). RES0 if StreamWorld == any-EL2 or EL3. IGNORED if EPD1 effective value is 1. ILLEGAL if address outside eff_IPS range.

### DS, bit [186] — 52-bit Address Size Enable (LPA2)

| DS | Meaning |
|----|---------|
| 0  | 52-bit addresses for 4 KB/16 KB granules disabled. |
| 1  | 52-bit addresses for 4 KB/16 KB granules enabled (FEAT_LPA2). |

Affects both input and output address sizes. RES0 if `SMMU_IDR5.DS == 0`, VMSAv9-128, VMSAv8-32 LPAE, or if StreamWorld is EL1/EL2-E2H with both TG0 and TG1 selecting 64 KB.

### PIE, bit [187] — Stage 1 Permission Indirection Enable (SMMUv3.4)

| PIE | Meaning |
|-----|---------|
| 0   | Direct permission scheme (from translation table descriptor AP bits). |
| 1   | Indirect permission scheme using PIIP/PIIU tables. |

RES0 if `SMMU_IDR3.S1PI == 0`, `STE.S1PIE == 0`, VMSAv9-128 (implicit), or VMSAv8-32.

### HWU159, bit [188] / HWU160, bit [189] — PBHA Bits for TTB1

Same as HWU059/HWU060 but for TTB1 descriptor bits [59] and [60]. IGNORED when `HAD1 == 0`.

### SKL1, bits [191:190] — VMSAv9-128 TTB1 Skip Level

(When `SMMU_IDR5.D128 == 1` and AA64 selects VMSAv9-128.) Skip Level for initial TTB1 lookup. RES0 if StreamWorld == EL3 or any-EL2. IGNORED if EPD1 is 1.

(Otherwise: HWU162/HWU161 — PBHA hardware use bits for TTB1 descriptor bits [62:61].)

### MAIR0, bits [223:192] / MAIR1, bits [255:224] — Memory Attribute Index Registers

Equivalent to A-profile architecture MAIR registers. Combined as `{MAIR1, MAIR0}` (64-bit value). Indexed by AttrIndx from stage 1 Block/Page descriptors.

**SMMU-specific exceptions from VMSAv8-64:**
- Reserved unpredictable encodings in VMSAv8-64 are Reserved in SMMU (fixed behavior, not unpredictable).
- Encoding 0x F0 ("Tagged Normal Memory") is not supported in SMMUv3.
- When `AIE==1`: indexed by AttrIndx[3:0] (16-entry mode); AttrIndx[3] from descriptor bit [59] (VMSAv8-64) or bit [5] (VMSAv9-128).

### AMAIR0, bits [287:256] / AMAIR1, bits [319:288] — Auxiliary MAIR

Equivalent to PE AMAIR registers. Content is IMPLEMENTATION DEFINED. Software with no implementation-specific knowledge must set to 0. When `AIE==1`, indexed by AttrIndx[3:0] same as MAIR.

### PARTID, bits [367:352] — MPAM Partition ID (SMMUv3.2+)

Used when `STE.S1MPAM == 1`:
- `Config == 0b111` (nested): bits [4:0] are a 5-bit virtual PARTID; bits [15:5] RES0. Translated to physical PARTID via `VMS.PARTID_MAP`.
- `Config == 0b101` (stage 1 only): full physical PARTID.

RES0 if MPAM not supported or `STE.S1MPAM == 0`.

### PMG, bits [375:368] — MPAM Performance Monitor Group (SMMUv3.2+)

Physical PMG when `STE.S1MPAM == 1`. RES0 if MPAM not supported or `STE.S1MPAM == 0`. Interpreted as UNKNOWN if > `SMMU_(*_)MPAMIDR.PMG_MAX`.

### PIIU\<p\>, bits [3p+386:3p+384] for p=15 to 0 — Stage 1 Unprivileged Permission Interpretations (SMMUv3.4)

Set of 16 stage 1 Unprivileged base permission interpretations indexed by PIIndex from translation table descriptor:

| PIIU\<p\> | Meaning |
|-----------|---------|
| 0b000     | No Access |
| 0b001     | Read-only |
| 0b010     | Execute-only |
| 0b011     | Read-execute |
| 0b101     | Read-write |
| 0b111     | Read-write-execute |
| 0b100, 0b110 | Reserved → C_BAD_CD |

RES0 if StreamWorld is EL3 or any-EL2. RES0 if stage 1 permission indirection disabled.

### PIIP\<p\>, bits [3p+450:3p+448] for p=15 to 0 — Stage 1 Privileged Permission Interpretations (SMMUv3.4)

Set of 16 stage 1 Privileged base permission interpretations. Same encoding as PIIU. ILLEGAL if PIIP[x] grants Privileged execute AND PIIU[x] grants Unprivileged write (results in C_BAD_CD). RES0 if stage 1 permission indirection disabled.

### PnCH, bit [122] — Protected Attribute Enable (SMMUv3.4 / FEAT_THE)

| PnCH | Meaning |
|------|---------|
| 0    | Protected attribute disabled. |
| 1    | Protected attribute enabled. |

RES0 if VMSAv9-128 (function implicitly enabled). RES0 if VMSAv8-32 LPAE. RES0 if `SMMU_IDR3.THE == 0`.

### EPAN, bit [123] — Enhanced PAN (SMMUv3.4)

| EPAN | Meaning |
|------|---------|
| 0    | PAN affects data access permissions only. |
| 1    | PAN==1 also prevents instruction-access-permitted addresses from being data-accessed by privileged code. |

RES0 if VMSAv8-32 LPAE, StreamWorld == any-EL2 or EL3, or stage 1 permission indirection enabled. RES0 if `SMMU_IDR3.EPAN == 0`.

## §5.4.1 CD Notes

**StreamWorld == any-EL2 or EL3 (not E2H):**
- Only TTB0 is supported; TTB1 is unreachable.
- ASID is IGNORED.
- T1SZ, TBI1, TG1, SH1, OR1, IR1, TTB1, HAD1, NSCFG1 are RES0.
- T0SZ must cover the full required VA input space.

**Shared CD across multiple STEs:** Only permitted when all STEs configure the same StreamWorld (Exception level). Mixing NS-EL2 and NS-EL1 STEs sharing a CD is incorrect because TTB1 would be both enabled and unused.

**CD legality depends on STE properties:** StreamWorld, `STE.S1STALLD`, and stage 2 configuration (AA64 and Config[1]) from the locating STE affect CD legality.

**Caching:** A successfully fetched CD may be cached. Cached CDs are keyed by `{StreamID, SubstreamID}` (qualified by SEC_SID). A common CD shared by multiple STEs must be invalidated using every `{StreamID, SubstreamID}` combination it is reachable from. A failed fetch does not cache.

**TLB-cacheable fields** (require TLB invalidation in addition to CMD_CFGI_CD when altered): HAD{0,1}, AFFD, ASID+ASET, MAIR, AMAIR, EPD{0,1}, TTB{0,1}, T{0,1}SZ, OR{0,1}, IR{0,1}, SH{0,1}, ENDI, TG{0,1}, HA, HD, WXN, UWXN, AA64, TBI, IPS, NSCFG{0,1}, PAN, EPAN, PnCH, PIE, PIIP, PIIU, DisCH0, DisCH1, SKL0, SKL1, AIE, HAFT.

**Non-cacheable fields** (only CD cache invalidation required): A, R, S.

### §5.4.1.1 EPDx Behavior

EPD0/EPD1 disable translation walks and IGNORE the corresponding TxSZ/TGx/IRx/ORx/SHx/TTBx fields. A TLB miss on an EPDx-disabled region → F_TRANSLATION. A TLB hit is still allowed (A-profile compatible behavior for performance). ILLEGAL field values in disabled TTBx do not cause C_BAD_CD.

## §5.4.2 Validity of CD — ILLEGAL Conditions

A CD is ILLEGAL (and causes C_BAD_CD) when any of:
- `V == 0`.
- `STE.S1STALLD == 1` and `CD.S == 1`.
- `SMMU_IDR0.TERM_MODEL == 1` and `CD.A == 0`.
- `STALL_MODEL == 0b01` and `CD.S == 1` (stall not supported).
- `STALL_MODEL == 0b10` and `CD.S == 0` (stall forced).
- ENDI selects unsupported endianness.
- AA64 selects VMSAv8-32 LPAE when not supported or for incompatible StreamWorld (any-EL2-E2H, S-EL2, EL3).
- AA64 selects VMSAv8-64 when not supported, or stage 2 uses VMSAv8-32 LPAE.
- AA64 selects VMSAv9-128 for StreamWorld == EL2, or when `STE.S1PIE == 0`.
- HA or HD set when `HTTU == 0b00`, or HD set when `HTTU == 0b01`, for VMSAv8-64/VMSAv9-128.
- `HAFT == 1` and `HA == 0` (for VMSAv8-64/VMSAv9-128).
- `ASID[15:8] != 0` when `SMMU_IDR0.ASID16 == 0`, for StreamWorlds that use ASID tagging.
- T0SZ or T1SZ out of range (SMMUv3.1+, when not IGNORED by EPDx).
- TTB0 or TTB1 address outside eff_IPS range (when not IGNORED by EPDx).
- Invalid or unsupported granule selection (when not IGNORED by EPDx).
- VMSAv9-128: invalid SKL0/SKL1 relative to TxSZ.
- Permission indirection enabled (`PIE==1` or VMSAv9-128) and PIIP or PIIU contain Reserved encodings.
- PIIP[x] grants Privileged execute AND PIIU[x] grants Unprivileged write.
- TTB address outside 48-bit range when `DS==0`, VMSAv8-64, and granule < 64 KB.

## Stage 1 Fault Behavior Flags Summary

The three fault flag bits `{A, R, S}` control behavior for Translation-related faults:

| CD.S | CD.A | Behavior on stage 1 Translation-related fault |
|------|------|------------------------------------------------|
| 0    | 0    | Terminate with RAZ/WI (only when `SMMU_IDR0.TERM_MODEL == 0`) |
| 0    | 1    | Terminate with abort |
| 1    | —    | Stall (if `STE.S1STALLD == 0` and stall model supported) |

`CD.R == 1`: events may be recorded. `CD.R == 0`: events may not be recorded.

## Model Implementation Notes

- The CD is fetched from memory (PA or IPA) during the configuration lookup phase. A functional model must implement the full fetch + validity check before beginning the stage 1 table walk.
- For nested configurations, the CD fetch address is an IPA, meaning a stage 2 walk occurs before stage 1 walk begins. A stage 2 fault during CD fetch is reported as a stage 2 fault with `CLASS == CD`.
- `CD.ASID` + `CD.ASET` are both critical for TLB tagging — a model must track both. ASET==0 entries participate in broadcast invalidations; ASET==1 do not (for address-targeted broadcasts).
- `CD.TBI{0,1}` affects address range checking: VA[63:56] bits are ignored for sign-extension validation.
- For EL2/EL3 StreamWorlds: only TTB0 is valid; ASID is ignored; all TTB1-related fields are RES0.
- Multiple CDs representing the same address space (same security state, StreamWorld, VMID, ASID) must have identical values for all TLB-cacheable fields.
- PARTID/PMG in the CD are never cached in TLBs; they may differ between CDs with the same ASID.

## Related Concepts

- [stream-table-entry.md](stream-table-entry.md) — STE contains `S1ContextPtr` pointing to CD
- [two-stage-translation.md](two-stage-translation.md) — CD governs stage 1 of the translation
- [fault-models.md](fault-models.md) — CD.{S, R, A} flags configure stage 1 fault behavior
- [streamid-substreamid.md](streamid-substreamid.md) — SubstreamID selects the CD within a CD table
- [tlb-invalidation.md](tlb-invalidation.md) — ASID/ASET from CD used to tag and invalidate TLB entries
- [httu.md](httu.md) — CD.HA/HD/HAFT control stage 1 HTTU behavior
- [permission-indirections.md](permission-indirections.md) — CD.PIE, CD.PIIP, CD.PIIU for stage 1 indirect permissions
- [mpam.md](mpam.md) — CD.PARTID/PMG when STE.S1MPAM==1

## Sources That Use This Concept

- [../sources/ihi0070g-b-smmuv3-architecture-spec.md](../sources/ihi0070g-b-smmuv3-architecture-spec.md) — §3.3.2 StreamIDs to Context Descriptors; §5.3 L1CD format; §5.4 CD data structure format; §5.4.1 CD notes; §5.4.1.1 EPDx behavior; §5.4.2 Validity of CD; §5.5 Fault configuration bits; §3.4.1 Input address size
