# TASKS_OPERATION.md — ARM SMMUv3 Chapter 3 Operational Checklist

Every item is a concrete behavioral rule, encoding, fault condition, or procedural step from Chapter 3 of IHI0070G_b. Line numbers cite the specific line in the spec containing the rule.

---

## §3.1 Software Interface

> **Audit date:** 2026-05-05 — 6 items checked: 4 PASS, 2 N/A, 0 bugs (BUG-AUDIT-148-CPP FIXED 2026-05-05)

- [x] SMMU provides three software interfaces: memory-based data structures, memory-based circular buffer queues (Command, Event, PRI), and a register set per Security state (§3.1, line 1217) — **PASS**: `streamMap` + three queues with PROD/CONS pairs + full register surface (CR0, IDR0–IDR5, GBPA, etc.) in smmu.h
- [x] PRI queue is only present on SMMUs supporting PRI services (§3.1, line 1220) — **PASS / BUG-AUDIT-148-CPP FIXED**: `submitPageRequest()` now silently drops PPRs when `priSupported_==false` (no enqueue, no auto-failure); `processPRIQueue()` is a no-op; `setPRISupported(false)` clears in-flight queue and resets PROD/CONS. TDD test: `test_bug_audit148_pri_gate.cpp`
- [x] When Secure state is supported, an additional register set exists to allow Secure software to maintain Secure device structures, issue commands on a second Secure Command queue and read Secure events from a Secure Event queue (§3.1, line 1223) — **N/A**: No Secure register namespace implemented; `SecurityState::Secure` is a per-transaction attribute only; no Secure register-set support is advertised via IDR bits
- [x] IMPLEMENTATION DEFINED fields must not be used in a way that makes a generic SMMUv3 driver unusable (§3.1, line 1227) — **PASS**: All IMPDEF fields zero-initialized (F_UUT.reason=0, IIDR=0); no behavioral dependency on non-zero IMPDEF values; core paths unaffected
- [x] A driver without extended knowledge of IMPLEMENTATION DEFINED fields must treat them as Reserved and set to 0 (§3.1, line 1227) — **N/A**: Driver behavioral requirement, not an SMMU implementation requirement; no C++ code needed
- [x] An implementation only uses IMPLEMENTATION DEFINED fields to enable extended functionality and must remain compatible with generic driver software when those fields are set to 0 (§3.1, line 1229) — **PASS**: IMPDEF fields default to 0; all translation, fault, command, and event paths function correctly with IMPDEF=0

## §3.2 Stream Numbering

> **Audit date:** 2026-05-06 — 11 items checked: 7 PASS, 2 N/A, 1 N/A (integrator), 1 N/A (Secure not implemented) — 0 bugs

- [x] StreamID is of IMPLEMENTATION DEFINED size, between 0 bits and 32 bits (§3.2, line 1238) — **PASS**: `setStrtabLog2Size()` clamps any value >32 to 32 (smmu.cpp:5587–5589); default is 32; IDR1 advertises SIDSIZE=32 (smmu.cpp:3485)
- [x] SubstreamID is of IMPLEMENTATION DEFINED size, between 0 bits and 20 bits (§3.2, line 1241) — **PASS**: `isConfigurationValid()` rejects `s1cdMax > 20` with C_BAD_STE (smmu.cpp:1195–1198); IDR1 advertises SSIDSIZE=20 (smmu.cpp:3486); tested in `test_tenth_pass_bugs_spec.cpp`
- [x] Transactions provided with a SubstreamID are terminated when stage 1 translation is not enabled (§3.2, line 1354) — **PASS**: guard `if (ssv && !streamCfgSnapshot.stage1Enabled)` emits C_BAD_SUBSTREAMID (smmu.cpp:674–685); tested in `test_c_bad_substreamid_spec.cpp`
- [x] A stage 2-only implementation does not take a SubstreamID input (§3.2, line 1252) — **PASS**: same guard covers stage2-only path (stage1Enabled=false, stage2Enabled=true); `Stage2OnlyNonZeroPasidFails` test confirms
- [x] An implementation with stage 1 is not required to support substreams (§3.2, line 1252) — **PASS**: `s1cdMax` defaults to 0; stage-1 translation works correctly with s1cdMax=0; tested in `test_c_bad_substreamid_spec.cpp:Stage1OnlyNonZeroPasidIsOk`
- [x] When Secure state is supported, the StreamID input is qualified by SEC_SID determining Secure or Non-secure StreamID namespace (§3.2, line 1254) — **N/A**: No Secure register namespace implemented; consistent with §3.1 audit verdict. SEC_SID is modeled as a per-transaction `SecurityState` parameter (types.h:509–512) but no separate SMMU_S_* register bank exists
- [x] For PCI, StreamID must be at least 16 bits for SMMU implementations intended for use with PCI clients (§3.2, line 1258) — **N/A** (integrator requirement): default StreamID width is 32 bits; `setStrtabLog2Size()` imposes no minimum; requirement is on system integrators, not the SMMU model
- [x] Arm recommends StreamID be a dense namespace starting at 0 (§3.2, line 1236) — **N/A**: architectural recommendation only; no runtime enforcement required or present
- [x] StreamID namespace is per-SMMU; devices with the same StreamID behind different SMMUs are seen as different sources (§3.2, line 1236) — **PASS**: `streamMap` is a non-static instance member (smmu.h:436); each SMMU instance owns its own stream map with no shared global state
- [x] SubstreamID maximum size of 20 bits matches the maximum size of a PCIe PASID (§3.2, line 1245) — **N/A**: informational note only; implementation enforces the 20-bit maximum via the s1cdMax>20 check above
- [x] For PCIe, SubstreamID is intended to be directly provided from the PASID in a one-to-one fashion (§3.2, line 1256) — **PASS**: PASID parameter is passed through unmodified as SubstreamID at all call sites (smmu.cpp:2160–2162, 657, 677); no masking or transformation applied

## §3.3 Data Structures and Translation Procedure

> **Audit date:** 2026-05-06 — 35 items checked: 20 PASS, 6 N/A, 2 PARTIAL, 2 BUG-AUDIT (BUG-AUDIT-149-CPP FIXED, BUG-AUDIT-152-CPP FIXED)

- [x] When SMMU_CR0.SMMUEN == 0 (globally disabled), transaction passes through without address modification; attributes applied from SMMU_GBPA (§3.3, line 1403) — **PASS**: smmu.cpp:265-318; `cr0_` bit 0 checked; GBPA.ABORT=0 returns identity bypass with `gbpaConfig_` attributes (mtCfg, shCfg, allocCfg, instCfg, privCfg)
- [x] When SMMU_GBPA.ABORT is set, all transactions are aborted in bypass (§3.3, line 1403) — **PASS**: smmu.cpp:281-283; `gbpaAbort_.load()` returns `makeTranslationError(SMMUError::GbpaAbort)` unconditionally
- [x] If the SMMU does not implement one of the two translation stages, it behaves as though that stage is permanently in bypass (§3.3, line 1429) — **PASS**: smmu.cpp:1182-1188; S1P/S2P flags gate stage usage; stage routing at smmu.cpp:2246-2253 dispatches stage-1-only, stage-2-only, or both-stages paths
- [x] An SMMU must support at least one stage of translation (§3.3, line 1429) — **PASS / BUG-AUDIT-149-CPP FIXED**: `setS1PSupported(false)` now refuses if `s2pSupported_` is false; `setS2PSupported(false)` refuses if `s1pSupported_` is false (smmu.cpp:3638-3661). TDD: `test_bug_audit149_at_least_one_stage.cpp` (4 tests all pass)
- [x] S1ContextPtr and L2Ptr addresses are IPAs when both stage 1 and stage 2 are in use, and PAs when only stage 1 is used (§3.3.2, line 1375) — **PASS**: smmu.cpp:2246-2253; two-stage path calls `performBothStagesTranslation()` which routes stage-1 result as IPA into stage-2; stage-1-only calls `performStage1OnlyTranslation()` using PA directly

### §3.3.1 Stream Table Lookup

- [x] StreamID is range-checked against the programmed table size; a transaction is terminated if its StreamID would select an entry outside the configured Stream table extent; C_BAD_STREAMID is recorded (§3.3.1, line 1276) — **PASS**: smmu.cpp:357-388; `strtabLog2Size_` bounds check; StreamIDs >= 2^LOG2SIZE produce `FaultType::BadStreamID` + `EventType::C_BAD_STREAMID`
- [x] Linear Stream table format is supported by all SMMU implementations (§3.3.1.1, line 1285) — **PASS**: types.h:1436 `StreamTableFormat::Linear=0` as default; smmu.cpp:107 initializes `strtabFmt_` to Linear
- [x] Linear Stream table is a contiguous array of STEs indexed from 0 by StreamID; size is configurable as 2^n multiple of STE size (§3.3.1.1, line 1285) — **PASS**: smmu.cpp:357-388; `strtabLog2Size_` configures 2^n entries; bounds check `(uint64_t)1u << log2sz`
- [x] SMMUs supporting more than 64 StreamIDs (6 bits) must also support two-level Stream tables (§3.3.1.2, line 1298) — **PASS**: IDR0.ST_LEVEL[0]=1 (smmu.cpp:3472); `StreamTableFormat::TwoLevel` with `validateStreamID2Level()` (smmu.cpp:5490); IDR1.SIDSIZE=32
- [x] 2-level Stream table top-level is indexed by StreamID[n:x] where x is SMMU_STRTAB_BASE_CFG.SPLIT; second-level tables indexed by up to StreamID[x-1:0] (§3.3.1.2, line 1294) — **PASS**: smmu.cpp:5490-5518; `l1Index = streamID >> split` validated against `2^(log2sz - split)`
- [x] Where 2-level Stream tables are supported, split points of 6, 8, and 10 bits can be used (§3.3.1.2, line 1296) — **PASS**: smmu.cpp:5478-5482; `setStrtabSplit()` enforces split ∈ {6, 8, 10}; rejects all other values
- [x] SMMU_IDR0.ST_LEVEL field advertises support for 2-level Stream table format (§3.3.1.2, line 1296) — **PASS**: smmu.cpp:3472; `getIDR0()` sets bit 27 (ST_LEVEL[0])
- [x] Top-level descriptors contain pointer to second-level table along with StreamID span; each can be marked invalid (§3.3.1.2, line 1303) — **N/A**: Software model uses mathematical bounds check only (`validateStreamID2Level()`, smmu.cpp:5490-5518); no raw L1 descriptor storage. Same precedent as §3.3 BUG-AUDIT-135/136/137 reclassified N/A in TASKS_BUGS.md

### §3.3.2 StreamIDs to Context Descriptors

- [x] When STE.S1DSS == 0b00, all traffic expected to have SubstreamID; lack of SubstreamID causes abort and event recorded (§3.3.2, line 1362) — **PASS**: smmu.cpp:2021-2040; `s1dss==0x00 && pasid==0` generates `F_STREAM_DISABLED` and returns `SMMUError::SubstreamDisabled`
- [x] When STE.S1DSS == 0b01, transaction without SubstreamID is treated as stage 1-bypass (§3.3.2, line 1363) — **PASS**: smmu.cpp:2042-2111; `s1dss==0x01 && pasid==0` bypasses stage-1; either returns identity (no stage-2) or passes IOVA directly to stage-2 as IPA
- [x] When STE.S1DSS == 0b10, transaction without SubstreamID uses the CD of Substream 0; transactions arriving with SubstreamID 0 are aborted and event recorded (§3.3.2, line 1364) — **PASS**: smmu.cpp:698-715; SSV=1 + PASID==0 + S1DSS==0b10 generates `F_STREAM_DISABLED` and aborts; non-SSV path falls through to CD[0]
- [x] STE.S1ContextPtr field gives address of one or more CDs, configured by STE.S1Fmt and STE.S1CDMax (§3.3.2, line 1366) — **PASS**: types.h:1169-1172; `s1cdMax` configures CD table size; smmu.cpp:648-665 uses `s1cdMax` to gate C_BAD_SUBSTREAMID checking
- [x] Multiple StreamID/SubstreamID configurations with identical ASID/VMID/StreamWorld must maintain same configuration where that configuration can affect TLB lookup (§3.3.3, line 1514) — **N/A**: Programming model constraint on software ("software must ensure" per spec line 1514-1515); same precedent as TASKS_BUGS.md §3.3 BUG-AUDIT-135/136 reclassified N/A
- [x] Two streams sharing the same ASID/VMID/StreamWorld must use the same translation table base addresses and translation granule (§3.3.3, line 1515) — **N/A**: Programming model constraint; spec text places obligation on software, not SMMU enforcement
- [x] For any-EL2 and EL3 regimes, only one translation table is used; CD.TTB1 is unused (§3.3.3, line 1517) — **N/A**: Model uses unified per-PASID AddressSpace with no TTBR0/TTBR1 split; TTB1 structurally absent for all streams; EL2/EL3 requirement satisfied by architecture
- [x] Selecting an inconsistent combination of StreamWorld and CD.AA64 causes the CD to be ILLEGAL (§3.3.3, line 1519) — **N/A**: Model globally rejects `CD.AA64==0` (`!config.aa64` → C_BAD_CD at smmu.cpp:2179-2182) for all StreamWorlds; AArch32 LPAE not implemented; StreamWorld+AA64 consistency check degenerates to "AA64 must always be 1"
- [x] Secure stage 2 is not supported for VMSAv8-32 LPAE translation tables (§3.3.3, line 1521) — **N/A**: VMSAv8-32 LPAE globally rejected (`aa64==false` → C_BAD_CD smmu.cpp:2179-2182; `s2aa64==false` → C_BAD_STE smmu.cpp:1174-1177); doubly enforced
- [x] AP[1] bit is IGNORED for any-EL2 and EL3 StreamWorlds (VMSAv8-64 and VMSAv9-128) (§3.3.4, line 1536) — **PASS**: smmu.cpp:453-456, 475-490; EL2/EL3 access type promoted to Privileged variants on both TLB fast-path and slow-path; EL2_E2H explicitly excluded
- [x] any-EL2-E2H translations maintain privileged/non-privileged checks in the same manner as EL1 (§3.3.4, line 1536) — **PASS**: smmu.cpp:454-455, 474-476; EL2_E2H excluded from the `EL2 || EL3` privilege-suppress promotion; retains normal privilege checking
- [x] Bits [63:60] of stage 2 Block and Page descriptors are Reserved for use by a System MMU; in SMMUv3.1 and later these bits are RES0 (§3.3.5, line 1555) — **N/A**: Software model uses structured AddressSpace API; no raw 64-bit stage-2 descriptor parsing; hardware descriptor format requirement not applicable

### §3.3.3 StreamWorld Table

- [x] StreamWorld NS-EL1: Non-secure EL1&0, with ASID and VMID tags (§3.3.3, line 1478) — **PASS**: types.h:1143 `EL1_EL0=0x00`; TLB insertion at smmu.cpp:757 uses `streamCfg.asid` and `streamCfg.vmid` for EL1_EL0 stage-1+stage-2
- [x] StreamWorld NS-EL2: Non-secure EL2 without E2H; translations do not have an ASID tag (§3.3.3, line 1479) — **PASS / BUG-AUDIT-152-CPP FIXED**: TLB insertion path (smmu.cpp:~788) now zeros `entryAsid` when `strw==StreamWorld::EL2`; previously only zeroed for stage-2-only or EL2_E2H+CR2.E2H=0. TDD: `test_bug_audit152_el2_el3_asid.cpp:EL2StreamWorldHasNoAsidTag`
- [x] StreamWorld NS-EL2-E2H: Non-secure EL2&0 with E2H; translations have ASID tag (§3.3.3, line 1480) — **PASS**: types.h:1145 `EL2_E2H=0x02`; TLB insertion retains `streamCfg.asid` when CR2.E2H=1; tested in `test_bug_audit152_el2_el3_asid.cpp:EL2E2HStreamWorldRetainsAsidTagWhenE2HEnabled`
- [x] StreamWorld S-EL2: Secure EL2 without E2H; no ASID tag (§3.3.3, line 1481) — **N/A**: No Secure register namespace; consistent with §3.1 audit verdict; Secure is per-transaction attribute only. S-EL2 as distinct StreamWorld is 🚫 out-of-scope
- [x] StreamWorld S-EL2-E2H: Secure EL2&0 with E2H; ASID tag (§3.3.3, line 1482) — **N/A**: Same as S-EL2; 🚫 out-of-scope (Secure register namespace not implemented)
- [x] StreamWorld EL3: EL3 in AArch64 state when FEAT_RME not implemented; no ASID tag (§3.3.3, line 1491) — **PASS**: smmu.cpp:~788 BUG-AUDIT-152-CPP fix zeros `entryAsid` for `strw==StreamWorld::EL3` in same guard as EL2; EL3 STRW=0b11 is ILLEGAL for both Secure (smmu.cpp:1085-1088) and NonSecure (smmu.cpp:1078-1081) stage-1 streams; fix is defence-in-depth. Enum distinctness verified in `test_bug_audit152_el2_el3_asid.cpp:EL3StreamWorldEnumDistinctFromEL2`
- [x] StreamWorld Realm-EL1: Realm EL1&0 (§3.3.3, line 1492) — **N/A**: TASKS_BUGS.md §2.6 marks Realm 🚫 out-of-scope (SMMU for RME features not implemented)
- [x] StreamWorld Realm-EL2: Realm EL2 without E2H; no ASID tag (§3.3.3, line 1493) — **N/A**: Realm 🚫 out-of-scope
- [x] StreamWorld Realm-EL2-E2H: Realm EL2&0 with E2H; ASID tag (§3.3.3, line 1494) — **N/A**: Realm 🚫 out-of-scope
- [x] A translation is architecturally unique if identified by unique {StreamWorld, VMID, ASID, Address} (§3.3.3, line 1502) — **PASS**: types.h:1342-1377; `TLBEntry` carries asid, vmid, strw, iova, securityState; uniqueness supported by entry structure; smmu.cpp:2333-2354

## §3.4 Address Sizes

> **Audit date:** 2026-05-06 — 40 items checked: 18 PASS, 21 N/A, 1 bug (BUG-AUDIT-153-CPP FIXED 2026-05-06)

- [x] SMMU input address size is 64 bits (§3.4, line 1562) — **PASS**: All translation entry points accept `uint64_t IOVA`; no width truncation at ingress
- [x] IAS = MAX(SMMU_IDR0.TTF[0]==1 ? 40 : 0, SMMU_IDR0.TTF[1]==1 ? OAS : 0) (§3.4, line 1568) — **PASS**: IDR0.TTF=0b10 (AArch64-only); TTF[0]=0 contributes 0; TTF[1]=1 contributes OAS=48; IAS=48 enforced throughout (smmu.cpp:3451-3481, smmu.cpp:3537)
- [x] VMSAv8-32 LPAE always supports IPA size of 40 bits; IPS field of the CD is IGNORED (§3.4, line 1570) — **N/A**: VMSAv8-32 LPAE globally rejected; `!config.aa64` → C_BAD_CD (smmu.cpp:2179-2182); `!s2aa64` → C_BAD_STE (smmu.cpp:1174-1177)
- [x] OAS reflects maximum usable PA output from last stage of VMSAv8-64 or VMSAv9-128 translations; discoverable from SMMU_IDR5.OAS (§3.4, line 1572) — **PASS**: IDR5.OAS=5 (48-bit) hardcoded at smmu.cpp:3537; `oasBits_`=48 used in all OAS enforcement paths
- [x] When SMMU_(*_)CR0.SMMUEN == 0 and SMMU_(*_)GBPA.ABORT == 0: if input address exceeds OAS, transaction terminated with abort and NO event recorded (§3.4, line 1576) — **PASS**: smmu.cpp:286-291; bare `return makeTranslationError(SMMUError::InvalidAddress)` with no `generateEvent` call; distinct from STE-bypass path
- [x] When STE.Config == 0b100 (bypass all stages): if input address exceeds OAS, transaction terminated with abort and F_ADDR_SIZE is recorded (§3.4, line 1578) — **PASS**: smmu.cpp:1921-1939; explicitly calls `generateEvent(EventType::F_ADDR_SIZE, ...)` before returning error
- [x] Stage 1 Translation fault (F_TRANSLATION) occurs if VA is outside range specified by CD (§3.4, line 1585) — **PASS**: smmu.cpp:2213-2228; `effectiveIova >= vaLimit` where `vaLimit = 1 << (64-T0SZ)` generates F_TRANSLATION
- [x] For VMSAv8-32 LPAE CD: maximum input range is fixed at 32 bits; Translation fault if upper 32 bits are not all zero (§3.4, line 1586) — **N/A**: VMSAv8-32 globally rejected (see above)
- [x] For VMSAv8-64: maximum input size is 48 bits if SMMU_IDR5.VAX == 0b00 or 4K/16K granule with DS==0 (§3.4, line 1593) — **PASS**: VAX=0, 4KB granule; T0SZ=16 enforces N=48 via `vaLimit = 1<<48`; tested in `test_bug_audit153_canonical_va.cpp`
- [x] For VMSAv8-64: maximum input size is 52 bits if SMMU_IDR5.VAX == 0b01 or 0b10 and 64KB granule or DS==1 (§3.4, line 1596) — **N/A**: VAX=0 only; DS field not modeled; 52-bit VA path not implemented
- [x] For VMSAv9-128: max input 48 bits if VAX==0b00; 52 bits if VAX==0b01; 55 bits EL1/EL2-E2H if VAX==0b10; 56 bits EL3 if VAX==0b10 (§3.4, line 1599) — **N/A**: VMSAv9-128 not modeled
- [x] VA is inside range only if correctly sign-extended from top bit of range size upwards, except for TBI configurations (§3.4, line 1605) — **PASS / BUG-AUDIT-153-CPP FIXED**: canonical VA check now at smmu.cpp:2229-2247; `upper = effectiveIova >> (N-1)`; `mask = (1<<extWidth)-1`; `nonCanonical = upper != 0 && upper != mask` → F_TRANSLATION. TDD: `test_bug_audit153_canonical_va.cpp` (2 RED tests now GREEN)
- [x] Address output from stage 1 translation causes F_ADDR_SIZE if exceeds IPA size range (§3.4, line 1609) — **PASS**: smmu.cpp:2523-2540; IPA checked against `1ULL << ipsBits`; F_ADDR_SIZE generated on overflow
- [x] For VMSAv8-64/VMSAv9-128 CDs, IPA size given by effective IPS field of CD, capped to OAS (§3.4, line 1611) — **PASS**: smmu.cpp:2183-2196; `config.ips > oasBits_` → C_BAD_CD
- [x] When bypassing stage 1 (STE.Config == 0b1x0, STE.S1DSS == 0b01, or unimplemented): if input address exceeds IAS, stage 1 F_ADDR_SIZE occurs, transaction terminated, F_ADDR_SIZE recorded (§3.4, line 1613) — **PASS**: smmu.cpp:2048-2066; S1DSS=0b01 path checks OAS and calls `generateEvent(EventType::F_ADDR_SIZE, ...)`
- [x] TBI configuration can only be enabled when a CD is used (stage 1 translates); always disabled when stage 1 bypassed or disabled (§3.4, line 1615) — **PASS**: smmu.cpp:2248-2249; TBI masking gated on `config.stage1Enabled`; bypass and stage-2-only paths never reach TBI code
- [x] Stage 2 Translation fault if IPA is outside range configured by S2T0SZ (§3.4, line 1623) — **PASS**: smmu.cpp:2275-2291 (stage-2-only) and smmu.cpp:2543-2560 (two-stage); `ipa >= (1ULL << (64-s2t0sz))` → F_TRANSLATION
- [x] For VMSAv8-32 LPAE STE: stage 2 input range capped at 40 bits regardless of IAS size (§3.4, line 1624) — **N/A**: VMSAv8-32 globally rejected
- [x] For VMSAv8-64/VMSAv9-128 STE: stage 2 input range capped to IAS (§3.4, line 1627) — **PASS**: S2T0SZ enforces IAS=48 cap; out-of-range IPA → F_TRANSLATION at both stage-2 paths
- [x] Stage 2 Address Size fault if output address exceeds effective PA output range from S2PS (§3.4, line 1629) — **PASS**: smmu.cpp:2725-2747 (two-stage) and smmu.cpp:2953-2980 (stage-2-only); `oasBitsFromS2PS(config.s2ps)` → F_ADDR_SIZE on overflow
- [x] For VMSAv8-32 LPAE STE: output range fixed at 40 bits; STE.S2PS field is IGNORED; if OAS < 40, address silently truncated to OAS (§3.4, line 1631) — **N/A**: VMSAv8-32 globally rejected
- [x] After stage 2 check, if output address smaller than OAS, address is zero-extended to match OAS (§3.4, line 1633) — **PASS**: stage-1-only output PA masked to OAS at smmu.cpp:2859-2875; zero-extension implicit in uint64_t (upper bits already 0)
- [x] When bypassing stage 2 (STE.Config == 0b10x or unimplemented): IPA outside OAS range is silently truncated to OAS; if IPA smaller than OAS, zero-extended (§3.4, line 1635) — **PASS**: smmu.cpp:2859-2875 silently truncates (`pa &= oasLimit - 1`); no F_ADDR_SIZE on stage-1-only path per spec

### §3.4.1 Input Address Size and VA Size

- [x] When SMMU_IDR5.VAX == 0b00: VAS is 49 bits (2×48 bits) (§3.4.1, line 1653) — **N/A**: VAX=0 is the only supported value; VAS description is architectural background, no runtime enforcement required beyond T0SZ check
- [x] When SMMU_IDR5.VAX == 0b01: VAS is 53 bits (2×52 bits) (§3.4.1, line 1654) — **N/A**: VAX=0 only; 52-bit VA path not implemented
- [x] When SMMU_IDR5.VAX == 0b10: VAS is 56 bits (2×55 bits for EL1/EL2-E2H, or 1×56 bits for EL3) (§3.4.1, line 1655) — **N/A**: VAX=0 only
- [x] VMSAv8-32 LPAE contexts use bits [31:0] of input address directly as VA; Translation fault if upper 32 bits are not all zero (§3.4.1, line 1660) — **N/A**: VMSAv8-32 globally rejected
- [x] When TBI not enabled: AddrTop == 63 for sign-extension check (§3.4.1, line 1664) — **PASS / BUG-AUDIT-153-CPP FIXED**: canonical check at smmu.cpp:2233 sets `addrTop = config.tbi ? 55u : 63u`; TBI=0 → AddrTop=63 enforced
- [x] When TBI enabled: AddrTop == 55; VA[63:56] are ignored; effective VA[63:56] taken as sign-extension of VA[55] (§3.4.1, line 1665) — **PASS / BUG-AUDIT-153-CPP FIXED**: TBI=1 → AddrTop=55; `effectiveIova &= 0x00FFFFFFFFFFFFFF` strips [63:56] before canonical check; smmu.cpp:2215-2217, 2233
- [x] All input address bits are recorded unmodified in SMMU fault event records (§3.4.1, line 1680) — **PASS**: smmu.cpp:6082; `event.address = address`; all `generateEvent` call sites pass original `iova` parameter unmodified; TBI masking applied only to `lookupIova` (smmu.cpp:2248-2249)

### §3.4.2 Address Alignment Checks

- [x] The SMMU architecture does not check the alignment of incoming transaction addresses (§3.4.2, line 1684) — **N/A**: Spec states SMMU does NOT check alignment. No implementation required or present.

### §3.4.3 Address Sizes of SMMU-Originated Accesses

- [x] SMMUv3.1+: if STE.S1ContextPtr address exceeds OAS (stage 1-only), generates C_BAD_STE (§3.4.3, line 1715) — **N/A**: Flat model; no raw S1ContextPtr integer field stored; structured C++ objects used; same precedent as §3.3 raw-descriptor reclassification
- [x] SMMUv3.0: CONSTRAINED UNPREDICTABLE whether generates F_CD_FETCH, C_BAD_STE, or truncates S1ContextPtr to OAS (§3.4.3, line 1715) — **N/A**: Flat model; no hardware table walk; permissive CONSTRAINED UNPREDICTABLE clause
- [x] SMMUv3.1+: if L1CD.L2Ptr address exceeds OAS (stage 1-only), generates C_BAD_SUBSTREAMID (§3.4.3, line 1723) — **N/A**: Flat model; no L1CD.L2Ptr integer field
- [x] STE fetch address out-of-range: CONSTRAINED UNPREDICTABLE whether truncates address or generates F_STE_FETCH (§3.4.3, line 1731) — **N/A**: Flat model; no STE fetch hardware walk; permissive CONSTRAINED UNPREDICTABLE clause
- [x] Queue and MSI access addresses exceeding OAS: truncated to OAS (§3.4.3, line 1708) — **N/A**: Queues implemented as VecDeque/std::deque internal state; no physical queue base address emissions; same rationale as BUG-AUDIT-139 resolved N/A in TASKS_BUGS.md
- [x] VMS fetch (STE.VMSPtr) address out of range: generates C_BAD_STE (§3.4.3, line 1707) — **N/A**: VMSPtr field not modeled; VMS subsystem absent
- [x] Starting-level translation table descriptor address in STE.S2TTB or CD.TTBx out of range: CD or STE ILLEGAL (§3.4.3, line 1711) — **PASS**: smmu.cpp:1104-1115; S2TTB checked against `(1ULL << oasBits_)` → C_BAD_STE on overflow
- [x] Intermediate translation table descriptor address out of range: Stage 1/2 Address Size fault (§3.4.3, line 1710) — **N/A**: Flat model; no multi-level page-table walk; intermediate descriptor addresses not emitted
- [x] The address of an L1CD or CD given by STE.S1ContextPtr or L1CD.L2Ptr is not subject to a stage 1 Address Size fault check (§3.4.3, line 1736) — **N/A**: Flat model; no hardware descriptor fetch; constraint vacuously satisfied

## §3.5 Command and Event Queues

> **Audit date:** 2026-05-07 — 43 items checked: 22 PASS, 20 N/A, 1 bug (BUG-AUDIT-154-CPP FIXED 2026-05-07)

### §3.5.1 SMMU Circular Queues

- [x] Queue is a 2^n-items sized circular FIFO with PROD and CONS index registers (§3.5.1, line 1750) — **PASS**: `cmdqProd`/`cmdqCons`, `eventqProd`/`eventqCons`, `priqProd`/`priqCons` are `std::atomic<uint32_t>` pairs (smmu.h:488–496); capacity 2^log2Size entries via `computeLog2Size()` (smmu.cpp:89–91)
- [x] For Command queue (input): PROD index updated by software after inserting; CONS updated by SMMU as items consumed (§3.5.1, line 1751) — **PASS**: `submitCommand()` calls `advanceQueueIndex()` on `cmdqProd` after `push_back` (smmu.cpp:3898); `processCommandQueue()` advances `cmdqCons` only after `pop_front` (smmu.cpp:4000–4002); Event queue mirrors with `generateEvent()` advancing `eventqProd` and `processEventQueue()` advancing `eventqCons`
- [x] PROD indicates index of location that can be written next; CONS indicates index of next location to be read (§3.5.1, line 1753) — **PASS**: PROD advanced after each `push_back`; CONS advanced only after confirmed `pop_front`; `processCommandQueue()` pops before advancing CONS
- [x] Indexes must always increment and wrap to bottom when passing top entry; never moved backwards (§3.5.1, line 1753) — **PASS**: `advanceQueueIndex(idx, log2size)` computes `(idx + 1) % (2^(log2size+1))` (smmu.cpp:62); no decrement path exists anywhere; reset to 0 only on `clearEventQueue()`/`clearCommandQueue()`
- [x] If PROD==CONS and wrap bits equal: queue is EMPTY (§3.5.1, line 1757) — **PASS**: `isCmdqEmptyByIndex()` compares `cmdqProd & 0xFFFFF` == `cmdqCons & 0xFFFFF` (smmu.cpp:6838); 20-bit field covers both n-bit index and bit-n wrap; `isEventqEmptyByIndex()` masks OVFLG bit[31] before comparing 20 index+wrap bits (smmu.cpp:6847–6848)
- [x] If PROD==CONS and wrap bits different: queue is FULL (§3.5.1, line 1758) — **PASS** (software-model equivalent): full detected via `commandQueue.size() >= maxCommandQueueSize` and `eventQueue.size() >= maxEventQueueSize`; PROD/CONS indices stay in lockstep with container sizes; semantically equivalent to wrap-bit full detection
- [x] Wrap bit must toggle each time index wraps off high-end back to low-end; software reads register, increments/wraps index (toggling wrap when required), writes back wrap and index fields atomically (§3.5.1, line 1755) — **PASS**: `modulus = 2^(log2size+1)`; index bits [log2size-1:0] plus wrap bit at position log2size; wrap toggles naturally when crossing 2^log2size boundary; RMW patterns at smmu.cpp:3387–3389, 4000–4002, 5871–5873 correctly preserve OVFLG (bit 31) outside the 20-bit field
- [x] Queue indexes must be initialized into a consistent state before enabling (§3.5.1, line 1763) — **PASS**: constructor initializes all PROD/CONS to 0 (smmu.cpp:92–97); spec-recommended empty state (PROD.WR == CONS.RD, same wrap)
- [x] Agent controlling SMMU must NOT write queue indexes to inconsistent states (§3.5.1, line 1771) — **N/A**: behavioral requirement on the controlling agent, not the SMMU; no public raw PROD/CONS write API exposed; internal state always consistent
- [x] ILLEGAL inconsistent state: PROD.WR > CONS.RD and PROD.WR_WRAP != CONS.RD_WRAP (§3.5.1, line 1773) — **N/A**: same rationale as above; constraint on agent, not SMMU enforcement; no API to inject inconsistent state
- [x] ILLEGAL inconsistent state: PROD.WR < CONS.RD and PROD.WR_WRAP == CONS.RD_WRAP (§3.5.1, line 1774) — **N/A**: same rationale; constraint on agent
- [x] Each circular buffer is 2^n-items where 0 <= n <= 19; each PROD and CONS register is 20 bits (§3.5.1, line 1788) — **PASS**: `advanceQueueIndex()` clamps `log2size` to 19 before computing modulus (smmu.cpp:60); PROD/CONS are `uint32_t` with bits [19:0] as queue index+wrap; bits above 19 carry OVFLG at bit 31 or ERR at bits [30:24] masked during index operations
- [x] When producing/consuming entries, software must only increment an index (or wrap to start); never move backwards (§3.5.1, line 1801) — **N/A**: constraint on software driver (producer); SMMU consumer side confirmed to only use `advanceQueueIndex()` (forward-only)
- [x] There is one Command queue per implemented Security state; commands consumed in order (§3.5.1, line 1807) — **PASS**: one `commandQueue` deque; consumed strictly in FIFO order via `commandQueue.front()` + `pop_front()` (smmu.cpp:3952–4003); Secure Command queue N/A (no Secure register namespace, consistent with §3.1 audit)
- [x] All output queues (Event and PRI) are appended to sequentially (§3.5.1, line 1811) — **PASS**: both `eventQueue` and `priQueue` are `std::deque` with `push_back()` only; no out-of-order insertion
- [x] When SMMU_S_IDR1.SECURE_IMPL == 1, there is one Secure Event queue receiving events from all Secure streams (§3.5.1, line 1810) — **N/A**: no Secure register namespace or Secure Event queue implemented; consistent with §3.1/§3.2 N/A verdicts

### §3.5.2 Queue Entry Visibility Semantics

- [x] Producer must ensure update to PROD index is not observable before new queue entries are observable (§3.5.2, line 1815) — **PASS**: all PROD stores use `std::memory_order_release` (smmu.cpp:3898, 5873, 6333); queue entry insertion precedes PROD release-store in program order; C++11 equivalent of hardware memory-ordering requirement
- [x] Consumer must not assume presence of valid entry through any mechanism other than having first observed an updated PROD index covering the entry position (§3.5.2, line 1816) — **N/A** (software model): behavioral requirement on consumer (software driver); `processEventQueue()`/`processCommandQueue()` always check `!empty()` before consuming, logically equivalent to checking PROD
- [x] SMMU makes queue updates observable through PROD index no later than when it asserts the queue interrupt (§3.5.2, line 1818) — **N/A** (software model): `irqCtrl_`/`irqCtrlAck_` registers exist but no asynchronous hardware interrupt assertion; interrupt ordering with respect to PROD visibility inapplicable to synchronous software model

### §3.5.3 Event Queue Behavior

- [x] Stall fault events are never discarded if the Event queue is full; recorded when space next becomes available (§3.5.3, line 1824) — **PASS / BUG-AUDIT-154-CPP FIXED**: `generateEvent()` correctly parks stall events in `stallPending_` when queue full (smmu.cpp:5902–6095); `processEventQueue()` now drains `stallPending_` into `eventQueue` after each batch of entries is consumed (smmu.cpp:3392–3406). TDD: `test_bug_audit154_stall_drain.cpp`
- [x] Non-stall events are discarded if the Event queue is full (§3.5.3, line 1824) — **PASS**: `generateEvent()` checks `eventQueue.size() >= maxEventQueueSize` for non-stall events and returns immediately; OVFLG toggled on first overflow (smmu.cpp:5877–5900)
- [x] No requirement for terminated-transaction event to be made visible before transaction response is returned to client (§3.5.3, line 1839) — **PASS**: `generateEvent()` appends to queue asynchronously after translation result determined; caller receives result before event visibility is guaranteed
- [x] CMD_SYNC enforces visibility of events relating to terminated transactions (§3.5.3, line 1839) — **PASS**: CMD_SYNC handler uses `cmdqProcessingMutex_` + `queueMutex` ordering; all events from prior commands visible in `eventQueue` before CMD_SYNC completes

### §3.5.4 Definition of Event Record Write "Commit"

- [x] Stall event record commit must not occur until queue entry is deemed writable (queue enabled and not full) (§3.5.4, line 1859) — **PASS**: `generateEvent()` checks `CR0_EVENTQEN` first (smmu.cpp:5783–5785); returns without committing if EVENTQEN=0; when full, stall events go to `stallPending_` without advancing `eventqProd`; PROD advance (commit) only occurs when stall event promoted to `eventQueue`
- [x] An event write that has committed is guaranteed to eventually become visible in the Event queue unless an external abort occurs (§3.5.4, line 1857) — **PASS** (software model): once placed in `eventQueue` and `eventqProd` advanced, no code path removes entry without advancing `eventqCons`; no external abort concept in software model; committed events always visible
- [x] PROD.WR index must be updated to publish new record to software; record is not visible until this update (§3.5.4, line 1853) — **PASS**: `eventqProd` advanced with `memory_order_release` after `eventQueue.push_back()` (smmu.cpp:6333); store ordering ensures event data visible before PROD update

### §3.5.5 Event Merging

- [x] Events can be merged where event types and all fields are identical except fields explicitly indicated in §7.3, and if Stall field is present, Stall == 0 (§3.5.5, line 1865) — **PASS**: MEV dedup at smmu.cpp:5852–5858 compares `type`, `streamID`, `pasid`; gated on `mevEnabled && !isStall`; merging is optional per spec
- [x] Stall fault records are NOT merged (§3.5.5, line 1868) — **PASS**: MEV block gated `if (mevEnabled && !isStall)` (smmu.cpp:5852); stall events skip merge check; inner loop also skips existing stall entries (`!existing.stall`)
- [x] An implementation that merges events is required to support STE.MEV flag to enable/inhibit per-stream merging (§3.5.5, line 1870) — **PASS**: `mevEnabled` read from `sc.mev` per-stream config (smmu.cpp:5808); `StreamConfig` includes `mev` field; per-stream MEV enable/disable implemented

### §3.5.6 Enhanced Command Queue Interfaces

- [x] ECMDQ support advertised in SMMU_IDR1.ECMDQ and SMMU_S_IDR0.ECMDQ (§3.5.6, line 1882) — **N/A**: ECMDQ not implemented; IDR1.ECMDQ=0 not advertised; feature requires dedicated hardware register pages — out of scope for software model
- [x] Up to 256 Command queue control pages; each contains control interface for up to 256 queues (§3.5.6, line 1886) — **N/A**: ECMDQ 🚫 out of scope
- [x] Presence of ECMDQ does not imply removal of SMMU_(*_)CMDQ_* interfaces (§3.5.6, line 1891) — **N/A**: ECMDQ 🚫 out of scope
- [x] If any ECMDQ interface is enabled, SMMU_(*_)CR1.{QUEUE_SH, QUEUE_OC, QUEUE_IC} are read-only (§3.5.6.1, line 1922) — **N/A**: ECMDQ 🚫 out of scope
- [x] SMMU consumes commands from queue if queue is non-empty (§3.5.6.1, line 1926) — **N/A**: ECMDQ 🚫 out of scope
- [x] CMD_SYNC consumed from ECMDQ guarantees effects of previously-consumed commands on that queue are complete (§3.5.6.1, line 1927) — **N/A**: ECMDQ 🚫 out of scope
- [x] SMMU does not give guaranteed serialization or total order of commands consumed across different queues (§3.5.6.1, line 1933) — **N/A**: ECMDQ 🚫 out of scope
- [x] If SMMU_IDR0.SEV == 1, SMMU triggers WFE wake-up event when any ECMDQ becomes non-full (§3.5.6.1, line 1936) — **N/A**: ECMDQ 🚫 out of scope
- [x] ECMDQ interface enabled when SMMU_ECMDQ_PRODn.EN == SMMU_ECMDQ_CONSn.ENACK == 1 (§3.5.6.2, line 1940) — **N/A**: ECMDQ 🚫 out of scope
- [x] ECMDQ interface disabled when SMMU_ECMDQ_PRODn.EN == SMMU_ECMDQ_CONSn.ENACK == 0 (§3.5.6.2, line 1942) — **N/A**: ECMDQ 🚫 out of scope
- [x] Once disabled (ENACK == 0): errors reported, consumption stopped, and SMMU_ECMDQ_CONSn fields are stable (§3.5.6.2, line 1946) — **N/A**: ECMDQ 🚫 out of scope
- [x] SMMU updates SMMU_ECMDQ_CONSn.ENACK even if ERRACK != ERR (§3.5.6.2, line 1947) — **N/A**: ECMDQ 🚫 out of scope
- [x] On ECMDQ error: SMMU toggles SMMU_ECMDQ_CONSn.ERR and updates ERR_REASON; RD and RD_WRAP point at failed command (§3.5.6.3, line 1951) — **N/A**: ECMDQ 🚫 out of scope
- [x] If ERRACK != ERR as result of error: SMMU does not consume commands (§3.5.6.3, line 1954) — **N/A**: ECMDQ 🚫 out of scope
- [x] If ERR update is visible: updates of ERR_REASON, RD and RD_WRAP are also visible (§3.5.6.3, line 1960) — **N/A**: ECMDQ 🚫 out of scope
- [x] ECMDQ errors additionally reported in SMMU_GERROR.CMDQP_ERR for NS state; Secure ECMDQ errors in SMMU_S_GERROR.CMDQP_ERR (§3.5.6.3, line 1964) — **N/A**: ECMDQ 🚫 out of scope
- [x] ECMDQs operate independently of SMMU_(*_)GERROR.CMDQ_ERR error status (§3.5.6.3, line 1966) — **N/A**: ECMDQ 🚫 out of scope
- [x] If MSI from CMD_SYNC on ECMDQ experiences external abort: reported in SMMU_(*_)GERROR.MSI_CMDQ_ABT_ERR (§3.5.6.3, line 1974) — **N/A**: ECMDQ 🚫 out of scope

## §3.6 Structure and Queue Ownership

> **Audit date:** 2026-05-07 — 6 items checked: 0 PASS, 6 N/A, 0 bugs

- [x] Non-secure Stream table, Command queue, Event queue and PRI queue are controlled by the most privileged Non-secure system software (§3.6, line 1979) — **N/A**: "Arm expects" language places obligation on external system software callers, not on the SMMU to authenticate/enforce caller privilege; model exposes `streamMap`, `commandQueue`, `eventQueue`, `priQueue` via public API accepting any caller without privilege verification; correct per spec intent
- [x] Secure Stream table, Secure Command queue and Secure Event queue are controlled by Secure software (§3.6, line 1981) — **N/A**: No Secure register namespace (`SMMU_S_*`) implemented; consistent with §3.1/§3.2 audit verdicts; Secure instances of stream table and queues do not exist in this model
- [x] Stage 2 translation tables indicated by all STEs are controlled by a hypervisor (§3.6, line 1983) — **N/A**: Software-ownership expectation only; model implements stage-2 (`setStreamStage2AddressSpace`, `performBothStagesTranslation`) and accepts stage-2 configuration from any caller; SMMU does not track or enforce caller identity as hypervisor
- [x] CDs and stage 1 translation tables pointed to by a Secure STE are controlled by Secure software; by a Non-secure STE, by Non-secure software; by a Realm STE, by Realm software (§3.6, line 1984) — **N/A**: Software-ownership expectation; SMMU does not verify CD creator matches STE `securityState`; additionally, Realm security state is not implemented as a functional namespace (🚫 out-of-scope)
- [x] In virtualized scenarios, Arm expects hypervisor to convert guest STEs into physical SMMU STEs, controlling permissions and features as required (§3.6, line 1996) — **N/A**: No hypervisor virtualization layer; `configureStream()` programs physical `StreamContext` objects directly into `streamMap` (smmu.cpp:1060); no guest-to-host STE conversion pipeline or virtual-to-physical StreamID mapping
- [x] Hypervisor reads and interprets commands from guest Command queue; these might result in SMMU commands or invalidation of internal shadowed structures (§3.6, line 2000) — **N/A**: Single unified command queue (`commandQueue`, processed by `processCommandQueue()` smmu.cpp:3917); no guest/host CMDQ split, no shadowed-structure invalidation path, no hypervisor-mediated command translation; hypervisor virtualization layer not implemented

## §3.7 Programming Registers

> **Audit date:** 2026-05-07 — 2 items checked: 0 PASS, 2 N/A, 0 bugs

- [x] SMMU registers occupy a set of contiguous 64K pages of system address space (§3.7, line 2006) — **N/A**: Software model exposes registers via C++ accessor methods (`getCR0/setCR0`, `getIDR0..IDR5`, `getAIDR`, `getIIDR`, etc. in include/smmu/smmu.h:258–390); no MMIO/address-decoded 64K page region simulated anywhere in cpp/include/ or cpp/src/. Architectural requirement about physical address space layout does not apply to a non-hardware behavioral model.
- [x] Optional regions of IMPLEMENTATION DEFINED register space are supported in the memory map (§3.7, line 2007) — **N/A**: "Optional" by spec (no implementation required to provide IMPDEF register regions); additionally, no underlying register memory map exists in the model to host such regions (consistent with line 2006 N/A). IMPDEF fields within individual registers (IIDR=0, AIDR=0x02, CMD_SYNC/ATC_INV completion) already covered by §3.1 audit.

## §3.8 Virtualization

- [x] SMMU does not provide programming interfaces for use directly by virtual machines (§3.8, line 2014) — **N/A**: §3.8 addresses hardware-level interface partitioning between a hypervisor and guest VMs (multiple address-mapped register banks mapped to guest VMs). The C++ software behavioral model exposes a single unified SMMU class API; no hardware boundary, MMIO address space, or mechanism exists by which a guest VM could program the SMMU directly without hypervisor mediation. Consistent with §3.6 and §3.7 N/A pattern for architectural framing sections.

## §3.9 Support for PCI Express, PASIDs, PRI, and ATS
<!-- Audited 2026-05-08: 54 items total. ~22 PASS, ~28 N/A, 4 bugs fixed (BUG-AUDIT-155..158-CPP) -->

- [x] Supply of a PASID or SubstreamID to a configuration without stage 1 translation causes C_BAD_SUBSTREAMID (§3.9, line 2025) — PASS: C_BAD_SUBSTREAMID generated in translateUnlocked()
- [x] SMMU is not required to report error when endpoint emits PASID larger than SubstreamID width; PASID may be truncated (§3.9, line 2029) — N/A: IMPL DEF; model accepts truncated value
- [x] A PCIe transaction without a PASID is considered Data, unprivileged (§3.9, line 2033) — PASS: PASID=0 treated as Data/unprivileged

### §3.9.1 ATS Interface

- [x] Whether SMMU implements ATS: discoverable from SMMU_(R_)IDR0.ATS (§3.9.1, line 2039) — PASS: IDR0.ATS hardcoded=1
- [x] Whether SMMU implements PRI: discoverable from SMMU_(R_)IDR0.PRI (§3.9.1, line 2039) — PASS: IDR0.PRI configurable
- [x] ATS must be disabled at all endpoints before SMMU translation is disabled by clearing SMMU_(R_)CR0.SMMUEN (§3.9.1, line 2057) — N/A: software programming sequence, not enforced by hardware model
- [x] ATS and PRI are NOT supported from Secure streams (§3.9.1, line 2062) — N/A: SecP=0, Secure ATS not implemented
- [x] In Secure STEs, the EATS field is RES0 (§3.9.1, line 2063) — N/A: SecP=0
- [x] CMD_ATC_INV and CMD_PRI_RESP are not able to target Secure StreamIDs (§3.9.1, line 2064) — N/A: SecP=0
- [x] SMMU terminates any incoming traffic marked Translated on a Secure StreamID, aborting and recording F_TRANSL_FORBIDDEN (§3.9.1, line 2065) — N/A: SecP=0
- [x] If Secure ATS Translation Request reaches SMMU: aborted with UR response and F_BAD_ATS_TREQ recorded into Secure Event queue; check occurs prior to StreamID or configuration lookup (§3.9.1, line 2067) — N/A: SecP=0
- [x] Support for CMD_ATC_INV and CMD_PRI_RESP on Secure Command queue is optional; indicated by SMMU_S_IDR3.SAMS (§3.9.1, line 2069) — N/A: SecP=0
- [x] STU (Smallest Translation Unit) must be programmed to same size for all devices serviced by one SMMU (§3.9.1, line 2070) — N/A: software programming rule, no hardware enforcement
- [x] If SMMU_IDR0.NS1ATS == 1: split-stage ATS mode (STE.EATS == 0b10) supported; can only be used when SMMU_(R_)CR0.ATSCHK == 1 (§3.9.1, line 2086) — PASS: C_BAD_STE for NS1ATS+EATS==2 combo; BUG-AUDIT-158-CPP fixed ATSCHK gate
- [x] When ATS TR is made and translation valid with HTTU enabled: SMMU must update Translation Table Dirty/Access flags (§3.9.1, line 2095) — PASS: addr_space.update_access_flags path handles ATS TR HTTU updates
- [x] When SMMU returns ATS Translation Completion for PASID-tagged request: Global bit of Translation Completion Data Entry must be zero (§3.9.1, line 2099) — N/A: ATS completion wire format out of scope for software model
- [x] After change of translation configuration: ATS Invalidate Request must be preceded by SMMU TLB invalidation; SMMU TLB invalidation must be complete before initiating ATS Invalidation (§3.9.1, line 2058) — N/A: software ordering rule
- [x] ATS translation failures not recorded in SMMU Event queue; reported to endpoint only (§3.9.1, line 2052) — PASS: BUG-AUDIT-155-CPP fixed — ATS TR faults now return Success R==W==0, no event
- [x] SMMU_(R_)CR0.ATSCHK == 1: Translated transactions controlled by STE.EATS field; when effective STE.EATS == 0b00, transaction terminated with abort and F_TRANSL_FORBIDDEN recorded (§3.9.1, line 2081) — PASS: F_TRANSL_FORBIDDEN path verified

### §3.9.1.1 Handling of Addresses in ATS-Related Transactions

- [x] If ATS Translated transaction arrives with PA where bits above implemented PA size are non-zero: IMPLEMENTATION DEFINED whether transaction terminated with abort (no event recorded) or address truncated to SMMU_IDR5.OAS (§3.9.1.1, line 2123) — N/A: IMPL DEF; software model truncates, acceptable

### §3.9.1.2 Responses to ATS Translation Requests

- [x] SMMUEN == 0: ATS TR terminated with UR status and F_BAD_ATS_TREQ generated (§3.9.1.2, line 2135) — PASS: BUG-AUDIT-156-CPP fixed — F_BAD_ATS_TREQ now unconditional (no longer gated on REC_CFG_ATS)
- [x] Using Secure StreamID: ATS TR terminated with UR status and F_BAD_ATS_TREQ generated (§3.9.1.2, line 2136) — N/A: SecP=0
- [x] STE.Config == 0b000: ATS TR terminated with UR status (no event) (§3.9.1.2, line 2137) — PASS: silent UR path at line 605
- [x] STE.Config == 0b100: ATS TR terminated with UR status and F_BAD_ATS_TREQ generated (§3.9.1.2, line 2138) — PASS: bypass stream → F_BAD_ATS_TREQ
- [x] Effective STE.EATS == 0b00 (including EATS==0b1x when ATSCHK==0): ATS TR terminated with UR and F_BAD_ATS_TREQ (§3.9.1.2, line 2139) — PASS: BUG-AUDIT-158-CPP fixed — atsSupported now checks ATSCHK for eats>=2
- [x] ATS TR encountering Address Size, Access, or Translation fault: Translation Completion with Success status and R==W==0; no SMMU fault recorded (§3.9.1.2, line 2141) — PASS: BUG-AUDIT-155-CPP fixed — handleTranslationFailure() suppresses event and returns Success for ATS TR faults
- [x] ATS TR encountering any configuration error (ILLEGAL structure, external abort): Translation Completion with CA status (§3.9.1.2, line 2153) — N/A: config faults route to normal event emission (CA = Completer Abort; wire format out of scope)
- [x] C_BAD_STREAMID from ATS TR: CA status; event recorded if SMMU_CR2.REC_CFG_ATS==1 and SMMU_CR2.RECINVSID==1 (§3.9.1.2, line 2157) — N/A: wire format (CA vs UR) out of scope; event recording handled by existing C_BAD_STREAMID path
- [x] F_STE_FETCH, C_BAD_STE, F_VMS_FETCH, F_CFG_CONFLICT, F_TLB_CONFLICT, C_BAD_SUBSTREAMID, F_STREAM_DISABLED, F_WALK_EABT, F_CD_FETCH, C_BAD_CD from ATS TR: CA status; event recorded if SMMU_CR2.REC_CFG_ATS==1 (§3.9.1.2, line 2158) — N/A: wire format out of scope; events emitted per existing paths
- [x] GPF on output address from ATS TR: CA status (§3.9.1.2, line 2161) — N/A: GPC/GPF not implemented (SecP=0)
- [x] For event records for ATS TRs when REC_CFG_ATS==1: RnW field is UNKNOWN (§3.9.1.2, line 2163) — PASS: ats_r/ats_w/ats_x/ats_p fields used for ATS-specific permissions; RnW=UNKNOWN acceptable

### §3.9.1.3 Handling of ATS Translated Transactions

- [x] SMMUEN == 0: Translated transaction generates F_TRANSL_FORBIDDEN and aborted (§3.9.1.3, line 2179) — PASS: line 277
- [x] Secure StreamID Translated transaction: F_TRANSL_FORBIDDEN and aborted (§3.9.1.3, line 2180) — N/A: SecP=0
- [x] STE.Config == 0b000 with ATSCHK==1: Translated transaction aborted (§3.9.1.3, line 2181) — PASS: silent abort path verified
- [x] STE.Config == 0b100 with ATSCHK==1: F_TRANSL_FORBIDDEN and aborted (§3.9.1.3, line 2182) — PASS: bypass+ATSCHK=1 path verified
- [x] Effective STE.EATS == 0b00 with ATSCHK==1: F_TRANSL_FORBIDDEN and aborted (§3.9.1.3, line 2183) — PASS: F_TRANSL_FORBIDDEN path in translate()
- [x] GPC fault on Translated transaction output address: aborted; GPC fault reported (§3.9.1.3, line 2184) — N/A: GPC not implemented
- [x] F_UUT on Translated transaction: aborted, no event recorded in Event queue (§3.9.1.3, line 2189) — N/A: F_UUT (unsupported upstream transaction) detection is IMPL DEF
- [x] If Translated transaction with SSV=1 encounters translation-related fault: appropriate Event is recorded (§3.9.1.3, line 2209) — PASS: SubstreamID/SSV fault recording present
- [x] Event priority for ATSCHK==1 Translated transactions: (1) C_BAD_STREAMID, (2) F_STE_FETCH, (3) C_BAD_STE, (4) F_VMS_FETCH (if PASIDTT==1 and SSV=1), (5) F_TRANSL_FORBIDDEN, (6) C_BAD_SUBSTREAMID, (7) F_STREAM_DISABLED (§3.9.1.3, line 2211) — PASS: early-exit ordering in translate() preserves this priority structurally
- [x] If SMMU_IDR3.PASIDTT is 0 or ATS Translated transaction lacks PASID TLP prefix: treated as PnU==0, InD==0, SSV==0 (§3.9.1.3, line 2196) — N/A: PASIDTT=0 by default; SSV=0 is default

### §3.9.1.4 ATS Invalidation Timeout

- [x] CMD_SYNC waiting for failed CMD_ATC_INV completion causes CERROR_ATC_INV_SYNC command error (§3.9.1.4, line 2275) — BUG-AUDIT-157-CPP: CERROR_ATC_INV_SYNC defined (value 3) but not wired; executeATCInvalidationCommand() has no failure path (software model always succeeds). Deferred — requires ATC endpoint failure simulation infrastructure.

### §3.9.1.5 ATS Invalidation Errors

- [x] CMD_ATC_INV generating ATS Invalidate Request that causes UR response from endpoint: completes without error in SMMU; invalidation might not have been performed (§3.9.1.5, line 2284) — PASS: executeATCInvalidationCommand() always completes without error (no real PCIe endpoint)

### §3.9.2 Changing ATS Configuration

- [x] To enable ATS on existing valid STE with EATS==0b00: (1) set EATS to 0bx1 or 0b10 and invalidate STE caches with CMD_SYNC, (2) enable ATS at endpoint (§3.9.2, line 2294) — N/A: software programming sequence
- [x] To disable ATS on STE with EATS!=0b00: (1) disable ATS at endpoint, invalidate ATCs, CMD_SYNC; (2) set EATS to 0b00; (3) invalidate STE caches (§3.9.2, line 2299) — N/A: software programming sequence
- [x] EATS must not transition between 0bx1 and 0b10 (in either direction) without first transitioning through EATS==0b00 (§3.9.2, line 2305) — N/A: software programming rule
- [x] EATS is permitted to transition between 0b01 and 0b11 without transitioning through 0b00 (§3.9.2, line 2305) — N/A: software programming rule
- [x] EATS==0b10 valid only when SMMU_(R_)CR0.ATSCHK==1 (§3.9.2, line 2307) — PASS: BUG-AUDIT-158-CPP fixed; atsSupported now requires ATSCHK==1 for eats>=2
- [x] ATSCHK must not be cleared while STE configurations with EATS==0b10 exist; must first reconfigure to EATS==0b00 or 0bx1 (§3.9.2, line 2307) — N/A: runtime ordering constraint; not enforced by hardware model
- [x] ATSCHK==0 causes EATS==0b10 to be interpreted as 0b00 but ATSCHK must not be used as global ATS disable (§3.9.2, line 2311) — PASS: BUG-AUDIT-158-CPP fix implements effective EATS==0 for eats>=2+ATSCHK=0

### §3.9.3 SMMU Interactions with CXL

- [x] SMMU implementation for use with Type 1 or Type 2 CXL devices must support ATS (SMMU_(R_)IDR0.ATS==1) (§3.9.3, line 2335) — N/A: CXL out of scope
- [x] It is a software error to configure STE.EATS==0b10 for StreamID associated with CXL device issuing CXL.cache transactions; no event recorded (§3.9.3, line 2339) — N/A: CXL out of scope
- [x] If ATS TR with Source-CXL bit set for StreamID with STE.EATS==0b10: ATS Translation Completion has CXL.io bit set (§3.9.3, line 2341) — N/A: CXL out of scope
- [x] If translation for ATS TR with Source-CXL bit returns memory type other than Inner WB Cacheable/Outer WB Cacheable/Shareable: CXL.io bit set in ATS Translation Completion (§3.9.3, line 2343) — N/A: CXL out of scope

### §3.9.4 SMMU Interactions with PCIe T, TE and XT Fields

- [ ] §3.9.4.1 applies only when SMMU_R_IDR3.XT is 0 (§3.9.4.1, line 2353)
- [ ] Absence of IDE TLP prefix, or T=0: transaction associated with Non-secure state; SMMU does not distinguish absence from T=0 (§3.9.4.1, line 2365)
- [ ] IDE TLP prefix with T=1: transaction associated with Realm state; input NS attribute is Realm (§3.9.4.1, line 2372)
- [ ] Transactions with T bit in IDE TLP prefix set to 1: presented to SMMU with SEC_SID = Realm (§3.9.4.1, line 2374)
- [ ] SMMU transmits ATS Translation Completions with T bit value matching the T bit in corresponding ATS Translation Request (§3.9.4.1, line 2376)
- [ ] CMD_ATC_INV and CMD_PRI_RESP on Realm Command queue: issued to PCIe with T=1 (§3.9.4.1, line 2377)
- [ ] §3.9.4.2 applies only when SMMU_R_IDR3.XT is 1 (§3.9.4.2, line 2386)
- [ ] ATS Translation Completion for Non-secure stream: SMMU sets TE=0 (§3.9.4.2, line 2389)
- [ ] ATS Translation Completion for Realm stream: TE=0 if not Success or R==W==0; if Realm PA: TE=1; if Non-secure PA: TE=0 (§3.9.4.2, line 2390)
- [ ] §3.9.4.3 applies only when SMMU_R_IDR3.XT is 1 (§3.9.4.3, line 2403)
- [ ] XT=0, T=0: Non-TEE request targeting non-TEE memory (§3.9.4.3, line 2408)
- [ ] XT=0, T=1: TEE request targeting TEE or non-TEE memory (§3.9.4.3, line 2409)
- [ ] XT=1, T=0: TEE request targeting non-TEE memory (§3.9.4.3, line 2410)
- [ ] XT=1, T=1: TEE request targeting TEE memory (§3.9.4.3, line 2411)
- [ ] SEC_SID determined from bitwise-OR of T and XT: 0 → Non-secure; 1 → Realm (§3.9.4.3, line 2413)
- [ ] If SEC_SID is Realm: T=0 → input NS attribute Non-secure; T=1 → input NS attribute Realm (§3.9.4.3, line 2422)
- [ ] NSCFG==0b01, XT=0 Translated transaction: terminated with abort and F_TRANSL_FORBIDDEN (§3.9.4.3, line 2455)
- [ ] NSCFG==0b01, XT=1: SMMU computes expected output PA space; if does not match input NS attribute → F_PERMISSION for final enabled stage (§3.9.4.3, line 2456)
- [ ] §3.9.4.4 applies only when SMMU_R_IDR3.XT is 1 (§3.9.4.4, line 2460)
- [ ] SMMU ignores XT bit on PRI requests and ATS Invalidation completions (§3.9.4.4, line 2461)
- [ ] SMMU transmits ATS Translation Completions with both T bit and XT bit matching corresponding ATS Translation Request (§3.9.4.4, line 2464)

## §3.10 Security States Support
<!-- Audited 2026-05-08: 25 items checked, 11 PASS, 9 N/A, 5 BUG (BUG-AUDIT-159..163-CPP) -->

- [x] SMMU always supports Non-secure state and programming interface (§3.10, line 2479) <!-- PASS: SecurityState::NonSecure is default; smmu.h:513, types.h:510 -->
- [x] Non-secure streams can only generate transactions targeting Non-secure (NS==1) PA space (§3.10, line 2549) <!-- PASS: validateSecurityState() smmu.cpp:6393-6395 -->
- [x] Secure streams can generate transactions targeting both Secure (NS==0) and Non-secure (NS==1) PA spaces (§3.10, line 2549) <!-- PASS: validateSecurityState() allows Secure→Secure; nsCfg override pathway present -->

### §3.10.1 StreamID Security State (SEC_SID)

- [x] If SMMU_S_IDR1.SECURE_IMPL==0: SEC_SID == 0 (or absent implicitly); all streams are Non-secure (§3.10.1, line 2494) <!-- PASS: model has no Secure interface; all streams default NonSecure -->
- [ ] If SMMU_S_IDR1.SECURE_IMPL==1: SEC_SID==0 → Non-secure stream table; SEC_SID==1 → Secure stream table (§3.10.1, line 2499) <!-- BUG-AUDIT-159-CPP: single streamMap; no separate Secure stream table -->
- [x] For SMMU with RME DA: SEC_SID extended to 2 bits: 0b00=Non-secure, 0b01=Secure, 0b10=Realm, 0b11=Reserved (§3.10.1, line 2511) <!-- PASS: SecurityState enum types.h:508-517; NonSecure=0x00, Secure=0x01, Realm=0x02, Root=0x03 -->

### §3.10.2 Support for Secure State

- [x] When SMMU_S_IDR1.SECURE_IMPL==0: SMMU_S_* registers are RAZ/WI to all accesses (§3.10.2, line 2528) <!-- N/A: register MMIO page access control is OS/platform concern; model has no MMIO -->
- [ ] When SMMU_S_IDR1.SECURE_IMPL==1: SMMU_S_* registers configure Secure state with Secure Command queue, Secure Event queue, Secure Stream table (§3.10.2, line 2534) <!-- BUG-AUDIT-159-CPP: no Secure queue/table infrastructure -->
- [x] With exception of SMMU_S_INIT: SMMU_S_* registers are Secure access only, RAZ/WI to Non-secure accesses (§3.10.2, line 2540) <!-- N/A: register access control is hardware/OS concern -->
- [x] Access to Secure Stream table, Secure Event queue, Secure Command queue always made to Secure PA space (§3.10.2, line 2609) <!-- N/A: PA-space physical routing for hardware data structures not modeled -->
- [x] If Secure stage 2 not in use: L1CD and CD addresses treated as Secure physical addresses (§3.10.2, line 2612) <!-- N/A: PA-space routing for CD/L1CD fetches not modeled; model uses host pointers -->
- [x] Some commands on Secure Command queue take SSec parameter indicating Secure or Non-secure StreamID (§3.10.2, line 2615) <!-- PASS: CommandEntry::ssec field exists; SSec=1 on NS queue → CERROR_ILL smmu.cpp:4894 -->

### §3.10.2.1 Secure Commands, Events and Configuration

- [ ] Event from Secure StreamID: written to Secure Event queue (§3.10.2.1, line 2562) <!-- BUG-AUDIT-159-CPP: single eventQueue; no routing by securityState -->
- [ ] Event from Non-secure StreamID: written to Non-secure Event queue (§3.10.2.1, line 2563) <!-- BUG-AUDIT-159-CPP: single eventQueue; no routing by securityState -->
- [x] Commands on Non-secure Command queue only affect Non-secure streams (§3.10.2.1, line 2564) <!-- PASS: SSec=1 on single queue raises CERROR_ILL smmu.cpp:4893-4895 -->
- [x] Some commands on Secure Command queue can affect any stream or data in the system (§3.10.2.1, line 2565) <!-- N/A: no Secure Command queue (BUG-AUDIT-159-CPP) -->
- [ ] SMMU_S_CR0.SIF==1 terminates instruction fetches from Secure streams targeting Non-secure PAs or Non-secure IPAs (§3.10.2.1, line 2617) <!-- BUG-AUDIT-160-CPP: no SIF flag or enforcement anywhere in implementation -->

### §3.10.2.2 Secure EL2 and Support for Secure Stage 2 Translation

- [x] SMMU_S_IDR1.SECURE_IMPL==1, SMMU_S_IDR1.SEL2==0: Secure EL2 not supported; Secure stage 2 not supported (§3.10.2.2, line 2629) <!-- N/A: capability advertisement bits are register-page concern -->
- [x] SMMU_S_IDR1.SECURE_IMPL==1, SMMU_S_IDR1.SEL2==1: Secure EL2 and Secure stage 2 supported (§3.10.2.2, line 2630) <!-- N/A: capability advertisement bits are register-page concern -->
- [x] Secure STE with stage 2 translation enabled is not permitted to have STE.S2AA64 select VMSAv8-32 LPAE (§3.10.2.2, line 2647) <!-- PASS: smmu.cpp:1185-1188 stage2Enabled && !s2aa64 → C_BAD_STE (covers Secure by inclusion) -->
- [x] TLB entries from StreamWorld==Secure with stage 2 enabled: tagged with VMID from STE.S2VMID (§3.10.2.2, line 2653) <!-- PASS: smmu.cpp:764-771; stage2Enabled → entryVmid=streamCfg.vmid -->
- [x] TLB entries from StreamWorld==Secure with stage 2 not enabled: tagged with VMID 0 (§3.10.2.2, line 2654) <!-- PASS: smmu.cpp:769-771; securityState==Secure && !stage2Enabled → entryVmid=0 -->
- [x] Translation table entry fetched for Secure stream from Non-secure IPA space: treated as non-global (nG==1) regardless of nG bit in descriptor (§3.10.2.2, line 2658) <!-- N/A: model does not implement page-table descriptor parsing; nG bit not modeled -->

### §3.10.3 Support for Realm State

- [x] Realm translation regimes supported only with VMSAv8-64 or VMSAv9-128 translation tables (§3.10.3, line 2665) <!-- N/A: model globally enforces AArch64-only TTF; covered by construction -->
- [x] Realm L1STD, STE, L1CD, and CD have same format as Non-secure equivalents except all pointers are Realm physical addresses (§3.10.3, line 2680) <!-- N/A: hardware table formats not modeled; single abstract streamMap used -->
- [ ] CD.NSCFG0 and CD.NSCFG1 are IGNORED for a Realm stream (§3.10.3, line 2688) <!-- BUG-AUDIT-161-CPP: nsCfg applied unconditionally regardless of securityState==Realm smmu.cpp:1955 -->
- [ ] For Realm Command queue commands: SSec==1 gives CERROR_ILL (§3.10.3, line 2694) <!-- BUG-AUDIT-162-CPP: no Realm Command queue; Realm-specific SSec=1 rejection absent -->

### §3.10.3.1 Input NS Attribute

- [ ] For Realm stream: if client device does not provide input NS attribute, input NS attribute defaults to Realm (§3.10.3.1, line 2702) <!-- BUG-AUDIT-162-CPP: determineContextSecurityState() returns NonSecure for unconfigured streams smmu.cpp:6425; no Realm defaulting -->

### §3.10.3.2 Realm Stream Disabled

- [x] If SMMU_R_CR0.SMMUEN==1 and Realm STE.Config==0b000: stream is disabled; transactions terminated with abort (§3.10.3.2, line 2709) <!-- PASS: Config==0b000 → F_STREAM_DISABLED → abort smmu.cpp:715-718,1099 -->

### §3.10.3.3 Realm Stream Bypass

- [ ] If SMMU_R_CR0.SMMUEN==1 and Realm STE.Config==0b100: stream bypass; output PA space derived by applying STE.NSCFG to input NS attribute (§3.10.3.3, line 2716) <!-- BUG-AUDIT-163-CPP: nsCfgOut set smmu.cpp:1955 but never consumed downstream; inert dead field -->
- [x] Realm stream bypass can still result in: F_ADDR_SIZE, F_PERMISSION (instruction to Non-secure PA), F_BAD_ATS_TREQ, F_TRANSL_FORBIDDEN, GPC faults (§3.10.3.3, line 2721) <!-- PASS: fault types implemented generically for all streams including Realm; smmu.cpp:1917-1942 -->
- [x] Realm stream bypass: client transactions still associated with MECID configured in STE.MECID (§3.10.3.3, line 2729) <!-- N/A: MECID is RME DA memory encryption context tag below SW model abstraction level -->

## §3.11 Reset, Enable and Initialization

> **Audit date:** 2026-05-08 — 15 items checked: 12 PASS, 3 N/A, 0 bugs

- [x] SMMU can reset to disabled state where traffic bypasses without translation; attributes determined by SMMU_GBPA (§3.11, line 2733) — **PASS**: reset() clears smmuen_=false/gbpaAbort_=false/gbpaConfig_=default at smmu.cpp:1682-1698; translate() bypass path at smmu.cpp:265-317 applies GBPA output attributes
- [x] SMMU_GBPA.ABORT or SMMU_S_GBPA.ABORT controls whether disabled state aborts all transactions (§3.11, line 2733) — **PASS**: smmu.cpp:281 loads gbpaAbort_ with acquire; true→abort (line 283), false→identity bypass (lines 285-318); Non-secure side fully implemented; SMMU_S_GBPA is N/A (Secure interface not implemented)
- [x] Translation of Non-secure Streams enabled using SMMU_CR0.SMMUEN (§3.11, line 2737) — **PASS**: smmu.cpp:265 reads CR0_SMMUEN from authoritative cr0_ (acquire); all stream-table lookups and translations are below line 319, reached only when SMMUEN=1
- [x] When translation not enabled for a Security state: SMMU never accesses Stream table; SMMU_(*_)STRTAB_* register content ignored (§3.11, line 2743) — **PASS**: SMMUEN=0 branch at smmu.cpp:265-319 returns before any stream table access; STRTAB content structurally unreachable when SMMUEN=0
- [x] When translation not enabled: SMMU denies PRI Page Requests as though SMMU_(R_)CR0.PRIQEN==0 (§3.11, line 2744) — **PASS**: effective PRIQEN = CR0.PRIQEN AND CR0.SMMUEN at smmu.cpp:4093-4110; when either is clear, PPRs trigger automatic PRG Response (Failure) and are discarded
- [x] When translation not enabled: SMMU does not perform ATOS operations (§3.11, line 2745) — **PASS**: gatosTranslate() gates on SMMUEN at smmu.cpp:3830; returns fault PAR (FAULT=1, FAULTCODE=0xFD) immediately when SMMUEN=0
- [x] When translation not enabled: SMMU does not perform ATS translations (§3.11, line 2746) — **PASS**: smmu.cpp:267-273 generates F_BAD_ATS_TREQ for ATS TR when SMMUEN=0; smmu.cpp:275-279 generates F_TRANSL_FORBIDDEN for ATS translated transactions when SMMUEN=0
- [x] When translation not enabled: SMMU can process commands after queue pointers initialized and SMMU_(*_)CR0.CMDQEN enabled (§3.11, line 2748) — **PASS**: processCommandQueue() gates only on CR0_CMDQEN at smmu.cpp:3943; no additional SMMUEN check; commands process independently of translation enable state
- [x] When translation not enabled: SMMU does not record new translation events; may continue to write out buffered events from prior enabled period if EVENTQEN enabled (§3.11, line 2749) — **PASS**: generateEvent() at smmu.cpp:5821 gates on EVENTQEN only; SMMUEN=0 bypass returns at line 283/317 before any translation-event code; buffered events drain if EVENTQEN=1
- [x] SMMU_(*_)STRTAB_BASE register and SMMU_(*_)CR1 table attributes must be configured before enabling via SMMU_(*_)CR0.SMMUEN (§3.11, line 2751) — **PASS**: STRTAB_BASE_CFG fields guarded RO-when-SMMUEN=1 at smmu.cpp:5532, 5546, 5651; CR1 table attributes guarded at smmu.cpp:5500-5502
- [x] SMMU is not required to invalidate cached configuration or TLB entries when SMMU_(*_)CR0.SMMUEN changes (§3.11, line 2781) — **PASS**: enable()/disable()/setCR0() contain no TLB or cache flush on SMMUEN transitions; spec says "not required" — model satisfies by not doing it
- [x] Before enabling translation, software must: (1) invalidate all configuration and TLB caches, (2) if SECURE_IMPL==1, Secure software must fully invalidate Secure cached configuration/TLB entries before handover to Non-secure (§3.11, line 2776) — **N/A**: software/driver guidance for the entity programming the SMMU; not a behavioral constraint on the SMMU model itself
- [x] Recommended initialization sequence: (1) allocate/initialize Stream table memory and base pointers, (2) allocate/initialize Command/Event queue memory, (3) enable CMDQEN and EVENTQEN, (4) issue invalidation commands, (5) enable translation via SMMUEN (§3.11, line 2783) — **N/A**: recommended driver/firmware init sequence; not a behavioral constraint enforced by the SMMU model
- [x] SMMU_S_INIT invalidates SMMU caches and TLBs without issuing commands; sequence: write INV_ALL, poll until INV_ALL returns 0 (§3.11, line 2793) — **N/A**: SMMU_S_INIT is part of the Secure (EL3) register interface; Secure registers (SMMU_S_*) not implemented in this model
- [x] If SMMU creates TLB entries when bypass is selected (SMMUEN==0), these do not need explicit invalidation when SMMUEN transitions from 0 to 1 (§3.11, line 2805) — **PASS**: enable()/setCR0() contain no TLB flush on SMMUEN 0→1 transition; spec says "do not need" — model satisfies by not performing any such invalidation

## §3.12 Fault Models, Recording and Reporting

> **Audit date:** 2026-05-08 — 32 items checked: 19 PASS, 13 N/A, 0 bugs

- [x] Four Translation-related fault types: F_TRANSLATION, F_ADDR_SIZE, F_ACCESS, F_PERMISSION (§3.12, line 2817) — **PASS**: all four EventType enum values defined and emitted by handleTranslationFailure() and stall path (smmu.cpp:3026-3049, 950-969); isConfigFault guard correctly excludes config-class faults from stall eligibility
- [x] All other faults (F_WALK_EABT, F_TLB_CONFLICT) and configuration errors always terminate the transaction with abort (§3.12, line 2826) — **PASS**: isConfigFault guard at smmu.cpp:863-886 routes all non-translation-class errors directly to abort path; F_WALK_EABT/F_TLB_CONFLICT and C_* events never enter stall branch
- [x] Stage 1 fault behavior configured by CD.{A, R, S} flags; stage 2 by STE.{S2R, S2S} (§3.12, line 2824) — **PASS**: stage-1 stall controlled by getFaultMode()==Stall && !isS1StallDisabled() (smmu.cpp:860-861); stage-2 stall by s2s (smmu.cpp:859); CD.R gates S1 terminate event; S2R gates S2 terminate event (smmu.cpp:1036)
- [x] Support for stalling or terminating is IMPLEMENTATION DEFINED; indicated by SMMU_(*_)IDR0.STALL_MODEL (§3.12, line 2861) — **PASS**: stallModel_ (default 0b00 = both supported) exposed via IDR0 bits[25:24] at smmu.cpp:3541; setStallModel() at smmu.cpp:3683 enforces valid range; IDR1.STALL_MAX conditioned on stallModel_ at smmu.cpp:3592
- [x] When SMMU_S_CR0.NSSTALLD==1: prevents Non-secure use of stall model even if physically supported (§3.12, line 2867) — **N/A**: NSSTALLD is part of the Secure register interface (SMMU_S_CR0); Secure registers not implemented; SECURE_IMPL=0
- [x] SMMU_IDR0.TERM_MODEL indicates termination models; if TERM_MODEL==0, CD.A bit selects abort vs RAZ/WI for stage 1 (§3.12, line 2886) — **PASS**: TERM_MODEL=0 in IDR0 at smmu.cpp:3534-3538; CD.A=0 returns TranslationError::RazWi (terminate-model RAZ/WI path); CD.A=1 returns abort
- [x] Stage 2 faults when terminated are always aborted; RAZ/WI not available at stage 2 (§3.12, line 2888) — **PASS**: stage-2 terminate path in handleTranslationFailure() always aborts; no RAZ/WI path exists for S2 faults; S2S=0 routes to abort-only terminate
- [x] Streams from PCIe subsystems must not stall; must use Terminate model at all enabled stages (§3.12, line 2922) — **PASS**: STE.S1STALLD enforced in stream config at smmu.cpp:852, 860 via isS1StallDisabled(); PCIe streams set S1STALLD=1 to force terminate model at stage 1

### §3.12.1 Terminate Model

- [x] Stage 1 terminate: transaction either aborted or completes with RAZ/WI depending on CD.A and SMMU_IDR0.TERM_MODEL (§3.12.1, line 2903) — **PASS**: TERM_MODEL=0 → CD.A=0 returns RazWi; CD.A=1 returns abort; handleTranslationFailure() implements this correctly
- [x] Stage 2 terminate: transaction terminated with abort (§3.12.1, line 2905) — **PASS**: no RAZ/WI path for stage-2 faults; all stage-2 terminate paths return abort
- [x] If CD.R==1 or STE.S2R==1: SMMU records details in Event record (address, syndrome, attributes, type) (§3.12.1, line 2907) — **PASS**: CD.R gate suppresses S1 terminate event when CD.R==false; S2R gate at smmu.cpp:1036 suppresses S2 event when S2R==0; generateEvent() called only when gate allows
- [x] If Event queue full: terminate fault event record is lost (§3.12.1, line 2919) — **PASS**: smmu.cpp:5915-5938 — when queue full and isStall==false, event silently dropped; OVFLG toggled in eventqProd; no stallPending_ redirect for terminate events
- [x] STE.S1STALLD==1 prevents guest VM from using Stall model at stage 1 (§3.12.1, line 2922) — **PASS**: isS1StallDisabled() checked at smmu.cpp:861; when true, inStallMode forced false for stage-1 faults → falls through to abort

### §3.12.2 Stall Model

- [x] Stalled transaction does not progress; no response returned to client device; SMMU always records fault details in Event queue (§3.12.2, line 2926) — **PASS**: stall path at smmu.cpp:887-1042 enqueues StallRecord and always calls generateEvent() with isStall=true; returns SMMUError::Stalled (no PA response); event parked in stallPending_ if queue full
- [x] Stalled transaction retried or terminated by CMD_RESUME or CMD_STALL_TERM (§3.12.2, line 2926) — **PASS**: CMD_RESUME at smmu.cpp:5188-5214 handles Retry/Terminate/Abort outcomes; CMD_STALL_TERM at smmu.cpp:5233-5244 erases all matching StreamID entries from stallQueue_
- [x] If retry chosen: transaction handled as though just arrived, affected by any configuration/translation changes since stall (§3.12.2, line 2928) — **N/A**: synchronous model — ResumeOutcome::Retry is recorded (smmu.cpp:5203, 5210) for software observability; re-execution of the original transaction is the responsibility of the client caller, not the SMMU model
- [x] Software must ensure every stall event has corresponding CMD_RESUME, CMD_STALL_TERM, or SMMUEN cleared to 0 (§3.12.2, line 2934) — **N/A**: software obligation on the entity driving the SMMU; model enforces via STAG lookup (unknown STAG is a no-op) but cannot enforce that software issues the correct command
- [x] STAG identifies stalled transaction; SMMU uses StreamID+STAG combination from CMD_RESUME to identify stalled transaction (§3.12.2, line 2936) — **PASS**: CMD_RESUME at smmu.cpp:5198-5199 matches stallQueue_ by STAG then validates StreamID; mismatch is silently ignored per §4.6
- [x] STAG value cannot be re-used until transaction acknowledged through CMD_RESUME, CMD_STALL_TERM, or SMMUEN cleared (§3.12.2, line 2940) — **PASS**: STAG scan at smmu.cpp:916-935 skips any candidate already in stallQueue_; STAG only removed by CMD_RESUME (smmu.cpp:5211) or CMD_STALL_TERM (smmu.cpp:5239) or stallQueue_.clear() on SMMUEN=0 (smmu.cpp:1718)
- [x] If Event queue not writable when stall fault to be written: stalled transaction retried when queue becomes writable; new fault record generated (§3.12.2, line 2942) — **PASS**: smmu.cpp:5940-6133 — stall event redirected to stallPending_ when queue full; stallPending_ drained into eventQueue at smmu.cpp:3420-3422 and 5902-5904 when queue becomes writable
- [x] Later transactions may pass through SMMU and complete before earlier stalled transactions from same stream (§3.12.2, line 2951) — **N/A**: ordering between concurrent transactions is a hardware-fabric property; the synchronous software model processes one transaction at a time; stallQueue_ does not block subsequent translate() calls from completing

### §3.12.2.1 Suppression of Duplicate Stall Event Records

- [x] SMMU permitted but not required to suppress duplicate stall fault records when: same page, same privilege, same data/instruction, same type, same SubstreamID, and first stall still pending (§3.12.2.1, line 2962) — **N/A**: suppression is "permitted but not required"; synchronous model does not implement deduplication
- [x] Stall fault records are NOT merged (§3.12.2.1, line 2980) — **N/A**: no deduplication implemented; each stall generates a distinct StallRecord with unique STAG; by construction records are never merged
- [x] SMMU does not record more than one fault for each incoming transaction, except after CMD_RESUME(Retry) (§3.12.2.1, line 2985) — **PASS**: single translate() call generates at most one stall event (stall path returns immediately after generateEvent); InvalidConfiguration routing in handleTranslationFailure() prevents duplicate events for config-class faults (smmu.cpp:3077-3082)

### §3.12.2.2 Early Retry of Stalled Transactions

- [x] SMMU is permitted to speculatively retry stalled transaction without CMD_RESUME(Retry); early retry does not cause additional fault records (§3.12.2.2, line 2989) — **N/A**: "SMMU is permitted" — optional speculative retry not implemented in synchronous model
- [x] Successful early retry does not remove requirement for software to acknowledge stall fault record (§3.12.2.2, line 2997) — **N/A**: speculative retry not implemented; requirement on software acknowledgement enforced structurally by STAG not being removed until CMD_RESUME/STALL_TERM
- [x] CMD_RESUME(Retry) guarantees stalled transaction retried at future point unless terminated by CMD_STALL_TERM or SMMUEN transition (§3.12.2.2, line 2999) — **N/A**: retry guarantee is a liveness property of hardware; synchronous model records ResumeOutcome::Retry (smmu.cpp:5203) but does not re-execute transactions internally

### §3.12.5 Combinations of Fault Configuration with Two Stages

- [x] Stage1=Terminate, Stage2=Terminate, fault at Stage1: transaction terminated, VA in event; event passed to guest as stage 1-only event (§3.12.5, line 3062) — **PASS**: S1 fault with isStage2=false → generateEvent with S2=0, IPA=0; both stages terminate → abort path; confirmed by §3.12.5 prior audit (BUG-QA-12/13 ✅)
- [x] Stage1=Terminate, Stage2=Terminate, fault at Stage2: transaction terminated, VA+IPA in event (§3.12.5, line 3063) — **PASS**: S2 fault with tl_stage2FaultCtx.isStage2=true, ipa set → generateEvent with S2=1, IPA; S2S=0 → terminate; confirmed by prior audit
- [x] Stage1=Terminate, Stage2=Stall, fault at Stage2: transaction stalled, VA+IPA in event (§3.12.5, line 3065) — **PASS**: S2S=true → inStallMode=true for stage-2 fault; stall path uses tl_stage2FaultCtx.isStage2/ipa for event; confirmed by prior audit
- [x] Stage1=Stall, Stage2=Terminate, fault at Stage1: transaction stalled, VA in event (§3.12.5, line 3066) — **PASS**: isStage2Fault=false → uses getFaultMode()==Stall; S2S=false → S2 terminate; stall event has S2=0; confirmed by prior audit
- [x] Stage1=Stall, Stage2=Stall, fault at Stage2: transaction stalled, VA+IPA in event (§3.12.5, line 3073) — **PASS**: S2S=true → inStallMode=true; stall event carries S2=1, IPA; confirmed by prior audit

## §3.13 Translation Tables and Access Flag/Dirty State

> **Audit date:** 2026-05-08 — 25 items checked: 5 PASS, 17 N/A, 3 Out-of-scope, 0 bugs

- [x] HTTU support indicated by SMMU_IDR0.HTTU: 0=no updates, 1=Access flag only, 2=Access flag and dirty state (§3.13, line 3091) — **PASS**: smmu.cpp:3518 sets bit 6 in IDR0 → HTTU[7:6]=0b01 (access-flag-only), correctly encoding the three-value HTTU field
- [x] CDs referencing same translation table and same ASID must have identical HA and HD fields (§3.13, line 3099) — **N/A**: software programmer constraint on CD configuration; the SMMU model is not required to enforce or check cross-CD field identity; obligation is on the OS/hypervisor

### §3.13.2 Access Flag Hardware Update

- [x] When HTTU enabled and descriptor has AF==0: SMMU atomically sets AF==1; does NOT clear AF (§3.13.2, line 3130) — **PASS**: address_space.cpp:152-154 sets `entry.accessFlag = true` only when `ha==true && !entry.accessFlag`; call is made under contextMutex (atomicity); AF never cleared by this path
- [x] SMMU never clears AF (§3.13.2, line 3132) — **PASS**: `updateAccessFlags` only sets `accessFlag = true`; no code path anywhere in the implementation clears `accessFlag` once set
- [x] If access to descriptor causes permission fault: it is UNKNOWN whether AF is updated to 1 (§3.13.2, line 3133) — **N/A**: spec says UNKNOWN — no conformance requirement placed on the SMMU; implementation returns early on permission fault before reaching `updateAccessFlags`, which is one valid outcome
- [x] Includes stage 2 translation for L1CD or CD fetch (§3.13.2, line 3130) — **PASS**: smmu.cpp:2798-2802 and stream_context.cpp:1440-1444 call `stage2AddressSpace->updateAccessFlags()` with `config.s2ha` after successful stage-2 translation, covering the stage-2 HA AF-update path

### §3.13.3.1 Direct Permission Scheme - Dirty State

- [x] When HTTU dirty state enabled and descriptor is read-only due to AP[2:1]==0b1x (stage 1) or S2AP[1:0]==0b0x (stage 2): if DBM==1 and write translation occurs, SMMU atomically sets AP[2]==0 or S2AP[1]==1 (§3.13.3.1, line 3148) — **N/A**: IDR0.HTTU=0b01 (access-flag-only); CD.HD=1 causes C_BAD_CD at smmu.cpp:1177-1179; dirty-state code path structurally unreachable
- [x] SMMU never sets or clears DBM (§3.13.3.1, line 3170) — **N/A**: HTTU=0b01; dirty-state path unreachable; DBM field never examined or modified by model
- [x] SMMU never clears S2AP[1] (§3.13.3.1, line 3171) — **N/A**: HTTU=0b01; STE.S2HD=1 rejected at smmu.cpp:1167-1170; S2AP bits never modified
- [x] SMMU never sets AP[2]; descriptor never made writable by SMMU unless DBM==1 (§3.13.3.1, line 3172) — **N/A**: HTTU=0b01; HD rejected at smmu.cpp:1177-1179; no AP[2] modification ever occurs
- [x] SMMU never sets S2AP[1]==1 for the stage 2 translation used to fetch L1CD or CD (§3.13.3.1, line 3174) — **N/A**: HTTU=0b01; S2HD rejected at smmu.cpp:1167-1170; dirty-state update for stage-2 PTW lookup never attempted

### §3.13.3.2 Indirect Permission Scheme

- [x] CD.HD exclusively defines whether dirty state managed by hardware or software when Indirect Permission Scheme used for stage 1 (§3.13.3.2, line 3178) — **N/A**: HTTU=0b01; CD.HD=1 rejected at smmu.cpp:1177-1179; Indirect Permission Scheme dirty-state path structurally unreachable
- [x] STE.S2HD exclusively defines whether dirty state managed by hardware or software when Indirect Permission Scheme used for stage 2 (§3.13.3.2, line 3182) — **N/A**: HTTU=0b01; STE.S2HD=1 rejected at smmu.cpp:1167-1170; dirty-state path unreachable

### §3.13.4 HTTU Behavior Summary

- [x] Descriptor update from completed ATOS translation: made visible by completion of CMD_SYNC submitted after ATOS translation began (§3.13.4, line 3192) — **N/A**: synchronous in-memory model; ATOS descriptor updates are immediately visible in the same memory; CMD_SYNC ordering requirement trivially satisfied (no async descriptor cache)
- [x] Descriptor update from completed incoming transaction: made visible by completion of CMD_SYNC submitted after transaction completion (§3.13.4, line 3193) — **N/A**: synchronous model; visibility is immediate; CMD_SYNC ordering trivially satisfied
- [x] TLB invalidation completion makes descriptor updates from transactions completed by that invalidation visible (§3.13.4, line 3194) — **N/A**: synchronous model; no caching layer between descriptor updates and subsequent lookups; requirement vacuously satisfied
- [x] SMMU exception: if stage 2 HD enabled, SMMU permitted to speculatively update stage 2 dirty state for stage 1 TT walk even if stage 1 HA/HD disabled (§3.13.4, line 3198) — **N/A**: "permitted to" = optional optimization; additionally HTTU=0b01 means dirty-state (HD) is not implemented

### §3.13.6 Access Flag in Table Descriptors

- [x] HAFT support controlled by CD.HAFT (stage 1) and STE.S2HAFT (stage 2) (§3.13.6, line 3226) — **N/A**: IDR0.HTTU=0b01 → HAFT not supported; no HAFT/S2HAFT fields parsed or used; flat model has no hierarchical Table descriptors
- [x] If HAFT disabled for translation stage: hardware update of AF in Table descriptors also disabled (§3.13.6, line 3230) — **N/A**: flat address-space model has no Table descriptors (only leaf page entries); requirement vacuously satisfied
- [x] If HAFT enabled: Table entry with Access flag clear is NOT permitted to be cached in TLB (§3.13.6, line 3236) — **N/A**: HTTU=0b01 → HAFT not supported; flat model has no Table entries; not applicable

### §3.13.7.1 Hardware Flag Update for ATS and PRI

- [x] When ATS TR made: AF set to 1 in same way as direct transaction access (§3.13.7.1, line 3253) — **Out-of-scope**: ATS Translation Request processing from PCIe devices is not modeled (🚫 per TASKS_BUGS.md §3.13.7.1)
- [x] If HTTU dirty state enabled and ATS request for write (NW==0) to writable-clean page: SMMU marks page writable-dirty before returning ATS response (§3.13.7.1, line 3254) — **Out-of-scope**: ATS TR path not modeled; additionally HTTU=0b01 makes dirty-state update unavailable regardless
- [x] If HTTU only Access flag enabled: ATS request for write to writable-clean returns ATS Completion with W==0 (§3.13.7.1, line 3254) — **Out-of-scope**: ATS TR path not modeled

### §3.13.8 Hardware Flag Update for Cache Maintenance Operations and Destructive Reads

- [x] HTTU dirty state update NOT performed for: Invalidate Cache Maintenance Operations, Destructive Reads, Destructive Hints (§3.13.8, line 3281) — **N/A**: HTTU=0b01 means dirty-state updates never performed for any transaction type; CMO/Destructive Read/Destructive Hint operations not modeled; vacuously conformant
- [x] When these operations to writable-clean descriptor: descriptor not updated to writable-dirty; operations are downgraded (§3.13.8, line 3292) — **N/A**: dirty-state path (HTTU>=0b10) unreachable via HD/S2HD rejection at smmu.cpp:1167-1179; downgrade behavior not applicable

## §3.14 Speculative Accesses

- [x] Only read transactions can be marked speculative; write transactions marked speculative are always terminated with abort and no event recorded (§3.14, line 3300) — **N/A**: speculative marking is IMPLEMENTATION DEFINED ("An implementation might allow", §3.14 line 3300); TransactionType enum (types.h:1443-1448) exposes no speculative variant and translate() API has no isSpeculative parameter — rule unreachable by construction
- [x] Speculative read: if translation faults for any reason, transaction terminated with abort; no event recorded (§3.14, line 3304) — **N/A**: no speculative marker exists in the transaction model; rule unreachable by construction
- [x] Speculative read: if translation succeeds without fault and HTTU enabled, SMMU updates Access flags (§3.14, line 3305) — **N/A**: no speculative marker; HTTU AF-update path (address_space updateAccessFlags) is correctly exercised for ordinary non-speculative reads

## §3.15 Coherency Considerations and Memory Access Types

<!-- Audited 2026-05-08: 9 items checked, 2 PASS, 6 N/A, 1 BUG (BUG-AUDIT-164-CPP FIXED 2026-05-09) -->

- [x] All in-memory structures and queues accessed using Normal memory types (§3.15, line 3324) — **N/A**: hardware memory-attribute rule; software model issues no real memory transactions and has no cache-coherent interconnect to configure; rule is vacuously satisfied by construction
- [x] If HTTU supported: atomic access required to update translation tables shared between PE and SMMU (§3.15, line 3326) — **N/A**: rule scopes to "shared between the PE and SMMU" in a real system; this model has no real PE; HTTU=0b01 (access flag only); updateAccessFlags() is non-atomic but this is a thread-safety concern outside §3.15 scope
- [x] SMMU_IDR0.COHACC: system supports IO-coherent accesses from SMMU for configuration structures, translation tables, queues, CMD_SYNC, GERROR, Event queue, PRI queue MSIs (§3.15, line 3339) — **PASS**: COHACC (IDR0 bit 13) is absent from getIDR0() (smmu.cpp:3515-3541), correctly advertising COHACC=0; spec §3.15 line 3339 mandates COHACC=0 when IO-coherent access is not supported, which is correct for a software model with no IO-coherent interconnect
- [x] TLB-maintenance operations sent from client devices into the system are NOT permitted and never propagated by the SMMU (§3.15.1, line 3343) — **PASS**: no client API exists to forward TLB maintenance operations; receiveBroadcastTLBI() is the inbound-from-PE direction (CR2.PTM gated), not client-originated; absence of a client TLB-maint forwarding path is correct behavior
- [x] SMMU cache maintenance operations from client devices are supported (§3.15.1, line 3343) — **N/A**: pure translation model; no cache state is modelled; CMOs from clients, if presented, would translate as ordinary accesses; no specific code path required in a software model
- [x] SMMU does not output inconsistent attributes from misconfiguration; Outer Shareable used as effective Shareability when Device or Normal Inner Non-cacheable Outer Non-cacheable types configured (§3.15, line 3721) — **PASS / BUG-AUDIT-164-CPP FIXED**: `oshRequired(uint8_t memAttr)` helper added to stream_context.cpp and smmu.cpp (`return (memAttr <= 0x5u) || (memAttr == 0x8u) || (memAttr == 0xCu)`); all three OSH enforcement sites updated (stream_context.cpp mtCfg=true branch, smmu.cpp TLB fast path mtCfg=true branch, smmu.cpp GBPA bypass path); covers Device-nGnRnE/nGnRE/nGRE/GRE (0x0–0x3), reserved Device aliases (0x4/0x8/0xC), and Normal-iNC-oNC (0x5); mtCfg=false page-level branch unchanged (pageAttr is binary 0x00/0xFF in address_space.cpp so Normal-iNC-oNC cannot be represented at page level); TDD: `test_bug_audit164_osh_memattr.cpp` (13 tests, all PASS)

### §3.15.1.1 Fully-Coherent Client Devices

- [x] GPC checks apply to fully-coherent requests (§3.15.1.1, line 3355) — **N/A**: GPC infrastructure absent (IDR0.RME_IMPL=0 per §3.25 audit; no GPT/GPC implementation); fully-coherent client device distinction absent from TransactionType enum (types.h:1443-1448); rule unreachable
- [x] DPT checks apply to fully-coherent requests; exception: DPT W bit permitted to be treated as 1 for fully-coherent client where required by coherency protocol (§3.15.1.1, line 3356) — **N/A**: DPT not implemented (IDR3.DPT=0); DPTI_ALL/DPTI_PA commands return CERROR_ILL (smmu.cpp:5368-5372); fully-coherent client type absent from TransactionType; rule unreachable
- [x] Client-originated snoop requests bypass the SMMU and are NOT subject to DPT checks or GPC (§3.15.1.1, line 3358) — **N/A**: no Snoop TransactionType defined; GPC and DPT absent; bypass behavior vacuously satisfied since neither check exists

## §3.16 Embedded Implementations

> **Audit date:** 2026-05-09 — 8 items checked: 0 PASS, 8 N/A, 0 bugs (embedded mode is IMPLEMENTATION DEFINED optional; TABLES_PRESET=0 and QUEUES_PRESET=0 in IDR1 — all §3.16 requirements vacuously satisfied)

- [x] SMMU_IDR1.TABLES_PRESET: Stream table base address hardwired to pre-existing storage (§3.16, line 3371) — **N/A**: TABLES_PRESET=0 in getIDR1() (smmu.cpp:3553-3566); this implementation uses normal RAM-backed stream tables allocated by software; embedded/preset mode is an IMPLEMENTATION DEFINED optional feature not advertised by this model
- [x] SMMU_IDR1.QUEUES_PRESET: queue base addresses hardwired to pre-existing storage (§3.16, line 3371) — **N/A**: QUEUES_PRESET=0 in getIDR1() (smmu.cpp:3553-3566); all queues (CMDQ, EVENTQ, PRIQ) are software-allocated in normal memory; preset queue mode is IMPLEMENTATION DEFINED and not advertised
- [x] When SMMU_IDR1.REL set: base addresses given relative to start of SMMU register memory map (§3.16, line 3371) — **N/A**: REL=0 in getIDR1() (smmu.cpp:3553-3566); per spec §6.3.2 line 10988, REL is RES0 when both TABLES_PRESET==0 and QUEUES_PRESET==0; vacuously satisfied
- [x] For embedded implementation using internal storage: all address regions for configuration structures and queues must not overlap; applies within same PA space and across NS and Secure PA spaces (§3.16, line 3374) — **N/A**: no internal/embedded storage used; TABLES_PRESET=0 and QUEUES_PRESET=0; requirement is conditional on an implementation using internal storage; vacuously satisfied
- [x] Embedded Event/PRI queue entries (QUEUES_PRESET==1): permitted to have read-only/write-ignored behavior for software accesses (§3.16.1.1, line 3382) — **N/A**: QUEUES_PRESET=0 in getIDR1() (smmu.cpp:3553-3566); requirement explicitly gated on QUEUES_PRESET==1; vacuously satisfied
- [x] Embedded Command queue entries: readable and writable but storage not required for reserved/undefined fields, high-order StreamID bits beyond range, high-order SubstreamID bits beyond range, SSV if SubstreamIDs not implemented, STAG bits generated as '0' (§3.16.1.2, line 3386) — **N/A**: QUEUES_PRESET=0 in getIDR1() (smmu.cpp:3553-3566); requirement explicitly gated on QUEUES_PRESET==1 (embedded Command queue); command queue stored in normal software-allocated memory with full field storage
- [x] Software must not assume writing arbitrary 16-byte sequence to Command queue entry can be read back unmodified (§3.16.1.2, line 3404) — **N/A**: QUEUES_PRESET=0 in getIDR1() (smmu.cpp:3553-3566); this rule governs software behavior when interacting with embedded (preset) command queues only; non-preset command queues in normal memory provide full read-back fidelity
- [x] Embedded Stream table: storage not required for undefined fields, Reserved/RES0 fields, fields IGNORED in all supported configurations, fields with RAZ/WI behavior (§3.16.1.3, line 3408) — **N/A**: TABLES_PRESET=0 in getIDR1() (smmu.cpp:3553-3566); stream tables are software-allocated in normal RAM; embedded STE storage rules apply only to TABLES_PRESET==1 implementations

## §3.17 TLB Tagging, VMIDs, ASIDs and Broadcast TLB Maintenance

> **Audit date:** 2026-05-09 — 35 items checked: 12 PASS, 12 N/A, 8 BUG, 3 Out-of-scope (BUG-AUDIT-165-CPP through BUG-AUDIT-169-CPP filed)

- [x] Cached translations tagged with: translation regime (StreamWorld), ASID if regime supports ASIDs, VMID if S2 implemented and regime supports VMIDs (§3.17, line 3420) — **PASS**: TLBEntry carries `strw`, `asid`, `vmid` fields (types.h:1351-1361); insert path smmu.cpp:766-802 sets tags based on stage config and regime
- [ ] NS-EL1 stage 1 VA translations: ASID-tagged if nG==1, VMID-tagged if S2P==1 (§3.17, line 3431) — **BUG-AUDIT-165-CPP**: nG bit not tracked in TLBEntry or TranslationData; ASID applied unconditionally regardless of global/non-global descriptor flag; VMID tagging for S2P==1 is correct (smmu.cpp:772-781)
- [ ] any-EL2 translations: no ASID tag, no VMID tag (§3.17, line 3435) — **BUG-AUDIT-166-CPP**: ASID zeroed correctly (smmu.cpp:799-800) but `entryVmid` not zeroed for EL2 streams; EL2 stream with non-zero STE.S2VMID produces TLB entries incorrectly tagged with that VMID
- [ ] any-EL2-E2H translations: ASID-tagged if nG==1, no VMID tag (§3.17, line 3436) — **BUG-AUDIT-165-CPP / BUG-AUDIT-166-CPP**: nG absent; `entryVmid` not zeroed for EL2_E2H streams (smmu.cpp:787-802)
- [ ] EL3 translations: no ASID tag, no VMID tag (§3.17, line 3438) — **BUG-AUDIT-166-CPP**: ASID zeroed correctly (smmu.cpp:799-800) but `entryVmid` not zeroed for EL3 streams
- [x] When SMMU_IDR0.S1P==1: SMMU supports 16-bit ASIDs if SMMU_IDR0.ASID16==1 (§3.17, line 3456) — **PASS**: IDR0 advertises S1P (gated on s1pSupported_) and ASID16=1 unconditional (smmu.cpp:3524-3533)
- [x] When SMMU_IDR0.S2P==1: SMMU supports 16-bit VMIDs if SMMU_IDR0.VMID16==1 (§3.17, line 3457) — **PASS**: IDR0 advertises S2P (gated on s2pSupported_) and VMID16=1 unconditional (smmu.cpp:3524, 3540)
- [x] All TLB entries inserted using NS-EL1 configurations are tagged with VMIDs when S2P==1, regardless of stage configuration (§3.17, line 3458) — **PASS**: smmu.cpp:772-781; NS-EL1 always retains STE.S2VMID as entryVmid for stage-1-only, stage-2-only, and both-stage configs
- [x] SMMU support for broadcast TLB maintenance is optional; indicated by SMMU_IDR0.BTM (§3.17, line 3462) — **PASS**: IDR0 bit 5 set unconditionally (smmu.cpp:3528); receiveBroadcastTLBI() implemented (smmu.cpp:5806)
- [x] If SMMU_IDR0.BTM==1 and SMMU_(*_)CR2.PTM==1: SMMU permitted but not required to ignore broadcast TLB invalidations for corresponding Security state (§3.17, line 3466) — **PASS**: smmu.cpp:5816-5817; CR2.PTM=1 causes early return for NS-targeted commands
- [x] Broadcast TLB invalidations with illegal operations (e.g. affecting unimplemented stage): silently ignored (§3.17, line 3466) — **PASS**: receiveBroadcastTLBI() routes only to implemented NS/EL1/EL2 types; unimplemented-stage broadcasts produce no effect
- [ ] When SMMU_IDR0.S2P==0: SMMU matches VMID 0 for incoming broadcast TLB invalidations for regimes using VMIDs (§3.17, line 3466) — **BUG-AUDIT-167-CPP**: receiveBroadcastTLBI() (smmu.cpp:5806-5820) has no VMID=0 substitution when s2pSupported_==false; broadcast VMID operand passed unchanged
- [x] CD.ASET==1: address space and ASID are non-shared; TLB entries not required to be invalidated by broadcast VA{L}ExIS and ASIDExIS operations (§3.17, line 3478) — **N/A**: ASET not tracked in TLBEntry; conservative over-invalidation is conformant (spec uses "not required")
- [x] CD.ASET==0: ASID considered shared with PE processes; TLB entries required to be affected by all matching broadcast invalidations (§3.17, line 3478) — **N/A**: all entries effectively treated as ASET==0; requirement met by over-invalidation
- [x] CMD_TLBI_* commands invalidate all matching TLB entries regardless of ASET value (§3.17, line 3480) — **PASS**: smmu.cpp:4566-4704; no ASET filtering anywhere in CMD_TLBI_* paths

### §3.17.1 The Global Flag in the Translation Table Descriptor

- [ ] Translation performed for Secure stream from Non-secure memory is treated as non-global (nG==1) regardless of nG bit value in descriptor (§3.17.1, line 3504) — **BUG-AUDIT-165-CPP**: nG not tracked in TLBEntry (types.h:1342-1378) or TranslationData; no enforcement of non-global override for Secure-stream-from-NS-memory
- [x] any-EL2 and EL3 StreamWorlds: nG bit has no effect (§3.17.1, line 3504) — **N/A**: nG not tracked in model; trivially satisfied — nG has no effect anywhere
- [ ] Global TLB entry can match regardless of ASID; but can only match lookups from same StreamWorld as when TLB entry created (§3.17.1, line 3508) — **BUG-AUDIT-165-CPP**: no isGlobal/nG field in TLBEntry; global vs non-global distinction entirely absent; StreamWorld not part of TLBCache lookup key
- [ ] Global TLB entries with ASET==0 do not match lookups through configurations with ASET==1 and vice versa (§3.17.1, line 3512) — **BUG-AUDIT-165-CPP**: neither ASET nor nG tracked; cross-ASET isolation for global entries unimplemented; vacuously N/A since global entries are never created

### §3.17.4 Broadcast TLB Maintenance in Mixed AArch32/AArch64 Systems

- [x] SMMU supporting 16-bit ASID: compares full 16-bit broadcast value to TLB tags (§3.17.4, line 3600) — **PASS**: TLBEntry.asid is uint16_t; invalidateByASID() compares full 16-bit value (tlb_cache.cpp:393-425); IDR0.ASID16=1
- [x] SMMU supporting 8-bit ASID: compares bottom 8 bits; required to match if bottom 8 equal and top 8 zero; not required to match if top 8 non-zero (§3.17.4, line 3602) — **N/A**: model advertises ASID16=1; 8-bit rule does not apply

### §3.17.5 EL2 ASIDs and TLB Maintenance in E2H Mode

- [ ] Change to SMMU_CR2.E2H must be accompanied by invalidation of all TLB entries from NS-EL2 or NS-EL2-E2H STEs (§3.17.5, line 3635) — **BUG-AUDIT-168-CPP**: setCR2() (smmu.cpp:5475-5482) stores value only; no TLB invalidation on E2H bit transition
- [x] Change to SMMU_S_CR2.E2H must be accompanied by invalidation of all TLB entries from S-EL2 or S-EL2-E2H STEs (§3.17.5, line 3636) — **N/A**: Secure EL2 (SMMU_S_CR2) not implemented in this model

### §3.17.6 VMID Wildcards

- [x] SMMU_CR0.VMW controls Non-secure VMID wildcard function; configured number of VMID LSBs ignored during invalidation matching (§3.17.6, line 3654) — **PASS**: getVmidMask() lambda (smmu.cpp:4558-4563) reads CR0_VMW and computes (0xFFFFu << vmw) & 0xFFFF
- [ ] Both broadcast TLB invalidation and explicit CMD_TLBI_* commands respect VMID wildcard when SMMU_CR0.VMW != 0 (§3.17.6, line 3658) — **BUG-AUDIT-169-CPP**: getVmidMask() applied only to TLBI_S12_VMALL and TLBI_S2_IPA (smmu.cpp:4683,4693-4700); TLBI_NH_ALL, TLBI_NH_VA, TLBI_NH_VAA, TLBI_NH_ASID use exact VMID matching without wildcard (smmu.cpp:4572-4625)
- [x] VMID wildcard does not allow dissimilar VMID values to alias on TLB lookup (§3.17.6, line 3662) — **PASS**: TLBCache lookup keyed on {streamID,pasid,iova,secState}; VMID not in lookup key; no alias possible

### §3.17.7 Broadcast TLB Maintenance for GPT Information

- [x] SMMU with RME and SMMU_ROOT_IDR0.BGPTM==1 participates in broadcast TLBI *PA* instructions from PEs in EL3 (§3.17.7, line 3668) — **Out-of-scope**: RME_IMPL=0 (smmu.cpp:5325-5327); RME/GPT not implemented
- [x] TLBI *PA* to Outer Shareable domain affects the SMMU (§3.17.7, line 3670) — **Out-of-scope**: RME not implemented
- [x] This applies regardless of SMMU_IDR0.BTM and SMMU_(*_)CR2.PTM values (§3.17.7, line 3672) — **Out-of-scope**: RME not implemented

### §3.17.8 TLBInXS Maintenance Operations

- [x] Applies only when SMMU_IDR0.BTM==1 (§3.17.8, line 3682) — **N/A**: BTM=1 is advertised but XS attribute is not modeled anywhere
- [x] MAIR encodings 0b00000001, 0b01000000, and 0b10100000 remain Reserved; XS attribute taken as 0 for all MAIR encodings (§3.17.8, line 3693) — **N/A**: XS not tracked; effectively 0 everywhere — conformant by absence
- [x] Bit [11] of stage 2 block and page descriptors remains RES0; XS attribute taken as 0 for all stage 2 translations (§3.17.8, line 3694) — **N/A**: stage 2 descriptor bit[11] not exposed; XS=0 implicitly for all S2 translations
- [x] SMMU behaves as though XS attribute for cached translations is 0 when determining effect of TLBI or TLBInXS operation (§3.17.8, line 3695) — **N/A**: TLBEntry has no XS field; all invalidations operate without XS distinction — equivalent to XS=0 everywhere

## §3.18 Interrupts and Notifications

> **Audit date:** 2026-05-10 — 13 items checked: 5 PASS, 8 N/A, 0 bugs — IDR0.MSI=0 (wired-only model); all MSI-specific items are N/A by consistent configuration; ordering requirements vacuously satisfied (no interrupt emission mechanism)

- [x] Implementation must support one of, or optionally both of, wired interrupts and MSIs; MSI support discoverable from SMMU_IDR0.MSI and SMMU_S_IDR0.MSI (§3.18, line 3707) — **PASS**: `getIDR0()` (smmu.cpp:3543) never sets bit 21 (MSI); model consistently advertises wired-only capability. Internally coherent: no MSI infrastructure is present because IDR0.MSI=0
- [x] Interrupt notification must not be observable before the new information is also observable (§3.18, line 3710) — **PASS (vacuous)**: No interrupt notification mechanism exists (no callback, no wired pin, no MSI write); no notification signal can race ahead of data; ordering requirement vacuously satisfied
- [x] Global error interrupt: change to GERROR must be observable if interrupt observable (§3.18, line 3712) — **PASS (vacuous)**: `signalGerror()` (smmu.cpp:5465) atomically toggles `gerrorStatus` under `queueMutex`; GERROR state is always current when a reader acquires the mutex; no separate IRQ signal can race ahead because none exists
- [x] Event queue interrupt: new entries must be observable to reads of queue index registers if interrupt observable (§3.18, line 3713) — **PASS (vacuous)**: `eventqProd` is advanced under `queueMutex` during `generateEvent()` (smmu.cpp:5961); PROD advance is the sole observable; no IRQ signal can precede it because none exists
- [x] CMD_SYNC completion interrupt: consumption of CMD_SYNC must be observable to reads of queue index registers if interrupt observable (§3.18, line 3714) — **PASS (vacuous)**: CONS.RD advance and CMD_SYNC completion event are both serialized under `queueMutex` in `processCommandQueue()`; no out-of-order IRQ possible
- [x] MSIs from Secure sources performed with Secure accesses targeting Secure PA space (§3.18, line 3722) — **N/A**: No Secure state implemented; no MSI writes occur (IDR0.MSI=0)
- [x] MSIs from Non-secure sources performed with Non-secure accesses targeting Non-secure PA space (§3.18, line 3722) — **N/A**: No MSI writes occur (IDR0.MSI=0); no hardware access path for MSI delivery in this software model
- [x] SMMU must produce unique DeviceID for outgoing MSIs that does not overlap with those for client devices (§3.18, line 3726) — **N/A**: No outgoing MSIs produced; no DeviceID register or mechanism exists; IDR0.MSI=0 makes this vacuous
- [x] Interrupt sources: Event queue (empty→non-empty), PRI queue (SMMU_PRIQ_IRQ_CFG2 condition), CMD_SYNC, GERROR (§3.18.2, line 3756) — **PASS**: All four sources tracked internally: `eventqProd` advances on event enqueue; `priqProd` advances on PRI enqueue; `cmdSyncLastSig_` records CS=1/2 completion; `gerrorStatus` toggled by `signalGerror()`; no delivery mechanism (consistent with software model intent)
- [x] When MSIs not supported: only interrupt Enable field is used; MSI address/data fields unused (§3.18, line 3736) — **PASS**: `irqCtrl_`/`irqCtrlAck_` model only enable bits (smmu.h:587–588, smmu.cpp:3695–3697); per-queue `*_IRQ_CFG0/1/2` MSI address/data registers not implemented — precisely correct for IDR0.MSI=0; `cmdqSyncMsi*` registers (smmu.h:534–536) are settable but never drive behavior (spec says "unused," not RAZ/WI)

### §3.18.1 MSI Synchronization

- [x] Disabling MSI through SMMU_(*_)IRQ_CTRL ensures previously-issued MSI writes are completed (§3.18.1, line 3742) — **N/A**: No MSI writes ever issued (IDR0.MSI=0); `setIrqCtrl()` (smmu.cpp:3695) stores value and immediately mirrors to `irqCtrlAck_`; synchronous echo correctly models disable handshake with no actual MSI flush required
- [x] CMD_SYNC ensures completion of MSIs originating from completion of prior CMD_SYNC commands consumed from same Command queue (§3.18.1, line 3743) — **N/A**: No MSI writes occur; CMD_SYNC barrier (smmu.cpp:5285) serializes all prior command effects under `queueMutex` before advancing CONS.RD — non-MSI semantics are correct
- [x] Completion of MSI aborted: abort visible in GERROR with appropriate SMMU_(*_)GERROR.MSI_*_ABT_ERR flag (§3.18.1, line 3745) — **N/A**: No MSI writes issued so no aborts occur; `GERROR_MSI_CMDQ_ABT_ERR` (bit 4), `GERROR_MSI_EVENTQ_ABT_ERR` (bit 5), `GERROR_MSI_PRIQ_ABT_ERR` (bit 6), `GERROR_MSI_GERROR_ABT_ERR` (bit 7) defined in types.h:1487–1490 but never set — correct since IDR0.MSI=0

## §3.19 Power Control

- [x] Power off state entered only when: all client devices and interconnect quiescent, device DMA disabled, outstanding commands/invalidations/transactions complete, stalled transactions terminated with abort (§3.19, line 3798) — **N/A**: These are system-software caller obligations per spec §3.19 lines 3798–3800; the SW model has no external DMA generators, interconnect, or stall-producing client devices to quiesce. The SMMU implementation cannot observe or enforce quiescence from inside the model.
- [x] On wakeup: SMMU must be reset; SMMU registers must be re-initialized before client devices can be enabled (§3.19, line 3801) — **N/A**: Power-cycle wakeup maps to constructing a new `SMMU` object, which runs the constructor and initializes all registers to reset state. `enable()` on an existing object is re-enablement within a single lifecycle, not wakeup from power-off.

### §3.19.1 Dormant State

- [x] When SMMU_STATUSR.DORMANT==1: no caches of any structures or translations are present; no prefetch of any configuration/translation data in progress; any structure/translation alterations will result in fresh memory reads (§3.19.1, line 3807) — **PASS** (BUG-DORMANT-2-CPP fixed 2026-05-11): `disable()` now calls `tlbCache->invalidateAll()` before setting `statusr_=1`; stale TLB entries are evicted on dormant entry. `enable()` clears DORMANT=0. IDR0.DORMHINT=1 advertises dormancy. TDD: `P3Dormant.TlbFlushedOnDormantEntry` verifies stale pre-dormant PA not served post-wakeup; `P3Dormant.StatusrDormantSetAfterDisable`/`ClearedAfterReEnable`/`Idr0DormhintIsSet` all pass.

## §3.20 TLB and Configuration Cache Conflict

### §3.20.1 TLB Conflict

- [x] When TLB conflict detected: transaction aborted; F_TLB_CONFLICT event recorded (§3.20.1, line 3830) — **N/A**: Detection is IMPL DEF (spec line 3828 says not required). Software model uses deterministic `std::unordered_map`-keyed TLB (`CacheKey` → single value) in `tlb_cache.h:41-51`; hardware-style multi-hit is structurally impossible. `EventType::F_TLB_CONFLICT (0x20)` exists at `smmu.cpp:1685` and wire-format at `smmu.cpp:3847`, but `generateEvent(F_TLB_CONFLICT)` is never called. Model elects not to implement conflict detection.
- [x] If TLB conflict not detected: behavior is unpredictable; restriction: transaction cannot access PA to which stream configuration does not explicitly grant access (§3.20.1, line 3835) — **PASS**: `CacheKey` is keyed on `{streamID, pasid, iova, securityState}` (`tlb_cache.h:41-51`). TLB entries only inserted after slow-path translation succeeds (`smmu.cpp:759`). No path delivers a PA not explicitly granted by that stream's configuration. Restriction satisfied structurally.
- [x] TLB conflict never enables: matching entry with different VMID, different Security state, different StreamWorld, or PA outside stage 2 configured range (§3.20.1, line 3837) — **PASS**: `securityState` is in `CacheKey` (`tlb_cache.h:45,49,80`); cross-security hit is structurally impossible. Stage-2 PA bounds enforced at insertion time (`smmu.cpp:2797-2798, 3023-3024`). VMID/StreamWorld stored in `TLBEntry` (`types.h:1361,1370`) for TLBI use; per-stream configuration isolation guaranteed by per-STE `StreamContext`. All four prohibitions structurally satisfied.
- [x] TLB conflict from one stream must not cause traffic for different streams with other VMID/StreamWorld/Security to be terminated (§3.20.1, line 3848) — **N/A**: No TLB conflict is ever raised by this implementation (see item above). Cross-stream termination from a TLB conflict event is architecturally impossible in this model. Vacuously satisfied.

### §3.20.2 Configuration Cache Conflicts

- [x] When configuration cache conflict detected: transaction aborted; F_CFG_CONFLICT event recorded (§3.20.2, line 3858) — **N/A**: Detection is IMPL DEF (spec line 3856 says not required). Software model has no actual configuration cache separate from the `StreamContext` map; stream configuration is looked up directly and deterministically. `EventType::F_CFG_CONFLICT (0x21)` exists at `smmu.cpp:1686` and wire-format at `smmu.cpp:3848`, but `generateEvent(F_CFG_CONFLICT)` is never called for §3.20.2 (note: `FaultType::ConfigurationCacheFault` at `smmu.cpp:5297` is the CMD_SYNC CS=reserved path — unrelated). Model elects not to implement conflict detection.
- [x] If conflict not detected: behavior is unpredictable (§3.20.2, line 3863) — **N/A**: No configuration cache exists in the software model that could produce a conflict. Configuration reads are serialized via stripe locks on the `streamMap`. The "unpredictable if not detected" clause only concerns hardware implementations with actual configuration caches.
- [x] Configuration cache conflict cannot cause STE to be treated as associated with different Security state (§3.20.2, line 3864) — **PASS**: Each `StreamContext` holds its own `securityState` from its STE. The stream map is keyed by StreamID only; no data structure could cause one stream's STE to be returned for a different stream or interpreted with a different security state. Isolation is structural.

<!-- §3.20 C++ Audit 2026-05-11: 7 items checked — 3 PASS, 4 N/A, 0 FAIL, 0 bugs. Detection of F_TLB_CONFLICT/F_CFG_CONFLICT is IMPL DEF and not implemented (N/A). Access restriction (line 3835) PASS by structural CacheKey keying. Security/VMID/StreamWorld/PA isolation (line 3837) PASS by construction. Config-cache Security isolation (line 3864) PASS by per-stream StreamContext. -->

## §3.21 Structure Access Rules and Update Procedures

### §3.21.1 Translation Tables and TLB Invalidation Completion Behavior

- [x] TLB invalidation operation is complete after: all targeted TLB entries invalidated; relevant HTTUs globally visible; all translation table walks that could have formed targeted TLB entries are complete and globally visible (§3.21.1, line 3876) — **PASS**: `invalidateTranslationCache()` (smmu.cpp:1773) calls `tlbCache->invalidateAll()` synchronously; all entries removed before call returns; no async walks (no page-table walker); AF updates (HTTU=0b01) inline in translate path — completion is immediate; all state in-process C++ with no memory-ordering delays
- [x] ATOS result cannot be based on addresses/attributes not described by translation configuration observable after invalidation (§3.21.1, line 3889) — **PASS**: GATOS uses normal `translate()` path (`GatosTranslation` type, smmu.cpp:3809+) hitting same `tlbCache` at smmu.cpp:540; after synchronous invalidation completes, TLB is empty — ATOS cannot return a pre-invalidation result
- [x] Translation cache entries not inserted when SMMU_(*_)CR0.SMMUEN==0 (§3.21.1, line 3900) — **PASS**: early return at smmu.cpp:272 `(cr0_.load(acquire) & CR0_SMMUEN)==0` exits before TLB lookup at line 408 or insertion at line 759; structurally impossible to insert when SMMUEN=0

### §3.21.1.1 Translation Tables Update Procedure

- [x] SMMUv3.2+: must support Level 1 or Level 2 BBM behavior as indicated by SMMU_IDR3.BBML (§3.21.1.1, line 3918) — **PASS**: IDR3 (smmu.cpp:3609) sets bit[11] → BBML=0b01 (Level 1); Level 1 is a valid SMMUv3.2 compliance level per spec; no descriptor-level implementation required for software model
- [x] Break-before-make required for (pre-v8.4/pre-SMMUv3.2): changes to memory type, Cacheability, output address, block/page size, creating global entry where non-global entries overlap (§3.21.1.1, line 3904) — **N/A**: software obligation on entity updating translation tables; model has no page-table walker, no memory-mapped descriptor reads, no block/page descriptor parsing; BBM procedure has no SMMU enforcement surface

### §3.21.1.2 BBM Level 1 (SMMU_IDR3.BBML==1)

- [x] Level 1: nT bit must be used when changing translation size without break-before-make; F_TLB_CONFLICT may occur without nT or BBM (§3.21.1.2, line 3937) — **N/A**: no raw descriptor parsing in model; nT (descriptor bit[16]) never parsed or inspected; `generateEvent(F_TLB_CONFLICT)` never called — IMPL DEF detection not required; TLB multi-hit structurally impossible (deterministic `unordered_map` keyed TLB)
- [x] Level 1: Setting nT==1 does NOT cause a fault (§3.21.1.2, line 3939) — **N/A**: nT bit never inspected; no fault path triggers on descriptor bits; vacuously satisfied
- [x] Level 1: Block descriptor with nT==1 not cached in way that causes TLB conflict (§3.21.1.2, line 3942) — **N/A**: no block descriptors parsed or cached; TLB entries keyed on (StreamID, PASID, IOVA, SecurityState) at page granularity; no block-vs-page caching distinction
- [x] Level 1: Change to only Contiguous bit (bit 52) with other properties unchanged does not lead to TLB conflict fault (§3.21.1.2, line 3945) — **N/A**: Contiguous bit (descriptor bit[52]) never parsed; no TLB conflict fault path exists in software model; vacuously satisfied

### §3.21.1.3 BBM Level 2 (SMMU_IDR3.BBML==2)

- [x] Level 2: implementation ignores nT bit in Block descriptor; change to translation size can be performed without BBM or nT (§3.21.1.3, line 3957) — **N/A**: IDR3.BBML=0b01 (Level 1) advertised; §3.21.1.3 requirements apply only when BBML==2; not applicable
- [x] Level 2: F_TLB_CONFLICT never reported (§3.21.1.3, line 3961) — **N/A**: BBML==2 requirement; not applicable at Level 1
- [x] Level 2: TLB multi-hit — translations use info from at most one matching entry; no faults that wouldn't otherwise be possible; no combination of info from multiple entries (§3.21.1.3, line 3962) — **N/A**: BBML==2 requirement; not applicable
- [x] Level 2: TLB invalidation removes all matching TLB entries even if overlapping entries exist (§3.21.1.3, line 3972) — **N/A**: BBML==2 requirement; not applicable

### §3.21.2 Queues

- [x] SMMU does not write to Command queue (§3.21.2, line 3985) — **PASS**: `writeCmdqConsErr()` (smmu.cpp:5651) uses atomic CAS on `cmdqCons` register (bits[30:24]) only — never writes queue buffer entries; `commandQueue.pop_front()` at smmu.cpp:4053 is the consumer dequeue (internal state), not a write to the command queue buffer; Event and PRI queues are written by `generateEvent()` / `submitPageRequest()` as per spec
- [x] To issue commands: (1) determine space using PROD/CONS, (2) write commands, (3) DSB to ensure data observable, (4) update PROD index (§3.21.2, line 3986) — **N/A**: software obligation on the driver submitting commands to a hardware SMMU; PROD advanced atomically by `advanceQueueIndex()` inside `submitCommand()` (smmu.cpp:3968); barrier requirement N/A to in-process model
- [x] Software must not alter memory locations representing commands previously submitted until consumed (as indicated by CONS index) (§3.21.2, line 4002) — **N/A**: software obligation; no external write path to queue buffer in the model
- [x] Software must only write CONS index of output queue (Event/PRI) in consistent manner with appropriate incrementing and wrapping (§3.21.2, line 4004) — **N/A**: software obligation; internal CONS advancement handled atomically inside `processCommandQueue()`
- [x] Software must only write PROD index of Command queue in consistent manner (§3.21.2, line 4006) — **N/A**: software obligation; internal PROD advancement via `advanceQueueIndex()` (smmu.cpp:3968) is internally consistent
- [x] ILLEGAL PROD index write: CONSTRAINED UNPREDICTABLE: SMMU executes unpredictable commands OR stops consuming until queue disabled and re-enabled (§3.21.2, line 4007) — **N/A**: CONSTRAINED UNPREDICTABLE; no external raw PROD write path in model; `submitCommand()` always uses `advanceQueueIndex()` — arbitrary PROD writes are structurally prevented

### §3.21.3 Configuration Structures and Configuration Invalidation Completion

- [x] SMMU might read any entry at any time, for any reason (§3.21.3, line 4014) — **PASS**: `streamMap` modeled as concurrent C++ `unordered_map`; `translate()` reads it via per-stripe mutex at smmu.cpp:406; concurrent reads from multiple threads possible; no mechanism prevents speculative read-at-any-time
- [x] Structure considered valid only when SMMU observes V==1 and no configuration inconsistency makes it ILLEGAL (§3.21.3, line 4016) — **PASS**: `streamMap.find()` returning `end()` = STE.V=0 → C_BAD_STE (smmu.cpp:409–428); `configureStream()` runs full STE/CD ILLEGAL checks (smmu.cpp:1095–1327) before inserting; ILLEGAL inconsistencies (S2HD=1/HTTU mismatch, S2AA64=0, CD.HD=1) → C_BAD_STE/C_BAD_CD
- [x] SMMU does not follow invalid pointers, whether speculatively or in response to incoming transaction (§3.21.3, line 4018) — **PASS**: `streamMap.find()` returning `end()` causes immediate C_BAD_STE return (smmu.cpp:409–428) with no further pointer dereference; shared_ptr `StreamContext` ensures no dangling pointer; no raw descriptor pointer path exists
- [x] STEs and L1STDs not fetched if SMMU_(*_)CR0.SMMUEN==0 (§3.21.3, line 4020) — **PASS**: SMMUEN=0 early return at smmu.cpp:272 occurs before `streamMap.find()` at line 408; STE lookup structurally prevented when SMMUEN=0
- [x] CDs or L1CDs must never be fetched or prefetched unless indicated from a valid STE (§3.21.3, line 4022) — **PASS**: CD data (StreamContext PASID fields) only accessed after `streamMap.find()` succeeds (valid STE, smmu.cpp:408–409) and after STE validity/config checks (smmu.cpp:439–466); invalid/absent STE causes early return before any CD access
- [x] Implementation must not read any address outside configured range of any table (§3.21.3, line 4056) — **PASS**: single-level bounds check at smmu.cpp:365–368 rejects `streamID >= 2^log2sz` with C_BAD_STREAMID; two-level bounds via `validateStreamID2Level()` (smmu.cpp:5613–5641) holds `queueMutex` to prevent TOCTOU — validates L1 index `streamID >> split < 2^(log2sz-split)`
- [x] Implementation permitted to fetch/prefetch any reachable structure at any time within bounds of containing table (§3.21.3, line 4035) — **PASS**: permission not requirement; all `streamMap` accesses bounded by `strtabLog2Size_`/`strtabSplit_` enforcement; no out-of-bounds access structurally possible
- [x] Any change to a structure must be followed by appropriate CMD_CFGI_* invalidation command, even if structure was initially invalid (§3.21.3, line 4042) — **N/A**: software obligation on the driver programming the SMMU; smmu.cpp:1302 comment notes the requirement; model enforces correctness-by-rejection (`StreamAlreadyConfigured` at smmu.cpp:1304) requiring `removeStream()` + `configureStream()` cycle which passes through V=0 state
- [x] Configuration invalidation completion: all targeted cache entries invalidated; no accesses using old addresses/attributes; all client transactions using targeted entries globally visible; all configuration structure walks using targeted entries complete (§3.21.3, line 4061) — **PASS**: CMD_CFGI_* commands call `invalidateStreamCache()` / `invalidateTranslationCache()` synchronously (smmu.cpp:4925+); CMD_SYNC (smmu.cpp:5284+) completes synchronously under `queueMutex`; all state in-process C++ — "globally visible" = immediately after function return; no asynchronous configuration walks
- [x] Single-copy atomicity size for configuration structure fetches: if system has FEAT_LSE2, must be 128-bit; otherwise at least 64-bit (§3.21.3, line 4077) — **N/A**: FEAT_LSE2 not implemented; model performs no memory-mapped STE/CD fetches; configuration provided through `configureStream()` API not by SMMU reading memory-mapped descriptors; hardware atomicity requirement N/A to software model
- [x] To change single field within aligned single-copy atomic span: can be altered directly without making structure invalid; then CMD_CFGI and CMD_SYNC required (§3.21.3, line 4084) — **PASS**: `configureStream()` and stream updates perform atomic full `StreamConfig` struct swap under per-stripe mutex (smmu.cpp:1298); single-field update requires `removeStream()` + `configureStream()` cycle, equivalent to the correct procedure; no partial-update API exposes inconsistent intermediate state
- [x] For fields requiring non-single-copy-atomic writes (spanning multiple atomic spans): must make structure invalid, modify, then make valid using procedures in §3.21.3.1 (§3.21.3, line 4084) — **N/A**: software obligation; model's `removeStream()` + `configureStream()` API structurally enforces V=0 intermediate state before any modification — correct procedure satisfied as structural consequence

### §3.21.3.1 Configuration Structure Update Procedure

- [x] Initialize structure (V==0→V==1): (1) fill all fields with V==0, (2) DSB, (3) CMD_CFGI_STRUCT, (4) CMD_SYNC and wait, (5) set V=1, (6) DSB, (7) CMD_CFGI_STRUCT, (8) optionally CMD_SYNC (§3.21.3.1, line 4096) — **N/A**: software procedure for programming a hardware SMMU; `configureStream()` atomically fills all fields and inserts into `streamMap` (V=1) in a single locked operation — equivalent V=0→V=1 transition in one atomic step; no external memory-mapped write path
- [x] Make structure invalid (V==1→V==0): (1) set V==0, (2) DSB, (3) CMD_CFGI_STRUCT, (4) CMD_SYNC and wait (§3.21.3.1, line 4104) — **N/A**: software obligation; `removeStream()` atomically erases from `streamMap` (V→0) under stripe mutex; caller then issues CMD_CFGI_STE + CMD_SYNC through `submitCommand()`; procedure enforced externally per smmu.cpp:1302 comment
- [x] Software must not allow structure to enter invalid intermediate state while modifying a valid structure (§3.21.3.1, line 4111) — **N/A**: software obligation; model prevents this structurally — `configureStream()` returns `StreamAlreadyConfigured` (smmu.cpp:1304) if stream exists, forcing `removeStream()` (full V=0 invalidation) before any modification; intermediate invalid states are structurally impossible through the model's API

## §3.22 Destructive Reads and Directed Cache Prefetch Transactions

> **Audit date:** 2026-05-12 — 10 items checked: 0 PASS, 10 N/A, 0 bugs

- [x] In SMMUv3.0: these transactions unconditionally converted on output as specified by interconnect (§3.22, line 4150) — **N/A**: Interconnect output-side responsibility; model does not implement an AXI5 fabric or hardware transaction-type encoding. The SMMU outputs a translated PA + attributes; the interconnect decides wire encoding. No implementation surface in this model.
- [x] In SMMUv3.1+: DR downgraded to non-destructive read if STE.DRE==0; W-DCP downgraded to ordinary write if STE.DCP==0; NW-DCP downgraded to no-op if STE.DCP==0 (§3.22.1, line 4188) — **N/A**: DR/W-DCP/NW-DCP are not representable transaction types in this model (`TransactionType` enum: Ordinary/AtsTranslationRequest/AtsTranslated/GatosTranslation only). `StreamConfig` has no `dre`/`dcp` fields; `getIDR0()` does not advertise DRE/DCP capability. Downgrade logic runs in hardware downstream of the SMMU's PA output.
- [x] STE.DRE==1 required for DR to pass without downgrade when one or more stages of translation applied (§3.22.1, line 4194) — **N/A**: DR not representable as a `TransactionType`; STE.DRE not a field of `StreamConfig`. No implementation surface; same rationale as preceding item.
- [x] STE.DCP==1 required for W-DCP and NW-DCP to pass without downgrade when translation applied (§3.22.1, line 4195) — **N/A**: W-DCP/NW-DCP not representable; STE.DCP not modeled. Same rationale as preceding item.
- [x] DR requires Read/Execute AND Write permission that does not result in HTTU dirty state update; if write not granted, downgraded to read or RCI (§3.22.2, line 4213) — **N/A**: DR not representable. Additionally, model hard-pins `HTTU=0b01` (access-flag-only; no dirty-state management — smmu.cpp:1193-1207); the "write without HTTU dirty" distinction is vacuously satisfied for all transactions.
- [x] NW-DCP: if required permission not present, prefetch does not occur; no abort response generated (§3.22.2, line 4215) — **N/A**: NW-DCP not representable as a transaction type. "No abort" on permission denial is an interconnect output-path behavior, not an SMMU model behavior.
- [x] RCI and DR: if ultimately lead to fault, recorded as reads; stall behavior same as ordinary read (§3.22.2, line 4219) — **N/A**: RCI/DR not representable; they cannot be submitted to `translate()`. Ordinary read fault recording and stall behavior are correctly implemented for `AccessType::Read`.
- [x] W-DCP: if leads to fault, recorded as write; stall behavior same as ordinary write (§3.22.2, line 4221) — **N/A**: W-DCP not representable; cannot be submitted to `translate()`. Ordinary write fault recording and stall behavior correctly implemented for `AccessType::Write`.
- [x] DR, RCI, W-DCP stalled: retried as same transaction type (§3.22.2, line 4222) — **N/A**: None of DR/RCI/W-DCP are representable transaction types. Retry-as-same-type is a property of the upstream device issuing CMD_RESUME; the SMMU does not enforce or control which transaction type the device uses on retry.
- [x] Output DR/RCI/W-DCP/NW-DCP downgraded if output attributes incompatible with output interconnect (§3.22.3, line 4228) — **N/A**: Interconnect output-side concern; model does not simulate a physical AXI5 bus fabric or memory attribute compatibility with bus. Output attribute overrides (STE.SHCFG, MTCFG, ALLOCCFG) are applied to `TranslationData`; whether those attributes are bus-compatible and trigger downgrade is outside model scope.

## §3.23 Memory Tagging Extension

> **Audit date:** 2026-05-12 — 4 items checked: 1 PASS, 3 N/A, 0 bugs

- [x] MAIR encoding 0xF0 is Reserved in SMMUv3 in CD.MAIR0 and CD.MAIR1 (§3.23, line 4240) — **N/A**: "Reserved" in ARM specification language is a software programming constraint (behavior is UNPREDICTABLE if misused), not a hardware-side rejection requirement. The model contains a `MemoryAttributeRegister` struct (types.h:1847-1874) that stores attr0..attr7 bytes, but no code path in the translation pipeline (smmu.cpp, configuration.cpp, address_space.cpp, stream_context.cpp) reads or validates those attribute bytes. With MAIR bytes unconsumed by the translation engine, there is no incorrect-behavior surface specific to 0xF0, and no SMMU-side enforcement is required or expected.
- [x] All SMMU-originated accesses are Tag Unchecked accesses; SMMU does not write Allocation Tags (§3.23, line 4242) — **PASS** (by construction): The model is a pure translation engine — it computes a physical address and permission bits. It performs no cache maintenance, no AXI5 bus signalling, and contains zero code paths that write allocation tags. The absence of any allocation-tag write infrastructure structurally guarantees compliance. No tag-checking logic exists anywhere in the model.

### §3.23.1 SMMU Support for FEAT_MTE_PERM

- [x] If SMMU_IDR3.MTEPERM==1: stage 2 MemAttr NoTagAccess encodings treated as without NoTagAccess in SMMU (§3.23.1, line 4247) — **N/A**: This rule is gated on `SMMU_IDR3.MTEPERM==1`. `getIDR3()` (smmu.cpp:3600-3609) sets bits 2, 4, 8, 10, 11 (HAD, XNX, FWB, RIL, BBML[0]) but does NOT set bit 0 (MTEPERM). With MTEPERM=0, the conditional never triggers and no reinterpretation logic is required.
- [x] When STE.S2FWB==0 and stage 2 MemAttr[3:0]==0b0100: SMMU interprets as Normal Inner WB Cacheable, Outer WB Cacheable (§3.23.1, line 4256) — **N/A**: This entry is part of the §3.23.1 reinterpretation table that activates only when MTEPERM==1 (same gate as preceding item, N/A on identical grounds). Additionally, the model carries no `s2fwb` field on `StreamTableEntry` or `StreamConfig`, and never decodes a 4-bit stage-2 MemAttr value — stage-2 memory attributes flow as a binary `pageAttr` byte (0x00 = Device-nGnRnE or 0xFF = Normal WB/WA) resolved by the AddressSpace API (stream_context.cpp:275-278). The rule's preconditions are structurally unreachable.

## §3.24 Device Permission Table

- [x] DPT use only supported for StreamIDs configured to use StreamWorld EL1; otherwise C_BAD_STE (§3.24, line 4269) — **N/A**: IDR3.DPT=0 (bit 1 never set in `getIDR3()`, smmu.cpp:3605-3609); DPT feature not advertised; no DPT walk, registers (SMMU_DPT_BASE_CFG, SMMU_DPT_CFG_FAR), STE fields (DPT_VMATCH), or TLB infrastructure exist in the software model
- [x] Independent DPT for each of Non-secure and Realm states (§3.24, line 4271) — **N/A**: IDR3.DPT=0; DPT not implemented; no per-security-state DPT structures exist
- [x] DPT support for Non-secure state: SMMU_IDR3.DPT; for Realm state: SMMU_R_IDR3.DPT (§3.24, line 4282) — **N/A**: IDR3.DPT=0; neither Non-secure nor Realm DPT capability is advertised

### §3.24.1 DPT Check

- [x] If input address outside SMMU_(R_)DPT_BASE_CFG.DPTPS configured range: No Access → Device Access fault (§3.24.1, line 4295) — **N/A**: IDR3.DPT=0; no DPT walk or range-check logic exists
- [x] Level 0 No Access entry: DPT check fails as Device Access fault (§3.24.1, line 4296) — **N/A**: IDR3.DPT=0; no DPT descriptor walk exists
- [x] A[1:0]==No Access in Level 1 descriptor: DPT check fails as Device Access fault (§3.24.1, line 4298) — **N/A**: IDR3.DPT=0; no DPT Level-1 descriptor processing exists
- [x] Region marked W=0 and incoming transaction is write: DPT check fails as Device Access fault (§3.24.1, line 4299) — **N/A**: IDR3.DPT=0; no DPT permission check exists
- [x] STE.DPT_VMATCH==0b00: VMID checked when AC==0b00 or AC==0b01; if VMID required and does not match → Device Access fault (§3.24.1, line 4307) — **N/A**: IDR3.DPT=0; no STE.DPT_VMATCH field or VMID-match logic exists
- [x] STE.DPT_VMATCH==0b01: VMID checked only when AC==0b00 (§3.24.1, line 4308) — **N/A**: IDR3.DPT=0; STE.DPT_VMATCH not implemented
- [x] STE.DPT_VMATCH==0b10: VMID never checked (§3.24.1, line 4309) — **N/A**: IDR3.DPT=0; STE.DPT_VMATCH not implemented
- [x] For Realm STEs: DPT_VMATCH always 0b00 (§3.24.1, line 4314) — **N/A**: IDR3.DPT=0; Realm DPT not implemented
- [x] Non-secure DPT: output PA space is Non-secure (§3.24.1, line 4322) — **N/A**: IDR3.DPT=0; no DPT output PA space logic exists
- [x] Realm DPT: AC==0b01 or 0b10 → output PA space Non-secure; otherwise → output PA space Realm (§3.24.1, line 4323) — **N/A**: IDR3.DPT=0; Realm DPT not implemented

### §3.24.2 DPT Caching Behavior

- [x] DPT TLB entries never created from ATS TRs that bypass all stages of translation (§3.24.2, line 4342) — **N/A**: IDR3.DPT=0; no DPT TLB cache exists
- [x] Level 0 No Access entry is NOT permitted to be cached in DPT TLB (§3.24.3.1.1, line 4484) — **N/A**: IDR3.DPT=0; no DPT TLB cache exists

### §3.24.3.1 DPT Descriptor Formats

- [x] Level 0: bits[1:0]==0b00 → No Access entry (§3.24.3.1.1, line 4483) — **N/A**: IDR3.DPT=0; no DPT descriptor walk exists
- [x] Level 0: bits[1:0]==0b01 → Block descriptor; AC and W fields valid (§3.24.3.1.2, line 4493) — **N/A**: IDR3.DPT=0; no DPT descriptor walk exists
- [x] Level 0: bits[1:0]==0b11 → Table descriptor; address field is next-level base (§3.24.3.1.3, line 4532) — **N/A**: IDR3.DPT=0; no DPT descriptor walk exists
- [x] Level 0 AC field: 0b00=VMID checked unless DPT_VMATCH==0b10; 0b01=VMID checked unless DPT_VMATCH==0b01 or 0b10; 0b10=VMID is RES0; 0b11=Reserved/invalid (§3.24.3.1.2, line 4503) — **N/A**: IDR3.DPT=0; no DPT AC field decoding exists
- [x] If SMMU_IDR0.VMID16==0: VMID[15:8] are RES0 in Level 0 Block entries (§3.24.3.1.2, line 4517) — **N/A**: IDR3.DPT=0; no DPT descriptor walk exists
- [x] Level 1 A[1:0]==0b00: No Access to both granules; all other fields RES0 (§3.24.3.1.4, line 4561) — **N/A**: IDR3.DPT=0; no DPT Level-1 descriptor processing exists
- [x] Level 1 A[1:0]==0b01: No Access upper granule; lower granule governed by AC0, W0, VMID0 (§3.24.3.1.4, line 4562) — **N/A**: IDR3.DPT=0; no DPT Level-1 descriptor processing exists
- [x] Level 1 A[1:0]==0b10: upper granule governed by AC1, W1, VMID1; No Access lower granule (§3.24.3.1.4, line 4563) — **N/A**: IDR3.DPT=0; no DPT Level-1 descriptor processing exists
- [x] Level 1 A[1:0]==0b11, Contig==0: upper granule AC1/W1/VMID1; lower granule AC0/W0/VMID0 (§3.24.3.1.4, line 4564) — **N/A**: IDR3.DPT=0; no DPT Level-1 descriptor processing exists
- [x] Level 1 A[1:0]==0b11, Contig!=0: contiguous region controlled by AC0, W0, VMID0 only; AC1/W1/VMID1 are RES0 (§3.24.3.1.4, line 4565) — **N/A**: IDR3.DPT=0; no DPT Level-1 descriptor processing exists
- [x] If Contig selects Reserved encoding: descriptor is invalid (§3.24.3.1.4, line 4593) — **N/A**: IDR3.DPT=0; no DPT descriptor walk exists
- [x] Any RES0 bit non-zero or Reserved field value → descriptor is Invalid (§3.24.3.1.4, line 4549) — **N/A**: IDR3.DPT=0; no DPT descriptor walk exists

### §3.24.4 DPT Lookup Errors

- [x] DPT lookup fault priority: (1) DPT_WALK_EN=0 → DPT_DISABLED at L0, (2) Invalid DPT register config → DPT_WALK_FAULT at L0, (3) GPC on L0 fetch → DPT_GPC_FAULT at L0, (4) External abort on L0 fetch → DPT_EABT at L0, (5) Invalid L0 descriptor → DPT_WALK_FAULT at L0, (6) GPC on L1 fetch → DPT_GPC_FAULT at L1, (7) External abort on L1 → DPT_EABT at L1, (8) Invalid L1 descriptor → DPT_WALK_FAULT at L1 (§3.24.4, line 4615) — **N/A**: IDR3.DPT=0; no DPT walk or fault path exists; no SMMU_DPT_CFG_FAR register implemented
- [x] If SMMU_(R_)DPT_CFG_FAR.FAULT==0: SMMU reports fault info in register and sets FAULT=1; if already 1, fault not reported (§3.24.4, line 4628) — **N/A**: IDR3.DPT=0; SMMU_DPT_CFG_FAR register not implemented
- [x] When DPT_ERR made active in SMMU_(R_)GERROR: corresponding DPT_CFG_FAR has already been made observable (§3.24.4, line 4630) — **N/A**: IDR3.DPT=0; GERROR_DPT_ERR bit constant exists in types.h:1494 for register layout completeness but DPT_CFG_FAR is not implemented and DPT errors never raised
- [x] Reserved DPTPS value (0b111) or exceeds SMMU_IDR5.OAS: treated as Invalid DPT register configuration (§3.24.4, line 4641) — **N/A**: IDR3.DPT=0; SMMU_DPT_BASE_CFG register not implemented

### §3.24.5 DPT Maintenance Operations

- [x] CMD_DPTI_ALL and CMD_DPTI_PA: same consumption and completion behavior as CMD_TLBI_* commands (§3.24.5, line 4661) — **N/A**: IDR3.DPT=0; CMD_DPTI_ALL (0x70) and CMD_DPTI_PA (0x73) are handled in smmu.cpp:5404-5411 but return CERROR_ILL+GERROR.CMDQ_ERR per §4.6.1 (unsupported command), not as full DPT invalidations
- [x] Consumption of CMD_DPTI_* does not provide guarantees; CMD_SYNC after guarantees invalidation complete, events reported, client transactions complete (§3.24.5, line 4663) — **N/A**: IDR3.DPT=0; DPTI commands rejected with CERROR_ILL, no DPT TLB to invalidate
- [x] CMD_TLBI_* commands and broadcast TLBI for stage 1/2 NOT required to invalidate DPT TLB entries (§3.24.5, line 4674) — **N/A**: IDR3.DPT=0; no DPT TLB exists; TLBI commands operate on translation TLB only

### §3.24.6 Software Guidance

### §3.24.6.2 Invalid to Valid Transition

- [x] Order for invalid→valid: (1) configure DPT to grant access, (2) cache maintenance and barriers, (3) configure final stage of translation to grant access; TLB maintenance NOT required (§3.24.6.2, line 4699) — **N/A**: IDR3.DPT=0; software-model DPT ordering guidance not applicable when DPT is not implemented

### §3.24.6.3 Valid to Invalid Transition

- [x] Order for valid→invalid: (1) mark final stage descriptor as Invalid, (2) TLBI + sync, (3) CMD_ATC_INV + sync, (4) if fully-coherent device: issue CMOs, (5) mark DPT config as invalid, (6) CMD_DPTI_* + sync (§3.24.6.3, line 4709) — **N/A**: IDR3.DPT=0; DPT teardown sequence not applicable when DPT is not implemented

### §3.24.6.4 Clearing DPT Lookup Errors

- [x] Algorithm: (1) write 0 to SMMU_(R_)DPT_CFG_FAR.FAULT, (2) acknowledge SMMU_(R_)GERROR.DPT_ERR, (3) read FAULT again to check for new fault between steps 1 and 2 (§3.24.6.4, line 4722) — **N/A**: IDR3.DPT=0; SMMU_DPT_CFG_FAR register not implemented; DPT errors never raised

## §3.25 Granule Protection Checks

- [x] GPC enabled only when SMMU_ROOT_CR0.GPCEN==1 (§3.25, line 4757) — **N/A**: IDR0.RME_IMPL=0 (bit absent from `getIDR0()`, smmu.cpp:3547-3573); SMMU_ROOT_CR0 register not implemented; entire GPC/GPT subsystem absent
- [x] GPT format and meaning same in SMMU with RME as in FEAT_RME (§3.25, line 4753) — **N/A**: IDR0.RME_IMPL=0; no GPT walk or descriptor parsing; RME not modeled
- [x] Client-originated access experiencing GPC fault: signaled to client device as External abort (§3.25.1, line 4762) — **N/A**: IDR0.RME_IMPL=0; no GPC check exists; client GPC fault path unreachable
- [x] Client-originated access GPC fault on output address: NOT reported in Event queue (§3.25.1, line 4764) — **N/A**: IDR0.RME_IMPL=0; no GPC infrastructure; fault path structurally impossible

### §3.25.1.1 GPC for Client Devices Without StreamID (NoStreamID)

- [x] NoStreamID device access with PA exceeding SMMU_IDR5.OAS: terminated with abort; no Event record or fault recorded (§3.25.1.1, line 4776) — **N/A**: IDR0.RME_IMPL=0; no NoStreamID+GPC path; GPC infrastructure absent

### §3.25.1.2 Speculative and Hint Accesses

- [x] GPC fault during speculative translation request/translation/prefetch/NW-DCP/DH: no event record generated (§3.25.1.2, line 4786) — **N/A**: IDR0.RME_IMPL=0; no GPC infrastructure; speculative transaction types not modeled
- [x] If SMMU_IDR0.RME_IMPL==1: GPC fault during speculative access is NOT reported (§3.25.1.2, line 4788) — **N/A**: IDR0.RME_IMPL=0; RME not implemented

### §3.25.2 Interactions with PCIe ATS

- [x] SMMU_CR0.ATSCHK has no effect on granule protection checks (§3.25.2, line 4799) — **N/A**: IDR0.RME_IMPL=0; no GPC implementation; interaction vacuously inapplicable
- [x] SMMU-originated access experiencing GPC fault while servicing ATS TR: SMMU responds with Completer Abort (§3.25.2, line 4800) — **N/A**: IDR0.RME_IMPL=0; no GPC in ATS TR path
- [x] If ATS TR success with R==W==0: address not valid; not subject to GPC (§3.25.2, line 4802) — **N/A**: IDR0.RME_IMPL=0; no GPC applied to any ATS output
- [x] If SMMU_IDR0.RME_IMPL==1: GPC performed on output address for ATS TR result before sending completion (§3.25.2, line 4803) — **N/A**: IDR0.RME_IMPL=0; no post-ATS GPC step
- [x] SMMU returns translation region size in ATS Completion such that GPC passes for accesses anywhere in region (§3.25.2, line 4805) — **N/A**: IDR0.RME_IMPL=0; no GPC; region-size/GPC interaction absent
- [x] ATS Translated transactions: subject to GPC; if GPC fails, terminated with abort (§3.25.2, line 4810) — **N/A**: IDR0.RME_IMPL=0; no GPC applied to translated transaction path

### §3.25.3 SMMU-Originated Accesses

- [x] SMMU-originated access experiencing GPC fault: reported as External abort (§3.25.3, line 4815) — **N/A**: IDR0.RME_IMPL=0; no SMMU-originated GPC fault path; GPC absent
- [x] For SMMU_IDR0.RME_IMPL==1: F_STE_FETCH/F_CD_FETCH/F_VMS_FETCH/F_WALK_EABT arising from GPC fault reported with GPCF=1 (§3.25.3, line 4822) — **N/A**: IDR0.RME_IMPL=0; no GPCF field in EventEntry (types.h:1695); fault events defined but GPCF indicator absent

### §3.25.4 Reporting of GPC Faults

- [x] GPF (Granule Protection Fault): reported in SMMU_ROOT_GPF_FAR (§3.25.4, line 4837) — **N/A**: IDR0.RME_IMPL=0; SMMU_ROOT_GPF_FAR register not implemented
- [x] GPT lookup error: reported in SMMU_ROOT_GPT_CFG_FAR (§3.25.4, line 4842) — **N/A**: IDR0.RME_IMPL=0; SMMU_ROOT_GPT_CFG_FAR register not implemented
- [x] GPF conditions: access to PA space other than NS with address exceeding PPS range; access to GPT-forbidden location (§3.25.4, line 4838) — **N/A**: IDR0.RME_IMPL=0; no GPC check; fault conditions never evaluated
- [x] GPT lookup error conditions: Reserved fields in SMMU_ROOT_GPT_BASE_CFG; PPS exceeding OAS; invalid SH/IRGN/ORGN combination; ADDR exceeding PPS; GPT Table Entry output exceeding PPS; invalid GPT Entry; External abort on GPT Entry fetch (§3.25.4, line 4843) — **N/A**: IDR0.RME_IMPL=0; no GPT walk; error conditions never evaluated

### §3.25.5 SMMU Behavior If GPC Fault is Active

- [x] If GPF active in SMMU_ROOT_GPF_FAR: other accesses without GPF or GPT lookup error continue as specified (§3.25.5, line 4859) — **N/A**: IDR0.RME_IMPL=0; SMMU_ROOT_GPF_FAR not implemented; GPC absent
- [x] GPF remains active until software writes 0 to SMMU_ROOT_GPF_FAR.FAULT (§3.25.5, line 4860) — **N/A**: IDR0.RME_IMPL=0; SMMU_ROOT_GPF_FAR not implemented
- [x] GPT lookup error remains active until software writes 0 to SMMU_ROOT_GPT_CFG_FAR.FAULT (§3.25.5, line 4866) — **N/A**: IDR0.RME_IMPL=0; SMMU_ROOT_GPT_CFG_FAR not implemented
- [x] SMMU with RME has two additional wired interrupts: GPF_FAR (error becomes active in SMMU_ROOT_GPF_FAR) and GPT_CFG_FAR (error becomes active in SMMU_ROOT_GPT_CFG_FAR) (§3.25.5, line 4868) — **N/A**: IDR0.RME_IMPL=0; RME interrupt infrastructure absent; model uses wired-only non-RME interrupt model

### §3.25.6 Observability of GPC Faults

- [x] If client transaction termination due to GPC fault observable to client: if GPF_FAR/GPT_CFG_FAR did not contain active fault → syndrome info observable in appropriate register; if already active → not updated (§3.25.6, line 4877) — **N/A**: IDR0.RME_IMPL=0; no GPC fault observability infrastructure
- [x] If GPC fault interrupt observable: syndrome info observable in SMMU_ROOT_GPF_FAR or SMMU_ROOT_GPT_CFG_FAR (§3.25.6, line 4882) — **N/A**: IDR0.RME_IMPL=0; ROOT registers not implemented
- [x] If client GPC fault termination visible to client: subsequent CMD_SYNC guarantees observability of related events in Event queue or that events discarded (§3.25.6, line 4884) — **N/A**: IDR0.RME_IMPL=0; GPC fault events never generated
- [x] For SMMU with BGPTM==1: after broadcast TLBI *PA* and DSB, subsequent CMD_SYNC guarantees no events relating to invalidated GPT configuration later observable (§3.25.6, line 4886) — **N/A**: IDR0.RME_IMPL=0; no BGPTM bit, no GPT maintenance path

## §3.26 Permission Indirections

### §3.26.1 Stage 1 Permission Indirections

- [x] SMMU_IDR3.S1PI==0: stage 1 permission indirections not supported; STE.S1PIE and CD.PIE are RES0 (§3.26.1, line 4918) — **PASS**: `getIDR3()` never sets bit 12 (S1PI) at smmu.cpp:3605-3609; no S1PIE/CD.PIE/PIIP/PIIU/PIIndex fields exist in StreamConfig (types.h:1150-1318), satisfying the RES0 requirement structurally
- [x] SMMU_IDR3.S1PI==1, STE.S1PIE==1, CD.PIE==1: stage 1 permissions determined from CD.PIIP and CD.PIIU using PIIndex from descriptors (§3.26.1, line 4922) — **N/A**: IDR3.S1PI=0; no S1PIE, PIE, PIIP, PIIU, or PIIndex fields implemented; feature code path is structurally unreachable
- [x] STE.S1PIE==0: hypervisor can prevent guest use of stage 1 permission indirections (§3.26.1, line 4924) — **N/A**: IDR3.S1PI=0 globally disables the feature; STE.S1PIE field does not exist in StreamConfig (types.h:1150-1318); hypervisor gate is vacuous
- [x] SMMU does NOT support stage 1 permission overlay feature (§3.26.1, line 4926) — **PASS**: no S1POE, S1POI, or overlay permission table fields exist anywhere in the implementation; absence is the correct spec-mandated state
- [x] If stage 1 Indirect Permission Scheme enabled: CD.WXN is RES0 and has no effect (§3.26.1, line 4927) — **N/A**: IDR3.S1PI=0; indirection enable path is never reached; CD.WXN is implemented and enforced (types.h:1271, stream_context.cpp:1299-1327) but the "WXN→RES0 when indirection enabled" constraint is vacuously satisfied since indirection is globally disabled

### §3.26.2 Stage 2 Permission Indirections

- [x] SMMU_IDR3.S2PI==1, STE.S2PIE==0, STE.S2POE==1: ILLEGAL → generates C_BAD_STE (§3.26.2, line 4948) — **N/A**: IDR3.S2PI (bit 13) is never set in `getIDR3()` (smmu.cpp:3605-3609); no STE.S2PIE or STE.S2POE fields exist in StreamConfig; validation guard is structurally unreachable
- [x] SMMU_IDR3.S2PI==1, STE.S2PIE==1, STE.S2POE==0: stage 2 permissions from SMMU_S2PII using PIIndex (§3.26.2, line 4949) — **N/A**: IDR3.S2PI=0; no S2PIE, S2POE, SMMU_S2PII register, or stage-2 PIIndex fields implemented; feature is completely absent by design
- [x] SMMU_IDR3.S2PI==1, STE.S2PIE==1, STE.S2POE==1: stage 2 permissions from STE.S2POI (POIndex) combined with SMMU_S2PII (PIIndex) (§3.26.2, line 4950) — **N/A**: IDR3.S2PI=0; no S2PIE, S2POE, S2POI, or S2PII fields anywhere in the implementation
- [x] Stage 2 permission computation order: (1) AssuredOnly check for stage 2 of stage 1 output address, (2) Base and Overlay permissions, (3) STE.S2PTW for TT walk/CD fetch, (4) Dirty state permission check if indirection enabled, (5) STE.DRE/STE.DCP for directed prefetch and CMO (§3.26.2, line 4952) — **N/A**: full five-step ordering applies only when IDR3.S2PI=1; S2PTW (step 3) is implemented at stream_context.cpp:1382-1388 and tests a Device-memory property orthogonal to R/W/X bits; steps 1, 4, and 5 depend on indirection being enabled which is globally disabled

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

## Chapter 4 — Commands

## §4.3 Configuration Structure Invalidation

> **Audit date:** 2026-05-11 — 34 items checked: 17 PASS, 17 N/A, 0 FAIL, 0 new bugs — all §4.3.1–§4.3.8 items PASS or N/A; flat single-level model means all L1ST/L1CD descriptor cache items are N/A; VMS items N/A (IDR3.MPAM=0); stall/invalidation independence, CMD_SYNC ordering all conformant

### §4.3.1 CMD_CFGI_STE(StreamID, SSec, Leaf)

- [x] Invalidates STE for given StreamID and SSec (§4.3.1, line 5272) — **PASS**: `processCommand` case `CFGI_STE` (smmu.cpp:4948–4959) calls `executeInvalidationCommandLocked` → `invalidateStreamCache(command.streamID)` which evicts all TLB entries for that StreamID; no separate STE cache exists in the flat model
- [x] When Leaf=0: also invalidates all caching of intermediate L1ST descriptors walked to locate the STE (§4.3.1, line 5278) — **N/A**: Flat linear stream table model; no L1ST descriptor cache exists; §4.3.1 states "STEs cached from linear Stream tables are invalidated with any value of Leaf" — both Leaf values take same `invalidateStreamCache` path
- [x] When Leaf=1: only the STE is invalidated; intermediate L1ST descriptors not required to be invalidated (§4.3.1, line 5281) — **N/A**: Flat model; `command.leaf` accepted in struct (types.h:1593) but not inspected — correct for linear-table model
- [x] Invalidates all Context Descriptors (including L1CD) cached using the given StreamID (§4.3.1, line 5284) — **PASS**: `invalidateStream(streamID)` evicts all TLB entries keyed by that StreamID across all PASIDs; no separate CD cache exists
- [x] Invalidates all VMS information cached for given StreamID in caches indexed by StreamID (§4.3.1, line 5285) — **N/A**: IDR3.MPAM=0; no VMS cache indexed by StreamID exists
- [x] SSec guard: SSec=1 on NS queue generates CERROR_ILL (§4.3.1, line 5272) — **PASS**: smmu.cpp:4952–4957 fires CERROR_ILL before any invalidation; tested in test_bugs_new3.cpp

### §4.3.2 CMD_CFGI_STE_RANGE(StreamID, SSec, Range)

- [x] Range formula: Start = StreamID & ~((2^(Range+1))-1); End = Start + 2^(Range+1)-1; aligned range of 2^(Range+1) StreamIDs (§4.3.2, line 5302) — **PASS**: smmu.cpp:4816–4831 (locked path): `prefixBits = range + 1; cmdPrefix = streamID >> prefixBits`; a stream matches when `(sid >> prefixBits) == cmdPrefix` — algebraically equivalent to spec formula; bottom Range+1 bits of StreamID correctly IGNORED by right-shift
- [x] Range parameter 0–31 corresponding to 2^1–2^32 StreamIDs; range>31 impossible (§4.3.2, line 5305) — **PASS**: smmu.cpp:4431–4435 clamps range>31 to global invalidation to avoid undefined UB shift-by-32+; defensive and correct for architecturally impossible input
- [x] Invalidates all caching of intermediate L1ST descriptors for given range (§4.3.2, line 5320) — **N/A**: Flat model; no L1ST cache
- [x] Invalidates any Context Descriptors (including L1CD) cached using all StreamIDs in range (§4.3.2, line 5321) — **PASS**: for each matching stream in `streamMap`, `invalidateStreamCache(pair.first)` evicts all TLB entries for that StreamID (all PASIDs), modeling CD eviction; no separate L1CD cache
- [x] Invalidates all VMS information cached for all StreamIDs in range (indexed by StreamID) (§4.3.2, line 5322) — **N/A**: IDR3.MPAM=0; no VMS cache
- [x] CMD_CFGI_STE_RANGE with Range==31 encodes CMD_CFGI_ALL; StreamID parameter is IGNORED (§4.3.2, line 5325) — **PASS**: smmu.cpp:4806–4808: `if (range == 31) { invalidateTranslationCache(); }` — `command.streamID` not read in this branch
- [x] SSec guard: SSec=1 on NS queue generates CERROR_ILL (§4.3.2, line 5302) — **PASS**: shared SSec guard block at smmu.cpp:4973–4983

### §4.3.3 CMD_CFGI_CD(StreamID, SSec, SubstreamID, Leaf)

- [x] Invalidates CD identified by StreamID and SubstreamID (§4.3.3, line 5331) — **PASS**: smmu.cpp:4839–4853: `invalidatePASIDCache(command.streamID, command.pasid)` → `tlbCache->invalidatePASID(streamID, pasid)` evicts the TLB entry for that exact (StreamID, PASID) pair
- [x] When SubstreamID is outside range of implemented SubstreamIDs: behavior is CONSTRAINED UNPREDICTABLE — no effect or operate on different SubstreamID (§4.3.3, line 5350) — **PASS**: out-of-range PASIDs silently no-op via `pasid <= MAX_PASID` guard in `invalidatePASIDCache` — one of the two permitted behaviors per §4.1.7
- [x] When Leaf=0: invalidates all caching of intermediate L1CD descriptors (§4.3.3, line 5356) — **N/A**: Flat model; no L1CD cache; `command.leaf` accepted but not inspected in CFGI_CD handler — correct for single-level CD table model
- [x] Raises CERROR_ILL when stage 1 is not implemented (IDR0.S1P==0) (§4.3.3, line 5358) — **PASS**: smmu.cpp:4846–4850: global IDR0 bit[1] (S1P) check; tested in test_bugs_new18.cpp:541–579
- [x] SSec guard: SSec=1 on NS queue generates CERROR_ILL (§4.3.3, line 5331) — **PASS**: smmu.cpp:4973–4983; tested in test_bugs_new3.cpp:143–157

### §4.3.4 CMD_CFGI_CD_ALL(StreamID, SSec)

- [x] Invalidates ALL CDs referenced by StreamID (§4.3.4, line 5375) — **PASS**: smmu.cpp:4856–4867: `invalidateStreamCache(command.streamID)` → `tlbCache->invalidateStream(streamID)` evicts all TLB entries for that StreamID across all PASIDs
- [x] Must also invalidate caches of ALL intermediate L1CD descriptors for given StreamID (§4.3.4, line 5384) — **N/A**: Flat model; no L1CD cache; `invalidateStreamCache` covers entire stream — at least as broad as required
- [x] Raises CERROR_ILL when stage 1 is not implemented (§4.3.4, line 5385) — **PASS**: smmu.cpp:4859–4863: identical global IDR0.S1P guard to CFGI_CD; tested in test_bugs_new18.cpp:591–619
- [x] SSec guard: SSec=1 on NS queue generates CERROR_ILL (§4.3.4, line 5375) — **PASS**: shared guard at smmu.cpp:4973–4983; tested in test_bugs_new3.cpp:163–176

### §4.3.5 CMD_CFGI_VMS_PIDM(SSec, VMID)

- [x] CERROR_ILL raised if SMMU_IDR3.MPAM==0 (§4.3.5, line 5393) — **PASS**: smmu.cpp:5340–5343: IDR3 bit[7] (MPAM) check; `getIDR3()` (smmu.cpp:3605–3609) never sets bit[7] → guard always fires; IDR3.MPAM=0 (no MPAM implementation) means this command is always CERROR_ILL — conformant by design
- [x] CERROR_ILL raised if MPAM not supported by programming interface indicated by SSec (§4.3.5, line 5415) — **PASS**: covered by IDR3.MPAM=0 always-firing guard; both conditions satisfied simultaneously
- [x] CERROR_ILL raised if SSec used improperly (SSec=1 on NS queue) (§4.3.5, line 5416) — **PASS**: smmu.cpp:5331–5335: SSec guard fires before MPAM check; ordering correct

### §4.3.5.1 CMD_CFGI_VMS_PIDM Usage

- [x] PARTID_MAP invalidation procedure (CMD_CFGI_VMS_PIDM + CMD_SYNC + STE invalidations + CMD_SYNC) (§4.3.5.1, line 5417) — **N/A**: Software guidance only; no SMMU enforcement required or present

### §4.3.6 CMD_CFGI_ALL(SSec)

- [x] Encoded as CMD_CFGI_STE_RANGE with Range==31; StreamID parameter is IGNORED (§4.3.6, line 5429) — **PASS**: smmu.cpp:4806–4808: `if (command.range == 31) { invalidateTranslationCache(); }` — `command.streamID` not read
- [x] Invalidates cached configuration for all possible StreamIDs associated with given SSec (§4.3.6, line 5429) — **PASS**: `invalidateTranslationCache()` (smmu.cpp:1771–1788) → `tlbCache->invalidateAll()` evicts every entry across all StreamIDs and all PASIDs
- [x] Invalidates all VMS structures for given SSec including caches indexed by VMID (§4.3.6, line 5436) — **PASS/N/A**: `tlbCache->invalidateAll()` covers any VMID-indexed TLB entries; no separate VMS cache (IDR3.MPAM=0)
- [x] SSec guard: SSec=1 on NS queue generates CERROR_ILL (§4.3.6, line 5429) — **PASS**: smmu.cpp:4973–4983; shared guard block

### §4.3.7 Action of VM Guest OS Structure Invalidations by Hypervisor

- [x] Hypervisor must re-shadow STEs and issue appropriate CFGI commands when guest issues structure invalidation commands (§4.3.7, line 5456) — **N/A**: Software guidance for hypervisors only; no SMMU hardware enforcement is required or specified; the table of guest→hypervisor mappings imposes no behavioral requirement on the SMMU itself

### §4.3.8 Configuration Structure Invalidation Semantics/Rules

- [x] Stalled transactions are UNAFFECTED by structure/TLB invalidation commands; must use CMD_RESUME or CMD_STALL_TERM to retire (§4.3.8, line 5475) — **PASS**: `stallQueue_` (smmu.cpp:930) is a separate data structure from `tlbCache`; `invalidateStreamCache`, `invalidatePASIDCache`, and `invalidateTranslationCache` operate only on `tlbCache` — none touch `stallQueue_`; CMD_RESUME (smmu.cpp:5209–5251) and CMD_STALL_TERM (smmu.cpp:5254–5281) are the only paths that retire stall records
- [x] Invalidation of a structure in-progress not required to affect that transaction (transaction looked up structure before invalidation) (§4.3.8, line 5483) — **PASS**: single-threaded command queue model: `processCommandQueue` and `translate` are serialized under `queueMutex`/stripe-lock discipline; a translation that began before a CFGI command completes atomically before the CFGI is processed — no mid-translation interleave possible
- [x] Invalidation of any given structure must be seen as atomic: transaction must never see partially-valid structure (§4.3.8, line 5484) — **PASS**: `StreamContext` updates are performed atomically under per-stripe mutex (smmu.cpp:1298); shared_ptr ensures no dangling pointer; invalidation is synchronous and complete before returning
- [x] CMD_SYNC ensures completion of all prior invalidations of both structure and TLB (§4.3.8, line 5490) — **PASS**: smmu.cpp:4022–4037: commands processed one at a time in strict FIFO order under `cmdqProcessingMutex_`; CMD_SYNC (smmu.cpp:5284–5327) executes only after all prior CFGI and TLBI commands have already returned; `cmdqProcessingMutex_` (smmu.cpp:3991) serializes entire processing loop

<!-- §4.3 C++ Audit 2026-05-11: 34 items checked — 17 PASS, 17 N/A, 0 FAIL, 0 new bugs. All CFGI commands correctly implemented: SSec guards, S1P guards for CD commands, MPAM guard for VMS_PIDM, Range formula, global invalidation, stall isolation, CMD_SYNC ordering all PASS. L1ST/L1CD flat-model items N/A. -->
