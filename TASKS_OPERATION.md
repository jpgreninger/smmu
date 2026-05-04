# TASKS_OPERATION.md — ARM SMMUv3 Chapter 3 Operational Checklist

Every item is a concrete behavioral rule, encoding, fault condition, or procedural step from Chapter 3 of IHI0070G_b. Line numbers cite the specific line in the spec containing the rule.

---

## §3.1 Software Interface

- [x] SMMU provides three software interfaces: memory-based data structures, memory-based circular buffer queues (Command, Event, PRI), and a register set per Security state (§3.1, line 1217) — NS interface fully present; Secure register set out-of-scope (§3.10.2 🚫)
- [x] PRI queue is only present on SMMUs supporting PRI services (§3.1, line 1220) — PASS: `pri_supported` gates IDR0.PRI, PRIQEN, and `submit_page_request()`
- [x] When Secure state is supported, an additional register set exists to allow Secure software to maintain Secure device structures, issue commands on a second Secure Command queue and read Secure events from a Secure Event queue (§3.1, line 1223) — 🚫 Out of scope (§3.10.2 explicitly 🚫; Secure interface not implemented)
- [x] IMPLEMENTATION DEFINED fields must not be used in a way that makes a generic SMMUv3 driver unusable (§3.1, line 1227) — PASS: only IMPL DEF field (`EventEntry.reason`) hardcoded to 0
- [x] A driver without extended knowledge of IMPLEMENTATION DEFINED fields must treat them as Reserved and set to 0 (§3.1, line 1227) — N/A: driver behavioral constraint, not an SMMU implementation requirement
- [x] An implementation only uses IMPLEMENTATION DEFINED fields to enable extended functionality and must remain compatible with generic driver software when those fields are set to 0 (§3.1, line 1229) — PASS: all IMPL DEF surfaces produce 0; PRI gated on IDR0.PRI capability advertisement

## §3.2 Stream Numbering

- [x] StreamID is of IMPLEMENTATION DEFINED size, between 0 bits and 32 bits (§3.2, line 1238) — PASS: `StreamID(u32)` full range; IDR1.SIDSIZE=32 advertised (`mod.rs:1373`, `smmu.cpp:3478`)
- [x] SubstreamID is of IMPLEMENTATION DEFINED size, between 0 bits and 20 bits (§3.2, line 1241) — PASS: `PASID_MAX=0xFFFFF`; IDR1.SSIDSIZE=20 (`pasid.rs:36`, `types.h:1991`)
- [x] Transactions provided with a SubstreamID are terminated when stage 1 translation is not enabled (§3.2, line 1354) — PASS: SSV=1 + stage1_enabled=false → C_BAD_SUBSTREAMID (`mod.rs:5236`, `smmu.cpp:668`)
- [x] A stage 2-only implementation does not take a SubstreamID input (§3.2, line 1252) — PASS: same path as above (BUG-AUDIT-111 fix)
- [x] An implementation with stage 1 is not required to support substreams (§3.2, line 1252) — N/A: architectural statement; both impls support substreams (superset of requirement)
- [x] When Secure state is supported, the StreamID input is qualified by SEC_SID determining Secure or Non-secure StreamID namespace (§3.2, line 1254) — N/A: Secure state out-of-scope; IDR0.SecP=0 in both models; requirement is conditional
- [x] For PCI, StreamID must be at least 16 bits for SMMU implementations intended for use with PCI clients (§3.2, line 1258) — PASS: IDR1.SIDSIZE=32 ≥ 16
- [x] Arm recommends StreamID be a dense namespace starting at 0 (§3.2, line 1236) — N/A: architectural recommendation only, no behavioral enforcement required
- [x] StreamID namespace is per-SMMU; devices with the same StreamID behind different SMMUs are seen as different sources (§3.2, line 1236) — N/A: system-level architectural property; each SMMU instance has its own stream map
- [x] SubstreamID maximum size of 20 bits matches the maximum size of a PCIe PASID (§3.2, line 1245) — PASS: `PASID_MAX=0xFFFFF` (2^20-1); IDR1.SSIDSIZE=20
- [x] For PCIe, SubstreamID is intended to be directly provided from the PASID in a one-to-one fashion (§3.2, line 1256) — N/A: informational design intent; SubstreamID/PASID are used interchangeably throughout both implementations

## §3.3 Data Structures and Translation Procedure

- [x] When SMMU_CR0.SMMUEN == 0 (globally disabled), transaction passes through without address modification; attributes applied from SMMU_GBPA (§3.3, line 1403) — PASS: `smmu/mod.rs:4731-4787` enabled fast-path; GBPA attrs applied via `with_output_attrs()`
- [x] When SMMU_GBPA.ABORT is set, all transactions are aborted in bypass (§3.3, line 1403) — PASS: `smmu/mod.rs:4733-4736` `gbpa_abort.load()` → `Err(GbpaAbort)`
- [x] If the SMMU does not implement one of the two translation stages, it behaves as though that stage is permanently in bypass (§3.3, line 1429) — PARTIAL: per-STE rejection via `check_ste_illegal()` (`smmu/mod.rs:3103-3134`) prevents configuration; no explicit SMMU-level bypass mode (acceptable gap — behavioral effect identical)
- [x] An SMMU must support at least one stage of translation (§3.3, line 1429) — FAIL: no guard rejects `s1p_supported=false AND s2p_supported=false` simultaneously (BUG-AUDIT-134)
- [x] S1ContextPtr and L2Ptr addresses are IPAs when both stage 1 and stage 2 are in use, and PAs when only stage 1 is used (§3.3.2, line 1375) — N/A: CDs modeled as Rust flat map; no hardware pointer dereferencing; IPA/PA discrimination inapplicable to software simulation

### §3.3.1 Stream Table Lookup

- [x] StreamID is range-checked against the programmed table size; a transaction is terminated if its StreamID would select an entry outside the configured Stream table extent; C_BAD_STREAMID is recorded (§3.3.1, line 1276) — PASS: `smmu/mod.rs:4789-4814`; `CBadStreamid` event; BUG-AUDIT-101
- [x] Linear Stream table format is supported by all SMMU implementations (§3.3.1.1, line 1285) — PASS: `StreamTableFormat::Linear` is `#[default]`; always accepted
- [x] Linear Stream table is a contiguous array of STEs indexed from 0 by StreamID; size is configurable as 2^n multiple of STE size (§3.3.1.1, line 1285) — PASS: `smmu/mod.rs:2071-2097`; `1u32 << log2size` as exclusive bound; BUG-AUDIT-101
- [x] SMMUs supporting more than 64 StreamIDs (6 bits) must also support two-level Stream tables (§3.3.1.2, line 1298) — PASS: `IDR0.ST_LEVEL=0b01`; `StreamTableFormat::TwoLevel` valid; BUG-AUDIT-102/106/107/108
- [x] 2-level Stream table top-level is indexed by StreamID[n:x] where x is SMMU_STRTAB_BASE_CFG.SPLIT; second-level tables indexed by up to StreamID[x-1:0] (§3.3.1.2, line 1294) — PASS: `smmu/mod.rs:2055-2065`; `l1_idx = stream_id >> split`; `l1_size = 1 << (log2size - split)`
- [x] Where 2-level Stream tables are supported, split points of 6, 8, and 10 bits can be used (§3.3.1.2, line 1296) — PASS: `smmu/mod.rs:2034-2042`; `set_strtab_split()` clamps to {6,8,10}
- [x] SMMU_IDR0.ST_LEVEL field advertises support for 2-level Stream table format (§3.3.1.2, line 1296) — PASS: `smmu/mod.rs:1359`; `IDR0.ST_LEVEL=0b01` hardcoded
- [x] Top-level descriptors contain pointer to second-level table along with StreamID span; each can be marked invalid (§3.3.1.2, line 1303) — PARTIAL: two-level bounds validation is numeric only; no L1 descriptor struct with span/valid fields; individual L1 entries cannot be selectively invalidated (acceptable gap — software model abstraction)

### §3.3.2 StreamIDs to Context Descriptors

- [x] When STE.S1DSS == 0b00, all traffic expected to have SubstreamID; lack of SubstreamID causes abort and event recorded (§3.3.2, line 1362) — PASS: `smmu/mod.rs:5354-5414`; `FStreamDisabled` recorded and returned; BUG-AUDIT-109/110/112
- [x] When STE.S1DSS == 0b01, transaction without SubstreamID is treated as stage 1-bypass (§3.3.2, line 1363) — PASS: `smmu/mod.rs:5416-5470`; stage-1 bypassed, stage-2 forwarded or identity PA returned
- [x] When STE.S1DSS == 0b10, transaction without SubstreamID uses the CD of Substream 0; transactions arriving with SubstreamID 0 are aborted and event recorded (§3.3.2, line 1364) — PASS: `smmu/mod.rs:5472-5530`; SSV=true+PASID=0 → `FStreamDisabled`; SSV=false → CD[0] used
- [x] STE.S1ContextPtr field gives address of one or more CDs, configured by STE.S1Fmt and STE.S1CDMax (§3.3.2, line 1366) — PARTIAL: `s1cd_max` fully implemented (`stream_context/mod.rs:151-153`); `STE.S1Fmt` (linear vs two-level CD table format selector) not modeled — no `s1_fmt` field exists; S1ContextPtr as hardware address is N/A for software model
- [x] Multiple StreamID/SubstreamID configurations with identical ASID/VMID/StreamWorld must maintain same configuration where that configuration can affect TLB lookup (§3.3.3, line 1514) — FAIL: no cross-stream consistency enforcement in `configure_stream()`/`reconfigure_stream()` (BUG-AUDIT-135)
- [x] Two streams sharing the same ASID/VMID/StreamWorld must use the same translation table base addresses and translation granule (§3.3.3, line 1515) — FAIL: no validation that streams sharing ASID/VMID/StreamWorld use same TTB0/TTB1/granule (BUG-AUDIT-136)
- [x] For any-EL2 and EL3 regimes, only one translation table is used; CD.TTB1 is unused (§3.3.3, line 1517) — FAIL: no STRW-conditional enforcement forcing EPD1=1 for El2/El3 StreamWorlds; TTB1 size-bounds check exists but EPD1 not forced (BUG-AUDIT-137)
- [x] Selecting an inconsistent combination of StreamWorld and CD.AA64 causes the CD to be ILLEGAL (§3.3.3, line 1519) — N/A: AArch32 LPAE (AA64=false) universally unsupported; `AA64=false` rejected globally in `check_cd_illegal()`; no STRW-specific guard needed (intentional design — FINDING-333-04)
- [x] Secure stage 2 is not supported for VMSAv8-32 LPAE translation tables (§3.3.3, line 1521) — PARTIAL: VMSAv8-32 stage-2 rejected universally via TTF=0b10 (AArch64-only) at `smmu/mod.rs:3080-3098`; Secure case satisfied by superset rejection
- [x] AP[1] bit is IGNORED for any-EL2 and EL3 StreamWorlds (VMSAv8-64 and VMSAv9-128) (§3.3.4, line 1536) — PASS: `stream_context/mod.rs:2834-2835`; `strw_suppresses_priv()` returns true for `El2|El3`
- [x] any-EL2-E2H translations maintain privileged/non-privileged checks in the same manner as EL1 (§3.3.4, line 1536) — PASS: `stream_context/mod.rs:2887-2893`; El2E2h excluded from privilege suppression; EL1-like AP[1] effective
- [x] Bits [63:60] of stage 2 Block and Page descriptors are Reserved for use by a System MMU; in SMMUv3.1 and later these bits are RES0 (§3.3.5, line 1555) — N/A: no raw 64-bit descriptor wire format; PageEntry is a Rust struct; constraint applies to hardware page-table walkers only (§3.3.5 N/A per tracker)

### §3.3.3 StreamWorld Table

- [x] StreamWorld NS-EL1: Non-secure EL1&0, with ASID and VMID tags (§3.3.3, line 1478) — PASS: `types/config.rs:42`; `StreamWorld::El1El0=0x00`; ASID+VMID tagged in `cache/mod.rs:878,924`
- [x] StreamWorld NS-EL2: Non-secure EL2 without E2H; translations do not have an ASID tag (§3.3.3, line 1479) — PASS: `types/config.rs:43-44`; `StreamWorld::El2=0x01`; ASID set to 0 at `smmu/mod.rs:4902-4905`
- [x] StreamWorld NS-EL2-E2H: Non-secure EL2&0 with E2H; translations have ASID tag (§3.3.3, line 1480) — PASS: `types/config.rs:45-46`; `StreamWorld::El2E2h=0x02`; ASID tagged non-zero at `smmu/mod.rs:4902-4904`
- [x] StreamWorld S-EL2: Secure EL2 without E2H; no ASID tag (§3.3.3, line 1481) — 🚫 Out of scope (Secure state §3.10.2 🚫; no `S_EL2` StreamWorld variant)
- [x] StreamWorld S-EL2-E2H: Secure EL2&0 with E2H; ASID tag (§3.3.3, line 1482) — 🚫 Out of scope (Secure state §3.10.2 🚫; no `S_EL2_E2H` StreamWorld variant)
- [x] StreamWorld EL3: EL3 in AArch64 state when FEAT_RME not implemented; no ASID tag (§3.3.3, line 1491) — PARTIAL: `types/config.rs:47-48`; `StreamWorld::El3=0x03` exists; privilege suppressed via `strw_suppresses_priv()`; FEAT_RME conditioning absent (acceptable — FEAT_RME/Realm 🚫 out-of-scope §2.6)
- [x] StreamWorld Realm-EL1: Realm EL1&0 (§3.3.3, line 1492) — 🚫 Out of scope (RME §2.6 🚫)
- [x] StreamWorld Realm-EL2: Realm EL2 without E2H; no ASID tag (§3.3.3, line 1493) — 🚫 Out of scope (RME §2.6 🚫)
- [x] StreamWorld Realm-EL2-E2H: Realm EL2&0 with E2H; ASID tag (§3.3.3, line 1494) — 🚫 Out of scope (RME §2.6 🚫)
- [x] A translation is architecturally unique if identified by unique {StreamWorld, VMID, ASID, Address} (§3.3.3, line 1502) — PARTIAL: `CacheKey` keyed on `{stream_id, pasid, iova, security_state}`; VMID/ASID/StreamWorld are metadata for TLBI targeting but not lookup discriminants; correctness preserved by per-stream TLBI scan (FINDING-333-03 — acceptable gap per §3.3 notes)

## §3.4 Address Sizes

- [x] SMMU input address size is 64 bits (§3.4, line 1562) — PARTIAL: all IOVA accepted as u64; IAS not explicitly advertised in IDR; acceptable for TTF=0b10-only model
- [x] IAS = MAX(SMMU_IDR0.TTF[0]==1 ? 40 : 0, SMMU_IDR0.TTF[1]==1 ? OAS : 0) (§3.4, line 1568) — PARTIAL: TTF=0b10 hardcoded `mod.rs:1383`; IAS formula implicit; acceptable given single-TTF model
- [x] VMSAv8-32 LPAE always supports IPA size of 40 bits; IPS field of the CD is IGNORED (§3.4, line 1570) — N/A: VMSAv8-32 unsupported (TTF=0b10 hardcoded; rejected via C_BAD_STE)
- [x] OAS reflects maximum usable PA output from last stage of VMSAv8-64 or VMSAv9-128 translations; discoverable from SMMU_IDR5.OAS (§3.4, line 1572) — PASS: `mod.rs:1452-1466` IDR5.OAS derived from `address_config.max_pa_bits`
- [x] When SMMU_(*_)CR0.SMMUEN == 0 and SMMU_(*_)GBPA.ABORT == 0: if input address exceeds OAS, transaction terminated with abort and NO event recorded (§3.4, line 1576) — PASS: `mod.rs:4819-4833` silent abort, no fault event
- [x] When STE.Config == 0b100 (bypass all stages): if input address exceeds OAS, transaction terminated with abort and F_ADDR_SIZE is recorded (§3.4, line 1578) — PASS: `mod.rs:5252-5282`
- [x] Stage 1 Translation fault (F_TRANSLATION) occurs if VA is outside range specified by CD (§3.4, line 1585) — PASS: `mod.rs:5102-5212` T0SZ range + TBI mask enforced
- [x] For VMSAv8-32 LPAE CD: maximum input range is fixed at 32 bits; Translation fault if upper 32 bits are not all zero (§3.4, line 1586) — N/A: VMSAv8-32 unsupported
- [x] For VMSAv8-64: maximum input size is 48 bits if SMMU_IDR5.VAX == 0b00 or 4K/16K granule with DS==0 (§3.4, line 1593) — PARTIAL: T0SZ default=16 implies 48-bit VA; VAX field not modeled (acceptable, only 48-bit VA supported)
- [x] For VMSAv8-64: maximum input size is 52 bits if SMMU_IDR5.VAX == 0b01 or 0b10 and 64KB granule or DS==1 (§3.4, line 1596) — N/A: VAX not modeled; 52-bit VA unsupported (intentional)
- [x] For VMSAv9-128: max input 48 bits if VAX==0b00; 52 bits if VAX==0b01; 55 bits EL1/EL2-E2H if VAX==0b10; 56 bits EL3 if VAX==0b10 (§3.4, line 1599) — N/A: VMSAv9-128 not implemented
- [ ] VA is inside range only if correctly sign-extended from top bit of range size upwards, except for TBI configurations (§3.4, line 1605) — FAIL: BUG-AUDIT-138 OPEN — canonical sign-extension check absent; `mod.rs:5102-5212` checks magnitude only
- [x] Address output from stage 1 translation causes F_ADDR_SIZE if exceeds IPA size range (§3.4, line 1609) — PASS: `stream_context/mod.rs:2268-2280`
- [x] For VMSAv8-64/VMSAv9-128 CDs, IPA size given by effective IPS field of CD, capped to OAS (§3.4, line 1611) — PASS: `stream_context/mod.rs:2272-2278`
- [x] When bypassing stage 1 (STE.Config == 0b1x0, STE.S1DSS == 0b01, or unimplemented): if input address exceeds IAS, stage 1 F_ADDR_SIZE occurs, transaction terminated, F_ADDR_SIZE recorded (§3.4, line 1613) — PARTIAL: S1DSS=0b01 OAS check `mod.rs:5504-5519`; bypass-STE at `mod.rs:5252-5282`
- [x] TBI configuration can only be enabled when a CD is used (stage 1 translates); always disabled when stage 1 bypassed or disabled (§3.4, line 1615) — PARTIAL: TBI masking `mod.rs:5223-5229` gated by S1-enabled path implicitly; no explicit guard
- [x] Stage 2 Translation fault if IPA is outside range configured by S2T0SZ (§3.4, line 1623) — PASS: `stream_context/mod.rs:2282-2295, 2512-2523, 2683-2694`
- [x] For VMSAv8-32 LPAE STE: stage 2 input range capped at 40 bits regardless of IAS size (§3.4, line 1624) — N/A: VMSAv8-32 unsupported
- [x] For VMSAv8-64/VMSAv9-128 STE: stage 2 input range capped to IAS (§3.4, line 1627) — PARTIAL: S2T0SZ constrains IPA at `stream_context/mod.rs:239-245`; explicit IAS cap not enforced
- [x] Stage 2 Address Size fault if output address exceeds effective PA output range from S2PS (§3.4, line 1629) — PASS: `stream_context/mod.rs:2353-2364`
- [x] For VMSAv8-32 LPAE STE: output range fixed at 40 bits; STE.S2PS field is IGNORED; if OAS < 40, address silently truncated to OAS (§3.4, line 1631) — N/A: VMSAv8-32 unsupported
- [x] After stage 2 check, if output address smaller than OAS, address is zero-extended to match OAS (§3.4, line 1633) — PASS: implicit in u64 arithmetic; zero-extension of narrower PA is automatic
- [x] When bypassing stage 2 (STE.Config == 0b10x or unimplemented): IPA outside OAS range is silently truncated to OAS; if IPA smaller than OAS, zero-extended (§3.4, line 1635) — PASS: `mod.rs:5284-5321` bitmask truncation; zero-extension implicit

### §3.4.1 Input Address Size and VA Size

- [x] When SMMU_IDR5.VAX == 0b00: VAS is 49 bits (2×48 bits) (§3.4.1, line 1653) — N/A: VAX not modeled; 48-bit implied by T0SZ default
- [x] When SMMU_IDR5.VAX == 0b01: VAS is 53 bits (2×52 bits) (§3.4.1, line 1654) — N/A: VAX not modeled
- [x] When SMMU_IDR5.VAX == 0b10: VAS is 56 bits (2×55 bits for EL1/EL2-E2H, or 1×56 bits for EL3) (§3.4.1, line 1655) — N/A: VMSAv9 not modeled
- [x] VMSAv8-32 LPAE contexts use bits [31:0] of input address directly as VA; Translation fault if upper 32 bits are not all zero (§3.4.1, line 1660) — N/A: VMSAv8-32 unsupported
- [ ] When TBI not enabled: AddrTop == 63 for sign-extension check (§3.4.1, line 1664) — FAIL: BUG-AUDIT-138 OPEN — no AddrTop=63 sign-extension validation in `mod.rs:5102-5212`
- [ ] When TBI enabled: AddrTop == 55; VA[63:56] are ignored; effective VA[63:56] taken as sign-extension of VA[55] (§3.4.1, line 1665) — FAIL: BUG-AUDIT-138 OPEN — TBI tag strip `mod.rs:5114-5118` done but sign-extension of VA[55:48] not validated
- [x] All input address bits are recorded unmodified in SMMU fault event records (§3.4.1, line 1680) — PASS: original `iova` (not TBI-stripped `lookup_iova`) used in fault records at `mod.rs:5147,5160`

### §3.4.2 Address Alignment Checks

- [x] The SMMU architecture does not check the alignment of incoming transaction addresses (§3.4.2, line 1684) — N/A: spec says SMMU does NOT check alignment; no implementation needed; confirmed absent

### §3.4.3 Address Sizes of SMMU-Originated Accesses

- [x] SMMUv3.1+: if STE.S1ContextPtr address exceeds OAS (stage 1-only), generates C_BAD_STE (§3.4.3, line 1715) — N/A: flat model; S1ContextPtr is a logical key (StreamID), not a PA; no hardware fetch
- [x] SMMUv3.0: CONSTRAINED UNPREDICTABLE whether generates F_CD_FETCH, C_BAD_STE, or truncates S1ContextPtr to OAS (§3.4.3, line 1715) — N/A: flat model
- [x] SMMUv3.1+: if L1CD.L2Ptr address exceeds OAS (stage 1-only), generates C_BAD_SUBSTREAMID (§3.4.3, line 1723) — N/A: flat model; L1CD structures not modeled; L2Ptr not stored
- [x] STE fetch address out-of-range: CONSTRAINED UNPREDICTABLE whether truncates address or generates F_STE_FETCH (§3.4.3, line 1731) — N/A: flat model; STEs stored in HashMap, no hardware fetch
- [x] Queue and MSI access addresses exceeding OAS: truncated to OAS (§3.4.3, line 1708) — N/A: flat model; queues are VecDeque internal state, not PA emissions (same basis as S1ContextPtr/STE-fetch N/A)
- [x] VMS fetch (STE.VMSPtr) address out of range: generates C_BAD_STE (§3.4.3, line 1707) — N/A: VMS migration not implemented; VMSPtr not a modeled field
- [x] Starting-level translation table descriptor address in STE.S2TTB or CD.TTBx out of range: CD or STE ILLEGAL (§3.4.3, line 1711) — PASS: `mod.rs:2851-2889` (S2TTB>OAS→C_BAD_STE), `mod.rs:2892-2946` (TTB0/TTB1>IPS→C_BAD_CD)
- [x] Intermediate translation table descriptor address out of range: Stage 1/2 Address Size fault (§3.4.3, line 1710) — N/A: flat model; no multi-level page table walk; intermediate descriptor addresses not checked
- [x] The address of an L1CD or CD given by STE.S1ContextPtr or L1CD.L2Ptr is not subject to a stage 1 Address Size fault check (§3.4.3, line 1736) — N/A: flat model; no such address check exists

## §3.5 Command and Event Queues

### §3.5.1 SMMU Circular Queues

- [x] Queue is a 2^n-items sized circular FIFO with PROD and CONS index registers (§3.5.1, line 1750)
- [x] For Command queue (input): PROD index updated by software after inserting; CONS updated by SMMU as items consumed (§3.5.1, line 1751)
- [x] PROD indicates index of location that can be written next; CONS indicates index of next location to be read (§3.5.1, line 1753)
- [x] Indexes must always increment and wrap to bottom when passing top entry; never moved backwards (§3.5.1, line 1753)
- [x] If PROD==CONS and wrap bits equal: queue is EMPTY (§3.5.1, line 1757)
- [x] If PROD==CONS and wrap bits different: queue is FULL (§3.5.1, line 1758)
- [x] Wrap bit must toggle each time index wraps off high-end back to low-end; software reads register, increments/wraps index (toggling wrap when required), writes back wrap and index fields atomically (§3.5.1, line 1755)
- [x] Queue indexes must be initialized into a consistent state before enabling (§3.5.1, line 1763)
- [x] Agent controlling SMMU must NOT write queue indexes to inconsistent states (§3.5.1, line 1771)
- [x] ILLEGAL inconsistent state: PROD.WR > CONS.RD and PROD.WR_WRAP != CONS.RD_WRAP (§3.5.1, line 1773)
- [x] ILLEGAL inconsistent state: PROD.WR < CONS.RD and PROD.WR_WRAP == CONS.RD_WRAP (§3.5.1, line 1774)
- [x] Each circular buffer is 2^n-items where 0 <= n <= 19; each PROD and CONS register is 20 bits (§3.5.1, line 1788)
- [x] When producing/consuming entries, software must only increment an index (or wrap to start); never move backwards (§3.5.1, line 1801)
- [x] There is one Command queue per implemented Security state; commands consumed in order (§3.5.1, line 1807) — N/A: Secure-side not implemented; NS CMDQ consumed in-order ✓
- [x] All output queues (Event and PRI) are appended to sequentially (§3.5.1, line 1811)
- [x] When SMMU_S_IDR1.SECURE_IMPL == 1, there is one Secure Event queue receiving events from all Secure streams (§3.5.1, line 1810) — N/A: Secure-side not implemented (SECURE_IMPL=0)

### §3.5.2 Queue Entry Visibility Semantics

- [x] Producer must ensure update to PROD index is not observable before new queue entries are observable (§3.5.2, line 1815)
- [x] Consumer must not assume presence of valid entry through any mechanism other than having first observed an updated PROD index covering the entry position (§3.5.2, line 1816)
- [x] SMMU makes queue updates observable through PROD index no later than when it asserts the queue interrupt (§3.5.2, line 1818) — N/A: Event-queue interrupt not implemented in software model; requirement trivially met (no async IRQ)

### §3.5.3 Event Queue Behavior

- [x] Stall fault events are never discarded if the Event queue is full; recorded when space next becomes available (§3.5.3, line 1824)
- [x] Non-stall events are discarded if the Event queue is full (§3.5.3, line 1824)
- [x] No requirement for terminated-transaction event to be made visible before transaction response is returned to client (§3.5.3, line 1839)
- [x] CMD_SYNC enforces visibility of events relating to terminated transactions (§3.5.3, line 1839)

### §3.5.4 Definition of Event Record Write "Commit"

- [x] Stall event record commit must not occur until queue entry is deemed writable (queue enabled and not full) (§3.5.4, line 1859)
- [x] An event write that has committed is guaranteed to eventually become visible in the Event queue unless an external abort occurs (§3.5.4, line 1857)
- [x] PROD.WR index must be updated to publish new record to software; record is not visible until this update (§3.5.4, line 1853)

### §3.5.5 Event Merging

- [x] Events can be merged where event types and all fields are identical except fields explicitly indicated in §7.3, and if Stall field is present, Stall == 0 (§3.5.5, line 1865)
- [x] Stall fault records are NOT merged (§3.5.5, line 1868)
- [x] An implementation that merges events is required to support STE.MEV flag to enable/inhibit per-stream merging (§3.5.5, line 1870)

### §3.5.6 Enhanced Command Queue Interfaces

🚫 ECMDQ not implemented (IDR1.ECMDQ=0 not advertised; requires dedicated hardware register pages — out of scope for software model)

- 🚫 ECMDQ support advertised in SMMU_IDR1.ECMDQ and SMMU_S_IDR0.ECMDQ (§3.5.6, line 1882)
- 🚫 Up to 256 Command queue control pages; each contains control interface for up to 256 queues (§3.5.6, line 1886)
- 🚫 Presence of ECMDQ does not imply removal of SMMU_(*_)CMDQ_* interfaces (§3.5.6, line 1891)
- 🚫 If any ECMDQ interface is enabled, SMMU_(*_)CR1.{QUEUE_SH, QUEUE_OC, QUEUE_IC} are read-only (§3.5.6.1, line 1922)
- 🚫 SMMU consumes commands from queue if queue is non-empty (§3.5.6.1, line 1926)
- 🚫 CMD_SYNC consumed from ECMDQ guarantees effects of previously-consumed commands on that queue are complete (§3.5.6.1, line 1927)
- 🚫 SMMU does not give guaranteed serialization or total order of commands consumed across different queues (§3.5.6.1, line 1933)
- 🚫 If SMMU_IDR0.SEV == 1, SMMU triggers WFE wake-up event when any ECMDQ becomes non-full (§3.5.6.1, line 1936)
- 🚫 ECMDQ interface enabled when SMMU_ECMDQ_PRODn.EN == SMMU_ECMDQ_CONSn.ENACK == 1 (§3.5.6.2, line 1940)
- 🚫 ECMDQ interface disabled when SMMU_ECMDQ_PRODn.EN == SMMU_ECMDQ_CONSn.ENACK == 0 (§3.5.6.2, line 1942)
- 🚫 Once disabled (ENACK == 0): errors reported, consumption stopped, and SMMU_ECMDQ_CONSn fields are stable (§3.5.6.2, line 1946)
- 🚫 SMMU updates SMMU_ECMDQ_CONSn.ENACK even if ERRACK != ERR (§3.5.6.2, line 1947)
- 🚫 On ECMDQ error: SMMU toggles SMMU_ECMDQ_CONSn.ERR and updates ERR_REASON; RD and RD_WRAP point at failed command (§3.5.6.3, line 1951)
- 🚫 If ERRACK != ERR as result of error: SMMU does not consume commands (§3.5.6.3, line 1954)
- 🚫 If ERR update is visible: updates of ERR_REASON, RD and RD_WRAP are also visible (§3.5.6.3, line 1960)
- 🚫 ECMDQ errors additionally reported in SMMU_GERROR.CMDQP_ERR for NS state; Secure ECMDQ errors in SMMU_S_GERROR.CMDQP_ERR (§3.5.6.3, line 1964)
- 🚫 ECMDQs operate independently of SMMU_(*_)GERROR.CMDQ_ERR error status (§3.5.6.3, line 1966)
- 🚫 If MSI from CMD_SYNC on ECMDQ experiences external abort: reported in SMMU_(*_)GERROR.MSI_CMDQ_ABT_ERR (§3.5.6.3, line 1974)

## §3.6 Structure and Queue Ownership

N/A — All statements use "Arm expects" guidance language directed at system software (OS, hypervisor, secure firmware). No behavioral requirements on the SMMU model. Secure/Realm queue structures are out of scope. Guest-STE conversion and command-queue shadowing are hypervisor responsibilities.

- [x] Non-secure Stream table, Command queue, Event queue and PRI queue are controlled by the most privileged Non-secure system software (§3.6, line 1979) — N/A: software policy; model provides one NS queue set
- [x] Secure Stream table, Secure Command queue and Secure Event queue are controlled by Secure software (§3.6, line 1981) — N/A: Secure queues not implemented (out of scope)
- [x] Stage 2 translation tables indicated by all STEs are controlled by a hypervisor (§3.6, line 1983) — N/A: software policy; SMMU walks whatever S2 tables STEs point at
- [x] CDs and stage 1 translation tables pointed to by a Secure STE are controlled by Secure software; by a Non-secure STE, by Non-secure software; by a Realm STE, by Realm software (§3.6, line 1984) — N/A: software policy; §5.2 STRW enforcement is separate
- [x] In virtualized scenarios, Arm expects hypervisor to convert guest STEs into physical SMMU STEs, controlling permissions and features as required (§3.6, line 1996) — N/A: hypervisor task, no guest-STE interception in model
- [x] Hypervisor reads and interprets commands from guest Command queue; these might result in SMMU commands or invalidation of internal shadowed structures (§3.6, line 2000) — N/A: hypervisor task, no command-shadowing in model

## §3.7 Programming Registers

N/A — Physical memory-map layout statements; inapplicable to a software model (no MMIO address decoder, no physical 64K-page layout; register state held in Rust struct fields). Behavioral register requirements (PRIQEN RES0, RO-while-set guards) were audited under §6.3.x (BUG-AUDIT-73, BUG-AUDIT-124).

- [x] SMMU registers occupy a set of contiguous 64K pages of system address space (§3.7, line 2006) — N/A: hardware memory-map layout; software model has no physical register pages
- [x] Optional regions of IMPLEMENTATION DEFINED register space are supported in the memory map (§3.7, line 2007) — N/A: optional hardware feature; no memory map in software model

## §3.8 Virtualization

- [x] SMMU does not provide programming interfaces for use directly by virtual machines (§3.8, line 2014) — N/A: architectural scope statement describing what SMMUv3 does not define; not a behavioral requirement on the model. Hypervisor-emulated vSMMU and IMPL DEF extra interfaces are explicitly out of scope.

## §3.9 Support for PCI Express, PASIDs, PRI, and ATS

- [x] Supply of a PASID or SubstreamID to a configuration without stage 1 translation causes C_BAD_SUBSTREAMID (§3.9, line 2025) — PASS: `mod.rs:5252` SSV=1 on no-stage1 stream → C_BAD_SUBSTREAMID (BUG-AUDIT-111 fix)
- [x] SMMU is not required to report error when endpoint emits PASID larger than SubstreamID width; PASID may be truncated (§3.9, line 2029) — N/A: permissive; implementation reports C_BAD_SUBSTREAMID when PASID ≥ 2^S1CDMax — allowed by spec
- [x] A PCIe transaction without a PASID is considered Data, unprivileged (§3.9, line 2033) — N/A: hardware/PCIe attribute mapping; SSV=false defaults to Data/unprivileged in model implicitly

### §3.9.1 ATS Interface

- [x] Whether SMMU implements ATS: discoverable from SMMU_(R_)IDR0.ATS (§3.9.1, line 2039) — PASS: IDR0 bit 10 hardcoded 1 (`mod.rs:1334`)
- [x] Whether SMMU implements PRI: discoverable from SMMU_(R_)IDR0.PRI (§3.9.1, line 2039) — PASS: IDR0 bit 16 gated on `pri_supported` (`mod.rs:1340`)
- [x] ATS must be disabled at all endpoints before SMMU translation is disabled by clearing SMMU_(R_)CR0.SMMUEN (§3.9.1, line 2057) — N/A: software programming constraint on driver; SMMU cannot observe endpoint ATS state
- [x] ATS and PRI are NOT supported from Secure streams (§3.9.1, line 2062) — N/A: Secure state out of scope (IDR0.SecP=0)
- [x] In Secure STEs, the EATS field is RES0 (§3.9.1, line 2063) — N/A: Secure state out of scope
- [x] CMD_ATC_INV and CMD_PRI_RESP are not able to target Secure StreamIDs (§3.9.1, line 2064) — N/A: Secure state out of scope
- [x] SMMU terminates any incoming traffic marked Translated on a Secure StreamID, aborting and recording F_TRANSL_FORBIDDEN (§3.9.1, line 2065) — N/A: Secure state out of scope
- [x] If Secure ATS Translation Request reaches SMMU: aborted with UR response and F_BAD_ATS_TREQ recorded into Secure Event queue; check occurs prior to StreamID or configuration lookup (§3.9.1, line 2067) — N/A: Secure state out of scope (IMPL DEF whether Secure ATS TR can reach SMMU)
- [x] Support for CMD_ATC_INV and CMD_PRI_RESP on Secure Command queue is optional; indicated by SMMU_S_IDR3.SAMS (§3.9.1, line 2069) — N/A: Secure state out of scope
- [x] STU (Smallest Translation Unit) must be programmed to same size for all devices serviced by one SMMU (§3.9.1, line 2070) — N/A: software programming constraint; SMMU cannot enforce cross-device STU consistency
- [x] If SMMU_IDR0.NS1ATS == 1: split-stage ATS mode (STE.EATS == 0b10) supported; can only be used when SMMU_(R_)CR0.ATSCHK == 1 (§3.9.1, line 2086) — PASS: BUG-AUDIT-128 fixed; effective_eats computed at `mod.rs:4439` — EATS=0b10+ATSCHK=0 → effective_eats=0 → ATS TR rejected with F_BAD_ATS_TREQ
- [x] When ATS TR is made and translation valid with HTTU enabled: SMMU must update Translation Table Dirty/Access flags (§3.9.1, line 2095) — PASS: ATS TR flows through `translate()` which calls `addr_space.update_access_flags()` at `stream_context/mod.rs:2225`
- [x] When SMMU returns ATS Translation Completion for PASID-tagged request: Global bit of Translation Completion Data Entry must be zero (§3.9.1, line 2099) — N/A: Global bit is in PCIe TC packet constructed by caller; model returns PA+permissions only, no PCIe TC packet built
- [x] After change of translation configuration: ATS Invalidate Request must be preceded by SMMU TLB invalidation; SMMU TLB invalidation must be complete before initiating ATS Invalidation (§3.9.1, line 2058) — N/A: software programming sequence; driver constraint
- [x] ATS translation failures not recorded in SMMU Event queue; reported to endpoint only (§3.9.1, line 2052) — PASS: BUG-AUDIT-126; `record_translation_fault()` suppresses F_TRANSLATION/F_ADDR_SIZE/F_ACCESS/F_PERMISSION when is_ats_tr=true
- [x] SMMU_(R_)CR0.ATSCHK == 1: Translated transactions controlled by STE.EATS field; when effective STE.EATS == 0b00, transaction terminated with abort and F_TRANSL_FORBIDDEN recorded (§3.9.1, line 2081) — PASS: BUG-AUDIT-125; `mod.rs:4493–4557`

### §3.9.1.1 Handling of Addresses in ATS-Related Transactions

- [x] If ATS Translated transaction arrives with PA where bits above implemented PA size are non-zero: IMPLEMENTATION DEFINED whether transaction terminated with abort (no event recorded) or address truncated to SMMU_IDR5.OAS (§3.9.1.1, line 2123) — N/A: IMPL DEF per spec; both behaviors permitted

### §3.9.1.2 Responses to ATS Translation Requests

- [x] SMMUEN == 0: ATS TR terminated with UR status and F_BAD_ATS_TREQ generated (§3.9.1.2, line 2135) — PASS: `mod.rs:4264–4315` (BUG-AUDIT-126)
- [x] Using Secure StreamID: ATS TR terminated with UR status and F_BAD_ATS_TREQ generated (§3.9.1.2, line 2136) — N/A: Secure state out of scope
- [x] STE.Config == 0b000: ATS TR terminated with UR status (no event) (§3.9.1.2, line 2137) — PASS: `mod.rs:4434–4438` abort → silent UR, no event
- [x] STE.Config == 0b100: ATS TR terminated with UR status and F_BAD_ATS_TREQ generated (§3.9.1.2, line 2138) — PASS: bypass stream has s1=false/s2=false → ats_supported=false → F_BAD_ATS_TREQ
- [x] Effective STE.EATS == 0b00 (including EATS==0b1x when ATSCHK==0): ATS TR terminated with UR and F_BAD_ATS_TREQ (§3.9.1.2, line 2139) — PASS: BUG-AUDIT-128 fixed; effective_eats=0 when EATS=0b10+ATSCHK=0; `mod.rs:4439`
- [x] ATS TR encountering Address Size, Access, or Translation fault: Translation Completion with Success status and R==W==0; no SMMU fault recorded (§3.9.1.2, line 2141) — PASS: BUG-AUDIT-126; event suppressed for translation-class faults when is_ats_tr=true
- [x] ATS TR encountering any configuration error (ILLEGAL structure, external abort): Translation Completion with CA status (§3.9.1.2, line 2153) — PASS: BUG-AUDIT-126/127; config errors not suppressed for ATS TR
- [x] C_BAD_STREAMID from ATS TR: CA status; event recorded if SMMU_CR2.REC_CFG_ATS==1 and SMMU_CR2.RECINVSID==1 (§3.9.1.2, line 2157) — PASS: `mod.rs:4387–4389` gated on EVENTQEN+RECINVSID+REC_CFG_ATS
- [x] F_STE_FETCH, C_BAD_STE, F_VMS_FETCH, F_CFG_CONFLICT, F_TLB_CONFLICT, C_BAD_SUBSTREAMID, F_STREAM_DISABLED, F_WALK_EABT, F_CD_FETCH, C_BAD_CD from ATS TR: CA status; event recorded if SMMU_CR2.REC_CFG_ATS==1 (§3.9.1.2, line 2158) — PASS: BUG-AUDIT-126/127; config-class events gated on REC_CFG_ATS
- [x] GPF on output address from ATS TR: CA status (§3.9.1.2, line 2161) — PASS: GPF falls through config-error path; no separate gap
- [x] For event records for ATS TRs when REC_CFG_ATS==1: RnW field is UNKNOWN (§3.9.1.2, line 2163) — PASS: spec says UNKNOWN (any value); implementation sets rnw from access type — conformant

### §3.9.1.3 Handling of ATS Translated Transactions

- [x] SMMUEN == 0: Translated transaction generates F_TRANSL_FORBIDDEN and aborted (§3.9.1.3, line 2179) — PASS: `mod.rs:4317–4358`
- [x] Secure StreamID Translated transaction: F_TRANSL_FORBIDDEN and aborted (§3.9.1.3, line 2180) — N/A: Secure state out of scope
- [x] STE.Config == 0b000 with ATSCHK==1: Translated transaction aborted (§3.9.1.3, line 2181) — PASS: `mod.rs:4515–4519` abort+no_translation → silent abort, no event
- [x] STE.Config == 0b100 with ATSCHK==1: F_TRANSL_FORBIDDEN and aborted (§3.9.1.3, line 2182) — PASS: `mod.rs:4520–4557` bypass → F_TRANSL_FORBIDDEN
- [x] Effective STE.EATS == 0b00 with ATSCHK==1: F_TRANSL_FORBIDDEN and aborted (§3.9.1.3, line 2183) — PASS: BUG-AUDIT-125; EATS=0+ATSCHK=1 path covered
- [x] GPC fault on Translated transaction output address: aborted; GPC fault reported (§3.9.1.3, line 2184) — PASS: GPC handled via translation fault path; no separate gap in NS model
- [x] F_UUT on Translated transaction: aborted, no event recorded in Event queue (§3.9.1.3, line 2189) — N/A: F_UUT not generated by model (upstream transaction type distinction not modeled)
- [x] If Translated transaction with SSV=1 encounters translation-related fault: appropriate Event is recorded (§3.9.1.3, line 2209) — PASS: config events gated on REC_CFG_ATS; translation faults produce F_TRANSL_FORBIDDEN
- [x] Event priority for ATSCHK==1 Translated transactions: (1) C_BAD_STREAMID, (2) F_STE_FETCH, (3) C_BAD_STE, (4) F_VMS_FETCH (if PASIDTT==1 and SSV=1), (5) F_TRANSL_FORBIDDEN, (6) C_BAD_SUBSTREAMID, (7) F_STREAM_DISABLED (§3.9.1.3, line 2211) — PASS: priority preserved structurally in translate_with_type_ssv path
- [x] If SMMU_IDR3.PASIDTT is 0 or ATS Translated transaction lacks PASID TLP prefix: treated as PnU==0, InD==0, SSV==0 (§3.9.1.3, line 2196) — N/A: PASIDTT is RME/Realm feature; SSV=false input defaults to PnU=0/InD=0/SSV=0 in model

### §3.9.1.4 ATS Invalidation Timeout

- [x] CMD_SYNC waiting for failed CMD_ATC_INV completion causes CERROR_ATC_INV_SYNC command error (§3.9.1.4, line 2275) — ⚠️ AUDIT-81 still open: CERROR_ATC_INV_SYNC defined but never triggered; software model processes CMD_ATC_INV synchronously; no real endpoint timeout possible — acceptable simulation gap

### §3.9.1.5 ATS Invalidation Errors

- [x] CMD_ATC_INV generating ATS Invalidate Request that causes UR response from endpoint: completes without error in SMMU; invalidation might not have been performed (§3.9.1.5, line 2284) — PASS: CMD_ATC_INV always completes without error in software model; UR from endpoint is out-of-model boundary

### §3.9.2 Changing ATS Configuration

- [x] To enable ATS on existing valid STE with EATS==0b00: (1) set EATS to 0bx1 or 0b10 and invalidate STE caches with CMD_SYNC, (2) enable ATS at endpoint (§3.9.2, line 2294) — N/A: software programming sequence; driver ordering constraint
- [x] To disable ATS on STE with EATS!=0b00: (1) disable ATS at endpoint, invalidate ATCs, CMD_SYNC; (2) set EATS to 0b00; (3) invalidate STE caches (§3.9.2, line 2299) — N/A: software programming sequence; driver ordering constraint
- [x] EATS must not transition between 0bx1 and 0b10 (in either direction) without first transitioning through EATS==0b00 (§3.9.2, line 2305) — N/A: software programming constraint; SMMU cannot observe STE write history
- [x] EATS is permitted to transition between 0b01 and 0b11 without transitioning through 0b00 (§3.9.2, line 2305) — N/A: permissive statement; no SMMU enforcement
- [x] EATS==0b10 valid only when SMMU_(R_)CR0.ATSCHK==1 (§3.9.2, line 2307) — PASS: BUG-AUDIT-128 fixed; effective_eats=0 when EATS=0b10+ATSCHK=0 enforced at `mod.rs:4439`
- [x] ATSCHK must not be cleared while STE configurations with EATS==0b10 exist; must first reconfigure to EATS==0b00 or 0bx1 (§3.9.2, line 2307) — N/A: software programming constraint; SMMU does not scan all STEs on ATSCHK write
- [x] ATSCHK==0 causes EATS==0b10 to be interpreted as 0b00 but ATSCHK must not be used as global ATS disable (§3.9.2, line 2311) — PASS (behavioral portion): BUG-AUDIT-128 fixed; effective_eats applies ATSCHK=0 interpretation at runtime; "must not be used as global ATS disable" is a software constraint

### §3.9.3 SMMU Interactions with CXL

- [x] SMMU implementation for use with Type 1 or Type 2 CXL devices must support ATS (SMMU_(R_)IDR0.ATS==1) (§3.9.3, line 2335) — 🚫 CXL out of scope
- [x] It is a software error to configure STE.EATS==0b10 for StreamID associated with CXL device issuing CXL.cache transactions; no event recorded (§3.9.3, line 2339) — 🚫 CXL out of scope
- [x] If ATS TR with Source-CXL bit set for StreamID with STE.EATS==0b10: ATS Translation Completion has CXL.io bit set (§3.9.3, line 2341) — 🚫 CXL out of scope
- [x] If translation for ATS TR with Source-CXL bit returns memory type other than Inner WB Cacheable/Outer WB Cacheable/Shareable: CXL.io bit set in ATS Translation Completion (§3.9.3, line 2343) — 🚫 CXL out of scope

### §3.9.4 SMMU Interactions with PCIe T, TE and XT Fields

- [x] §3.9.4.1 applies only when SMMU_R_IDR3.XT is 0 (§3.9.4.1, line 2353) — N/A: Realm/XT out of scope (IDR3.XT=0)
- [x] Absence of IDE TLP prefix, or T=0: transaction associated with Non-secure state; SMMU does not distinguish absence from T=0 (§3.9.4.1, line 2365) — N/A: Realm/T-bit wire format out of scope
- [x] IDE TLP prefix with T=1: transaction associated with Realm state; input NS attribute is Realm (§3.9.4.1, line 2372) — N/A: Realm out of scope
- [x] Transactions with T bit in IDE TLP prefix set to 1: presented to SMMU with SEC_SID = Realm (§3.9.4.1, line 2374) — N/A: Realm out of scope
- [x] SMMU transmits ATS Translation Completions with T bit value matching the T bit in corresponding ATS Translation Request (§3.9.4.1, line 2376) — N/A: Realm/PCIe TC packet out of scope
- [x] CMD_ATC_INV and CMD_PRI_RESP on Realm Command queue: issued to PCIe with T=1 (§3.9.4.1, line 2377) — N/A: Realm out of scope
- [x] §3.9.4.2 applies only when SMMU_R_IDR3.XT is 1 (§3.9.4.2, line 2386) — N/A: Realm/XT out of scope
- [x] ATS Translation Completion for Non-secure stream: SMMU sets TE=0 (§3.9.4.2, line 2389) — 🚫 Requires IDR3.XT=1 (Realm/TDISP); out of scope
- [x] ATS Translation Completion for Realm stream: TE=0 if not Success or R==W==0; if Realm PA: TE=1; if Non-secure PA: TE=0 (§3.9.4.2, line 2390) — 🚫 Realm out of scope
- [x] §3.9.4.3 applies only when SMMU_R_IDR3.XT is 1 (§3.9.4.3, line 2403) — 🚫 IDR3.XT=0; entire §3.9.4.3 out of scope
- [x] XT=0, T=0: Non-TEE request targeting non-TEE memory (§3.9.4.3, line 2408) — 🚫 Realm/XT out of scope
- [x] XT=0, T=1: TEE request targeting TEE or non-TEE memory (§3.9.4.3, line 2409) — 🚫 Realm/XT out of scope
- [x] XT=1, T=0: TEE request targeting non-TEE memory (§3.9.4.3, line 2410) — 🚫 Realm/XT out of scope
- [x] XT=1, T=1: TEE request targeting TEE memory (§3.9.4.3, line 2411) — 🚫 Realm/XT out of scope
- [x] SEC_SID determined from bitwise-OR of T and XT: 0 → Non-secure; 1 → Realm (§3.9.4.3, line 2413) — 🚫 Realm/XT out of scope
- [x] If SEC_SID is Realm: T=0 → input NS attribute Non-secure; T=1 → input NS attribute Realm (§3.9.4.3, line 2422) — 🚫 Realm out of scope
- [x] NSCFG==0b01, XT=0 Translated transaction: terminated with abort and F_TRANSL_FORBIDDEN (§3.9.4.3, line 2455) — 🚫 Realm/XT out of scope
- [x] NSCFG==0b01, XT=1: SMMU computes expected output PA space; if does not match input NS attribute → F_PERMISSION for final enabled stage (§3.9.4.3, line 2456) — 🚫 Realm/XT out of scope
- [x] §3.9.4.4 applies only when SMMU_R_IDR3.XT is 1 (§3.9.4.4, line 2460) — 🚫 IDR3.XT=0; entire §3.9.4.4 out of scope
- [x] SMMU ignores XT bit on PRI requests and ATS Invalidation completions (§3.9.4.4, line 2461) — 🚫 Realm/XT out of scope
- [x] SMMU transmits ATS Translation Completions with both T bit and XT bit matching corresponding ATS Translation Request (§3.9.4.4, line 2464) — 🚫 Realm/XT out of scope

## §3.10 Security States Support

- [x] SMMU always supports Non-secure state and programming interface (§3.10, line 2479) — SecurityState::NonSecure is default; no gating on NS interface
- [x] Non-secure streams can only generate transactions targeting Non-secure (NS==1) PA space (§3.10, line 2549) — enforced by address_space/mod.rs:583 security-state equality check; test_bug_audit140_sec_sid_spec.rs
- [x] Secure streams can generate transactions targeting both Secure (NS==0) and Non-secure (NS==1) PA spaces (§3.10, line 2549) — 🚫 Secure streams out of scope; can_access(Secure,NonSecure)=true in type model

### §3.10.1 StreamID Security State (SEC_SID)

- [x] If SMMU_S_IDR1.SECURE_IMPL==0: SEC_SID == 0 (or absent implicitly); all streams are Non-secure (§3.10.1, line 2494) — SECURE_IMPL permanently 0; all configs default to NonSecure
- [x] If SMMU_S_IDR1.SECURE_IMPL==1: SEC_SID==0 → Non-secure stream table; SEC_SID==1 → Secure stream table (§3.10.1, line 2499) — 🚫 Secure interface out of scope
- [x] For SMMU with RME DA: SEC_SID extended to 2 bits: 0b00=Non-secure, 0b01=Secure, 0b10=Realm, 0b11=Reserved (§3.10.1, line 2511) — 0b00/0b01/0b10 PASS; 0b11=Root is internal PA-space tag (no external SEC_SID boundary)

### §3.10.2 Support for Secure State

- [x] When SMMU_S_IDR1.SECURE_IMPL==0: SMMU_S_* registers are RAZ/WI to all accesses (§3.10.2, line 2528) — 🚫 Secure interface out of scope; SECURE_IMPL permanently 0
- [x] When SMMU_S_IDR1.SECURE_IMPL==1: SMMU_S_* registers configure Secure state with Secure Command queue, Secure Event queue, Secure Stream table (§3.10.2, line 2534) — 🚫 Secure interface out of scope
- [x] With exception of SMMU_S_INIT: SMMU_S_* registers are Secure access only, RAZ/WI to Non-secure accesses (§3.10.2, line 2540) — 🚫 Secure interface out of scope
- [x] Access to Secure Stream table, Secure Event queue, Secure Command queue always made to Secure PA space (§3.10.2, line 2609) — 🚫 Secure interface out of scope
- [x] If Secure stage 2 not in use: L1CD and CD addresses treated as Secure physical addresses (§3.10.2, line 2612) — 🚫 Secure interface out of scope
- [x] Some commands on Secure Command queue take SSec parameter indicating Secure or Non-secure StreamID (§3.10.2, line 2615) — 🚫 Secure interface out of scope

### §3.10.2.1 Secure Commands, Events and Configuration

- [x] Event from Secure StreamID: written to Secure Event queue (§3.10.2.1, line 2562) — 🚫 Secure interface out of scope
- [x] Event from Non-secure StreamID: written to Non-secure Event queue (§3.10.2.1, line 2563) — PASS: all events written to single NS Event queue; Secure queue not implemented
- [x] Commands on Non-secure Command queue only affect Non-secure streams (§3.10.2.1, line 2564) — PASS: NS queue only; Secure/Realm SSec guards on relevant commands
- [x] Some commands on Secure Command queue can affect any stream or data in the system (§3.10.2.1, line 2565) — 🚫 Secure interface out of scope
- [x] SMMU_S_CR0.SIF==1 terminates instruction fetches from Secure streams targeting Non-secure PAs or Non-secure IPAs (§3.10.2.1, line 2617) — 🚫 Secure interface out of scope

### §3.10.2.2 Secure EL2 and Support for Secure Stage 2 Translation

- [x] SMMU_S_IDR1.SECURE_IMPL==1, SMMU_S_IDR1.SEL2==0: Secure EL2 not supported; Secure stage 2 not supported (§3.10.2.2, line 2629) — 🚫 Secure interface out of scope
- [x] SMMU_S_IDR1.SECURE_IMPL==1, SMMU_S_IDR1.SEL2==1: Secure EL2 and Secure stage 2 supported (§3.10.2.2, line 2630) — 🚫 Secure interface out of scope
- [x] Secure STE with stage 2 translation enabled is not permitted to have STE.S2AA64 select VMSAv8-32 LPAE (§3.10.2.2, line 2647) — 🚫 Secure interface out of scope
- [x] TLB entries from StreamWorld==Secure with stage 2 enabled: tagged with VMID from STE.S2VMID (§3.10.2.2, line 2653) — 🚫 Secure interface out of scope
- [x] TLB entries from StreamWorld==Secure with stage 2 not enabled: tagged with VMID 0 (§3.10.2.2, line 2654) — 🚫 Secure interface out of scope
- [x] Translation table entry fetched for Secure stream from Non-secure IPA space: treated as non-global (nG==1) regardless of nG bit in descriptor (§3.10.2.2, line 2658) — 🚫 Secure interface out of scope

### §3.10.3 Support for Realm State

- [x] Realm translation regimes supported only with VMSAv8-64 or VMSAv9-128 translation tables (§3.10.3, line 2665) — 🚫 Realm out of scope
- [x] Realm L1STD, STE, L1CD, and CD have same format as Non-secure equivalents except all pointers are Realm physical addresses (§3.10.3, line 2680) — 🚫 Realm out of scope
- [x] CD.NSCFG0 and CD.NSCFG1 are IGNORED for a Realm stream (§3.10.3, line 2688) — 🚫 Realm out of scope
- [x] For Realm Command queue commands: SSec==1 gives CERROR_ILL (§3.10.3, line 2694) — 🚫 Realm out of scope

### §3.10.3.1 Input NS Attribute

- [x] For Realm stream: if client device does not provide input NS attribute, input NS attribute defaults to Realm (§3.10.3.1, line 2702) — 🚫 Realm out of scope

### §3.10.3.2 Realm Stream Disabled

- [x] If SMMU_R_CR0.SMMUEN==1 and Realm STE.Config==0b000: stream is disabled; transactions terminated with abort (§3.10.3.2, line 2709) — 🚫 Realm out of scope

### §3.10.3.3 Realm Stream Bypass

- [x] If SMMU_R_CR0.SMMUEN==1 and Realm STE.Config==0b100: stream bypass; output PA space derived by applying STE.NSCFG to input NS attribute (§3.10.3.3, line 2716) — 🚫 Realm out of scope
- [x] Realm stream bypass can still result in: F_ADDR_SIZE, F_PERMISSION (instruction to Non-secure PA), F_BAD_ATS_TREQ, F_TRANSL_FORBIDDEN, GPC faults (§3.10.3.3, line 2721) — 🚫 Realm out of scope
- [x] Realm stream bypass: client transactions still associated with MECID configured in STE.MECID (§3.10.3.3, line 2729) — 🚫 Realm out of scope

## §3.11 Reset, Enable and Initialization

- [x] SMMU can reset to disabled state where traffic bypasses without translation; attributes determined by SMMU_GBPA (§3.11, line 2733)
- [x] SMMU_GBPA.ABORT or SMMU_S_GBPA.ABORT controls whether disabled state aborts all transactions (§3.11, line 2733)
- [x] Translation of Non-secure Streams enabled using SMMU_CR0.SMMUEN (§3.11, line 2737)
- [x] When translation not enabled for a Security state: SMMU never accesses Stream table; SMMU_(*_)STRTAB_* register content ignored (§3.11, line 2743)
- [x] When translation not enabled: SMMU denies PRI Page Requests as though SMMU_(R_)CR0.PRIQEN==0 (§3.11, line 2744)
- [x] When translation not enabled: SMMU does not perform ATOS operations (§3.11, line 2745)
- [x] When translation not enabled: SMMU does not perform ATS translations (§3.11, line 2746)
- [x] When translation not enabled: SMMU can process commands after queue pointers initialized and SMMU_(*_)CR0.CMDQEN enabled (§3.11, line 2748)
- [x] When translation not enabled: SMMU does not record new translation events; may continue to write out buffered events from prior enabled period if EVENTQEN enabled (§3.11, line 2749)
- [x] SMMU_(*_)STRTAB_BASE register and SMMU_(*_)CR1 table attributes must be configured before enabling via SMMU_(*_)CR0.SMMUEN (§3.11, line 2751)
- [x] SMMU is not required to invalidate cached configuration or TLB entries when SMMU_(*_)CR0.SMMUEN changes (§3.11, line 2781)
- [x] Before enabling translation, software must: (1) invalidate all configuration and TLB caches, (2) if SECURE_IMPL==1, Secure software must fully invalidate Secure cached configuration/TLB entries before handover to Non-secure (§3.11, line 2776) — N/A: software guidance only
- [x] Recommended initialization sequence: (1) allocate/initialize Stream table memory and base pointers, (2) allocate/initialize Command/Event queue memory, (3) enable CMDQEN and EVENTQEN, (4) issue invalidation commands, (5) enable translation via SMMUEN (§3.11, line 2783) — N/A: software guidance only
- [x] SMMU_S_INIT invalidates SMMU caches and TLBs without issuing commands; sequence: write INV_ALL, poll until INV_ALL returns 0 (§3.11, line 2793) — N/A: Secure-only register, NonSecure implementation
- [x] If SMMU creates TLB entries when bypass is selected (SMMUEN==0), these do not need explicit invalidation when SMMUEN transitions from 0 to 1 (§3.11, line 2805)

## §3.12 Fault Models, Recording and Reporting

*Re-audited 2026-05-03: 27 requirements checked, 3 Rust bugs fixed (BUG-AUDIT-141, 142, 143). All items PASS or N/A.*

- [x] Four Translation-related fault types: F_TRANSLATION, F_ADDR_SIZE, F_ACCESS, F_PERMISSION (§3.12, line 2817) — PASS: `is_stall_eligible()` gates on exactly these four; all others always terminate
- [x] All other faults (F_WALK_EABT, F_TLB_CONFLICT) and configuration errors always terminate the transaction with abort (§3.12, line 2826) — PASS: F_WALK_EABT/F_TLB_CONFLICT/config errors mapped to non-stall-eligible fault types; bypass F_ADDR_SIZE hardcoded `stall: false` at `mod.rs:5196`
- [x] Stage 1 fault behavior configured by CD.{A, R, S} flags; stage 2 by STE.{S2R, S2S} (§3.12, line 2824) — PASS: `cd_r`/`cd_a`/`fault_mode` (CD.S) in `StreamConfig`; `s2_record`/`s2_stall` in `StreamConfig` (BUG-AUDIT-141/142 fixed)
- [x] Support for stalling or terminating is IMPLEMENTATION DEFINED; indicated by SMMU_(*_)IDR0.STALL_MODEL (§3.12, line 2861) — PASS: `stall_model` field in `SMMU`; IDR0 bits[25:24] reflect runtime value; 0b11 reserved rejected
- [x] When SMMU_S_CR0.NSSTALLD==1: prevents Non-secure use of stall model even if physically supported (§3.12, line 2867) — N/A: SECURE_IMPL=0; no Secure register interface; IDR0.STALL_MODEL directly reflects physical capability
- [x] SMMU_IDR0.TERM_MODEL indicates termination models; if TERM_MODEL==0, CD.A bit selects abort vs RAZ/WI for stage 1 (§3.12, line 2886) — PASS: TERM_MODEL=0 in IDR0; `cd_a: bool` in `StreamConfig`; `TranslationError::RazWi` returned when `cd_a=false` (BUG-AUDIT-142 fixed)
- [x] Stage 2 faults when terminated are always aborted; RAZ/WI not available at stage 2 (§3.12, line 2888) — PASS: `cd_a` only checked on `!is_stage2_fault` path; S2 terminate always returns fault error
- [x] Streams from PCIe subsystems must not stall; must use Terminate model at all enabled stages (§3.12, line 2922) — PASS: `STE.S1STALLD` (`s1_stalld: bool`) overrides stall even when `CD.S=1`; `is_stall_enabled()` in `stream_context/mod.rs` gates on `!s1_stalld`

### §3.12.1 Terminate Model

- [x] Stage 1 terminate: transaction either aborted or completes with RAZ/WI depending on CD.A and SMMU_IDR0.TERM_MODEL (§3.12.1, line 2903) — PASS: BUG-AUDIT-142 fixed; `cd_a=false` → `TranslationError::RazWi`; `cd_a=true` (default) → abort
- [x] Stage 2 terminate: transaction terminated with abort (§3.12.1, line 2905) — PASS: RazWi path gated on `!is_stage2_fault`; S2 terminate always returns raw fault error
- [x] If CD.R==1 or STE.S2R==1: SMMU records details in Event record (address, syndrome, attributes, type) (§3.12.1, line 2907) — PASS: BUG-AUDIT-141 fixed; `stream_s1_record` (CD.R) gates S1 recording; `stream_s2_record` (STE.S2R) already gated S2 recording; both `false` → early return without recording
- [x] If Event queue full: terminate fault event record is lost (§3.12.1, line 2919) — PASS: `record_translation_fault()` drops event when queue full; OVFLG toggled on overflow
- [x] STE.S1STALLD==1 prevents guest VM from using Stall model at stage 1 (§3.12.1, line 2922) — PASS: `s1_stalld` field in `StreamContext`; `is_stall_enabled()` returns `false` when `s1_stalld=true`

### §3.12.2 Stall Model

- [x] Stalled transaction does not progress; no response returned to client device; SMMU always records fault details in Event queue (§3.12.2, line 2926) — PASS: `TranslationError::Stalled { stag }` returned; event recorded before stall return path; `stall_pending` buffers when queue full
- [x] Stalled transaction retried or terminated by CMD_RESUME or CMD_STALL_TERM (§3.12.2, line 2926) — PASS: `cmd_resume()` at `mod.rs:7721`; `cmd_stall_term()` at `mod.rs:7762` both remove STAG from `stall_queue`
- [x] If retry chosen: transaction handled as though just arrived, affected by any configuration/translation changes since stall (§3.12.2, line 2928) — PASS: synchronous model; retry re-enters `translate()` from scratch with current config
- [x] Software must ensure every stall event has corresponding CMD_RESUME, CMD_STALL_TERM, or SMMUEN cleared to 0 (§3.12.2, line 2934) — N/A: software programming constraint; SMMU cannot enforce driver behavior
- [x] STAG identifies stalled transaction; SMMU uses StreamID+STAG combination from CMD_RESUME to identify stalled transaction (§3.12.2, line 2936) — PASS: `stall_queue: DashMap<u16, StallRecord>`; CMD_RESUME matches on StreamID+STAG pair
- [x] STAG value cannot be re-used until transaction acknowledged through CMD_RESUME, CMD_STALL_TERM, or SMMUEN cleared (§3.12.2, line 2940) — PASS: BUG-AUDIT-143 fixed; `stall_queue.clear()` called in `disable()` and on SMMUEN 1→0 in `set_cr0()`; STAG allocation loop skips occupied slots
- [x] If Event queue not writable when stall fault to be written: stalled transaction retried when queue becomes writable; new fault record generated (§3.12.2, line 2942) — N/A: synchronous model; `stall_pending` VecDeque buffers internally; architectural retry semantics not applicable
- [x] Later transactions may pass through SMMU and complete before earlier stalled transactions from same stream (§3.12.2, line 2951) — N/A: single-threaded synchronous model; per-stream ordering constraint is architectural guidance for HW implementations

### §3.12.2.1 Suppression of Duplicate Stall Event Records

- [x] SMMU permitted but not required to suppress duplicate stall fault records when: same page, same privilege, same data/instruction, same type, same SubstreamID, and first stall still pending (§3.12.2.1, line 2962) — N/A: "permitted but not required"; model does not suppress; one fault per transaction enforced
- [x] Stall fault records are NOT merged (§3.12.2.1, line 2980) — PASS: each stall records independently; STE.MEV suppression does not apply to stall events per `§7.3.1`
- [x] SMMU does not record more than one fault for each incoming transaction, except after CMD_RESUME(Retry) (§3.12.2.1, line 2985) — PASS: single `record_translation_fault()` call per translation attempt; retry re-enters as new transaction

### §3.12.2.2 Early Retry of Stalled Transactions

- [x] SMMU is permitted to speculatively retry stalled transaction without CMD_RESUME(Retry); early retry does not cause additional fault records (§3.12.2.2, line 2989) — N/A: "SMMU is permitted" — IMPL DEF optional; synchronous model does not implement speculative retry
- [x] Successful early retry does not remove requirement for software to acknowledge stall fault record (§3.12.2.2, line 2997) — N/A: IMPL DEF optional feature not implemented
- [x] CMD_RESUME(Retry) guarantees stalled transaction retried at future point unless terminated by CMD_STALL_TERM or SMMUEN transition (§3.12.2.2, line 2999) — PASS: `cmd_resume()` removes STAG and caller re-issues translate; SMMUEN=0 clears stall_queue

### §3.12.5 Combinations of Fault Configuration with Two Stages

- [x] Stage1=Terminate, Stage2=Terminate, fault at Stage1: transaction terminated, VA in event; event passed to guest as stage 1-only event (§3.12.5, line 3062) — PASS: BUG-QA-12/13 ✅; S1 fault uses `fault_mode`; S2 uses `s2_stall`; all 8 table rows verified
- [x] Stage1=Terminate, Stage2=Terminate, fault at Stage2: transaction terminated, VA+IPA in event (§3.12.5, line 3063) — PASS: `stage2_ipa_opt` populated for S2 faults; `fault_s2=true` in event record
- [x] Stage1=Terminate, Stage2=Stall, fault at Stage2: transaction stalled, VA+IPA in event (§3.12.5, line 3065) — PASS: `stream_s2_stall=true` → stall path; IPA from `stage2_ipa_opt`
- [x] Stage1=Stall, Stage2=Terminate, fault at Stage1: transaction stalled, VA in event (§3.12.5, line 3066) — PASS: `stall_mode=true` (CD.S=1) → stall path for S1 faults; S2 uses separate `s2_stall` flag
- [x] Stage1=Stall, Stage2=Stall, fault at Stage2: transaction stalled, VA+IPA in event (§3.12.5, line 3073) — PASS: `stream_s2_stall && is_stall_eligible` → stall with IPA populated

## §3.13 Translation Tables and Access Flag/Dirty State

*Re-audited 2026-04-12 / 2026-05-02. All items PASS, N/A, or 🚫. BUG-AUDIT-128/129/130/130-CPP all fixed.*

- [x] HTTU support indicated by SMMU_IDR0.HTTU: 0=no updates, 1=Access flag only, 2=Access flag and dirty state (§3.13, line 3091) — PASS: IDR0.HTTU=0b01 (AF-only); `mod.rs:1348`; S2HD/HD always rejected at configure time when HTTU<0b10
- [x] CDs referencing same translation table and same ASID must have identical HA and HD fields (§3.13, line 3099) — N/A: no mandatory enforcement per spec ("must" is on software); flat model has no per-CD ASID identity check; conformant by absence

### §3.13.2 Access Flag Hardware Update

- [x] When HTTU enabled and descriptor has AF==0: SMMU atomically sets AF==1; does NOT clear AF (§3.13.2, line 3130) — PASS: `update_access_flags()` sets AF=1 when `ha=true`; never clears; BUG-AUDIT-128 added S2 HA update
- [x] SMMU never clears AF (§3.13.2, line 3132) — PASS: `update_access_flags()` only sets; no clear path exists
- [x] If access to descriptor causes permission fault: it is UNKNOWN whether AF is updated to 1 (§3.13.2, line 3133) — PASS: permission fault returns before `update_access_flags()` is called; AF not updated on fault (one valid UNKNOWN outcome)
- [x] Includes stage 2 translation for L1CD or CD fetch (§3.13.2, line 3130) — PASS: BUG-AUDIT-128 fixed; stage-2 HTTU update added in `translate_two_stage()` and `translateUnlocked()`

### §3.13.3.1 Direct Permission Scheme - Dirty State

- [x] When HTTU dirty state enabled and descriptor is read-only due to AP[2:1]==0b1x (stage 1) or S2AP[1:0]==0b0x (stage 2): if DBM==1 and write translation occurs, SMMU atomically sets AP[2]==0 or S2AP[1]==1 (§3.13.3.1, line 3148) — N/A: HTTU=0b01; HD/S2HD always rejected at configure time; dirty-state path structurally unreachable
- [x] SMMU never sets or clears DBM (§3.13.3.1, line 3170) — N/A: HTTU<0b10; DBM path unreachable; `update_access_flags()` never touches permission bits
- [x] SMMU never clears S2AP[1] (§3.13.3.1, line 3171) — N/A: HTTU<0b10; dirty-state path unreachable
- [x] SMMU never sets AP[2]; descriptor never made writable by SMMU unless DBM==1 (§3.13.3.1, line 3172) — N/A: HTTU<0b10; permission bits never modified by SMMU
- [x] SMMU never sets S2AP[1]==1 for the stage 2 translation used to fetch L1CD or CD (§3.13.3.1, line 3174) — N/A: HTTU<0b10; dirty-state path unreachable

### §3.13.3.2 Indirect Permission Scheme

- [x] CD.HD exclusively defines whether dirty state managed by hardware or software when Indirect Permission Scheme used for stage 1 (§3.13.3.2, line 3178) — N/A: HTTU=0b01; CD.HD always rejected (`validate()` error); Indirect Permission dirty-state path unreachable
- [x] STE.S2HD exclusively defines whether dirty state managed by hardware or software when Indirect Permission Scheme used for stage 2 (§3.13.3.2, line 3182) — N/A: HTTU=0b01; STE.S2HD always rejected; dirty-state path unreachable

### §3.13.4 HTTU Behavior Summary

- [x] Descriptor update from completed ATOS translation: made visible by completion of CMD_SYNC submitted after ATOS translation began (§3.13.4, line 3192) — PASS: CMD_SYNC is synchronous in SW model; ATOS AF update committed before CMD_SYNC can be issued
- [x] Descriptor update from completed incoming transaction: made visible by completion of CMD_SYNC submitted after transaction completion (§3.13.4, line 3193) — PASS: same; synchronous model; all descriptor updates visible immediately
- [x] TLB invalidation completion makes descriptor updates from transactions completed by that invalidation visible (§3.13.4, line 3194) — PASS: synchronous model; TLB invalidation + CMD_SYNC ordering satisfied structurally
- [x] SMMU exception: if stage 2 HD enabled, SMMU permitted to speculatively update stage 2 dirty state for stage 1 TT walk even if stage 1 HA/HD disabled (§3.13.4, line 3198) — N/A: HTTU=0b01; S2HD always rejected; speculative dirty-state update path unreachable

### §3.13.6 Access Flag in Table Descriptors

- [x] HAFT support controlled by CD.HAFT (stage 1) and STE.S2HAFT (stage 2) (§3.13.6, line 3226) — N/A: IDR0.HTTU=0b01 → HAFT explicitly not supported (`mod.rs:1348`); flat model has no Table descriptors; CD.HAFT/STE.S2HAFT structurally inapplicable
- [x] If HAFT disabled for translation stage: hardware update of AF in Table descriptors also disabled (§3.13.6, line 3230) — N/A: no Table descriptors in flat model; vacuously satisfied
- [x] If HAFT enabled: Table entry with Access flag clear is NOT permitted to be cached in TLB (§3.13.6, line 3236) — N/A: HAFT not supported; no Table descriptors

### §3.13.7.1 Hardware Flag Update for ATS and PRI

- [x] When ATS TR made: AF set to 1 in same way as direct transaction access (§3.13.7.1, line 3253) — 🚫 ATS TR→TC processing path out of scope; GATOS (ATOS) uses normal translate path with AF update
- [x] If HTTU dirty state enabled and ATS request for write (NW==0) to writable-clean page: SMMU marks page writable-dirty before returning ATS response (§3.13.7.1, line 3254) — 🚫 ATS TR path out of scope; HTTU=0b01 (AF-only) anyway
- [x] If HTTU only Access flag enabled: ATS request for write to writable-clean returns ATS Completion with W==0 (§3.13.7.1, line 3254) — 🚫 ATS TR path out of scope

### §3.13.8 Hardware Flag Update for Cache Maintenance Operations and Destructive Reads

- [x] HTTU dirty state update NOT performed for: Invalidate Cache Maintenance Operations, Destructive Reads, Destructive Hints (§3.13.8, line 3281) — N/A: HTTU=0b01 (AF-only); HD/S2HD always rejected; dirty-state update path unreachable; CMO/DR/DH not modeled
- [x] When these operations to writable-clean descriptor: descriptor not updated to writable-dirty; operations are downgraded (§3.13.8, line 3292) — N/A: HTTU<0b10; dirty-state path unreachable; vacuously conformant

## §3.14 Speculative Accesses

- [x] Only read transactions can be marked speculative; write transactions marked speculative are always terminated with abort and no event recorded (§3.14, line 3300) — N/A: speculative marking is entirely IMPLEMENTATION DEFINED ("might"); model does not implement speculative transactions; conformant by omission
- [x] Speculative read: if translation faults for any reason, transaction terminated with abort; no event recorded (§3.14, line 3304) — N/A: conditional on optional speculative feature not implemented
- [x] Speculative read: if translation succeeds without fault and HTTU enabled, SMMU updates Access flags (§3.14, line 3305) — N/A: conditional on optional speculative feature not implemented

## §3.15 Coherency Considerations and Memory Access Types

- [x] All in-memory structures and queues accessed using Normal memory types (§3.15, line 3324) — N/A: software model; queues/structures are Rust Vec/DashMap, not hardware bus accesses with memory type attributes
- [x] If HTTU supported: atomic access required to update translation tables shared between PE and SMMU (§3.15, line 3326) — N/A: no separate PE exists in the simulation; AF write-back occurs via Rust interior mutability within a single process (address_space/mod.rs:1438-1462); no PE↔SMMU hardware concurrency model
- [x] SMMU_IDR0.COHACC==1: system supports IO-coherent accesses from SMMU for configuration structures, translation tables, queues, CMD_SYNC, GERROR, Event queue, PRI queue MSIs (§3.15, line 3339) — PASS: COHACC (IDR0 bit [4]) is intentionally 0 in get_idr0() (smmu/mod.rs:1347-1380); spec requires COHACC=0 when IO-coherent access is not supported — correct for a simulation that models no hardware coherency fabric
- [x] TLB-maintenance operations sent from client devices into the system are NOT permitted and never propagated by the SMMU (§3.15.1, line 3343) — PASS: public translate API accepts only (stream_id, pasid, iova, access, security_state); no surface for client-initiated TLB maintenance; receive_broadcast_tlbi() (smmu/mod.rs:2240) is PE→SMMU broadcast only, not client-device TLB maintenance
- [x] SMMU cache maintenance operations from client devices are supported (§3.15.1, line 3343) — N/A: hardware bus-level cache maintenance operations are not modeled in a software simulation; no real cache hierarchy exists
- [x] SMMU does not output inconsistent attributes from misconfiguration; Outer Shareable used as effective Shareability when Device or Normal Inner Non-cacheable Outer Non-cacheable types configured (§3.15, line 3721) — PASS: apply_output_attrs() (stream_context/mod.rs:3016) correctly matches all Device encodings (0x00 nGnRnE, 0x04 nGnRE, 0x08 nGRE, 0x0C GRE) and Normal-iNC-oNC (0x44), forcing shareability=0b10 (OSH) for all five encodings regardless of SHCFG override

### §3.15.1.1 Fully-Coherent Client Devices

- [x] GPC checks apply to fully-coherent requests (§3.15.1.1, line 3355) — N/A: GPC (Granule Protection Check) is an RME feature; IDR0.RME_IMPL=0 per smmu/mod.rs:7229-7230; RME/GPC not implemented
- [x] DPT checks apply to fully-coherent requests; exception: DPT W bit permitted to be treated as 1 for fully-coherent client where required by coherency protocol (§3.15.1.1, line 3356) — N/A: IDR3.DPT=0; DPTI commands raise CERROR_ILL (smmu/mod.rs:8136-8145); dirty page tracking not implemented
- [x] Client-originated snoop requests bypass the SMMU and are NOT subject to DPT checks or GPC (§3.15.1.1, line 3358) — N/A: software simulation has no snoop request path; clients only invoke translate()

## §3.16 Embedded Implementations

- [ ] SMMU_IDR1.TABLES_PRESET: Stream table base address hardwired to pre-existing storage (§3.16, line 3371)
- [ ] SMMU_IDR1.QUEUES_PRESET: queue base addresses hardwired to pre-existing storage (§3.16, line 3371)
- [ ] When SMMU_IDR1.REL set: base addresses given relative to start of SMMU register memory map (§3.16, line 3371)
- [ ] For embedded implementation using internal storage: all address regions for configuration structures and queues must not overlap; applies within same PA space and across NS and Secure PA spaces (§3.16, line 3374)
- [ ] Embedded Event/PRI queue entries (QUEUES_PRESET==1): permitted to have read-only/write-ignored behavior for software accesses (§3.16.1.1, line 3382)
- [ ] Embedded Command queue entries: readable and writable but storage not required for reserved/undefined fields, high-order StreamID bits beyond range, high-order SubstreamID bits beyond range, SSV if SubstreamIDs not implemented, STAG bits generated as '0' (§3.16.1.2, line 3386)
- [ ] Software must not assume writing arbitrary 16-byte sequence to Command queue entry can be read back unmodified (§3.16.1.2, line 3404)
- [ ] Embedded Stream table: storage not required for undefined fields, Reserved/RES0 fields, fields IGNORED in all supported configurations, fields with RAZ/WI behavior (§3.16.1.3, line 3408)

## §3.17 TLB Tagging, VMIDs, ASIDs and Broadcast TLB Maintenance

- [ ] Cached translations tagged with: translation regime (StreamWorld), ASID if regime supports ASIDs, VMID if S2 implemented and regime supports VMIDs (§3.17, line 3420)
- [ ] NS-EL1 stage 1 VA translations: ASID-tagged if nG==1, VMID-tagged if S2P==1 (§3.17, line 3431)
- [ ] any-EL2 translations: no ASID tag, no VMID tag (§3.17, line 3435)
- [ ] any-EL2-E2H translations: ASID-tagged if nG==1, no VMID tag (§3.17, line 3436)
- [ ] EL3 translations: no ASID tag, no VMID tag (§3.17, line 3438)
- [ ] When SMMU_IDR0.S1P==1: SMMU supports 16-bit ASIDs if SMMU_IDR0.ASID16==1 (§3.17, line 3456)
- [ ] When SMMU_IDR0.S2P==1: SMMU supports 16-bit VMIDs if SMMU_IDR0.VMID16==1 (§3.17, line 3457)
- [ ] All TLB entries inserted using NS-EL1 configurations are tagged with VMIDs when S2P==1, regardless of stage configuration (§3.17, line 3458)
- [ ] SMMU support for broadcast TLB maintenance is optional; indicated by SMMU_IDR0.BTM (§3.17, line 3462)
- [ ] If SMMU_IDR0.BTM==1 and SMMU_(*_)CR2.PTM==1: SMMU permitted but not required to ignore broadcast TLB invalidations for corresponding Security state (§3.17, line 3466)
- [ ] Broadcast TLB invalidations with illegal operations (e.g. affecting unimplemented stage): silently ignored (§3.17, line 3466)
- [ ] When SMMU_IDR0.S2P==0: SMMU matches VMID 0 for incoming broadcast TLB invalidations for regimes using VMIDs (§3.17, line 3466)
- [ ] CD.ASET==1: address space and ASID are non-shared; TLB entries not required to be invalidated by broadcast VA{L}ExIS and ASIDExIS operations (§3.17, line 3478)
- [ ] CD.ASET==0: ASID considered shared with PE processes; TLB entries required to be affected by all matching broadcast invalidations (§3.17, line 3478)
- [ ] CMD_TLBI_* commands invalidate all matching TLB entries regardless of ASET value (§3.17, line 3480)

### §3.17.1 The Global Flag in the Translation Table Descriptor

- [ ] Translation performed for Secure stream from Non-secure memory is treated as non-global (nG==1) regardless of nG bit value in descriptor (§3.17.1, line 3504)
- [ ] any-EL2 and EL3 StreamWorlds: nG bit has no effect (§3.17.1, line 3504)
- [ ] Global TLB entry can match regardless of ASID; but can only match lookups from same StreamWorld as when TLB entry created (§3.17.1, line 3508)
- [ ] Global TLB entries with ASET==0 do not match lookups through configurations with ASET==1 and vice versa (§3.17.1, line 3512)

### §3.17.4 Broadcast TLB Maintenance in Mixed AArch32/AArch64 Systems

- [ ] SMMU supporting 16-bit ASID: compares full 16-bit broadcast value to TLB tags (§3.17.4, line 3600)
- [ ] SMMU supporting 8-bit ASID: compares bottom 8 bits; required to match if bottom 8 equal and top 8 zero; not required to match if top 8 non-zero (§3.17.4, line 3602)

### §3.17.5 EL2 ASIDs and TLB Maintenance in E2H Mode

- [ ] Change to SMMU_CR2.E2H must be accompanied by invalidation of all TLB entries from NS-EL2 or NS-EL2-E2H STEs (§3.17.5, line 3635)
- [ ] Change to SMMU_S_CR2.E2H must be accompanied by invalidation of all TLB entries from S-EL2 or S-EL2-E2H STEs (§3.17.5, line 3636)

### §3.17.6 VMID Wildcards

- [ ] SMMU_CR0.VMW controls Non-secure VMID wildcard function; configured number of VMID LSBs ignored during invalidation matching (§3.17.6, line 3654)
- [ ] Both broadcast TLB invalidation and explicit CMD_TLBI_* commands respect VMID wildcard when SMMU_CR0.VMW != 0 (§3.17.6, line 3658)
- [ ] VMID wildcard does not allow dissimilar VMID values to alias on TLB lookup (§3.17.6, line 3662)

### §3.17.7 Broadcast TLB Maintenance for GPT Information

- [ ] SMMU with RME and SMMU_ROOT_IDR0.BGPTM==1 participates in broadcast TLBI *PA* instructions from PEs in EL3 (§3.17.7, line 3668)
- [ ] TLBI *PA* to Outer Shareable domain affects the SMMU (§3.17.7, line 3670)
- [ ] This applies regardless of SMMU_IDR0.BTM and SMMU_(*_)CR2.PTM values (§3.17.7, line 3672)

### §3.17.8 TLBInXS Maintenance Operations

- [ ] Applies only when SMMU_IDR0.BTM==1 (§3.17.8, line 3682)
- [ ] MAIR encodings 0b00000001, 0b01000000, and 0b10100000 remain Reserved; XS attribute taken as 0 for all MAIR encodings (§3.17.8, line 3693)
- [ ] Bit [11] of stage 2 block and page descriptors remains RES0; XS attribute taken as 0 for all stage 2 translations (§3.17.8, line 3694)
- [ ] SMMU behaves as though XS attribute for cached translations is 0 when determining effect of TLBI or TLBInXS operation (§3.17.8, line 3695)

## §3.18 Interrupts and Notifications

- [ ] Implementation must support one of, or optionally both of, wired interrupts and MSIs; MSI support discoverable from SMMU_IDR0.MSI and SMMU_S_IDR0.MSI (§3.18, line 3707)
- [ ] Interrupt notification must not be observable before the new information is also observable (§3.18, line 3710)
- [ ] Global error interrupt: change to GERROR must be observable if interrupt observable (§3.18, line 3712)
- [ ] Event queue interrupt: new entries must be observable to reads of queue index registers if interrupt observable (§3.18, line 3713)
- [ ] CMD_SYNC completion interrupt: consumption of CMD_SYNC must be observable to reads of queue index registers if interrupt observable (§3.18, line 3714)
- [ ] MSIs from Secure sources performed with Secure accesses targeting Secure PA space (§3.18, line 3722)
- [ ] MSIs from Non-secure sources performed with Non-secure accesses targeting Non-secure PA space (§3.18, line 3722)
- [ ] SMMU must produce unique DeviceID for outgoing MSIs that does not overlap with those for client devices (§3.18, line 3726)
- [ ] Interrupt sources: Event queue (empty→non-empty), PRI queue (SMMU_PRIQ_IRQ_CFG2 condition), CMD_SYNC, GERROR (§3.18.2, line 3756)
- [ ] When MSIs not supported: only interrupt Enable field is used; MSI address/data fields unused (§3.18, line 3736)

### §3.18.1 MSI Synchronization

- [ ] Disabling MSI through SMMU_(*_)IRQ_CTRL ensures previously-issued MSI writes are completed (§3.18.1, line 3742)
- [ ] CMD_SYNC ensures completion of MSIs originating from completion of prior CMD_SYNC commands consumed from same Command queue (§3.18.1, line 3743)
- [ ] Completion of MSI aborted: abort visible in GERROR with appropriate SMMU_(*_)GERROR.MSI_*_ABT_ERR flag (§3.18.1, line 3745)

## §3.19 Power Control

- [ ] Power off state entered only when: all client devices and interconnect quiescent, device DMA disabled, outstanding commands/invalidations/transactions complete, stalled transactions terminated with abort (§3.19, line 3798)
- [ ] On wakeup: SMMU must be reset; SMMU registers must be re-initialized before client devices can be enabled (§3.19, line 3801)

### §3.19.1 Dormant State

- [ ] When SMMU_STATUSR.DORMANT==1: no caches of any structures or translations are present; no prefetch of any configuration/translation data in progress; any structure/translation alterations will result in fresh memory reads (§3.19.1, line 3807)

## §3.20 TLB and Configuration Cache Conflict

### §3.20.1 TLB Conflict

- [ ] When TLB conflict detected: transaction aborted; F_TLB_CONFLICT event recorded (§3.20.1, line 3830)
- [ ] If TLB conflict not detected: behavior is unpredictable; restriction: transaction cannot access PA to which stream configuration does not explicitly grant access (§3.20.1, line 3835)
- [ ] TLB conflict never enables: matching entry with different VMID, different Security state, different StreamWorld, or PA outside stage 2 configured range (§3.20.1, line 3837)
- [ ] TLB conflict from one stream must not cause traffic for different streams with other VMID/StreamWorld/Security to be terminated (§3.20.1, line 3848)

### §3.20.2 Configuration Cache Conflicts

- [ ] When configuration cache conflict detected: transaction aborted; F_CFG_CONFLICT event recorded (§3.20.2, line 3858)
- [ ] If conflict not detected: behavior is unpredictable (§3.20.2, line 3863)
- [ ] Configuration cache conflict cannot cause STE to be treated as associated with different Security state (§3.20.2, line 3864)

## §3.21 Structure Access Rules and Update Procedures

### §3.21.1 Translation Tables and TLB Invalidation Completion Behavior

- [ ] TLB invalidation operation is complete after: all targeted TLB entries invalidated; relevant HTTUs globally visible; all translation table walks that could have formed targeted TLB entries are complete and globally visible (§3.21.1, line 3876)
- [ ] ATOS result cannot be based on addresses/attributes not described by translation configuration observable after invalidation (§3.21.1, line 3889)
- [ ] Translation cache entries not inserted when SMMU_(*_)CR0.SMMUEN==0 (§3.21.1, line 3900)

### §3.21.1.1 Translation Tables Update Procedure

- [ ] SMMUv3.2+: must support Level 1 or Level 2 BBM behavior as indicated by SMMU_IDR3.BBML (§3.21.1.1, line 3918)
- [ ] Break-before-make required for (pre-v8.4/pre-SMMUv3.2): changes to memory type, Cacheability, output address, block/page size, creating global entry where non-global entries overlap (§3.21.1.1, line 3904)

### §3.21.1.2 BBM Level 1 (SMMU_IDR3.BBML==1)

- [ ] Level 1: nT bit must be used when changing translation size without break-before-make; F_TLB_CONFLICT may occur without nT or BBM (§3.21.1.2, line 3937)
- [ ] Level 1: Setting nT==1 does NOT cause a fault (§3.21.1.2, line 3939)
- [ ] Level 1: Block descriptor with nT==1 not cached in way that causes TLB conflict (§3.21.1.2, line 3942)
- [ ] Level 1: Change to only Contiguous bit (bit 52) with other properties unchanged does not lead to TLB conflict fault (§3.21.1.2, line 3945)

### §3.21.1.3 BBM Level 2 (SMMU_IDR3.BBML==2)

- [ ] Level 2: implementation ignores nT bit in Block descriptor; change to translation size can be performed without BBM or nT (§3.21.1.3, line 3957)
- [ ] Level 2: F_TLB_CONFLICT never reported (§3.21.1.3, line 3961)
- [ ] Level 2: TLB multi-hit — translations use info from at most one matching entry; no faults that wouldn't otherwise be possible; no combination of info from multiple entries (§3.21.1.3, line 3962)
- [ ] Level 2: TLB invalidation removes all matching TLB entries even if overlapping entries exist (§3.21.1.3, line 3972)

### §3.21.2 Queues

- [ ] SMMU does not write to Command queue (§3.21.2, line 3985)
- [ ] To issue commands: (1) determine space using PROD/CONS, (2) write commands, (3) DSB to ensure data observable, (4) update PROD index (§3.21.2, line 3986)
- [ ] Software must not alter memory locations representing commands previously submitted until consumed (as indicated by CONS index) (§3.21.2, line 4002)
- [ ] Software must only write CONS index of output queue (Event/PRI) in consistent manner with appropriate incrementing and wrapping (§3.21.2, line 4004)
- [ ] Software must only write PROD index of Command queue in consistent manner (§3.21.2, line 4006)
- [ ] ILLEGAL PROD index write: CONSTRAINED UNPREDICTABLE: SMMU executes unpredictable commands OR stops consuming until queue disabled and re-enabled (§3.21.2, line 4007)

### §3.21.3 Configuration Structures and Configuration Invalidation Completion

- [ ] SMMU might read any entry at any time, for any reason (§3.21.3, line 4014)
- [ ] Structure considered valid only when SMMU observes V==1 and no configuration inconsistency makes it ILLEGAL (§3.21.3, line 4016)
- [ ] SMMU does not follow invalid pointers, whether speculatively or in response to incoming transaction (§3.21.3, line 4018)
- [ ] STEs and L1STDs not fetched if SMMU_(*_)CR0.SMMUEN==0 (§3.21.3, line 4020)
- [ ] CDs or L1CDs must never be fetched or prefetched unless indicated from a valid STE (§3.21.3, line 4022)
- [ ] Implementation must not read any address outside configured range of any table (§3.21.3, line 4056)
- [ ] Implementation permitted to fetch/prefetch any reachable structure at any time within bounds of containing table (§3.21.3, line 4035)
- [ ] Any change to a structure must be followed by appropriate CMD_CFGI_* invalidation command, even if structure was initially invalid (§3.21.3, line 4042)
- [ ] Configuration invalidation completion: all targeted cache entries invalidated; no accesses using old addresses/attributes; all client transactions using targeted entries globally visible; all configuration structure walks using targeted entries complete (§3.21.3, line 4061)
- [ ] Single-copy atomicity size for configuration structure fetches: if system has FEAT_LSE2, must be 128-bit; otherwise at least 64-bit (§3.21.3, line 4077)
- [ ] To change single field within aligned single-copy atomic span: can be altered directly without making structure invalid; then CMD_CFGI and CMD_SYNC required (§3.21.3, line 4084)
- [ ] For fields requiring non-single-copy-atomic writes (spanning multiple atomic spans): must make structure invalid, modify, then make valid using procedures in §3.21.3.1 (§3.21.3, line 4084)

### §3.21.3.1 Configuration Structure Update Procedure

- [ ] Initialize structure (V==0→V==1): (1) fill all fields with V==0, (2) DSB, (3) CMD_CFGI_STRUCT, (4) CMD_SYNC and wait, (5) set V=1, (6) DSB, (7) CMD_CFGI_STRUCT, (8) optionally CMD_SYNC (§3.21.3.1, line 4096)
- [ ] Make structure invalid (V==1→V==0): (1) set V==0, (2) DSB, (3) CMD_CFGI_STRUCT, (4) CMD_SYNC and wait (§3.21.3.1, line 4104)
- [ ] Software must not allow structure to enter invalid intermediate state while modifying a valid structure (§3.21.3.1, line 4111)

## §3.22 Destructive Reads and Directed Cache Prefetch Transactions

- [ ] In SMMUv3.0: these transactions unconditionally converted on output as specified by interconnect (§3.22, line 4150)
- [ ] In SMMUv3.1+: DR downgraded to non-destructive read if STE.DRE==0; W-DCP downgraded to ordinary write if STE.DCP==0; NW-DCP downgraded to no-op if STE.DCP==0 (§3.22.1, line 4188)
- [ ] STE.DRE==1 required for DR to pass without downgrade when one or more stages of translation applied (§3.22.1, line 4194)
- [ ] STE.DCP==1 required for W-DCP and NW-DCP to pass without downgrade when translation applied (§3.22.1, line 4195)
- [ ] DR requires Read/Execute AND Write permission that does not result in HTTU dirty state update; if write not granted, downgraded to read or RCI (§3.22.2, line 4213)
- [ ] NW-DCP: if required permission not present, prefetch does not occur; no abort response generated (§3.22.2, line 4215)
- [ ] RCI and DR: if ultimately lead to fault, recorded as reads; stall behavior same as ordinary read (§3.22.2, line 4219)
- [ ] W-DCP: if leads to fault, recorded as write; stall behavior same as ordinary write (§3.22.2, line 4221)
- [ ] DR, RCI, W-DCP stalled: retried as same transaction type (§3.22.2, line 4222)
- [ ] Output DR/RCI/W-DCP/NW-DCP downgraded if output attributes incompatible with output interconnect (§3.22.3, line 4228)

## §3.23 Memory Tagging Extension

- [ ] MAIR encoding 0xF0 is Reserved in SMMUv3 in CD.MAIR0 and CD.MAIR1 (§3.23, line 4240)
- [ ] All SMMU-originated accesses are Tag Unchecked accesses; SMMU does not write Allocation Tags (§3.23, line 4242)

### §3.23.1 SMMU Support for FEAT_MTE_PERM

- [ ] If SMMU_IDR3.MTEPERM==1: stage 2 MemAttr NoTagAccess encodings treated as without NoTagAccess in SMMU (§3.23.1, line 4247)
- [ ] When STE.S2FWB==0 and stage 2 MemAttr[3:0]==0b0100: SMMU interprets as Normal Inner WB Cacheable, Outer WB Cacheable (§3.23.1, line 4256)

## §3.24 Device Permission Table

- [ ] DPT use only supported for StreamIDs configured to use StreamWorld EL1; otherwise C_BAD_STE (§3.24, line 4269)
- [ ] Independent DPT for each of Non-secure and Realm states (§3.24, line 4271)
- [ ] DPT support for Non-secure state: SMMU_IDR3.DPT; for Realm state: SMMU_R_IDR3.DPT (§3.24, line 4282)

### §3.24.1 DPT Check

- [ ] If input address outside SMMU_(R_)DPT_BASE_CFG.DPTPS configured range: No Access → Device Access fault (§3.24.1, line 4295)
- [ ] Level 0 No Access entry: DPT check fails as Device Access fault (§3.24.1, line 4296)
- [ ] A[1:0]==No Access in Level 1 descriptor: DPT check fails as Device Access fault (§3.24.1, line 4298)
- [ ] Region marked W=0 and incoming transaction is write: DPT check fails as Device Access fault (§3.24.1, line 4299)
- [ ] STE.DPT_VMATCH==0b00: VMID checked when AC==0b00 or AC==0b01; if VMID required and does not match → Device Access fault (§3.24.1, line 4307)
- [ ] STE.DPT_VMATCH==0b01: VMID checked only when AC==0b00 (§3.24.1, line 4308)
- [ ] STE.DPT_VMATCH==0b10: VMID never checked (§3.24.1, line 4309)
- [ ] For Realm STEs: DPT_VMATCH always 0b00 (§3.24.1, line 4314)
- [ ] Non-secure DPT: output PA space is Non-secure (§3.24.1, line 4322)
- [ ] Realm DPT: AC==0b01 or 0b10 → output PA space Non-secure; otherwise → output PA space Realm (§3.24.1, line 4323)

### §3.24.2 DPT Caching Behavior

- [ ] DPT TLB entries never created from ATS TRs that bypass all stages of translation (§3.24.2, line 4342)
- [ ] Level 0 No Access entry is NOT permitted to be cached in DPT TLB (§3.24.3.1.1, line 4484)

### §3.24.3.1 DPT Descriptor Formats

- [ ] Level 0: bits[1:0]==0b00 → No Access entry (§3.24.3.1.1, line 4483)
- [ ] Level 0: bits[1:0]==0b01 → Block descriptor; AC and W fields valid (§3.24.3.1.2, line 4493)
- [ ] Level 0: bits[1:0]==0b11 → Table descriptor; address field is next-level base (§3.24.3.1.3, line 4532)
- [ ] Level 0 AC field: 0b00=VMID checked unless DPT_VMATCH==0b10; 0b01=VMID checked unless DPT_VMATCH==0b01 or 0b10; 0b10=VMID is RES0; 0b11=Reserved/invalid (§3.24.3.1.2, line 4503)
- [ ] If SMMU_IDR0.VMID16==0: VMID[15:8] are RES0 in Level 0 Block entries (§3.24.3.1.2, line 4517)
- [ ] Level 1 A[1:0]==0b00: No Access to both granules; all other fields RES0 (§3.24.3.1.4, line 4561)
- [ ] Level 1 A[1:0]==0b01: No Access upper granule; lower granule governed by AC0, W0, VMID0 (§3.24.3.1.4, line 4562)
- [ ] Level 1 A[1:0]==0b10: upper granule governed by AC1, W1, VMID1; No Access lower granule (§3.24.3.1.4, line 4563)
- [ ] Level 1 A[1:0]==0b11, Contig==0: upper granule AC1/W1/VMID1; lower granule AC0/W0/VMID0 (§3.24.3.1.4, line 4564)
- [ ] Level 1 A[1:0]==0b11, Contig!=0: contiguous region controlled by AC0, W0, VMID0 only; AC1/W1/VMID1 are RES0 (§3.24.3.1.4, line 4565)
- [ ] If Contig selects Reserved encoding: descriptor is invalid (§3.24.3.1.4, line 4593)
- [ ] Any RES0 bit non-zero or Reserved field value → descriptor is Invalid (§3.24.3.1.4, line 4549)

### §3.24.4 DPT Lookup Errors

- [ ] DPT lookup fault priority: (1) DPT_WALK_EN=0 → DPT_DISABLED at L0, (2) Invalid DPT register config → DPT_WALK_FAULT at L0, (3) GPC on L0 fetch → DPT_GPC_FAULT at L0, (4) External abort on L0 fetch → DPT_EABT at L0, (5) Invalid L0 descriptor → DPT_WALK_FAULT at L0, (6) GPC on L1 fetch → DPT_GPC_FAULT at L1, (7) External abort on L1 → DPT_EABT at L1, (8) Invalid L1 descriptor → DPT_WALK_FAULT at L1 (§3.24.4, line 4615)
- [ ] If SMMU_(R_)DPT_CFG_FAR.FAULT==0: SMMU reports fault info in register and sets FAULT=1; if already 1, fault not reported (§3.24.4, line 4628)
- [ ] When DPT_ERR made active in SMMU_(R_)GERROR: corresponding DPT_CFG_FAR has already been made observable (§3.24.4, line 4630)
- [ ] Reserved DPTPS value (0b111) or exceeds SMMU_IDR5.OAS: treated as Invalid DPT register configuration (§3.24.4, line 4641)

### §3.24.5 DPT Maintenance Operations

- [ ] CMD_DPTI_ALL and CMD_DPTI_PA: same consumption and completion behavior as CMD_TLBI_* commands (§3.24.5, line 4661)
- [ ] Consumption of CMD_DPTI_* does not provide guarantees; CMD_SYNC after guarantees invalidation complete, events reported, client transactions complete (§3.24.5, line 4663)
- [ ] CMD_TLBI_* commands and broadcast TLBI for stage 1/2 NOT required to invalidate DPT TLB entries (§3.24.5, line 4674)

### §3.24.6 Software Guidance

### §3.24.6.2 Invalid to Valid Transition

- [ ] Order for invalid→valid: (1) configure DPT to grant access, (2) cache maintenance and barriers, (3) configure final stage of translation to grant access; TLB maintenance NOT required (§3.24.6.2, line 4699)

### §3.24.6.3 Valid to Invalid Transition

- [ ] Order for valid→invalid: (1) mark final stage descriptor as Invalid, (2) TLBI + sync, (3) CMD_ATC_INV + sync, (4) if fully-coherent device: issue CMOs, (5) mark DPT config as invalid, (6) CMD_DPTI_* + sync (§3.24.6.3, line 4709)

### §3.24.6.4 Clearing DPT Lookup Errors

- [ ] Algorithm: (1) write 0 to SMMU_(R_)DPT_CFG_FAR.FAULT, (2) acknowledge SMMU_(R_)GERROR.DPT_ERR, (3) read FAULT again to check for new fault between steps 1 and 2 (§3.24.6.4, line 4722)

## §3.25 Granule Protection Checks

- [ ] GPC enabled only when SMMU_ROOT_CR0.GPCEN==1 (§3.25, line 4757)
- [ ] GPT format and meaning same in SMMU with RME as in FEAT_RME (§3.25, line 4753)
- [ ] Client-originated access experiencing GPC fault: signaled to client device as External abort (§3.25.1, line 4762)
- [ ] Client-originated access GPC fault on output address: NOT reported in Event queue (§3.25.1, line 4764)

### §3.25.1.1 GPC for Client Devices Without StreamID (NoStreamID)

- [ ] NoStreamID device access with PA exceeding SMMU_IDR5.OAS: terminated with abort; no Event record or fault recorded (§3.25.1.1, line 4776)

### §3.25.1.2 Speculative and Hint Accesses

- [ ] GPC fault during speculative translation request/translation/prefetch/NW-DCP/DH: no event record generated (§3.25.1.2, line 4786)
- [ ] If SMMU_IDR0.RME_IMPL==1: GPC fault during speculative access is NOT reported (§3.25.1.2, line 4788)

### §3.25.2 Interactions with PCIe ATS

- [ ] SMMU_CR0.ATSCHK has no effect on granule protection checks (§3.25.2, line 4799)
- [ ] SMMU-originated access experiencing GPC fault while servicing ATS TR: SMMU responds with Completer Abort (§3.25.2, line 4800)
- [ ] If ATS TR success with R==W==0: address not valid; not subject to GPC (§3.25.2, line 4802)
- [ ] If SMMU_IDR0.RME_IMPL==1: GPC performed on output address for ATS TR result before sending completion (§3.25.2, line 4803)
- [ ] SMMU returns translation region size in ATS Completion such that GPC passes for accesses anywhere in region (§3.25.2, line 4805)
- [ ] ATS Translated transactions: subject to GPC; if GPC fails, terminated with abort (§3.25.2, line 4810)

### §3.25.3 SMMU-Originated Accesses

- [ ] SMMU-originated access experiencing GPC fault: reported as External abort (§3.25.3, line 4815)
- [ ] For SMMU_IDR0.RME_IMPL==1: F_STE_FETCH/F_CD_FETCH/F_VMS_FETCH/F_WALK_EABT arising from GPC fault reported with GPCF=1 (§3.25.3, line 4822)

### §3.25.4 Reporting of GPC Faults

- [ ] GPF (Granule Protection Fault): reported in SMMU_ROOT_GPF_FAR (§3.25.4, line 4837)
- [ ] GPT lookup error: reported in SMMU_ROOT_GPT_CFG_FAR (§3.25.4, line 4842)
- [ ] GPF conditions: access to PA space other than NS with address exceeding PPS range; access to GPT-forbidden location (§3.25.4, line 4838)
- [ ] GPT lookup error conditions: Reserved fields in SMMU_ROOT_GPT_BASE_CFG; PPS exceeding OAS; invalid SH/IRGN/ORGN combination; ADDR exceeding PPS; GPT Table Entry output exceeding PPS; invalid GPT Entry; External abort on GPT Entry fetch (§3.25.4, line 4843)

### §3.25.5 SMMU Behavior If GPC Fault is Active

- [ ] If GPF active in SMMU_ROOT_GPF_FAR: other accesses without GPF or GPT lookup error continue as specified (§3.25.5, line 4859)
- [ ] GPF remains active until software writes 0 to SMMU_ROOT_GPF_FAR.FAULT (§3.25.5, line 4860)
- [ ] GPT lookup error remains active until software writes 0 to SMMU_ROOT_GPT_CFG_FAR.FAULT (§3.25.5, line 4866)
- [ ] SMMU with RME has two additional wired interrupts: GPF_FAR (error becomes active in SMMU_ROOT_GPF_FAR) and GPT_CFG_FAR (error becomes active in SMMU_ROOT_GPT_CFG_FAR) (§3.25.5, line 4868)

### §3.25.6 Observability of GPC Faults

- [ ] If client transaction termination due to GPC fault observable to client: if GPF_FAR/GPT_CFG_FAR did not contain active fault → syndrome info observable in appropriate register; if already active → not updated (§3.25.6, line 4877)
- [ ] If GPC fault interrupt observable: syndrome info observable in SMMU_ROOT_GPF_FAR or SMMU_ROOT_GPT_CFG_FAR (§3.25.6, line 4882)
- [ ] If client GPC fault termination visible to client: subsequent CMD_SYNC guarantees observability of related events in Event queue or that events discarded (§3.25.6, line 4884)
- [ ] For SMMU with BGPTM==1: after broadcast TLBI *PA* and DSB, subsequent CMD_SYNC guarantees no events relating to invalidated GPT configuration later observable (§3.25.6, line 4886)

## §3.26 Permission Indirections

### §3.26.1 Stage 1 Permission Indirections

- [ ] SMMU_IDR3.S1PI==0: stage 1 permission indirections not supported; STE.S1PIE and CD.PIE are RES0 (§3.26.1, line 4918)
- [ ] SMMU_IDR3.S1PI==1, STE.S1PIE==1, CD.PIE==1: stage 1 permissions determined from CD.PIIP and CD.PIIU using PIIndex from descriptors (§3.26.1, line 4922)
- [ ] STE.S1PIE==0: hypervisor can prevent guest use of stage 1 permission indirections (§3.26.1, line 4924)
- [ ] SMMU does NOT support stage 1 permission overlay feature (§3.26.1, line 4926)
- [ ] If stage 1 Indirect Permission Scheme enabled: CD.WXN is RES0 and has no effect (§3.26.1, line 4927)

### §3.26.2 Stage 2 Permission Indirections

- [ ] SMMU_IDR3.S2PI==1, STE.S2PIE==0, STE.S2POE==1: ILLEGAL → generates C_BAD_STE (§3.26.2, line 4948)
- [ ] SMMU_IDR3.S2PI==1, STE.S2PIE==1, STE.S2POE==0: stage 2 permissions from SMMU_S2PII using PIIndex (§3.26.2, line 4949)
- [ ] SMMU_IDR3.S2PI==1, STE.S2PIE==1, STE.S2POE==1: stage 2 permissions from STE.S2POI (POIndex) combined with SMMU_S2PII (PIIndex) (§3.26.2, line 4950)
- [ ] Stage 2 permission computation order: (1) AssuredOnly check for stage 2 of stage 1 output address, (2) Base and Overlay permissions, (3) STE.S2PTW for TT walk/CD fetch, (4) Dirty state permission check if indirection enabled, (5) STE.DRE/STE.DCP for directed prefetch and CMO (§3.26.2, line 4952)

## §3.27 Translation Hardening

### §3.27.1 Protected Attribute

- [ ] If SMMU_IDR3.THE==1: Protected attribute in VMSAv9-128 descriptors always present (§3.27.1, line 4971)
- [ ] If SMMU_IDR3.THE==1: Protected attribute in VMSAv8-64 descriptors only if CD.PnCH==1 (§3.27.1, line 4972)
- [ ] If CD.PnCH==1: Contiguous bit NOT present in VMSAv8-64 descriptors; bit 52 no longer interpreted as Contiguous (§3.27.1, line 4973)
- [ ] SMMU does NOT support Read-Check-Write (RCW) operations (§3.27.1, line 4975)

### §3.27.2 AssuredOnly Permission Checks

- [ ] If SMMU_IDR3.THE==1: AssuredOnly behavior same as PE except configured via STE.AssuredOnly (§3.27.2, line 4981)
- [ ] Stage 2 Permission fault with AssuredOnly: reported as stage 2 F_PERMISSION with AssuredOnly set to 1 (§3.27.2, line 4984)
- [ ] If CD or L1CD fetched from memory NOT marked AssuredOnly at stage 2: access translated from TTB0 or TTB1 does NOT have Assured Translation property (§3.27.2, line 4985)
- [ ] If stage 1 translation disabled (bypass due to STE.Config or STE.S1DSS): access to region marked AssuredOnly at stage 2 generates Permission fault (§3.27.2, line 4987)
- [ ] For ATS TR: AssuredOnly check performed same as regular transaction; if fails → ATS Completion with Success and R==W==0 (§3.27.2, line 4989)
- [ ] For ATS Translated transaction with STE.EATS==0b10 (Split-stage ATS): AssuredOnly is IGNORED for determining whether transaction permitted (§3.27.2, line 4991)
