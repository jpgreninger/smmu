# Chapter 10: Performance Monitors Extension — Verification Checklist

> **Source:** ARM SMMU v3 Specification IHI0070G_b, Chapter 10
> **Purpose:** Verify C++ and Rust software models are operationally correct per spec
> **Legend:**
> - `[C++/Rust]` — applies to both models
> - `[MANDATORY]` — spec says MUST; failure = non-conformance
> - `[IMPL-DEF]` — implementation-defined; verify model documents and applies its choice consistently
> - `[L:XXXXX]` — line number in IHI0070G_b spec where the rule appears
> - `[ ]` — unchecked; `[x]` — verified

---

## 10.1 Support and Discovery

- [ ] **[C++/Rust]** **[IMPL-DEF]** PMCG is optional: verify model either implements PMCG or cleanly advertises its absence (no PMCG-related registers accessible). `[L:27878]`
- [ ] **[C++/Rust]** **[IMPL-DEF]** Each PMCG base address is implementation-defined: verify model documents and uses a fixed base address per group. `[L:27880]`
- [ ] **[C++/Rust]** **[IMPL-DEF]** PMCGs are standalone; verify model does not require PMCG to be physically inside the SMMU block. `[L:27880]`
- [ ] **[C++/Rust]** **[IMPL-DEF]** No centralized ID scheme: verify each PMCG has its own identification registers (IIDR, AIDR, ID_REGS) independent of other groups. `[L:27882]`

---

## 10.2 Overview of Counters and Groups

- [ ] **[C++/Rust]** **[MANDATORY]** Each PMCG has 1–64 counters: verify `SMMU_PMCG_CFGR.NCTR+1` is in range [1,64]. `[L:27891]`
- [ ] **[C++/Rust]** **[MANDATORY]** All counters in a group can count any event supported by that group: verify any counter index can be assigned any event in `SMMU_PMCG_CEID0/CEID1`. `[L:27891]`
- [ ] **[C++/Rust]** **[IMPL-DEF]** Different groups need not support the same events: verify model does not assert cross-group event identity. `[L:27891]`
- [ ] **[C++/Rust]** **[IMPL-DEF]** Counter group may be affiliated with a subset of StreamIDs: verify model documents which StreamIDs each group services. `[L:27900]`
- [ ] **[C++/Rust]** **[IMPL-DEF]** When `SMMUEN==0` for a Security state: verify model documents whether stream-filterable events are counted for that state's streams. `[L:27927]`
- [ ] **[C++/Rust]** **[IMPL-DEF]** For non-filterable events (e.g., Event 0): verify model documents whether they are counted when `SMMUEN==0`. `[L:27929]`
- [ ] **[C++/Rust]** **[MANDATORY]** Event 0 (clock cycle) is the ONLY architected event not filterable by StreamID: verify all other architected events (1–7) support StreamID filtering. `[L:27930]`
- [ ] **[C++/Rust]** **[IMPL-DEF]** Configuration change delay: verify model applies config register writes consistently (either immediately or with documented latency); accesses after write completion observe new config. `[L:27931]`

---

## 10.2.1 Overflow, Interrupts and Capture

- [ ] **[C++/Rust]** **[MANDATORY]** Counter overflow = carry-out past max unsigned value (wraps to smaller value): verify counter wraps correctly at its configured bit width (SIZE+1 bits). `[L:27937]`
- [ ] **[C++/Rust]** **[MANDATORY]** On overflow: `OVS[n]` bit is set in `SMMU_PMCG_OVSCLR0`/`SMMU_PMCG_OVSSET0`. `[L:27937]`
- [ ] **[C++/Rust]** **[MANDATORY]** Counter continues counting after overflow (OVS does NOT stop the counter). `[L:27937]`
- [ ] **[C++/Rust]** **[MANDATORY]** OVS state does NOT prevent overflow side effects (interrupt, capture) from occurring on the NEXT overflow. `[L:27943]`
- [ ] **[C++/Rust]** **[MANDATORY]** Per-counter group IRQ is triggered when: `SMMU_PMCG_IRQ_CTRL.IRQEN==1` AND counter n overflows AND `INTENSET0[n]==1`. `[L:27941]`
- [ ] **[C++/Rust]** **[IMPL-DEF]** IRQ type: wired edge-triggered and/or MSI; MSI support independent of core SMMU MSI capability (`SMMU_IDR0.MSI`). `[L:27945]`
- [ ] **[C++/Rust]** **[MANDATORY]** When `SMMU_PMCG_CFGR.CAPTURE==1`: capture mechanism is present; `SMMU_PMCG_SVRn` registers hold captured values. `[L:27949]`
- [ ] **[C++/Rust]** **[MANDATORY]** Capture trigger 1: write 1 to `SMMU_PMCG_CAPR.CAPTURE` → all `EVCNTRn` values copied to `SVRn`. `[L:27952]`
- [ ] **[C++/Rust]** **[MANDATORY]** Capture trigger 2: overflow of counter n with `EVTYPERn.OVFCAP==1` → capture triggered (post-overflow counter value is captured). `[L:27953]`
- [ ] **[C++/Rust]** **[MANDATORY]** When capture triggered by `CAPR` write: captured values are observable to an access that occurs after the write completes. `[L:27957]`
- [ ] **[C++/Rust]** **[IMPL-DEF]** Whether PMCG performs cache-coherent MSI writes when configured with cacheable/shareable attribute: `SMMU_IDR0.COHACC` does NOT apply to PMCG MSI writes. `[L:27963]`
- [ ] **[C++/Rust]** **[MANDATORY]** When overflow triggers interrupt: interrupt observable only AFTER all of: OVS update, new counter value, AND captured values (if capture was also triggered) are observable. `[L:27965]`

---

## 10.3 Monitor Events

- [ ] **[C++/Rust]** **[MANDATORY]** Event IDs 0x0000–0x007F are architected; 0x0080–0xFFFF are implementation-defined. `[L:27974]`
- [ ] **[C++/Rust]** **[MANDATORY]** Event 0 (Clock cycle): mandatory, NOT filterable by StreamID. `[L:27983]`
- [ ] **[C++/Rust]** **[MANDATORY]** Event 1 (Translation/request): mandatory, filterable by StreamID. `[L:27984]`
- [ ] **[C++/Rust]** **[MANDATORY]** Event 2 (TLB miss): mandatory, filterable by StreamID. `[L:27985]`
- [ ] **[C++/Rust]** **[MANDATORY]** Event 3 (Config cache miss): mandatory, filterable by StreamID. `[L:27992]`
- [ ] **[C++/Rust]** **[MANDATORY]** Event 4 (Translation table walk access): mandatory, filterable by StreamID. `[L:27993]`
- [ ] **[C++/Rust]** **[MANDATORY]** Event 5 (Configuration structure access): mandatory, filterable by StreamID. `[L:27994]`
- [ ] **[C++/Rust]** **[MANDATORY]** Event 6 (PCIe ATS Translation Request received): mandatory if ATS supported, filterable by StreamID. `[L:27995]`
- [ ] **[C++/Rust]** **[MANDATORY]** Event 7 (PCIe ATS Translated Transaction through SMMU): mandatory if ATS supported, filterable by StreamID. `[L:27996]`
- [ ] **[C++/Rust]** **[IMPL-DEF]** Events 1,2,3,7: model documents whether terminated transactions are included; Arm recommends all-or-none across these four events. `[L:27999]`
- [ ] **[C++/Rust]** **[IMPL-DEF]** Events 1,2,3: model documents whether speculative/prefetch transactions, ATOS requests, and CMOs are included; Arm recommends each option applies to all-or-none of events 1,2,3. `[L:28000]`
- [ ] **[C++/Rust]** **[MANDATORY]** If GPC enabled: Event IDs 2 and 4 count GPT accesses when a GPT access is performed; other architected events do NOT count GPT accesses. `[L:28007]`
- [ ] **[C++/Rust]** **[MANDATORY]** If DPT enabled: Event IDs 2 and 4 count DPT lookups for components that support DPT checks. `[L:28009]`
- [ ] **[C++/Rust]** **[IMPL-DEF]** Retried transactions: model documents whether retried transactions are re-counted; if re-counted, all relevant events are re-counted as a new transaction. `[L:28019]`
- [ ] **[C++/Rust]** **[MANDATORY]** When event is mandatory: (a) supported in at least one counter group; (b) supported in enough groups that it is countable for ALL possible StreamIDs in use. `[L:28020]`

---

## 10.4 StreamIDs and Filtering

- [ ] **[C++/Rust]** **[MANDATORY]** Non-filterable events (e.g., Event 0) are always counted regardless of StreamID filter configuration. `[L:28028]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_CFGR.SID_FILTER_TYPE==0`: each counter n uses its own `EVTYPERn.{FILTER_SID_SPAN, FILTER_SEC_SID}` and `SMRn.STREAMID`. `[L:28032]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_CFGR.SID_FILTER_TYPE==1`: `EVTYPER0.{FILTER_SID_SPAN, FILTER_SEC_SID}` and `SMR0.STREAMID` apply to ALL counters; `EVTYPERx` fields (x≥1) are RES0. `[L:28034]`

### Filtering Modes

- [ ] **[C++/Rust]** **[MANDATORY]** ExactSID mode (`FILTER_SID_SPAN==0`): counter increments only for events with StreamID exactly matching `SMRn.STREAMID`. `[L:28046]`
- [ ] **[C++/Rust]** **[MANDATORY]** PartialSID mode (`FILTER_SID_SPAN==1`, STREAMID neither all-ones nor all-ones-except-MSB): `STREAMID[Y-1]==0` (MSB of don't-care group); `STREAMID[(Y-2):0]` all 1s (where Y>1); upper bits must match event StreamID. `[L:28053]`
- [ ] **[C++/Rust]** **[MANDATORY]** PartialSID example: `...0111` matches `...xxxx` (4 LSBs don't care); `...0110` matches `...011x` (1 LSB don't care). `[L:28058]`
- [ ] **[C++/Rust]** **[MANDATORY]** AllSIDOneSECSID mode (`FILTER_SID_SPAN==1`, STREAMID=all-ones except MSB): matches all StreamIDs of ONE Security state (PartialSID special case where Y==`SMMU_IDR0.SIDSIZE`). `[L:28070]`
- [ ] **[C++/Rust]** **[MANDATORY]** AllSIDManySECSID mode (`FILTER_SID_SPAN==1`, STREAMID=all-ones in all implemented bits): matches any StreamID regardless of Security state. `[L:28072]`
- [ ] **[C++/Rust]** **[MANDATORY]** Writing 0xFFFFFFFE to `SMRn.STREAMID` sets all implemented bits (Arm recommendation to avoid needing to know field size). `[L:28074]`

### Secure State Filtering

- [ ] **[C++/Rust]** **[MANDATORY]** `EVTYPERn.FILTER_SEC_SID==0`: count Non-secure StreamID events; `==1`: count Secure StreamID events. `[L:28079]`
- [ ] **[C++/Rust]** **[MANDATORY]** When `SCR.SO==0` (Secure observation disabled): `FILTER_SEC_SID` is effectively forced to 0 (only Non-secure events counted). `[L:28079]`
- [ ] **[C++/Rust]** **[MANDATORY]** AllSIDManySECSID with all-ones STREAMID: if `SO==1` matches both Secure and NS; if `SO==0` matches only NS. `[L:28080]`
- [ ] **[C++/Rust]** **[IMPL-DEF]** SMMUv3.0 only: whether AllSIDManySECSID matches one namespace or both when SO enabled is implementation-defined. `[L:28080]`
- [ ] **[C++/Rust]** **[MANDATORY]** AllSIDOneSECSID (MSB=0, rest=1): matches all StreamIDs of ONE namespace as determined by `FILTER_SEC_SID`. `[L:28083]`

### Realm State Filtering (when PMCG implements Realm)

- [ ] **[C++/Rust]** **[MANDATORY]** In ExactSID/PartialSID/AllSIDOneSECSID: Rel=`FILTER_REALM_SID & ROOTCR.RLO`; Sec=`FILTER_SEC_SID & SCR.SO`. `[L:28085]`
- [ ] **[C++/Rust]** **[MANDATORY]** Rel=0, Sec=0 → count only Non-secure events. `[L:28092]`
- [ ] **[C++/Rust]** **[MANDATORY]** Rel=0, Sec=1 → count only Secure events. `[L:28093]`
- [ ] **[C++/Rust]** **[MANDATORY]** Rel=1, Sec=0 → count only Realm events. `[L:28094]`
- [ ] **[C++/Rust]** **[MANDATORY]** Rel=1, Sec=1 → Reserved; behaves as {0,0} (Non-secure only). `[L:28095]`
- [ ] **[C++/Rust]** **[MANDATORY]** In AllSIDManySECSID mode: StreamID filtering NOT applied; SEC_SID filtering per: `FILTER_REALM_SID==0` → NS counted, if `SO==1` Secure also counted. `[L:28104]`
- [ ] **[C++/Rust]** **[MANDATORY]** AllSIDManySECSID + `FILTER_REALM_SID==1`, `FILTER_SEC_SID==0` → NS counted; if `RLO==1` Realm also counted. `[L:28111]`
- [ ] **[C++/Rust]** **[MANDATORY]** AllSIDManySECSID + `FILTER_REALM_SID==1`, `FILTER_SEC_SID==1` → NS; if `SO==1` Secure; if `RLO==1` Realm; if `RTO==1` NoStreamID Root PA also counted. `[L:28114]`
- [ ] **[C++/Rust]** **[MANDATORY]** NoStreamID accesses: target PA space treated as Security state; only counted in AllSIDOneSECSID and AllSIDManySECSID modes (NOT ExactSID or PartialSID). `[L:28120]`

---

## 10.4.1 Counter Group StreamID Size

- [ ] **[C++/Rust]** **[MANDATORY]** Low-order PMCG StreamID bits [N:0] MUST equal SMMU StreamID[N:0]. `[L:28126]`
- [ ] **[C++/Rust]** **[IMPL-DEF]** `SMRn.STREAMID` may implement fewer bits than `SMMU_IDR1.SIDSIZE`; implemented size is implementation-defined. `[L:28126]`
- [ ] **[C++/Rust]** **[MANDATORY]** Software must NOT depend on `SMRn.STREAMID` readback returning full SMMU StreamID; unimplemented bits read as zero (RAZ/WI). `[L:28130]`
- [ ] **[C++/Rust]** **[IMPL-DEF]** Association between PMCG StreamID span and overall SMMU StreamID namespace is implementation-defined; model must document this mapping. `[L:28126]`

---

## 10.4.2 Counting of NoStreamID Accesses

- [ ] **[C++/Rust]** **[MANDATORY]** NoStreamID accesses counted only if ALL: (1) counting for their effective Security state is enabled; (2) `FILTER_SID_SPAN==1` AND STREAMID programmed to count all streams AND that selection includes effective Security state (STREAMID all-ones, OR all-ones except MSB). `[L:28134]`
- [ ] **[C++/Rust]** **[MANDATORY]** If `FILTER_REALM_SID==0` (or not implemented): no PMCG events counted for NoStreamID devices to Root or Realm PA spaces. `[L:28142]`
- [ ] **[C++/Rust]** **[MANDATORY]** When permitted, NoStreamID accesses can count: Event ID 1 (transaction), Event IDs 2 and 4 (GPT events), implementation-defined GPT events. `[L:28144]`

---

## 10.4.3 PARTID- and PMG-based Filtering

- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_CFGR.FILTER_PARTID_PMG==1`: PMCG supports MPAM PARTID and PMG filtering (RES0 before SMMUv3.3). `[L:28157]`
- [ ] **[C++/Rust]** **[MANDATORY]** PARTID/PMG filtering enabled independently via `EVTYPERn.{FILTER_PARTID, FILTER_PMG, FILTER_MPAM_NS}`. `[L:28160]`
- [ ] **[C++/Rust]** **[MANDATORY]** In SMMU with RME DA: `FILTER_MPAM_NS` replaced by 2-bit `FILTER_MPAM_SP`; values 0b00 and 0b01 directly match first two `FILTER_MPAM_NS` values. `[L:28162]`
- [ ] **[C++/Rust]** **[MANDATORY]** If `FILTER_PARTID==1` OR `FILTER_PMG==1`: `FILTER_SID_SPAN` is IGNORED; events NOT filtered by StreamID. `[L:28163]`
- [ ] **[C++/Rust]** **[MANDATORY]** If `FILTER_PARTID==0` AND `FILTER_PMG==0`: `FILTER_MPAM_NS` is IGNORED. `[L:28553]`
- [ ] **[C++/Rust]** **[MANDATORY]** PARTID filter: compare `SMRn.PARTID` to output PARTID of each transaction/SMMU-originated access (sourced from GBPMPAM, GMPAM, STE, or CD). `[L:28164]`
- [ ] **[C++/Rust]** **[MANDATORY]** PMG filter: compare `SMRn.PMG` to output PMG of each transaction (sourced from GBPMPAM, STE, or CD). `[L:28166]`
- [ ] **[C++/Rust]** **[MANDATORY]** Event 0: cannot filter by PARTID or PMG. `[L:28178]`
- [ ] **[C++/Rust]** **[MANDATORY]** Events 1, 2, 4, 6, 7: can filter by PARTID or PMG. `[L:28179]`
- [ ] **[C++/Rust]** **[IMPL-DEF]** Events 3, 5: implementation-defined whether filterable by PARTID/PMG. `[L:28168]`
- [ ] **[C++/Rust]** **[MANDATORY]** If PMCG configured to filter by PARTID/PMG but SMMU doesn't support it for that event: event counted WITHOUT filtering (not suppressed). `[L:28171]`
- [ ] **[C++/Rust]** **[MANDATORY]** If configured PARTID/PMG values exceed `SMMU_PMCG_(S_)MPAMIDR` limits: NO events counted. `[L:28174]`

---

## 10.4.4 Counting of Non-Attributable Events

- [ ] **[C++/Rust]** **[MANDATORY]** None of the architected SMMUv3 events are non-attributable; verify model does not incorrectly classify them. `[L:28198]`
- [ ] **[C++/Rust]** **[MANDATORY]** If Realm NOT implemented AND Secure IS implemented: non-attributable events counted only if `SCR.SO==1`. `[L:28199]`
- [ ] **[C++/Rust]** **[MANDATORY]** If Realm programming interface present: non-attributable events counted only if ALL of: `ROOTCR.NAO==1` AND (`SCR.SO==1` OR `SCR.NAO==1`). `[L:28201]`

---

## 10.5 Registers — Physical Layout

- [ ] **[C++/Rust]** **[MANDATORY]** Each PMCG occupies one 4KB Page 0; optional additional 4KB Page 1 (both at implementation-defined base addresses). `[L:28213]`
- [ ] **[C++/Rust]** **[MANDATORY]** Page 1 present when `SMMU_PMCG_CFGR.RELOC_CTRS==1`: `EVCNTRn`, `SVRn`, `OVSCLR0`, `OVSSET0`, `CAPR` relocated to Page 1 at same offsets; Page 0 locations become RES0. `[L:28215]`
- [ ] **[C++/Rust]** **[MANDATORY]** All registers are little-endian. `[L:28223]`
- [ ] **[C++/Rust]** **[MANDATORY]** Aligned 32-bit access is permitted to 64-bit registers. `[L:28221]`
- [ ] **[C++/Rust]** **[IMPL-DEF]** Whether 64-bit accesses to 64-bit registers are atomic is implementation-defined. `[L:28222]`
- [ ] **[C++/Rust]** **[MANDATORY]** Software permitted to read/write any register at any time (unless noted); writes to read-only registers are ignored. `[L:28225]`

---

## 10.5.1 Page 0 Address Map

- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_EVCNTRn`: offset `0x000 + stride*n`; stride=4 if `SIZE<=31`, stride=8 if `SIZE>31`. `[L:28233]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_EVTYPERn`: offset `0x400 + 4*n`. `[L:28234]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_SVRn`: offset `0x600 + stride*n` (same stride as EVCNTRn). `[L:28235]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_SMRn`: offset `0xA00 + 4*n`. `[L:28236]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_CNTENSET0`: offset `0xC00`. `[L:28243]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_CNTENCLR0`: offset `0xC20`. `[L:28244]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_INTENSET0`: offset `0xC40`. `[L:28245]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_INTENCLR0`: offset `0xC60`. `[L:28246]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_OVSCLR0`: offset `0xC80`. `[L:28247]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_OVSSET0`: offset `0xCC0`. `[L:28248]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_CAPR`: offset `0xD88`. `[L:28249]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_SCR`: offset `0xDF8` (Secure only); alias at `0xE40` when `ROOTCR_IMPL==1`. `[L:28250]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_CFGR`: offset `0xE00`. `[L:28251]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_CR`: offset `0xE04`. `[L:28252]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_IIDR`: offset `0xE08`. `[L:28253]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_CEID0`: offset `0xE20`. `[L:28254]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_CEID1`: offset `0xE28`. `[L:28255]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_ROOTCR`: offset `0xE48` (Root access RW; others RO). `[L:28257]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_IRQ_CTRL`: offset `0xE50`. `[L:28258]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_IRQ_CTRLACK`: offset `0xE54`. `[L:28259]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_IRQ_CFG0`: offset `0xE58`. `[L:28260]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_IRQ_CFG1`: offset `0xE60`. `[L:28261]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_IRQ_CFG2`: offset `0xE64`. `[L:28262]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_IRQ_STATUS`: offset `0xE68`. `[L:28263]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_GMPAM`: offset `0xE6C`. `[L:28264]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_AIDR`: offset `0xE70`. `[L:28265]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_MPAMIDR`: offset `0xE74`. `[L:28266]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_S_MPAMIDR`: offset `0xE78` (Secure/Root only). `[L:28267]`

---

## 10.5.2.1 SMMU_PMCG_EVCNTR\<n\> (n=0–63)

- [ ] **[C++/Rust]** **[MANDATORY]** 32-bit when `CFGR.SIZE<=31`; 64-bit when `SIZE>31`. `[L:28300]`
- [ ] **[C++/Rust]** **[MANDATORY]** Counter bit width R = `SIZE+1`; if R<32: bits[31:R] are RES0; if R>32 and R<64: bits[63:R] are RES0. `[L:28322]`
- [ ] **[C++/Rust]** **[MANDATORY]** Counter increments when: (a) `EVTYPERn.EVENT` matches, (b) `CNTENSET0[n]==1` AND `CR.E==1`, (c) StreamID filter matches if event is filterable. `[L:28302]`
- [ ] **[C++/Rust]** **[MANDATORY]** Counter increment is atomic with respect to external writes to this register. `[L:28304]`
- [ ] **[C++/Rust]** **[MANDATORY]** Resets to UNKNOWN value. `[L:28328]`
- [ ] **[C++/Rust]** **[MANDATORY]** Unimplemented counter registers (n ≥ NCTR+1) are RES0. `[L:28306]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access when PMCG supports Secure AND `SCR.NSRA==0` → RAZ/WI; otherwise RW. `[L:28349]`

---

## 10.5.2.2 SMMU_PMCG_EVTYPER\<n\> (n=0–63)

- [ ] **[C++/Rust]** **[MANDATORY]** 32-bit; offset `0x400 + 4*n`. `[L:28375]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[31] `OVFCAP`: when `CFGR.CAPTURE==1`: overflow of counter n triggers group capture (same effect as CAPR write). When `CAPTURE==0`: RES0. Resets to UNKNOWN. `[L:28381]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[30] `FILTER_SEC_SID`: when n==0 or `SID_FILTER_TYPE==0`: 0=NS events, 1=Secure events. RES0 if Secure not implemented. Effectively 0 if `SCR.SO==0`. Otherwise RES0. Resets to UNKNOWN. `[L:28399]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[29] `FILTER_SID_SPAN`: when n==0 or `SID_FILTER_TYPE==0`: 0=ExactSID, 1=span/mask encoding. Otherwise RES0. Resets to UNKNOWN. `[L:28420]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[28] `FILTER_REALM_SID`: when `ROOTCR_IMPL==1` and (n==0 or `SID_FILTER_TYPE==0`): Realm filtering enable. If `RLO==0`: treated as 0 for all purposes except readback of the bit. Otherwise RES0. Resets to UNKNOWN. `[L:28440]`
- [ ] **[C++/Rust]** **[MANDATORY]** bits[27:20]: RES0. `[L:28462]`
- [ ] **[C++/Rust]** **[MANDATORY]** bits[19:18] `FILTER_MPAM_SP`: when `FILTER_PARTID_PMG==1` and (n==0 or `SID_FILTER_TYPE==0`): MPAM_SP selection (00=Secure if SO else NS; 01=NS; 10=reserved→00; 11=Realm if RLO else NS). bit[19] RES0 if `ROOTCR_IMPL==0`. Otherwise RES0. Resets to UNKNOWN. `[L:28466]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[17] `FILTER_PMG`: when `FILTER_PARTID_PMG==1` and (n==0 or `SID_FILTER_TYPE==0`): filter by `SMRn.PMG`. Otherwise RES0. Resets to UNKNOWN. `[L:28491]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[16] `FILTER_PARTID`: when `FILTER_PARTID_PMG==1` and (n==0 or `SID_FILTER_TYPE==0`): filter by `SMRn.PARTID`. Otherwise RES0. Resets to UNKNOWN. `[L:28511]`
- [ ] **[C++/Rust]** **[MANDATORY]** bits[15:0] `EVENT`: event type; implementation-defined number of low-order bits implemented; unimplemented upper bits RES0. Resets to UNKNOWN. `[L:28529]`
- [ ] **[C++/Rust]** **[MANDATORY]** When `SID_FILTER_TYPE==1`: `FILTER_SID_SPAN`, `FILTER_SEC_SID`, `FILTER_PARTID`, `FILTER_PMG`, `FILTER_MPAM_NS` only present in EVTYPER0; in EVTYPERx (x≥1) these fields are RES0. `[L:28543]`
- [ ] **[C++/Rust]** **[MANDATORY]** `FILTER_PARTID==1` OR `FILTER_PMG==1`: `FILTER_SID_SPAN` IGNORED; events not filtered by StreamID. `[L:28553]`
- [ ] **[C++/Rust]** **[MANDATORY]** `FILTER_PARTID==0` AND `FILTER_PMG==0`: `FILTER_MPAM_NS` IGNORED. `[L:28555]`
- [ ] **[C++/Rust]** **[MANDATORY]** Unimplemented counter EVTYPERn are RES0. `[L:28560]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access when PMCG supports Secure AND `SCR.NSRA==0` → RAZ/WI; otherwise RW. `[L:28567]`

---

## 10.5.2.3 SMMU_PMCG_SVR\<n\> (n=0–63)

- [ ] **[C++/Rust]** **[MANDATORY]** Present ONLY when `CFGR.CAPTURE==1`; direct accesses when `CAPTURE==0` are RES0. `[L:28587]`
- [ ] **[C++/Rust]** **[MANDATORY]** 32-bit when `SIZE<=31`; 64-bit otherwise; same stride as EVCNTRn; offset `0x600 + stride*n`. `[L:28581]`
- [ ] **[C++/Rust]** **[MANDATORY]** Holds captured values from corresponding `EVCNTRn` after capture event. `[L:28583]`
- [ ] **[C++/Rust]** **[MANDATORY]** Read-only (RO); unimplemented SVRn are RES0. `[L:28583]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access when PMCG supports Secure AND `SCR.NSRA==0` → RAZ/WI; otherwise RO. `[L:28631]`

---

## 10.5.2.4 SMMU_PMCG_SMR\<n\> (n=0–63)

- [ ] **[C++/Rust]** **[MANDATORY]** Present ONLY when n==0 or `SID_FILTER_TYPE==0`; direct accesses otherwise are RES0. `[L:28653]`
- [ ] **[C++/Rust]** **[MANDATORY]** 32-bit; offset `0xA00 + 4*n`. `[L:28657]`
- [ ] **[C++/Rust]** **[MANDATORY]** When `FILTER_PARTID==1` or `FILTER_PMG==1`: bits[31:24]=RES0; `PMG`=bits[23:16]; `PARTID`=bits[15:0]. `[L:28662]`
- [ ] **[C++/Rust]** **[MANDATORY]** Otherwise: `STREAMID`=bits[31:0]; implements implementation-defined contiguous bits from bit 0 upward; unimplemented bits RAZ/WI. `[L:28690]`
- [ ] **[C++/Rust]** **[MANDATORY]** Unimplemented counter SMRn are RES0. `[L:28714]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access when PMCG supports Secure AND `SCR.NSRA==0` → RAZ/WI; otherwise RW. `[L:28721]`

---

## 10.5.2.5 SMMU_PMCG_CNTENSET0

- [ ] **[C++/Rust]** **[MANDATORY]** 64-bit; offset `0xC00`; W1S semantics. `[L:28739]`
- [ ] **[C++/Rust]** **[MANDATORY]** `CNTEN[n]==1` AND `CR.E==1` → counter n is enabled. `[L:28756]`
- [ ] **[C++/Rust]** **[MANDATORY]** Write 1 to bit n sets enable; read returns current state; bits beyond `NCTR+1` are RES0. `[L:28758]`
- [ ] **[C++/Rust]** **[MANDATORY]** Resets to UNKNOWN. `[L:28748]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access blocked if `SCR.NSRA==0` → RAZ/WI; otherwise RW. `[L:28765]`

---

## 10.5.2.6 SMMU_PMCG_CNTENCLR0

- [ ] **[C++/Rust]** **[MANDATORY]** 64-bit; offset `0xC20`; W1C semantics. `[L:28783]`
- [ ] **[C++/Rust]** **[MANDATORY]** Write 1 to bit n clears enable; read returns state; bits beyond `NCTR+1` are RES0. `[L:28800]`
- [ ] **[C++/Rust]** **[MANDATORY]** Resets to UNKNOWN. Non-secure access blocked if `SCR.NSRA==0` → RAZ/WI. `[L:28808]`

---

## 10.5.2.7 SMMU_PMCG_INTENSET0

- [ ] **[C++/Rust]** **[MANDATORY]** 64-bit; offset `0xC40`; W1S semantics; `INTEN[n]` bits. `[L:28826]`
- [ ] **[C++/Rust]** **[MANDATORY]** Overflow of counter n triggers PMCG IRQ if `INTEN[n]==1` AND `IRQ_CTRL.IRQEN==1`. `[L:28846]`
- [ ] **[C++/Rust]** **[MANDATORY]** Write 1 sets; read returns state; bits beyond `NCTR+1` are RES0. Resets to UNKNOWN. `[L:28844]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access blocked if `SCR.NSRA==0` → RAZ/WI. `[L:28848]`

---

## 10.5.2.8 SMMU_PMCG_INTENCLR0

- [ ] **[C++/Rust]** **[MANDATORY]** 64-bit; offset `0xC60`; W1C semantics. `[L:28857]`
- [ ] **[C++/Rust]** **[MANDATORY]** Write 1 clears interrupt enable; bits beyond `NCTR+1` are RES0. Resets to UNKNOWN. `[L:28857]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access blocked if `SCR.NSRA==0` → RAZ/WI. `[L:28857]`

---

## 10.5.2.9 SMMU_PMCG_OVSCLR0

- [ ] **[C++/Rust]** **[MANDATORY]** 64-bit; offset `0xC80`; W1C semantics; `OVS[n]` bits. `[L:28912]`
- [ ] **[C++/Rust]** **[MANDATORY]** Counter n overflow (wraps past max unsigned value) sets `OVS[n]`. `[L:28933]`
- [ ] **[C++/Rust]** **[MANDATORY]** Write 1 clears OVS bit; read returns overflow status; bits beyond `NCTR+1` are RES0. Resets to UNKNOWN. `[L:28931]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access blocked if `SCR.NSRA==0` → RAZ/WI. `[L:28940]`

---

## 10.5.2.10 SMMU_PMCG_OVSSET0

- [ ] **[C++/Rust]** **[MANDATORY]** 64-bit; offset `0xCC0`; W1S semantics. `[L:28958]`
- [ ] **[C++/Rust]** **[MANDATORY]** Write 1 sets OVS bit (software-initiated); bits beyond `NCTR+1` are RES0. Resets to UNKNOWN. `[L:28977]`
- [ ] **[C++/Rust]** **[IMPL-DEF]** Whether software-set OVS triggers interrupt and/or capture side effects is implementation-specific. `[L:28979]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access blocked if `SCR.NSRA==0` → RAZ/WI. `[L:28986]`

---

## 10.5.2.11 SMMU_PMCG_CAPR

- [ ] **[C++/Rust]** **[MANDATORY]** 32-bit; offset `0xD88`; Write-only; reads as zero; resets to 0. `[L:29004]`
- [ ] **[C++/Rust]** **[MANDATORY]** bits[31:1] RES0; bit[0] `CAPTURE`. `[L:29013]`
- [ ] **[C++/Rust]** **[MANDATORY]** Write 1 to `CAPTURE` triggers capture of all `EVCNTRn` → `SVRn` (only when `CFGR.CAPTURE==1`). `[L:29021]`
- [ ] **[C++/Rust]** **[MANDATORY]** When `CFGR.CAPTURE==0`: `CAPTURE` field is RES0. `[L:29023]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access blocked if `SCR.NSRA==0` → RAZ/WI; otherwise WO. `[L:29035]`

---

## 10.5.2.12 SMMU_PMCG_SCR

- [ ] **[C++/Rust]** **[MANDATORY]** Present only when PMCG supports Secure state; else register is RAZ/WI. `[L:29049]`
- [ ] **[C++/Rust]** **[MANDATORY]** 32-bit; offset `0xDF8`; alias at `0xE40` when `ROOTCR_IMPL==1`. `[L:29180]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-Secure and non-Root accesses → RAZ/WI; Secure/Root → RW. `[L:29182]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[31] `READS_AS_ONE`: always reads as 1 (RO); allows Secure software to detect Secure state support. `[L:29060]`
- [ ] **[C++/Rust]** **[MANDATORY]** bits[30:5] RES0. `[L:29069]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[4] `NAO`: when `ROOTCR_IMPL==1`: permit counting non-attributable events. Resets to 0. `[L:29073]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[3] `MSI_MPAM_NS`: when `S_MPAMIDR.HAS_MPAM_NS==1`: 0=Secure PA uses Secure PARTID space; 1=Non-secure PARTID space. RES0 if NSMSI+NSRA configure NS MSIs. Resets to 0. `[L:29096]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[2] `NSMSI`: when `CFGR.MSI==1`: 0=MSIs target Secure PA; 1=Non-secure PA. Resets to 1. `[L:29114]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[1] `NSRA`: 0=Non-secure register access disabled (all NS→RAZ/WI); 1=enabled. Resets to 1. `[L:29133]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[0] `SO`: Secure observation; 0=disabled; 1=enabled. Resets to 0. `[L:29146]`
- [ ] **[C++/Rust]** **[MANDATORY]** MSI target NS field: `NS = NSMSI | NSRA`. `[L:30326]`
- [ ] **[C++/Rust]** **[MANDATORY]** If PMCG doesn't implement Secure but system does: both Secure and NS accesses permitted; MSIs must target NS PA space. `[L:29165]`

---

## 10.5.2.13 SMMU_PMCG_CFGR

- [ ] **[C++/Rust]** **[MANDATORY]** 32-bit; offset `0xE00`; RO. `[L:29205]`
- [ ] **[C++/Rust]** **[MANDATORY]** bits[31:26] RES0. `[L:29212]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[25] `FILTER_PARTID_PMG`: RES0 before SMMUv3.3; 1=supports PARTID/PMG event filtering. `[L:29216]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[24] `MPAM`: when `MSI==1`: 1=MPAM supported for MSI PARTID/PMG. RES0 before SMMUv3.2. (Note: MPAM here = MSI PARTID/PMG tagging, distinct from `FILTER_PARTID_PMG`.) `[L:29226]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[23] `SID_FILTER_TYPE`: 0=per-counter filter; 1=shared filter (SMR0/EVTYPER0 for all counters). `[L:29245]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[22] `CAPTURE`: 0=capture not supported (`SVRn` and `CAPR` are RES0); 1=supported. `[L:29254]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[21] `MSI`: 0=no MSI; 1=MSI supported (independent of core SMMU `SMMU_IDR0.MSI`). `[L:29263]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[20] `RELOC_CTRS`: 1=Page 1 present; Page 0 locations of EVCNTRn, SVRn, OVSCLR0, OVSSET0, CAPR become RES0. `[L:29272]`
- [ ] **[C++/Rust]** **[MANDATORY]** bits[19:14] RES0. `[L:29284]`
- [ ] **[C++/Rust]** **[MANDATORY]** bits[13:8] `SIZE`: valid values only: 0b011111(31), 0b100011(35), 0b100111(39), 0b101011(43), 0b101111(47), 0b111111(63); all others reserved. `[L:29288]`
- [ ] **[C++/Rust]** **[MANDATORY]** bits[7:6] RES0. `[L:29306]`
- [ ] **[C++/Rust]** **[MANDATORY]** bits[5:0] `NCTR`: number of counters = `NCTR+1` (range: 1–64). `[L:29310]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access blocked if `SCR.NSRA==0` → RAZ/WI; otherwise RO. `[L:29326]`

---

## 10.5.2.14 SMMU_PMCG_CR

- [ ] **[C++/Rust]** **[MANDATORY]** 32-bit; offset `0xE04`; RW. `[L:29344]`
- [ ] **[C++/Rust]** **[MANDATORY]** bits[31:1] RES0; bit[0] `E`: global counter enable. Resets to 0. `[L:29351]`
- [ ] **[C++/Rust]** **[MANDATORY]** `E==0`: no events counted; `EVCNTRn` values do not change; overrides all per-counter `CNTEN` bits. `[L:29359]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access blocked if `SCR.NSRA==0` → RAZ/WI. `[L:29371]`

---

## 10.5.2.15 SMMU_PMCG_IIDR

- [ ] **[C++/Rust]** **[MANDATORY]** 32-bit; offset `0xE08`; RO; implementation of this register is optional. `[L:29385]`
- [ ] **[C++/Rust]** **[MANDATORY]** `ProductID`[31:20] matches `{PIDR1.PART_1, PIDR0.PART_0}` if those registers present. `[L:29398]`
- [ ] **[C++/Rust]** **[MANDATORY]** `Variant`[19:16] matches `PIDR2.REVISION` if present. `[L:29405]`
- [ ] **[C++/Rust]** **[MANDATORY]** `Revision`[15:12] matches `PIDR3.REVAND` if present. `[L:29411]`
- [ ] **[C++/Rust]** **[MANDATORY]** `Implementer`[11:0]: bits[11:8]=JEP106 continuation; bit[7]=0; bits[6:0]=JEP106 ID code. `[L:29417]`
- [ ] **[C++/Rust]** **[MANDATORY]** Value 0 indicates register not implemented. `[L:29428]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access blocked if `SCR.NSRA==0` → RAZ/WI; otherwise RO. `[L:29437]`

---

## 10.5.2.16 SMMU_PMCG_CEID0

- [ ] **[C++/Rust]** **[MANDATORY]** 64-bit; offset `0xE20`; RO. `[L:29455]`
- [ ] **[C++/Rust]** **[MANDATORY]** Bit `(N & 63)` relates to event N for 0≤N<64: 0=cannot count, 1=can count. `[L:29466]`
- [ ] **[C++/Rust]** **[MANDATORY]** Mandatory events (0–5, and 6–7 if ATS supported) must have their corresponding bits set. `[L:29466]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access blocked if `SCR.NSRA==0` → RAZ/WI. `[L:29479]`

---

## 10.5.2.17 SMMU_PMCG_CEID1

- [ ] **[C++/Rust]** **[MANDATORY]** 64-bit; offset `0xE28`; RO. `[L:29497]`
- [ ] **[C++/Rust]** **[MANDATORY]** Bit `(N & 63)` relates to event N for 64≤N<128: 0=cannot count, 1=can count. `[L:29508]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access blocked if `SCR.NSRA==0` → RAZ/WI. `[L:29521]`

---

## 10.5.2.18 SMMU_PMCG_ROOTCR

- [ ] **[C++/Rust]** **[MANDATORY]** 32-bit; offset `0xE48`; Root→RW; otherwise→RO; present only when `ROOTCR_IMPL==1`. `[L:29539]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[31] `ROOTCR_IMPL`: RO; 1=register implemented. `[L:29546]`
- [ ] **[C++/Rust]** **[MANDATORY]** bits[30:4] RES0; bit[2] RES0. `[L:29556]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[3] `NAO`: 0=non-attributable event counting prevented; 1=not prevented. Resets to 1 (opposite polarity to `SCR.NAO` — Root must explicitly clear to restrict). `[L:29560]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[1] `RLO`: 0=Realm StreamID event counting not permitted; 1=permitted. Resets to 0. `[L:29584]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[0] `RTO`: 0=Root state event counting not permitted; 1=permitted. Resets to 0. `[L:29602]`

---

## 10.5.2.19 SMMU_PMCG_IRQ_CTRL

- [ ] **[C++/Rust]** **[MANDATORY]** 32-bit; offset `0xE50`; RW; bits[31:1] RES0; bit[0] `IRQEN`. Resets to 0. `[L:29639]`
- [ ] **[C++/Rust]** **[MANDATORY]** IRQ triggered when: `IRQEN==1` AND counter n overflows AND `INTEN[n]==1`. `[L:29671]`
- [ ] **[C++/Rust]** **[MANDATORY]** `IRQEN==0`: no interrupt generated regardless of per-counter `INTEN` flags; overrides them. `[L:29662]`
- [ ] **[C++/Rust]** **[MANDATORY]** Controls both wired edge-triggered output AND MSI writes. `[L:29662]`
- [ ] **[C++/Rust]** **[MANDATORY]** `IRQ_CFG{0,1,2}` must only be modified when `IRQEN==0`. `[L:29664]`
- [ ] **[C++/Rust]** **[MANDATORY]** `IRQEN` 0→1 Update: future MSIs guaranteed to use `IRQ_CFG{0,1,2}` configuration. `[L:29668]`
- [ ] **[C++/Rust]** **[MANDATORY]** `IRQEN` 1→0 Update completes: all prior MSIs visible; no new MSI writes or wired edge events will become visible. `[L:29669]`
- [ ] **[C++/Rust]** **[MANDATORY]** `IRQ_ABT` cleared to 0 when `IRQEN` Updated 0→1. `[L:29949]`
- [ ] **[C++/Rust]** **[MANDATORY]** Has corresponding `IRQ_CTRLACK` with same Update semantic as `CR0`/`CR0ACK`. `[L:29660]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access blocked if `SCR.NSRA==0` → RAZ/WI. `[L:29682]`

---

## 10.5.2.20 SMMU_PMCG_IRQ_CTRLACK

- [ ] **[C++/Rust]** **[MANDATORY]** 32-bit; offset `0xE54`; RO; bit[0] `IRQEN`: acknowledge for `IRQ_CTRL.IRQEN`. Resets to 0. `[L:29713]`
- [ ] **[C++/Rust]** **[MANDATORY]** Undefined bits read as zero; fields are RES0 if corresponding `IRQ_CTRL` field is Reserved. `[L:29724]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access blocked if `SCR.NSRA==0` → RAZ/WI. `[L:29731]`

---

## 10.5.2.21 SMMU_PMCG_IRQ_CFG0

- [ ] **[C++/Rust]** **[MANDATORY]** 64-bit; offset `0xE58`; present only when `CFGR.MSI==1`; else RES0. `[L:29745]`
- [ ] **[C++/Rust]** **[MANDATORY]** bits[63:56] RES0; bits[1:0] RES0; `ADDR`[55:2]: physical address of MSI target (bits[1:0] of effective address always zero). `[L:29763]`
- [ ] **[C++/Rust]** **[MANDATORY]** High-order `ADDR` bits above `SMMU_IDR5.OAS` are RESD (not required to be stored). `[L:29771]`
- [ ] **[C++/Rust]** **[MANDATORY]** `ADDR==0`: no MSI sent (allows wired IRQ fallback). `[L:29777]`
- [ ] **[C++/Rust]** **[MANDATORY]** Guarded by `IRQEN`: must only be modified when `IRQEN==0`; when `IRQEN==1` or `IRQCTRLACK.IRQEN==1` → RO. `[L:29794]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access blocked if `SCR.NSRA==0` → RAZ/WI. `[L:29799]`

---

## 10.5.2.22 SMMU_PMCG_IRQ_CFG1

- [ ] **[C++/Rust]** **[MANDATORY]** 32-bit; offset `0xE60`; present only when `CFGR.MSI==1`; else RES0. `[L:29818]`
- [ ] **[C++/Rust]** **[MANDATORY]** `DATA`[31:0]: MSI data payload. Resets to UNKNOWN. Guarded by `IRQEN`. `[L:29826]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access blocked if `SCR.NSRA==0` → RAZ/WI. `[L:29840]`

---

## 10.5.2.23 SMMU_PMCG_IRQ_CFG2

- [ ] **[C++/Rust]** **[MANDATORY]** 32-bit; offset `0xE64`; present only when `CFGR.MSI==1`; else RES0. `[L:29855]`
- [ ] **[C++/Rust]** **[MANDATORY]** bits[31:6] RES0. `[L:29866]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SH`[5:4]: 00=Non-shareable; 01=reserved→behaves as 00; 10=Outer Shareable; 11=Inner Shareable. When `MemAttr` encodes Device memory: `SH` IGNORED (effectively Outer Shareable). `[L:29870]`
- [ ] **[C++/Rust]** **[MANDATORY]** `MEMATTR`[3:0]: memory type, encoded same as `STE.MemAttr`. Resets to UNKNOWN. Guarded by `IRQEN`. `[L:29888]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access blocked if `SCR.NSRA==0` → RAZ/WI. `[L:29910]`

---

## 10.5.2.24 SMMU_PMCG_IRQ_STATUS

- [ ] **[C++/Rust]** **[MANDATORY]** 32-bit; offset `0xE68`; RO; present only when `CFGR.MSI==1`; else RES0. In SMMUv3.0: this location is RES0. `[L:29925]`
- [ ] **[C++/Rust]** **[MANDATORY]** bits[31:1] RES0; bit[0] `IRQ_ABT`: set to 1 if MSI terminated with abort. `[L:29942]`
- [ ] **[C++/Rust]** **[IMPL-DEF]** Whether abort condition is detectable is implementation-defined. `[L:29948]`
- [ ] **[C++/Rust]** **[MANDATORY]** Cleared to 0 when `IRQEN` Updated 0→1; NOT cleared by `IRQEN` 1→0 transition. `[L:29949]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access blocked if `SCR.NSRA==0` → RAZ/WI. `[L:29963]`

---

## 10.5.2.25 SMMU_PMCG_GMPAM

- [ ] **[C++/Rust]** **[MANDATORY]** 32-bit; offset `0xE6C`; present only when `CFGR.MPAM==1`; else RES0. `[L:29977]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[31] `Update`: completion flag. Resets to 0. `[L:29992]`
- [ ] **[C++/Rust]** **[MANDATORY]** bits[30:24] RES0; `PO_PMG`[23:16]: PMG for PMCG-originated MSIs, resets to 0x00; `PO_PARTID`[15:0]: PARTID for PMCG-originated MSIs, resets to 0x0000. `[L:30004]`
- [ ] **[C++/Rust]** **[MANDATORY]** Must only be written when `Update==0`; write when `Update==1` is constrained UNPREDICTABLE. `[L:30036]`
- [ ] **[C++/Rust]** **[MANDATORY]** Write without simultaneously setting `Update=1` is constrained UNPREDICTABLE. `[L:30042]`
- [ ] **[C++/Rust]** **[MANDATORY]** Values > `PARTID_MAX` or `PMG_MAX` → UNKNOWN value used. `[L:30010]`
- [ ] **[C++/Rust]** **[MANDATORY]** New value observable to future reads even before Update completes. `[L:30049]`
- [ ] **[C++/Rust]** **[MANDATORY]** RO when `Update==1`; non-secure access blocked if `SCR.NSRA==0` → RAZ/WI. `[L:30065]`

---

## 10.5.2.26 SMMU_PMCG_AIDR

- [ ] **[C++/Rust]** **[MANDATORY]** 32-bit; offset `0xE70`; RO; bits[31:8] RES0. `[L:30083]`
- [ ] **[C++/Rust]** **[MANDATORY]** `{ArchMajorRev[7:4], ArchMinorRev[3:0]}`: 0x00=SMMUv3.0; 0x01=SMMUv3.1; 0x02=SMMUv3.2; 0x03=SMMUv3.3; 0x04=SMMUv3.4; all others reserved. `[L:30100]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access blocked if `SCR.NSRA==0` → RAZ/WI. `[L:30115]`

---

## 10.5.2.27 SMMU_PMCG_MPAMIDR

- [ ] **[C++/Rust]** **[MANDATORY]** 32-bit; offset `0xE74`; RO; present when `CFGR.MPAM==1` OR `CFGR.FILTER_PARTID_PMG==1`; else RES0. `[L:30129]`
- [ ] **[C++/Rust]** **[MANDATORY]** bits[31:24] RES0; `PMG_MAX`[23:16]: max Non-secure PMG (RES0 if `CFGR.MPAM==0`); `PARTID_MAX`[15:0]: max Non-secure PARTID (RES0 if `CFGR.MPAM==0`). `[L:30146]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure PMG bit width = position of MSB 1 in `PMG_MAX[7:0]` + 1, or 0 if `PMG_MAX==0`. `[L:30162]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure PARTID bit width = position of MSB 1 in `PARTID_MAX[15:0]` + 1, or 0 if `PARTID_MAX==0`. `[L:30165]`
- [ ] **[C++/Rust]** **[MANDATORY]** Non-secure access blocked if `SCR.NSRA==0` → RAZ/WI. `[L:30178]`

---

## 10.5.2.28 SMMU_PMCG_S_MPAMIDR

- [ ] **[C++/Rust]** **[MANDATORY]** 32-bit; offset `0xE78`; Secure/Root→RO; NS/non-Root→RAZ/WI. `[L:30196]`
- [ ] **[C++/Rust]** **[MANDATORY]** Present when PMCG supports Secure AND (`CFGR.MPAM==1` OR `CFGR.FILTER_PARTID_PMG==1`); else RES0. `[L:30192]`
- [ ] **[C++/Rust]** **[MANDATORY]** bits[31:26] RES0; bit[24] RES0. `[L:30202]`
- [ ] **[C++/Rust]** **[MANDATORY]** bit[25] `HAS_MPAM_NS`: 0=MPAM_NS mechanism not implemented; 1=implemented. RES0 if `CFGR.MSI==0`. `[L:30206]`
- [ ] **[C++/Rust]** **[MANDATORY]** `PMG_MAX`[23:16]: max Secure PMG (RES0 if `CFGR.MPAM==0`); `PARTID_MAX`[15:0]: max Secure PARTID (RES0 if `CFGR.MPAM==0`). `[L:30221]`

---

## 10.5.2.29 SMMU_PMCG_ID_REGS (0xFB0–0xFFC)

- [ ] **[C++/Rust]** **[MANDATORY]** All registers in this range are read-only identification registers. `[L:30252]`
- [ ] **[C++/Rust]** **[MANDATORY]** `CIDR0` (0xFF0): bits[7:0]=0x0D; `CIDR1` (0xFF4): bits[7:4]=0x9, bits[3:0]=0x0; `CIDR2` (0xFF8): bits[7:0]=0x05; `CIDR3` (0xFFC): bits[7:0]=0xB1. `[L:30260]`
- [ ] **[C++/Rust]** **[MANDATORY]** `PIDR2` (0xFE8): bit[3]=1 (JEDEC-assigned value always used). `[L:30269]`
- [ ] **[C++/Rust]** **[MANDATORY]** `PIDR4` (0xFD0): bits[7:4]=0 (SIZE field). `[L:30280]`
- [ ] **[C++/Rust]** **[MANDATORY]** `PIDR5`–`PIDR7` (0xFD4–0xFDC): RES0. `[L:30282]`
- [ ] **[C++/Rust]** **[MANDATORY]** `PMDEVARCH` (0xFBC): bits[31:21]=0x23B (Arm architect); bit[20]=1 (PRESENT); bits[19:16]=0 (REVISION); bits[15:0]=0x2A56 (ARCHID). `[L:30285]`
- [ ] **[C++/Rust]** **[MANDATORY]** `PMDEVTYPE` (0xFCC): bits[7:4]=5 (associated with SMMU); bits[3:0]=6 (performance monitor type). `[L:30289]`
- [ ] **[C++/Rust]** **[MANDATORY]** Fields outside defined table entries are RES0. `[L:30292]`

---

## 10.6 Support for Secure State

- [ ] **[C++/Rust]** **[IMPL-DEF]** PMCG Secure state support is optional; model documents whether it is implemented. `[L:30299]`
- [ ] **[C++/Rust]** **[MANDATORY]** `NSRA==0`: ALL Non-secure accesses to PMCG registers → RAZ/WI; Secure accesses always permitted. `[L:30305]`
- [ ] **[C++/Rust]** **[MANDATORY]** `NSRA==1`: Non-secure register access enabled. `[L:30305]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SO==1` (Secure observation enabled): filterable events may count Secure or NS StreamIDs per `FILTER_SEC_SID`; non-filterable events count Secure or NS state events as appropriate. `[L:30309]`
- [ ] **[C++/Rust]** **[MANDATORY]** `SO==0` (Secure observation disabled): filterable events count only NS StreamIDs; non-filterable events count only NS state events. `[L:30315]`
- [ ] **[C++/Rust]** **[MANDATORY]** If counter group supports MSIs and Secure state NOT supported in PMCG: MSI target PA space is Non-secure. `[L:30323]`
- [ ] **[C++/Rust]** **[MANDATORY]** If Secure state IS supported: `NS = SMMU_PMCG_SCR.NSMSI | SMMU_PMCG_SCR.NSRA` determines MSI PA space. `[L:30326]`
- [ ] **[C++/Rust]** **[MANDATORY]** If PMCG does NOT support Secure but system does: PMCG observes only Non-secure events. `[L:30328]`
- [ ] **[C++/Rust]** **[MANDATORY]** If PMCG supports Secure but system does NOT: all streams treated as Non-secure. `[L:30328]`
- [ ] **[C++/Rust]** **[MANDATORY]** No centralized mechanism in SMMU for assigning counter groups to Security states. `[L:30306]`

---

## 10.7 Support for Realm State

- [ ] **[C++/Rust]** **[MANDATORY]** `SMMU_PMCG_ROOTCR.ROOTCR_IMPL` indicates presence of Root/Realm PMCG controls. `[L:30332]`
- [ ] **[C++/Rust]** **[MANDATORY]** If `SMMU_ROOT_IDR0.REALM_IMPL==1`: Arm strongly recommends associated PMCGs have `ROOTCR_IMPL==1`; verify model follows this recommendation. `[L:30333]`
- [ ] **[C++/Rust]** **[MANDATORY]** `ROOTCR.RLO==0`: counting events from Realm StreamIDs is not permitted; `FILTER_REALM_SID` treated as 0. `[L:29590]`
- [ ] **[C++/Rust]** **[MANDATORY]** `ROOTCR.RTO==0`: counting events from Root state is not permitted. `[L:29608]`
- [ ] **[C++/Rust]** **[MANDATORY]** `ROOTCR.NAO==1` (reset default): non-attributable event counting not prevented by Root; Secure must also enable via `SCR.NAO`. `[L:29569]`

---

## Summary Statistics

| Section | Items |
|---------|-------|
| 10.1 Support and Discovery | 4 |
| 10.2 Overview | 8 |
| 10.2.1 Overflow/Interrupts/Capture | 12 |
| 10.3 Monitor Events | 15 |
| 10.4 StreamIDs and Filtering | 22 |
| 10.4.1 Counter Group StreamID Size | 4 |
| 10.4.2 NoStreamID Accesses | 3 |
| 10.4.3 PARTID/PMG Filtering | 13 |
| 10.4.4 Non-Attributable Events | 3 |
| 10.5 Physical Layout | 6 |
| 10.5.1 Address Map | 28 |
| 10.5.2.1 EVCNTR | 7 |
| 10.5.2.2 EVTYPER | 15 |
| 10.5.2.3 SVR | 5 |
| 10.5.2.4 SMR | 6 |
| 10.5.2.5 CNTENSET0 | 5 |
| 10.5.2.6 CNTENCLR0 | 3 |
| 10.5.2.7 INTENSET0 | 4 |
| 10.5.2.8 INTENCLR0 | 3 |
| 10.5.2.9 OVSCLR0 | 4 |
| 10.5.2.10 OVSSET0 | 4 |
| 10.5.2.11 CAPR | 5 |
| 10.5.2.12 SCR | 13 |
| 10.5.2.13 CFGR | 13 |
| 10.5.2.14 CR | 4 |
| 10.5.2.15 IIDR | 7 |
| 10.5.2.16 CEID0 | 4 |
| 10.5.2.17 CEID1 | 3 |
| 10.5.2.18 ROOTCR | 6 |
| 10.5.2.19 IRQ_CTRL | 10 |
| 10.5.2.20 IRQ_CTRLACK | 3 |
| 10.5.2.21 IRQ_CFG0 | 6 |
| 10.5.2.22 IRQ_CFG1 | 3 |
| 10.5.2.23 IRQ_CFG2 | 5 |
| 10.5.2.24 IRQ_STATUS | 5 |
| 10.5.2.25 GMPAM | 8 |
| 10.5.2.26 AIDR | 3 |
| 10.5.2.27 MPAMIDR | 5 |
| 10.5.2.28 S_MPAMIDR | 5 |
| 10.5.2.29 ID_REGS | 8 |
| 10.6 Secure State Support | 10 |
| 10.7 Realm State Support | 5 |
| **TOTAL** | **~314** |

**STATUS: COMPLETE — All Chapter 10 requirements captured with spec line citations**
