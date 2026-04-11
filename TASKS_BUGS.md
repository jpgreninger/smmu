# TASKS_BUGS.md — ARM SMMU v3 Specification Coverage Checklist

## Purpose

Systematic section-by-section verification checklist for **IHI0070G_b** (ARM SMMU v3 Architecture
Specification). Used to ensure both C++ and Rust implementations correctly implement every
behavioral requirement and that bug fixes do not introduce regressions.

Each section must be independently audited against the spec, bugs filed in `bugs/`, fixed via
the TDD workflow, and re-verified before marking ✅.

---

CURRENT_SECTION = 13.1.5

## Status Legend

| Symbol | Meaning |
|--------|---------|
| ☐ | Not yet audited |
| ⚠️ | Audited at least once — bugs found and fixed — re-audit recommended |
| ✅ | Fully verified — no outstanding bugs |
| 🚫 | Out of scope (not implemented; will not audit) |
| N/A | Informational only — no behavioral implementation requirement |

**Columns**: Section | Title | C++ | Rust | Bug IDs (all fixed unless noted) | Notes

---

## Chapter 1 — About this specification

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §1.1 | References | N/A | N/A | | Informational |
| §1.2 | Terms and abbreviations | N/A | N/A | | Informational |
| §1.3 | Specification Scope | N/A | N/A | | Informational |

---

## Chapter 2 — Introduction

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §2.1 | History | N/A | N/A | | |
| §2.2 | SMMUv3.0 features | ✅ | ✅ | | All IDR0/IDR3/IDR5 feature bits consistent with implementation; VATOS/GRAN16K/GRAN64K correctly not advertised; HTTU=0b01 (HTTUA only) correct |
| §2.3 | SMMUv3.1 features | ⚠️ | ⚠️ | BUG-AUDIT-S2-XNX, BUG-AUDIT-S2-OAS | XNX: IDR3.XNX cleared (S2UXN not implemented); OAS: IDR5.OAS now derives from max_pa_bits (52→6); PBHA/VAX not advertised (correct) |
| §2.4 | SMMUv3.2 features | ⚠️ | ⚠️ | BUG-IDR3-FWB, BUG-IDR3-STT | FWB: IDR3.FWB cleared (S2FWB/combine_attrs_fwb not implemented); STT: IDR3.STT set to 1 (S2T0SZ already accepts up to 48); RIL=1 consistent; MPAM=0 correct; BBML=0b01 acceptable |
| §2.5 | SMMUv3.3 features | ⚠️ | ⚠️ | BUG-AUDIT-94, BUG-AUDIT-95 | E0PD: IDR3 bit 13 added (S2P-conditional, mandatory §6.3.4); PTWNNC: IDR3 bit 14 added (S2P-conditional, mandatory when S2P=1); ATSRECERR=1 correct; ECMDQ=0 correct |
| §2.6 | SMMU for RME features | 🚫 | 🚫 | | Realm — out of scope |
| §2.7 | SMMU for RME DA features | 🚫 | 🚫 | | Realm — out of scope |
| §2.8 | SMMUv3.4 features | ⚠️ | ⚠️ | BUG-AUDIT-96 | MTEPERM: IDR3 bit 0 added (mandatory §2.8); all 10 optional bits (EPAN,THE,S1PI,S2PI,S2PO,AIE,PASIDTT,DS,D128) correctly 0 |
| §2.9 | Permitted implementation of subsets | N/A | N/A | | Informational |
| §2.10 | System placement | N/A | N/A | | Informational |

---

## Chapter 3 — Operation

### §3.1–3.3 Stream Numbering and Translation Procedure

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §3.1 | Software interface | ✅ | ✅ | | IDR1.SIDSIZE=32 and SSIDSIZE=20 consistent; PRI queue correctly gated on IDR0.PRI; IMPL_DEF fields safe at 0 |
| §3.2 | Stream numbering | ✅ | ✅ | CONF-GAP, AUDIT-82, BUG-AUDIT-120 | Re-audited: C_BAD_STREAMID (RECINVSID gate), PASID cap, stage-2-only PASID!=0 guard all PASS; SSV=1/PASID=0 on stage-2-only accepted (PASID=0 = default context, spec-permissible). BUG-AUDIT-120: StreamID type now accepts full u32 range matching IDR1.SIDSIZE=32 advertisement; runtime enforcement via strtab_log2size. All 47 tests pass. |
| §3.3 | Data structures and translation procedure | ✅ | ✅ | | Global bypass (GBPA), 4-step translation procedure, stage-not-implemented bypass, StreamWorld, TLB key uses {StreamID,PASID} not {SW,VMID,ASID} — acceptable gap (TLBI correctly scans by ASID/VMID; correctness preserved) |
| §3.3.1 | Stream table lookup (overview) | ✅ | ✅ | CONF-GAP-3/6 | Re-audited: Linear+2-level formats supported (strtab_fmt/split), StreamID range→C_BAD_STREAMID, write-guard on SMMUEN=1; CONF-GAP-3 (2-level) FIXED, CONF-GAP-6 (TLBI) FIXED (→§4.4) |
| §3.3.1.1 | Linear Stream table | ✅ | ✅ | BUG-AUDIT-101 | Fully verified: 2^LOG2SIZE sizing correct, bounds check fires before DashMap, LOG2SIZE=0 single-entry table correct. BUG-AUDIT-101: added 3 boundary tests (last valid, first invalid, LOG2SIZE=0) — all pass |
| §3.3.1.2 | 2-level Stream table | ✅ | ✅ | BUG-AUDIT-102/106/107/108 | Implementation correct. BUG-AUDIT-102: fixed ST_LEVEL docstring (2-bit field). BUG-AUDIT-106: added invalid SPLIT clamping test. BUG-AUDIT-107: added split=8/10 boundary tests. BUG-AUDIT-108: added in-range/unconfigured stream test |
| §3.3.2 | StreamIDs to Context Descriptors | ✅ | ✅ | AUDIT-44, BUG-AUDIT-109/110/111/112, BUG-AUDIT-121 | S1CDMax, substream routing. BUG-AUDIT-109: s1cdMax==0+SSV=1+PASID!=0 must emit C_BAD_SUBSTREAMID. BUG-AUDIT-110: S1DSS==0b11 reserved→F_STREAM_DISABLED. BUG-AUDIT-111: bypass/stage-2-only+SSV=1→C_BAD_SUBSTREAMID. BUG-AUDIT-112: s1cdMax==0+SSV=1+PASID=0 must abort. BUG-AUDIT-121: stage-2-only+SSV=1+PASID=0 test gap filled — implementation already correct per §5.2 S1Fmt line 6645. 210/210 tests pass. |
| §3.3.3 | Configuration and Translation lookup | ✅ | ✅ | BUG-AUDIT-113 | BUG-AUDIT-113: C++ strwUnused=!stage1Enabled missing ||stage2Enabled — Config=0b111 two-stage NonSecure stream with STRW=EL2 incorrectly rejected; fixed per ARM §5.2 IgnoreSTESTRW(). Rust already correct. FINDING-333-03 (TLB CacheKey missing StreamWorld): acceptable gap per §3.3 notes. FINDING-333-04 (CD.AA64/StreamWorld): AArch32 LPAE unsupported (intentional). FINDING-333-05 (T1SZ for EL2/EL3): only affects Secure streams (out of scope). C++ 185/185 |
| §3.3.4 | Transaction attributes: incoming, two-stage and overrides | ✅ | ✅ | NEW-GAP-A-D, BUG-NEW-RUST-1/2 | INSTCFG, PRIVCFG, NSCFG, access type; rnw/ind fixed. Re-audited 2026-04-05: all fixes confirmed. MTCFG/SHCFG stage-1 TTD combine gap already tracked as §13.4.2/§13.1.5 ☐. C++ vs Rust STRW+two-stage divergence already tracked §5.2 ⚠️. No new bugs. |
| §3.3.5 | Translation table descriptors | N/A | N/A | | PBHA=0 (IDR3.PBHA not advertised). No raw descriptor walk in either implementation (structured PageEntry API, not raw 64-bit descriptor parsing). Bits [63:60] of stage 2 Block/Page descriptors are RES0 (SMMUv3.1+); no fault or strip requirement when PBHA disabled. Section is a software constraint, not an SMMU hardware behavioral requirement. |

### §3.4 Address Sizes

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §3.4 | Address sizes (overview) | ⚠️ | ✅ | NEW-GAP-C, AUDIT-47 | T0SZ range, IPS/PS |
| §3.4.1 | Input address size and Virtual Address size | ⚠️ | ✅ | AUDIT-47 | T0SZ [16,39] enforcement; T0SZ magnitude check correct for single-TTBR0 model (EPD1/upper-half not modelled). |
| §3.4.2 | Address alignment checks | N/A | N/A | | §3.4.2 states SMMU does NOT check address alignment. No implementation needed. |
| §3.4.3 | Address sizes of SMMU-originated accesses | N/A | ✅ | BUG-AUDIT-114, BUG-AUDIT-115, BUG-AUDIT-123 | BUG-AUDIT-114: stage-1-only PA > OAS silently truncates per §3.4 line 1635 — fixed. BUG-AUDIT-115: CD.TTB0/TTB1 out-of-IPS → C_BAD_CD — fixed. BUG-AUDIT-123: IPS=52/OAS=52 range checks skipped (< 52 guard); changed to <= 52 so 2^52 limit enforced — fixed. 5 new tests, 210/210 pass. Modeling gaps (BUG-AUDIT-116/117/118/119) require PA pointer fields not present in model — documented N/A. C++ not applicable. |

### §3.5 Command and Event Queues

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §3.5.1 | SMMU circular queues | ⚠️ | ✅ | CONF-GAP, BUG-RUST-Q2/Q4, BUG-CPP-1/2 | Re-audited 2026-04-07: advance_index uses 2^(log2size+1) modulus (wrap bit at position log2size ✓); OVFLG at bit[31] correct per ARM §6.3 EVENTQ_PROD register spec; stall events buffered in stall_pending on full queue ✓; enqueue_event only called for non-stall config events ✓; no new defects found. |
| §3.5.2 | Queue entry visibility semantics | ⚠️ | ✅ | 2026-03-21, BUG-CPP-1/RUST-1 | Re-audited 2026-04-07: push_back before eventq_prod.store(Release) at all push sites ✓; get_events() never modifies eventq_cons ✓; eventq_cons only written in consume/advance path ✓; no new defects. Software model trivially satisfies memory-ordering requirement. |
| §3.5.3 | Event queue behavior | ⚠️ | ✅ | AUDIT-75 | Re-audited 2026-04-07: EVENTQEN gate enforced at all enqueue_event call sites ✓; stall events buffered in stall_pending on full queue ✓; non-stall events discarded + OVFLG toggled ✓; stall drain on next write/read ✓; no overwrite of unconsumed events ✓; no new defects. |
| §3.5.4 | Definition of event record write "Commit" | ⚠️ | ✅ | BUG-2/BUG-5 | Re-audited 2026-04-07: stall buffered in stall_pending without PROD advance ✓; stall drain commits with Release store after push_back ✓; non-stall discard has no PROD advance ✓; all PROD.store use Ordering::Release ✓; no phantom commits ✓; no new defects. |
| §3.5.5 | Event merging | N/A | ✅ | | Merging is optional per §3.5.5; software implementations not required to respect STE.MEV (§3.5.5/§7.3.1). Rust implementation optionally deduplicates events gated on stream_mev flag (BUG-RUST-2/CONF-GAP-14) — more than required. Stall events (Stall==1) correctly never merged. No defects. |
| §3.5.6 | Enhanced Command queue interfaces (ECMDQ) | 🚫 | 🚫 | | IDR1.ECMDQ=0 not advertised; feature requires dedicated hardware register pages — out of scope for software model |
| §3.5.6.1 | ECMDQ behavior | 🚫 | 🚫 | | Out of scope (ECMDQ not implemented) |
| §3.5.6.2 | Enabling/disabling ECMDQ | 🚫 | 🚫 | | Out of scope (ECMDQ not implemented) |
| §3.5.6.3 | Errors relating to ECMDQ | 🚫 | 🚫 | | Out of scope (ECMDQ not implemented) |

### §3.6–3.8 Ownership, Registers, Virtualization

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §3.6 | Structure and queue ownership | N/A | N/A | | Informational only. All statements use "Arm expects" guidance language directed at system software (OS, hypervisor, secure firmware), not SMMU hardware. No behavioral requirements on the SMMU model. Secure/Realm interfaces out of scope. No code changes required. |
| §3.7 | Programming registers | ✅ | ✅ | BUG-AUDIT-73, BUG-AUDIT-124 | BUG-AUDIT-73: PRIQEN RES0 when IDR0.PRI==0. BUG-AUDIT-124: CMDQEN/EVENTQEN/PRIQEN RO-while-set guard in set_cr0() via (CR0|CR0ACK) preserve. ATSCHK behavior correct. SMMUEN write-guards on CR1/CR2/strtab all verified. 210 tests pass. |
| §3.8 | Virtualization | N/A | N/A | | Purely informational. All language is descriptive ("Arm expects," "might provide"). Stage 2-only mapping behavior already covered by §3.3. IMPL DEF extra interfaces explicitly "beyond scope." No SMMU model behavioral requirements. |

### §3.9 PCI Express, PASIDs, PRI, and ATS

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §3.9.1 | ATS Interface | ⚠️ | ⚠️ | AUDIT-01, NEW-FINDING-1, BUG-AUDIT-125 | S1DSS, NS1ATS, EATS; BUG-AUDIT-125: ATS TT+ATSCHK=1+abort stream spuriously emitted F_TRANSL_FORBIDDEN; must silently abort per §3.9.1.3 |
| §3.9.1.1 | Handling of addresses in ATS-related transactions | ⚠️ | ⚠️ | | IMPL DEF: truncate or abort on PA overflow. No behavioral gap. |
| §3.9.1.2 | Responses to ATS Translation Requests | ⚠️ | ⚠️ | BUG-AUDIT-126 | BUG-AUDIT-126: ATS TR encountering F_TRANSLATION/F_ADDR_SIZE/F_ACCESS/F_PERMISSION incorrectly recorded events; spec §3.9.1.2 requires no event. Fixed by is_ats_tr flag in record_translation_fault(). All UR/CA event-gating paths (SMMUEN=0, EATS=0, abort, bypass, C_BAD_STREAMID via REC_CFG_ATS+RECINVSID) verified correct. |
| §3.9.1.3 | Handling of ATS Translated transactions | ⚠️ | ⚠️ | BUG-AUDIT-125, BUG-AUDIT-127 | BUG-AUDIT-125: abort→silent, bypass→F_TRANSL_FORBIDDEN. BUG-AUDIT-127: ATS TT+ATSCHK=1 config errors (C_BAD_SUBSTREAMID, F_STREAM_DISABLED) were replaced with F_TRANSL_FORBIDDEN; fixed to keep config event gated on REC_CFG_ATS=1, no F_TRANSL_FORBIDDEN. Translation-class errors still produce F_TRANSL_FORBIDDEN. ATSCHK=0 passthrough verified. |
| §3.9.1.4 | ATS Invalidation timeout | ⚠️ | ⚠️ | AUDIT-81 | SMMUEN==0 → no-op |
| §3.9.1.5 | ATS Invalidation errors | N/A | N/A | | UR response to CMD_ATC_INV completes without error — software/RC behavior, no SMMU model requirement. |
| §3.9.2 | Changing ATS configuration | N/A | N/A | | Software programming sequence guide (enable/disable ATS ordering). No SMMU hardware behavioral requirement. EATS validity already enforced by configure_stream() SteIllegal() checks. |
| §3.9.3 | SMMU interactions with CXL | 🚫 | 🚫 | | CXL out of scope |
| §3.9.4 | SMMU interactions with PCle T/TE/XT fields | N/A | N/A | | PCIe TLP T/TE/XT fields + Realm/TDISP/TEE extensions. Realm out of scope; PCIe wire format out of scope. No SMMU model behavioral requirement. |
| §3.9.4.1 | T bit and PCle IDE TLP prefix | N/A | N/A | | Realm/T-bit wire format. Out of scope. |
| §3.9.4.2 | TE bit on ATS Translation Completions | N/A | N/A | | Requires SMMU_R_IDR3.XT=1 (Realm). Out of scope. |
| §3.9.4.3 | XT bit on Untranslated/Translation/Translated | N/A | N/A | | Requires IDR3.XT=1 (Realm+TDISP). Out of scope. |
| §3.9.4.4 | XT/T fields on PRI/ATS Invalidation messages | N/A | N/A | | Requires IDR3.XT=1 (Realm). Out of scope. |

### §3.10 Security States

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §3.10.1 | StreamID Security state (SEC_SID) | ⚠️ | ⚠️ | | NonSecure path audited |
| §3.10.2 | Support for Secure state | 🚫 | 🚫 | | Secure interface out of scope |
| §3.10.2.1 | Secure commands, events, configuration | 🚫 | 🚫 | | |
| §3.10.2.2 | Secure EL2 and stage-2 translation | 🚫 | 🚫 | | |
| §3.10.3 | Support for Realm state | 🚫 | 🚫 | | Realm out of scope |
| §3.10.3.1–3 | Realm stream behavior | 🚫 | 🚫 | | |

### §3.11 Reset, Enable, and Initialization

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §3.11 | Reset, Enable, and initialization | ⚠️ | ⚠️ | AUDIT-69–71, 73, 75, 78 | enable() guards, CR0ACK protocol |

### §3.12 Fault Models

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §3.12 | Fault models (overview) | ⚠️ | ⚠️ | | Terminate/Stall dispatch |
| §3.12.1 | Terminate model | ⚠️ | ⚠️ | | |
| §3.12.2 | Stall model | ⚠️ | ⚠️ | BUG-QA-12/13, BUG-CPP-S1, BUG-RUST-Q2/Q4 | S2S/S2R, stall decision; double-fault, stall_pending reset |
| §3.12.2.1 | Suppression of duplicate Stall event records | N/A | N/A | | Dedup is "permitted but not required". One fault per transaction enforced. No mandatory requirement unmet. |
| §3.12.2.2 | Early retry of Stalled transactions | N/A | N/A | | "SMMU is permitted" to early-retry — IMPL DEF optional. Synchronous model does not implement speculative retry. |
| §3.12.2.3 | Miscellaneous Stall considerations | N/A | N/A | | Backpressure and IMPL DEF capacity limits. No behavioral requirement for SW model. |
| §3.12.3 | Considerations for client devices using Stall | N/A | N/A | | Software guidance |
| §3.12.4 | Virtual Memory paging with SMMU | N/A | N/A | | Informational — three fault models; no additional behavioral requirement beyond §3.12.1, §3.12.2, §8 |
| §3.12.4.1 | Page-in request event | ✅ | ✅ | BUG-QA-6 ✅ | E_PAGE_REQUEST perm bits fixed; span field added to PRIEntry (§7.3.19 span*4096 encoding) |
| §3.12.5 | Combinations of fault config with two stages | ✅ | ✅ | BUG-QA-12/13 ✅ | S1 fault uses CD.S; S2 fault uses STE.S2S; S2R=0&&S2S=0 suppresses S2 events; all 8 table rows correct; F_ACCESS is stall-eligible per §7.3.22 |

### §3.13 Translation Tables and HTTU

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §3.13 | Translation tables and Access flag/Dirty state | ✅ | ✅ | NEW-GAP ✅ | IDR0.HTTU=0b01 (AF-only); S2HD/HD guards reject dirty-state when HTTU<0b10; HA hardware AF-update modeled; no mandatory enforcement for shared-ASID CD.HA/HD identity |
| §3.13.1 | Software update of flags | N/A | N/A | | Software guidance only; AFFD (suppress F_ACCESS when AF=0) implemented in stream_context |
| §3.13.2 | Access flag hardware update | ✅ | ✅ | NEW-GAP ✅, BUG-RUST-I ✅, BUG-AUDIT-128 ✅ | Stage-1 HA AF-update correct; stage-2 HA AF-update added (BUG-AUDIT-128); AFFD/S2AFFD suppress F_ACCESS; SMMU never clears AF |
| §3.13.3 | Dirty state hardware update | N/A | N/A | BUG-AUDIT-36 ✅, BUG-AUDIT-40 ✅ | HTTU=0b01: HD/S2HD always rejected at configure time; dirty-state HTTU unreachable by design |
| §3.13.3.1 | Direct Permission Scheme | N/A | N/A | | HTTU<0b10 — DBM path structurally unreachable; SMMU never touches permission bits in update_access_flags |
| §3.13.3.2 | Indirect Permission Scheme | N/A | N/A | | HTTU<0b10 — CD.HD/STE.S2HD always rejected; no DBM field in Indirect Scheme descriptors |
| §3.13.4 | HTTU behavior summary | N/A | ✅ | BUG-AUDIT-129 ✅, BUG-AUDIT-130 ✅ | HD=1 requires HA=1 (and S2HD=1 requires S2HA=1) enforced in validate(); translate_two_stage() S2 HTTU update added; CMD_SYNC/TLB visibility satisfied structurally; speculative S2-dirty-on-S1-PTW-walk permitted behavior not modeled (flat-table model has no intermediate walk entries) |
| §3.13.5 | HTTU with two stages | N/A | ✅ | | Flat-model: S1 and final-S2 descriptor updates correct; intermediate S2 PTW-walk descriptors structurally inapplicable (no multi-level walk); "S2 must allow writes for S1 PTE update" is out of scope for flat model |
| §3.13.6 | Access flag in Table descriptors | N/A | N/A | | IDR0.HTTU=0b01 → HAFT explicitly not supported (§6.3.1); flat model has no Table descriptors; CD.HAFT/STE.S2HAFT structurally inapplicable |
| §3.13.7 | ATS, PRI and translation table flag update | 🚫 | 🚫 | | PCIe ATS TR→TC path not implemented; GATOS (ARM ATOS) is separate and not covered here; STE.EATS gates ATS acceptance but no ATS TR processing path exists |
| §3.13.7.1 | Hardware flag update for ATS and PRI | 🚫 | 🚫 | | ATS TR path out of scope; GATOS handles HTTU via normal translate path |
| §3.13.7.2 | Flag maintenance for ATS/PRI without HTTU | 🚫 | 🚫 | | ATS TR path out of scope |
| §3.13.8 | Hardware flag update for CMO and Destructive Reads | N/A | N/A | | HTTU dirty-state (HD=1) always rejected (HTTU=0b01); CMO/DR/DH paths not modeled; AF update unaffected (fires normally); vacuously conformant |

### §3.14–3.16 Speculative, Coherency, Embedded

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §3.14 | Speculative accesses | N/A | N/A | | Entirely IMPLEMENTATION DEFINED; model does not implement speculative transactions; conformant by omission |
| §3.15 | Coherency considerations and memory access types | N/A | N/A | | Hardware coherency concerns; COHACC=0 correct for SW model (no IO-coherent access); atomicity satisfied by Rust DashMap AtomicU64 |
| §3.15.1 | Client devices | N/A | N/A | | Hardware snoop/cache topology; not applicable to SW simulation |
| §3.15.1.1 | Fully-coherent client devices | N/A | N/A | | GPC/DPT/snoop hardware behavior; out of scope |
| §3.16 | Embedded Implementations | N/A | N/A | | TABLES_PRESET/QUEUES_PRESET hardwired storage; SW model uses dynamic allocation; IDR1.TABLES_PRESET=0/QUEUES_PRESET=0 correct |
| §3.16.1 | Changes to structure/queue storage when fixed/preset | N/A | N/A | | Embedded preset storage behavior; not applicable |
| §3.16.1.1 | Event Queue and PRI Queue (embedded) | N/A | N/A | | |
| §3.16.1.2 | Command Queue (embedded) | N/A | N/A | | |
| §3.16.1.3 | Stream Table Entry (embedded) | N/A | N/A | | |

### §3.17 TLB Tagging, VMIDs, ASIDs, Broadcast TLB Maintenance

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §3.17 | TLB tagging, VMIDs, ASIDs overview | ⚠️ | ✅ | BUG-QA-14 ✅, BUG-NEW-20 ✅, BUG-AUDIT-131 ✅ | STRW field in TLBEntry; S2P==0 → VMID=0 substitution in receive_broadcast_tlbi; ASET over-invalidation is permitted conservative behavior (spec says "not required to invalidate") |
| §3.17.1 | The Global flag in translation table descriptor | N/A | N/A | | Flat model has no nG bit in page descriptors; always treats entries as non-global (ASID-tagged); conservative over-tagging is functionally safe; ASET-based ASID set matching not implemented (permitted optimization) |
| §3.17.2 | Broadcast TLB maintenance from Armv8-A PEs (EL3/AArch64) | ⚠️ | ✅ | BUG-NEW-37–40 ✅, AUDIT-54 ✅ | TLBI scoping, VMID vs global; PTM polarity fixed; NH/EL3 commands correctly scope Secure/EL3 entries; re-verified conformant |
| §3.17.2.1 | Broadcast TLB maintenance when Secure EL2 implemented | 🚫 | 🚫 | | Secure out of scope |
| §3.17.3 | Broadcast TLB maintenance from ARMv7-A or AArch32 PEs | N/A | ✅ | | AArch32 TLBIs mapped to same NH commands by interconnect; NH scope handling correct |
| §3.17.4 | Broadcast TLB maintenance in mixed AArch32/AArch64 | N/A | ✅ | | ASID16=1/VMID16=1 in IDR0; u16 fields; 16-bit direct comparison correct |
| §3.17.5 | EL2 ASIDs and TLB maintenance in E2H mode | ⚠️ | ✅ | BUG-NEW-A ✅ | TLBI_EL2_ASID E2H method; any-EL2 vs any-EL2-E2H not required to be differentiated per spec |
| §3.17.6 | VMID Wildcards | N/A | ✅ | | CR0.VMW wildcard masking implemented; applies to both broadcast and command-queue TLBI |
| §3.17.7 | Broadcast TLB maintenance for GPT information | 🚫 | 🚫 | | GPC/RME out of scope |
| §3.17.8 | TLBInXS maintenance operations | N/A | N/A | | FEAT_XS not implemented; XS attribute implicitly 0 for all translations; TLBInXS ≡ TLBI; conformant by equivalence |

### §3.18–3.19 Interrupts and Power

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §3.18 | Interrupts and notifications | ⚠️ | ✅ | | GERROR/GERRORN toggle protocol correct; MSI/wired simulated (no real HW writes); interrupt notification ordering satisfied structurally |
| §3.18.1 | MSI synchronization | N/A | ✅ | | CMD_SYNC ensures MSI completion; SW model has no actual MSI writes; ordering satisfied structurally |
| §3.18.2 | Interrupt sources | ⚠️ | ✅ | BUG-QA-7 ✅, AUDIT-55 ✅, AUDIT-65 ✅ | SEV gated on IDR0.SEV; CS=0b11 → CERROR_ILL; CS=0b10 with SEV=0 → SIG_NONE (deliberate safe fallback per AUDIT-65) |
| §3.19 | Power control | ⚠️ | ✅ | AUDIT-53 ✅ | DORMHINT=1 in IDR0; STATUSR.DORMANT set after shutdown(); power-off preconditions are SW responsibility per spec |
| §3.19.1 | Dormant state | N/A | ✅ | | Stalled-transaction abort on dormancy is SW precondition per §3.19.1; shutdown() clears streams/queue; DORMANT bit set |

### §3.20–3.21 TLB Conflicts and Structure Update Procedures

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §3.20 | TLB and configuration cache conflict | 🚫 | 🚫 | | IMPL DEF — out of scope |
| §3.20.1 | TLB conflict | 🚫 | 🚫 | | F_TLB_CONFLICT — IMPL DEF |
| §3.20.2 | Configuration cache conflicts | 🚫 | 🚫 | | F_CFG_CONFLICT — IMPL DEF |
| §3.21 | Structure access rules and update procedures | ⚠️ | ✅ | AUDIT-83 ✅ | disable_stream() TLB flush correct; all sub-sections verified |
| §3.21.1 | Translation tables and TLB invalidation completion | ⚠️ | ✅ | | TLB insert gated on SMMUEN; completion ordering satisfied structurally |
| §3.21.1.1 | Translation tables update procedure | N/A | N/A | | Flat model; no multi-level walk; BBM N/A |
| §3.21.1.2 | BBML==1 (Level 1) | N/A | ✅ | | IDR3.BBML=0b01 set; nT bit N/A in flat model |
| §3.21.1.3 | BBML==2 (Level 2) | N/A | N/A | | Not claimed (BBML=0b01); TLB conflict F_TLB_CONFLICT out of scope (§3.20) |
| §3.21.2 | Queues | N/A | ✅ | | PROD/CONS semantics correct; SW preconditions are user responsibility |
| §3.21.3 | Configuration structures and invalidation completion | ⚠️ | ✅ | CONF-GAP-14 ✅ | CMD_CFGI_* handlers correct; CfgiSte no-op correct (no STE cache); CfgiAll global flush |
| §3.21.3.1 | Configuration structure update procedure | N/A | N/A | | Software guidance only; not SMMU enforcement |

### §3.22–3.27 Specialized Features

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §3.22 | Destructive reads and directed cache prefetch | N/A | N/A | | AMBA AXI5 DR/RCI/W-DCP/NW-DCP HW transaction types not in SW model API; STE.DRE/DCP not implemented |
| §3.22.1 | Control of transaction downgrade | N/A | N/A | | STE.DRE/DCP not advertised; downgrade N/A |
| §3.22.2 | Permissions model | N/A | N/A | | DR/RCI permission model N/A (no HW transaction types) |
| §3.22.3 | Memory types and Shareability | N/A | N/A | | Output interconnect constraints N/A for SW model |
| §3.23 | Memory Tagging Extension | N/A | N/A | | IDR3.MTEPERM=1 set; MAIR 0xF0 Reserved enforced by SW model not implementing MAIR field parsing |
| §3.23.1 | SMMU support for FEAT_MTE_PERM | N/A | N/A | | stage-2 MemAttr NoTagAccess remapping N/A (flat model uses page_attr 0x00/0xFF only) |
| §3.24 | Device Permission Table | 🚫 | 🚫 | | DPT — out of scope |
| §3.24.1–7 | DPT sub-sections | 🚫 | 🚫 | | |
| §3.25 | Granule Protection Checks | 🚫 | 🚫 | | GPC/GPT — out of scope |
| §3.25.1–6 | GPC sub-sections | 🚫 | 🚫 | | |
| §3.26 | Permission Indirections | N/A | N/A | | S1PIE/S2PIE not advertised in IDR0; PIE not implemented |
| §3.26.1 | Stage 1 permission indirections | N/A | N/A | | |
| §3.26.2 | Stage 2 permission indirections | N/A | N/A | | |
| §3.27 | Translation Hardening | N/A | N/A | | AssuredOnly/Protected not advertised; not implemented |
| §3.27.1 | Protected attribute | N/A | N/A | | |
| §3.27.2 | AssuredOnly permission checks | N/A | N/A | | |

---

## Chapter 4 — Commands

### §4.1 Commands Overview

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §4.1.1 | Command opcodes | ⚠️ | ⚠️ | AUDIT-72, BUG-AUDIT-132 | Unknown/reserved opcode → CERROR_ILL+GERROR via submit_raw_opcode() |
| §4.1.2 | Submitting commands to the Command queue | ⚠️ | ⚠️ | | PROD write ordering correct; queue-full detection correct |
| §4.1.3 | Command errors | ⚠️ | ⚠️ | AUDIT-72, 93, BUG-AUDIT-132 | CERROR_ILL+GERROR for reserved opcode; ERR field set; recovery correct |
| §4.1.4 | Consumption of commands from the Command queue | ⚠️ | ⚠️ | AUDIT-93, BUG-CPP-1/RUST-1 | Two-step recovery (CONS then GERROR); peek-before-pop correct |
| §4.1.5 | Reserved fields | ⚠️ | ⚠️ | | MBZ fields silently ignored (spec-permitted; no CERROR_ILL required) |
| §4.1.6 | Common command fields | ⚠️ | ⚠️ | BUG-NEW-28/32, BUG-NEW-15/16/17/21, BUG-NEW-24/25, BUG-NEW-37/38 | SSec guards on NS queue; CMD_RESUME/STALL_TERM/CFGI/TLBI SSec all correct |
| §4.1.7 | Out-of-range parameters | ⚠️ | ⚠️ | AUDIT-72 | Out-of-range SID/SSID treated as no-op (CONSTRAINED UNPREDICTABLE — conformant) |

### §4.2 Prefetch Commands

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §4.2.1 | CMD_PREFETCH_CONFIG | ⚠️ | ⚠️ | BUG-NEW-25 | SSec guard correct; out-of-range → IGNORED; SSV+S1CDMax=0 CONSTRAINED UNPREDICTABLE documented |
| §4.2.2 | CMD_PREFETCH_ADDR | ⚠️ | ⚠️ | BUG-NEW-25 | SSec guard correct; out-of-range address → IGNORED (no fault); flat-model Leaf=0/1 both no-op (conformant) |

### §4.3 Configuration Structure Invalidation

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §4.3.1 | CMD_CFGI_STE | ⚠️ | ⚠️ | | SSec guard correct; Leaf=0/1 flat-model equivalence documented (no L1ST cache) |
| §4.3.2 | CMD_CFGI_STE_RANGE | ⚠️ | ⚠️ | BUG-3 | Range < 31 prefix match correct; range=31 global invalidation; range>31 clamped |
| §4.3.3 | CMD_CFGI_CD | ⚠️ | ⚠️ | NEW-AUDIT-02/04, BUG-NEW-15 | SSec+S1P guard correct; Leaf=0/1 flat-model equivalence documented (no L1CD cache) |
| §4.3.4 | CMD_CFGI_CD_ALL | ⚠️ | ⚠️ | NEW-AUDIT-02/04, BUG-NEW-15 | SSec+S1P guard correct; Leaf flat-model equivalence documented |
| §4.3.5 | CMD_CFGI_VMS_PIDM | ⚠️ | ⚠️ | BUG-NEW-A-G, BUG-G | SSec+MPAM guard correct; IDR3.MPAM=0 → always CERROR_ILL (conformant) |
| §4.3.5.1 | CMD_CFGI_VMS_PIDM usage | N/A | N/A | | Software guidance only |
| §4.3.6 | CMD_CFGI_ALL | ⚠️ | ⚠️ | BUG-NEW-15 | SSec guard correct; global TLB invalidation correct |
| §4.3.7 | VM guest OS structure invalidations by hypervisor | N/A | N/A | | Software-only guidance; no SMMU enforcement required |
| §4.3.8 | Configuration structure invalidation semantics/rules | ⚠️ | ⚠️ | | Single-threaded model; stall/invalidation independence, CMD_SYNC ordering all conformant |

### §4.4 TLB Invalidation

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §4.4.1 | Common TLB invalidation fields | ⚠️ | ⚠️ | AUDIT-NEW-01 | ASID16/VMID16 correct; 16-bit tags always used |
| §4.4.1.1 | Range-based invalidation and level hint (RIL) | ⚠️ | ⚠️ | AUDIT-86, AUDIT-NEW-01, BUG-NEW-B/C/E | RIL formula correct; TG encoding 1=4K/2=16K/3=64K; SCALE capped at 39 |
| §4.4.2 | TLB invalidation of stage 1 | ⚠️ | ⚠️ | AUDIT-NEW-03 | All guards and scoping correct |
| §4.4.2.1 | CMD_TLBI_NH_ALL | ⚠️ | ⚠️ | AUDIT-85, BUG-QA-14 | VMID-scoped; no S1P guard (correct) |
| §4.4.2.2 | CMD_TLBI_NH_ASID | ⚠️ | ⚠️ | BUG-CPP-2/RUST-2 | VMID+ASID joint invalidation correct |
| §4.4.2.3 | CMD_TLBI_NH_VAA | ⚠️ | ⚠️ | BUG-NEW-37 | VMID+VA scoped; RIL path correct |
| §4.4.2.4 | CMD_TLBI_NH_VA | ⚠️ | ⚠️ | BUG-NEW-37 | VMID+VA+ASID correct; global-entry (nG) requirement vacuously met (flat model, no nG entries) — documented |
| §4.4.2.5 | CMD_TLBI_EL3_ALL | ⚠️ | ⚠️ | BUG-QA-9 | CERROR_ILL on NS queue |
| §4.4.2.6 | CMD_TLBI_EL3_VA | ⚠️ | ⚠️ | BUG-QA-9 | CERROR_ILL on NS queue |
| §4.4.2.7 | CMD_TLBI_EL2_ALL | ⚠️ | ⚠️ | BUG-NEW-24–27, BUG-NEW-16/18 | IDR0.Hyp==0 → CERROR_ILL; EL2-scoped invalidation correct |
| §4.4.2.8 | CMD_TLBI_EL2_VA | ⚠️ | ⚠️ | BUG-NEW-A, BUG-NEW-24/27, BUG-NEW-E | Hyp guard; RIL formula; EL2-scoped filter all correct |
| §4.4.2.9 | CMD_TLBI_EL2_VAA | ⚠️ | ⚠️ | BUG-NEW-B, BUG-NEW-24/27, BUG-NEW-E | Hyp guard; EL2-scoped; RIL page-by-page (correct, perf note only) |
| §4.4.2.10 | CMD_TLBI_EL2_ASID | ⚠️ | ⚠️ | BUG-NEW-B, BUG-NEW-24/27, BUG-NEW-D | EL2E2H method; Hyp guard; EL2-scoped ASID correct |
| §4.4.2.11 | CMD_TLBI_S_EL2_ALL | ⚠️ | ⚠️ | BUG-NEW-28/32 | CERROR_ILL on NS queue |
| §4.4.2.12 | CMD_TLBI_S_EL2_VA | ⚠️ | ⚠️ | BUG-NEW-28/32 | CERROR_ILL on NS queue |
| §4.4.2.13 | CMD_TLBI_S_EL2_VAA | ⚠️ | ⚠️ | BUG-NEW-28/32 | CERROR_ILL on NS queue |
| §4.4.2.14 | CMD_TLBI_S_EL2_ASID | ⚠️ | ⚠️ | BUG-NEW-28/32 | CERROR_ILL on NS queue |
| §4.4.3 | TLB invalidation of stage 2 | ⚠️ | ⚠️ | | All S2P guards and RIL formulas correct |
| §4.4.3.1 | CMD_TLBI_S2_IPA | ⚠️ | ⚠️ | AUDIT-86, BUG-NEW-38, BUG-NEW-39 | S2P guard; RIL formula; Reserved RIL check all correct |
| §4.4.3.2 | CMD_TLBI_S12_VMALL | ⚠️ | ⚠️ | BUG-NEW-38, BUG-NEW-39 | S2P guard; CR0.VMW masking correct |
| §4.4.3.3 | CMD_TLBI_S_S2_IPA | ⚠️ | ⚠️ | BUG-NEW-28/32 | CERROR_ILL on NS queue |
| §4.4.3.4 | CMD_TLBI_S_S12_VMALL | ⚠️ | ⚠️ | BUG-NEW-28/32 | CERROR_ILL on NS queue |
| §4.4.4 | Common TLB invalidation | ⚠️ | ⚠️ | | |
| §4.4.4.1 | CMD_TLBI_NSNH_ALL | ⚠️ | ⚠️ | AUDIT-85, BUG-NEW-18 | EL1_EL0-scoped; no S1P guard (correct) |
| §4.4.4.2 | CMD_TLBI_SNH_ALL | ⚠️ | ⚠️ | BUG-NEW-28/32 | CERROR_ILL on NS queue |

### §4.5–4.8 ATS/PRI, DPT, Fault Response, Sync

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §4.5.1 | CMD_ATC_INV | ⚠️ | ⚠️ | AUDIT-81, BUG-NEW-38 | SMMUEN==0 → no-op; SSec guard; IDR0.ATS hardcoded=1; G+SSV: model conservatively treats G=1 as always active (no SSV field in CommandEntry — documented) |
| §4.5.2 | CMD_PRI_RESP | ⚠️ | ⚠️ | AUDIT-03, BUG-NEW-A | SMMUEN==0 ignored; Resp=0b11 CERROR_ILL; IDR0.PRI guard; Resp 0b00/01/10 not differentiated (no PCIe bus — documented) |
| §4.6.1 | CMD_DPTI_ALL | 🚫 | 🚫 | | DPT out of scope |
| §4.6.2 | CMD_DPTI_PA | 🚫 | 🚫 | | DPT out of scope |
| §4.7.1 | CMD_RESUME | ⚠️ | ⚠️ | BUG-NEW-16, BUG-NEW-21 | STAG, Action, Abort; SSec guard; STALL_MODEL guard all correct |
| §4.7.2 | CMD_STALL_TERM | ⚠️ | ⚠️ | BUG-NEW-17, BUG-NEW-21 | Stall termination; SSec guard correct |
| §4.7.2.1 | CMD_STALL_TERM notes and usage | N/A | N/A | | Guidance only |
| §4.7.3 | CMD_SYNC | ⚠️ | ⚠️ | AUDIT-55/80/87, BUG-QA-7, AUDIT-65, BUG-NEW-23/26/27, BUG-F | CS=0b11 CERROR_ILL; CS=0b01 MSI simulation note added; CS=0b00 clears last-signal-type; SEV gating correct |
| §4.8 | Command Consumption summary | ⚠️ | ⚠️ | | All per-command consume rules verified correct |

---

## Chapter 5 — Data Structure Formats

### §5.1–5.3 Stream Table Descriptors

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §5.1 | L1STD, Level 1 Stream Table Descriptor | ⚠️ | ⚠️ | | Flat-model: no in-memory L1STD structure; L1STD.V=0/Span=0/invalid L2 all map to missing DashMap entry → C_BAD_STREAMID (equivalent outcome). Span=0 enforcement, reserved Span 12-31, and Span-vs-SPLIT check not separately validated but semantically covered by DashMap absence fault. |
| §5.1.1 | General properties of L1STD | ⚠️ | ⚠️ | | SW write constraints not enforced by SMMU HW (per spec); CFGI_STE invalidation covers caching requirement. Flat-model justification same as §5.1. |
| §5.2 | STE, Stream Table Entry | ⚠️ | ✅ | AUDIT-01,36,40,42–48,58,88,91, BUG-AUDIT-133 ✅ | BUG-AUDIT-133: INSTCFG=0b01 (Reserved) incorrectly forced Instruction semantics; fixed to passthrough. INSTCFG=0b11 now correctly forces Instruction. All 4 encoding cases verified by TDD tests. |
| §5.2.1 | General properties of STE | ⚠️ | ⚠️ | AUDIT-36,42,43,46, AUDIT-58 | Config validation, S2AA64, HD; S2TG granule check |
| §5.2.2 | Validity of STE | ⚠️ | ⚠️ | AUDIT-44,45, CONF-GAP-16, BUG-CPP-3/RUST-2, BUG-NEW-39, BUG-A/B/C | S1CDMax, S2T0SZ, EATS validity; STRW EL2/EL3; S2P/Hyp gates |
| §5.2 (STRW) | STE.STRW validity rules | ⚠️ | ⚠️ | BUG-CPP-3/RUST-2, AUDIT-79 | EL2/EL3 promotion, illegal checks |
| §5.2 (EATS) | STE.EATS ATS mode | ⚠️ | ⚠️ | AUDIT-01 | NS1ATS dependency |
| §5.2 (S2T0SZ) | STE.S2T0SZ validation | ⚠️ | ⚠️ | AUDIT-45, 88, 91, AUDIT-48 | Sentinel vs spec minimum; s2_t0sz=0 sentinel guard |
| §5.2 (S2S/S2R) | STE.S2S/S2R stall/record | ⚠️ | ⚠️ | BUG-QA-12/13 | Two-stage stall decisions |
| §5.3 | L1CD, Level 1 Context Descriptor | N/A | N/A | | Flat-model: S1Fmt always 0b00; no L1CD struct, no L2Ptr fetch, no V-bit check. IDR0.CD2L=1 advertised (flat PASID map semantically equivalent). C_BAD_SUBSTREAMID for out-of-range PASID correctly generated. CMD_CFGI_CD Leaf=0/1 both handled with documented flat-model no-op for L1CD cache. |
| §5.3.1 | General properties of L1CD | N/A | N/A | | V==0→C_BAD_SUBSTREAMID path unreachable in flat model; equivalent outcome via PASID-out-of-range check at smmu/mod.rs:5001–5040. Invalidation semantics correct per §4.3.3 (Leaf vacuously no-op). |

### §5.4–5.6 Context Descriptor, Fault Config, VMS

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §5.4 | CD, Context Descriptor | ⚠️ | ⚠️ | AUDIT-40, 47, 50 | Extensively audited |
| §5.4.1 | CD notes | ⚠️ | ⚠️ | NEW-GAP-A-D | NSCFG, EPD, AFFD |
| §5.4.1.1 | EPDx behavior | ⚠️ | ⚠️ | NEW-GAP, NEW-04 | EPD0/EPD1 fault behavior; EPD0 in two-stage path |
| §5.4.2 | Validity of CD | ⚠️ | ⚠️ | AUDIT-40, BUG-NEW-F, AUDIT-47 | HD=1+HTTU=0b01, CD.S+Stall; T0SZ min bound |
| §5.5 | Fault configuration (A, R, S bits) | ⚠️ | ⚠️ | BUG-NEW-F, BUG-QA-12/13 | CD.S stall enable; S2S/S2R in STE |
| §5.6 | VMS, Virtual Machine Structure | 🚫 | 🚫 | | VMS out of scope |
| §5.6.1 | VMS presence and fetching | 🚫 | 🚫 | | |
| §5.6.2 | VMS caching and invalidation | 🚫 | 🚫 | | |

---

## Chapter 6 — Memory Map and Registers

> Register sections are grouped by functional area. The Secure (SMMU_S_*), Root, and Realm
> register pages are out of scope. MPAM registers are out of scope.

### §6.1–6.2 Memory Map and Overview

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §6.1 | Memory map | N/A | N/A | | Physical layout (64KB pages, offsets) — NA for software model. Behavioral: IDR0.VATOS=0 ✓, IDR1.ECMDQ=0 ✓, IDR2=0 ✓, guarded writes (CR2/CR1/STRTAB_BASE) ✓, queue-enable sticky bits ✓, PRIQEN masked when PRI=0 ✓. Gap note: CMDQ_BASE/EVENTQ_BASE/PRIQ_BASE write guards needed if those write APIs ever added. |
| §6.2 | Register overview | N/A | N/A | | Table of registers |
| §6.2.1 | Registers in Page 0 | N/A | N/A | | Physical offset table — NA for software model; behavioral coverage in §6.3.x |
| §6.2.2 | Registers in Page 1 | N/A | N/A | | Physical offset table — NA for software model |
| §6.2.3 | Registers in the VATOS page | N/A | N/A | | IDR0.VATOS=0 → VATOS absent; page NA |
| §6.2.4 | Registers in the S_VATOS page | 🚫 | 🚫 | | Secure out of scope |
| §6.2.5 | Registers in a Command queue control page | N/A | N/A | | IDR1.ECMDQ=0 → ECMDQ absent; page NA |
| §6.2.6 | Root Control Page | 🚫 | 🚫 | | Root out of scope |
| §6.2.7–9 | Realm Register Pages | 🚫 | 🚫 | | Realm out of scope |

### §6.3 Identification Registers (IDR0–IDR6, IIDR, AIDR)

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §6.3.1 | SMMU_IDR0 | ⚠️ | ⚠️ | AUDIT-01,02,41,53,55, BUG-A/B, AUDIT-37,41 | SEV, DORMHINT, Hyp, VMW, NS1ATS, HTTU, S1P/S2P, STALL_MODEL; IDR0.VMW/Hyp S2P/S1P gates |
| §6.3.2 | SMMU_IDR1 | ⚠️ | ⚠️ | AUDIT-44 | SSIDSIZE, SIDSIZE, ECMDQ |
| §6.3.3 | SMMU_IDR2 | ✅ | ✅ | | IDR2=0 correct (VATOS not implemented, IDR0.VATOS=0 → BA_VATOS is RES0) |
| §6.3.4 | SMMU_IDR3 | ⚠️ | ⚠️ | AUDIT-02,37, BUG-G | HAD, XNX, FWB, MPAM, BBML; IDR3.MPAM bit 7 |
| §6.3.5 | SMMU_IDR4 | N/A | N/A | | IMPL DEF — IDR4=0 is valid |
| §6.3.6 | SMMU_IDR5 | ⚠️ | ⚠️ | AUDIT-52,53 | GRAN*, OAS, D128, VAX, STALL_MAX; GRAN16K/64K cleared |
| §6.3.7 | SMMU_IIDR | ✅ | ✅ | | IIDR=0 valid (no Arm JEP106 code used); RO invariant held |
| §6.3.8 | SMMU_AIDR | ✅ | ✅ | BUG-AIDR-01 ✅ | ArchMajorRev fixed 0→1: 0x02→0x12. ArchMinorRev=2 (SMMUv3.2). TDD: 3 tests. |
| §6.3.45 | SMMU_IDR6 | 🚫 | 🚫 | | IDR1.ECMDQ=0 → register absent → RES0 conformant; ECMDQ feature out of scope |

### §6.3 Control Registers (CR0, CR0ACK, CR1, CR2, STATUSR)

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §6.3.9 | SMMU_CR0 | ⚠️ | ⚠️ | AUDIT-69–71,73,75,78 | SMMUEN, CMDQEN, EVENTQEN, PRIQEN |
| §6.3.9.1 | CR0.VMW | ⚠️ | ⚠️ | BUG-NEW-A-G | S2P dependency |
| §6.3.9.2 | CR0.ATSCHK | ✅ | ✅ | | CR0_ATSCHK bit 4 correct; ATS TR+EATS==0 abort unconditional (mandatory §3.9.1.2); ATS TT+ATSCHK=1 gated correctly |
| §6.3.9.3 | CR0.CMDQEN | ⚠️ | ⚠️ | AUDIT-69 | write-guard |
| §6.3.9.4 | CR0.EVENTQEN | ⚠️ | ⚠️ | AUDIT-75 | toggle/gate on submit |
| §6.3.9.5 | CR0.PRIQEN | ⚠️ | ⚠️ | AUDIT-71,73,78, BUG-PRIQEN-ASYMMETRY, BUG-NEW-15 | IDR0.PRI guard, gate on process_pri; effective PRIQEN = PRIQEN AND SMMUEN |
| §6.3.9.6 | CR0.SMMUEN | ⚠️ | ⚠️ | AUDIT-81 | CMD_ATC_INV no-op, gating |
| §6.3.10 | SMMU_CR0ACK | ⚠️ | ⚠️ | AUDIT-65,67–70 | Mirroring, write-guard conditions |
| §6.3.11 | SMMU_CR1 | ⚠️ | ⚠️ | AUDIT-61, AUDIT-66, AUDIT-67/68 | TABLE_*/QUEUE_* write guards; split guard per field group |
| §6.3.11.1 | CR1.TABLE_* attributes | ⚠️ | ⚠️ | AUDIT-67, AUDIT-66, AUDIT-68 | SMMUEN+CR0ACK guard |
| §6.3.11.2 | CR1.QUEUE_* attributes | ⚠️ | ⚠️ | AUDIT-66 | Queue-enable guard (not SMMUEN) |
| §6.3.12 | SMMU_CR2 | ⚠️ | ⚠️ | AUDIT-54,65–66, AUDIT-60, AUDIT-67 | E2H, PTM, RECINVSID; SMMUEN+CR0ACK write guard |
| §6.3.12.1 | CR2.PTM | ⚠️ | ⚠️ | AUDIT-54 | PTM polarity fixed (PTM=1 skips, PTM=0 participates) |
| §6.3.12.2 | CR2.RECINVSID | ⚠️ | ⚠️ | | |
| §6.3.12.3 | CR2.E2H | ⚠️ | ⚠️ | | |
| §6.3.13 | SMMU_STATUSR | ✅ | ✅ | | DORMANT bit; RO; bits[31:1]=RES0; DORMHINT=1 hardcoded; reset=0; conformant |

### §6.3 Global Bypass and Fault Registers

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §6.3.14 | SMMU_GBPA | ✅ | ✅ | AUDIT-76, BUG-QA-11, BUG-GBPA-01, BUG-GBPA-02, BUG-GBPA-03, BUG-GBPA-04 | INSTCFG/PRIVCFG 0b01→0b00 normalized; ALLOCCFG 0b0xxx→0 (use incoming); field width masking; Update bit sync model conformant |
| §6.3.14.1 | GBPA update procedure | ✅ | ✅ | AUDIT-76 | Synchronous model: Update always 0; no raw register exposed |
| §6.3.15 | SMMU_AGBPA | N/A | N/A | | All bits IMPL DEF; unsupported → RES0 per spec; no behavioral req |
| §6.3.16 | SMMU_IRQ_CTRL | ✅ | ✅ | BUG-IRQ-01, BUG-IRQ-02 | bits[31:3]=RES0 masked; PRIQ_IRQEN gated on IDR0.PRI; IRQ_CTRLACK sync model conformant |
| §6.3.17 | SMMU_S2PII | N/A | N/A | | IDR3.S2PI=0 → reg not present → RES0; FEAT_S2PIE not advertised; conformant |
| §6.3.18 | SMMU_IRQ_CTRLACK | ✅ | ✅ | | RO; mirrors IRQ_CTRL; bits[31:3]=RES0; PRIQ_IRQEN RAZ when PRI=0; reset=0; conformant via §6.3.16 fixes |
| §6.3.19 | SMMU_GERROR | ✅ | ✅ | AUDIT-93, BUG-QA-1–6 | RO; XOR-toggle protocol; RES0 bits correct; MSI/PRI/DPT/ECMDQ bits=0 (features not advertised); reset=0; conformant |
| §6.3.20 | SMMU_GERRORN | ✅ | ✅ | AUDIT-93 | RW; SMMU never writes it; CAS-atomic toggle; inactive-bit guard; RES0 implicit; reset=0; conformant |
| §6.3.21 | SMMU_GERROR_IRQ_CFG0 | N/A | N/A | | IDR0.MSI=0 → not present → RES0; MSI not implemented |
| §6.3.22 | SMMU_GERROR_IRQ_CFG1 | N/A | N/A | | IDR0.MSI=0 → not present → RES0 |
| §6.3.23 | SMMU_GERROR_IRQ_CFG2 | N/A | N/A | | IDR0.MSI=0 → not present → RES0 |

### §6.3 Stream Table Base Registers

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §6.3.24 | SMMU_STRTAB_BASE | ✅ | ✅ | AUDIT-69, AUDIT-63, AUDIT-67 | ADDR/RA not stored (SW model uses HashMap); write guard (SMMUEN=0) enforced on all behavioral fields; conformant |
| §6.3.25 | SMMU_STRTAB_BASE_CFG | ✅ | ✅ | AUDIT-70, AUDIT-63, AUDIT-69/70 | FMT reserved→0b00; SPLIT invalid→6; LOG2SIZE effective=MIN(v,SIDSIZE); write guard enforced; conformant |

### §6.3 Command Queue Registers

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §6.3.26 | SMMU_CMDQ_BASE | ✅ | ✅ | | ADDR/RA not stored (SW model); LOG2SIZE fixed==IDR1.CMDQS; write guard trivially satisfied; conformant |
| §6.3.27 | SMMU_CMDQ_PROD | ✅ | ✅ | BUG-NEW-6 | RES0 bits[31:20] implicit; wrap semantics correct; SMMU never writes PROD; conformant |
| §6.3.28 | SMMU_CMDQ_CONS | ✅ | ✅ | AUDIT-93, AUDIT-58 | ERR[30:24] separate atomic; RD RAZ above QS+1 implicit; RD frozen on error; two-step recovery; conformant |

### §6.3 Event Queue Registers

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §6.3.29 | SMMU_EVENTQ_BASE | ✅ | ✅ | | ADDR/WA not stored (SW model); LOG2SIZE fixed==IDR1.EVENTQS; EVENTQEN gate enforced throughout; conformant |
| §6.3.30 | SMMU_EVENTQ_IRQ_CFG0 | N/A | N/A | | IDR0.MSI=0 → not present → RES0 |
| §6.3.31 | SMMU_EVENTQ_IRQ_CFG1 | N/A | N/A | | IDR0.MSI=0 → not present → RES0 |
| §6.3.32 | SMMU_EVENTQ_IRQ_CFG2 | N/A | N/A | | IDR0.MSI=0 → not present → RES0 |
| §6.3.95 | SMMU_EVENTQ_PROD | ⚠️ | ⚠️ | BUG-QA-3, BUG-NEW-A/F (20Mar) | OVFLG toggle; OVFLG mask in empty check |
| §6.3.96 | SMMU_EVENTQ_CONS | ⚠️ | ⚠️ | BUG-QA-3, BUG-NEW-F (20Mar) | OVACKFLG, RD; OVACKFLG mask in empty check |

### §6.3 PRI Queue Registers

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §6.3.33 | SMMU_PRIQ_BASE | ✅ | ✅ | | ADDR/WA not stored (SW model); LOG2SIZE fixed==IDR1.PRIQS; present when PRI=1; write guard trivially satisfied; conformant |
| §6.3.34 | SMMU_PRIQ_IRQ_CFG0 | N/A | N/A | | IDR0.MSI=0 → not present → RES0 |
| §6.3.35 | SMMU_PRIQ_IRQ_CFG1 | N/A | N/A | | IDR0.MSI=0 → not present → RES0 |
| §6.3.36 | SMMU_PRIQ_IRQ_CFG2 | N/A | N/A | | IDR0.MSI=0 → not present → RES0 |
| §6.3.97 | SMMU_PRIQ_PROD | ⚠️ | ⚠️ | BUG-QA-4/5, BUG-NEW-D (20Mar) | OVFLG; bit-31 preservation at call sites only |
| §6.3.98 | SMMU_PRIQ_CONS | ⚠️ | ⚠️ | BUG-QA-4/5, BUG-RUST-Q4 | OVACKFLG; strict FIFO CONS advancement |

### §6.3 GATOS (Address Translation Operations) Registers

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §6.3.37 | SMMU_GATOS_CTRL | ✅ | ✅ | AUDIT-39, AUDIT-62 | SMMUEN=0→FAULT/0xFD; sync model: RUN always 0; IDR0.ATOS=1; conformant |
| §6.3.38 | SMMU_GATOS_SID | ✅ | ✅ | AUDIT-82 | Out-of-range SID→C_BAD_STREAMID; STREAMID passed directly; conformant |
| §6.3.39 | SMMU_GATOS_ADDR | ✅ | ✅ | NEW-GAP | AccessType encodes PnU/RnW/InD; HTTUI not modeled (SW model); TYPE stage-select is sim limitation; prior gaps addressed |
| §6.3.40 | SMMU_GATOS_PAR | ✅ | ✅ | AUDIT-49, AUDIT-38, AUDIT-64, AUDIT-82 | ATTR/SH/FAULTCODE/REASON all correct; doc references fixed (§9.3.3→§6.3.40/§9.1.4); stale test comment corrected; all 4 prior audit bugs verified fixed; 211 tests pass |

### §6.3 MPAM Registers (Out of Scope)

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §6.3.41 | SMMU_MPAMIDR | 🚫 | 🚫 | | MPAM out of scope |
| §6.3.42 | SMMU_GMPAM | 🚫 | 🚫 | | |
| §6.3.43 | SMMU_GBPMPAM | 🚫 | 🚫 | | |

### §6.3 VATOS and ECMDQ Registers

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §6.3.44 | SMMU_VATOS_SEL | 🚫 | 🚫 | | VATOS not implemented (IDR2.BA_VATOS=0) |
| §6.3.99 | SMMU_VATOS_CTRL | 🚫 | 🚫 | | VATOS not implemented |
| §6.3.100 | SMMU_VATOS_SID | 🚫 | 🚫 | | VATOS not implemented |
| §6.3.101 | SMMU_VATOS_ADDR | 🚫 | 🚫 | | VATOS not implemented |
| §6.3.102 | SMMU_VATOS_PAR | 🚫 | 🚫 | | VATOS not implemented |
| §6.3.46 | SMMU_DPT_BASE | 🚫 | 🚫 | | DPT out of scope |
| §6.3.47 | SMMU_DPT_BASE_CFG | 🚫 | 🚫 | | |
| §6.3.48 | SMMU_DPT_CFG_FAR | 🚫 | 🚫 | | |
| §6.3.49 | SMMU_CMDQ_CONTROL_PAGE_BASE<n> | 🚫 | 🚫 | | ECMDQ not implemented (IDR6.ECMDQ=0) |
| §6.3.50 | SMMU_CMDQ_CONTROL_PAGE_CFG<n> | 🚫 | 🚫 | | ECMDQ not implemented |
| §6.3.51 | SMMU_CMDQ_CONTROL_PAGE_STATUS<n> | 🚫 | 🚫 | | ECMDQ not implemented |
| §6.3.107 | SMMU_ECMDQ_BASE<n> | 🚫 | 🚫 | | ECMDQ not implemented |
| §6.3.108 | SMMU_ECMDQ_PROD<n> | 🚫 | 🚫 | | ECMDQ not implemented |
| §6.3.109 | SMMU_ECMDQ_CONS<n> | 🚫 | 🚫 | | ECMDQ not implemented |

### §6.3 Secure Registers (Out of Scope)

| Section | Title | C++ | Rust | Notes |
|---------|-------|-----|------|-------|
| §6.3.52–94 | SMMU_S_IDR*, S_CR*, S_GERROR, S_STRTAB_BASE, S_CMDQ, S_EVENTQ, S_GATOS, S_MPAMIDR, S_GMPAM, S_GBPMPAM, S_VATOS_SEL, S_IDR6, S_CMDQ_CONTROL_PAGE | 🚫 | 🚫 | Secure interface out of scope |

### §6.3 Root and Realm Registers (Out of Scope)

| Section | Title | C++ | Rust | Notes |
|---------|-------|-----|------|-------|
| §6.3.110–121 | SMMU_ROOT_IDR0, ROOT_IIDR, ROOT_CR0/ACK, ROOT_GPT_BASE/CFG, ROOT_GPF_FAR, ROOT_GPT_CFG_FAR, ROOT_TLBI, ROOT_TLBI_CTRL, ROOT_GPT_BASE2, ROOT_GPT_BASE_UPDATE | 🚫 | 🚫 | Root page out of scope |
| §6.3.122–169 | SMMU_R_IDR*, R_CR*, R_GERROR, R_STRTAB_BASE, R_CMDQ, R_EVENTQ, R_PRIQ, R_MPAMIDR, R_GMPAM, R_DPT*, R_MECIDR, R_GMECID, R_CMDQ_CONTROL_PAGE | 🚫 | 🚫 | Realm interface out of scope |
| §6.3.170 | ID_REGS | N/A | N/A | | Pure identification registers (CIDR0-3, PIDR0-7); no spec-mandated software flow consumes these; simulation model has no MMIO register map; out of scope |

---

## Chapter 7 — Faults, Errors and Event Queue

### §7.1–7.2 Command Queue Errors and Event Queue

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §7.1 | Command queue errors | ⚠️ | ⚠️ | AUDIT-72, 93, AUDIT-58 | CERROR_ILL, two-step CONS→GERROR recovery; CONS.ERR field |
| §7.2 | Event queue recorded faults and events | ⚠️ | ⚠️ | AUDIT-75 | EVENTQEN gate |
| §7.2.1 | Recording of events and conditions for writing to Event queue | ⚠️ | ⚠️ | AUDIT-75, BUG-NEW-1 | Conditions for event write; S1DSS+stage2 missing events fixed |
| §7.2.2 | Event queue access external abort | N/A | N/A | | IMPDEF: bus abort on queue write; simulation uses in-memory VecDeques with no external abort capability; EVENTQ_ABT_ERR not applicable |
| §7.2.3 | Secure and Non-secure Event queues | 🚫 | 🚫 | | Secure out of scope |

### §7.3 Event Records

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §7.3 | Event records (overview, common fields) | ⚠️ | ⚠️ | AUDIT-92, BUG-NEW-RUST-1/2, AUDIT-90 | SSV field computation (transaction vs capability); rnw/ind/pnu fixed |
| §7.3.1 | Event record merging | N/A | ✅ | BUG-7.3.1-01 ✅ | Stall events (Stall==1) never merged per §7.3.1; fixed missing `!event.stall` guard in MEV dedup predicate. MEV builder method added. 3 TDD tests. |
| §7.3.2 | F_UUT | ⚠️ | ⚠️ | AUDIT-92 | SSV, STALL=0 |
| §7.3.3 | C_BAD_STREAMID | ⚠️ | ⚠️ | CONF-GAP, AUDIT-82, BUG-NEW-RUST-1/2 | Out-of-range SID; GERROR/RECINVSID gating |
| §7.3.4 | F_STE_FETCH | ⚠️ | ⚠️ | NEW-AUDIT-05 | class=2 (TTE), SSV |
| §7.3.5 | C_BAD_STE | ⚠️ | ⚠️ | AUDIT-01,36,40,42–48,88, BUG-11, AUDIT-58 | Extensively audited; C_BAD_STE emitted before return |
| §7.3.6 | F_BAD_ATS_TREQ | ⚠️ | ⚠️ | AUDIT-38, 92 | SSV, STALL=0, FAULTCODE |
| §7.3.7 | F_STREAM_DISABLED | ⚠️ | ⚠️ | NEW-FINDING-1, BUG-E, RUST-4 (21Mar) | S1DSS=0b10+PASID=0 exception; RES0 fields zeroed |
| §7.3.8 | F_TRANSL_FORBIDDEN | ⚠️ | ⚠️ | AUDIT-38 | Forbidden config |
| §7.3.9 | C_BAD_SUBSTREAMID | ⚠️ | ⚠️ | | Out-of-range SSID |
| §7.3.10 | F_CD_FETCH | ⚠️ | ⚠️ | NEW-AUDIT-05 | class=2, SSV |
| §7.3.11 | C_BAD_CD | ⚠️ | ⚠️ | AUDIT-40, BUG-NEW-F, RUST-2 (21Mar) | CD.HD+HTTU, CD.S+Stall; SSV in C_BAD_CD record |
| §7.3.12 | F_WALK_EABT | ⚠️ | ⚠️ | AUDIT-89, BUG-QA-8/10, NEW-AUDIT-05 | event_type=0x0B, class=1 (TTD), SSV |
| §7.3.13 | F_TRANSLATION | ⚠️ | ⚠️ | AUDIT-90, BUG-NEW-CPP-A, AUDIT-91, NEW-03 | rnw from access_type; S2=true, IPA populated for stage-2 faults |
| §7.3.14 | F_ADDR_SIZE | ⚠️ | ⚠️ | AUDIT-84, BUG-CPP-TWOSTAGE-1, BUG-RUST-TWOSTAGE-S1-FAULT-CLASS | stage-1 PA > OAS; two-stage F_ADDR_SIZE; correct event type |
| §7.3.15 | F_ACCESS | ⚠️ | ⚠️ | | |
| §7.3.16 | F_PERMISSION | ⚠️ | ⚠️ | AUDIT-90, BUG-CPP-8, NEW-D/E/F (18Mar) | rnw from access_type; SecurityFault→F_PERMISSION; S2=true/IPA for PTW faults |
| §7.3.17 | F_TLB_CONFLICT | 🚫 | 🚫 | | IMPL DEF — out of scope |
| §7.3.18 | F_CFG_CONFLICT | 🚫 | 🚫 | | IMPL DEF — out of scope |
| §7.3.19 | E_PAGE_REQUEST | ⚠️ | ⚠️ | BUG-QA-6, BUG-NEW-7 | Permission bits; E_PAGE_REQUEST emit timing |
| §7.3.20 | F_VMS_FETCH | 🚫 | ⚠️ | BUG-7322-01 ✅ | VMS fetch not simulated; inject_vms_fetch_abort() added for completeness |
| §7.3.21 | IMPDEF_EVENTn | 🚫 | 🚫 | | IMPL DEF |
| §7.3.22 | Event queue record priorities | ☐ | ✅ | BUG-7322-01 ✅ | Priority ordering enforced by structural early-returns; F_VMS_FETCH inject added |

### §7.4–7.5 Overflow and Global Errors

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §7.4 | Event queue overflow | ⚠️ | ✅ | BUG-QA-1/3, RUST-2 (22Mar) | All 5 §7.4 requirements conformant; stale doc comment fixed; debug_assert added to enqueue_event; get_events() stall-drain test added |
| §7.5 | Global error recording | ⚠️ | ✅ | AUDIT-93, BUG-GERROR-01 ✅ | Toggle/active/CMDQ_ERR gate all conformant; GERROR_DPT_ERR bit 10 added; IRQ_CTRL named constants added |
| §7.5.1 | GERROR interrupt notification | ⚠️ | ✅ | BUG-GERROR-02/03 ✅ | gerror_irq_pending flag added; signal_gerror() now gates on GERROR_IRQEN; MSI_GERROR_ABT_ERR excluded per spec |

---

## Chapter 8 — Page Request Queue

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §8.1 | PRI queue overflow | ⚠️ | ⚠️ | BUG-QA-4/5, BUG-NEW-21/22 | OVFLG, inhibit new PPRs; priq_emitted CAS; E_PAGE_REQUEST emit |
| §8.1.1 | Recovery procedure | N/A | ✅ | BUG-8-1-1-01 ✅ | recover_priq_overflow(): classify complete/incomplete PRGs, clear queue, acknowledge OVFLG; 3 TDD tests added |
| §8.2 | Miscellaneous | ⚠️ | ✅ | BUG-QA-2, BUG-PRIQEN-ASYMMETRY, BUG-NEW-15, BUG-8-2-01/02 ✅ | PRIQ_ABT_ERR gate + Secure stream discard (ResponseCode=0b1111); 6 TDD tests; FINDING-NEW-32 test updated to spec |
| §8.3 | PRG Response Message codes | ⚠️ | ✅ | BUG-NEW-A (27Mar) ✅ | Resp=0b11 → CERROR_ILL; all 3 valid codes (0b00/01/10) accept+consume entry; 5 TDD tests added |

---

## Chapter 9 — Address Translation Operations (ATOS)

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §9.1 | Register usage (overview) | ✅ | ✅ | AUDIT-38,39,49 | All prior bugs fixed; §6.3.37-§6.3.40 verified ✅; re-audited 2026-04-10 — no new gaps |
| §9.1.1 | ATOS_CTRL | ✅ | ✅ | AUDIT-39, AUDIT-64 | SMMUEN check first; INV_STAGE for bypass/disabled; verified conformant |
| §9.1.2 | ATOS_SID | ✅ | ✅ | AUDIT-82 | Out-of-range SID→C_BAD_STREAMID; verified conformant |
| §9.1.3 | ATOS_ADDR | ✅ | ✅ | NEW-GAP | PnU, RnW, HTTUI; AccessType encoding correct; HTTUI=software model limitation; verified conformant |
| §9.1.4 | ATOS_PAR | ✅ | ✅ | AUDIT-49, 38, AUDIT-49, NEW-GAP-A | ATTR/SH/FAULTCODE/REASON all correct; 4-value REASON encoding confirmed; verified conformant |
| §9.1.5 | ATOS_PAR.FAULTCODE encodings | ✅ | ✅ | AUDIT-38, AUDIT-38 | All FAULTCODE values correct; priority ordering conformant; verified conformant |
| §9.1.6 | SMMU_(S_)VATOS_SEL | 🚫 | 🚫 | | VATOS not implemented; IDR0.VATOS=0 (optional feature, spec §6.3.1); IDR2 BA_VATOS=0; out of scope |

---

## Chapter 10 — Performance Monitors Extension

| Section | Title | C++ | Rust | Notes |
|---------|-------|-----|------|-------|
| §10.1–10.7 | All PMCG sections | 🚫 | 🚫 | Performance monitors out of scope |

---

## Chapter 11 — Debug/Trace

| Section | Title | C++ | Rust | Notes |
|---------|-------|-----|------|-------|
| §11 | Debug/Trace | 🚫 | 🚫 | Out of scope |

---

## Chapter 12 — RAS (Reliability, Availability, Serviceability)

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §12.1 | Error propagation, consumption, containment | N/A | N/A | | Guidance/informational only — no normative requirements; RAS optional (not implemented, no IDR0.RAS); SFM covered by §12.3 |
| §12.2 | Error consumption visible through SMMU interface | ✅ | ✅ | | F_WALK_EABT/F_STE_FETCH/F_CD_FETCH all defined+wired; CMDQ_ERR+CERROR_ABT conformant; EVENTQ_ABT_ERR/PRIQ_ABT_ERR constants defined+gated (hardware-only signals, N/A for SW model); §12.2 uses "expected to" guidance language — no normative gaps |
| §12.3 | Service Failure Mode (SFM) | ✅ | ✅ | BUG-QA-1, BUG-12.3-A | SFM_ERR bit defined; BUG-12.3-A: SFM gate added to translate() and submit_page_request() — returns ServiceFailureMode error; clear_gerror() restores operation; 4 TDD tests added (212 total); 0 clippy warnings |
| §12.4 | RAS fault handling/reporting | N/A | N/A | | Conditional: "When RAS facilities are implemented" — SW model does not implement RAS (no hardware error recording registers); all requirements IMPLEMENTATION DEFINED |
| §12.5 | Confidential information in RAS Error Records | 🚫 | 🚫 | | RME-only — out of scope |
| §12.6 | Recommendations for reporting SMMU events in RAS | N/A | N/A | | Explicitly non-normative: "This section is only a recommendation and is not normative" |
| §12.6.1 | SMMU architectural events | N/A | N/A | | Non-normative recommendation only |
| §12.6.1.1 | Deferred error on structure fetch | N/A | N/A | | Non-normative recommendation only |
| §12.6.1.2 | Uncorrectable error on structure fetch | N/A | N/A | | Non-normative recommendation only |
| §12.6.1.3 | Error on Command queue fetch | N/A | N/A | | Non-normative recommendation only |
| §12.6.2 | Common SMMU microarchitectural events | N/A | N/A | | Non-normative recommendation only |
| §12.6.2.1 | ECC/EDC error on TLB or config cache | N/A | N/A | | Non-normative recommendation only |
| §12.6.2.2 | Error on data payload of client transaction | N/A | N/A | | Non-normative recommendation only |

---

## Chapter 13 — Attribute Transformation

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §13.1 | SMMU handling of attributes | ⚠️ | ⚠️ | NEW-GAP-A-D | |
| §13.1.1 | Attribute definitions | N/A | N/A | | Informational |
| §13.1.2 | Attribute support | N/A | ⚠️ | BUG-13.1.2-A, BUG-13.1.2-B | BUG-13.1.2-A: effective_access_type() Write→Execute under INSTCFG=3 fixed (writes must stay Data per spec); BUG-13.1.2-B: GBPA bypass raw inst_cfg clamped to 0 for write accesses; 6 TDD tests added |
| §13.1.3 | Default input attributes | N/A | N/A | | SW model always receives all attributes explicitly (access_type + security_state); "interconnect can't convey attribute" precondition never applies |
| §13.1.4 | Replace | ⚠️ | ⚠️ | BUG-13.1.4-A, BUG-13.1.4-D | BUG-13.1.4-A: ATOS (gatos_translate) incorrectly applied INSTCFG/PRIVCFG overrides — fixed by adding TransactionType::Atos and bypassing effective_access_type() on ATOS path; BUG-13.1.4-D: Device/NC types must force OSH regardless of SHCFG — fixed in apply_output_attrs(); BUG-13.1.4-C/F: N/A (Far Atomic/DCP not modelled as distinct AccessTypes); 2 TDD tests added |
| §13.1.5 | Combine | ☐ | ☐ | | MemAttr combine rules |
| §13.1.5.1 | Combine examples | N/A | N/A | | |
| §13.1.6 | Stage 2 control of memory types | ☐ | ☐ | | S2FWB, S2ENDI |
| §13.1.7 | Ensuring consistent output attributes | ☐ | ☐ | | |
| §13.2 | SMMU disabled global bypass attributes | ⚠️ | ⚠️ | BUG-QA-11 | GBPA MemAttr/SH when bypass |
| §13.3 | STE bypasses stage 1 and stage 2 | ⚠️ | ⚠️ | BUG-QA-11, BUG-NEW-CPP-D | STE bypass output attrs (memType/shareability); bypass skips streamEnabled |
| §13.4 | Normal translation flow | ⚠️ | ⚠️ | | |
| §13.4.1 | Stage 1 page permissions | ⚠️ | ⚠️ | | WXN, UWXN, EPAN |
| §13.4.2 | Stage 1 memory attributes | ⚠️ | ⚠️ | NEW-GAP | NSCFG0/NSCFG1, MAIR |
| §13.4.3 | Stage 2 | ⚠️ | ⚠️ | | S2FWB |
| §13.4.4 | Output | ⚠️ | ⚠️ | | Final attr combination |
| §13.5 | Summary of attribute/permission configuration fields | N/A | N/A | | Reference table |
| §13.6 | PCle and ATS attribute/permissions handling | ⚠️ | ⚠️ | | |
| §13.6.1 | PCle memory type attributes | ☐ | ☐ | | No_snoop |
| §13.6.1.1 | No_snoop | ☐ | ☐ | | |
| §13.6.2 | ATS attribute overview | ☐ | ☐ | | |
| §13.6.2.1 | Supporting No_snoop with ATS | ☐ | ☐ | | |
| §13.6.3 | Split-stage ATS behavior | ⚠️ | ⚠️ | | EATS=0b10 |
| §13.6.4 | Full ATS skipping stage 1 | ☐ | ☐ | | EATS=0b01 |
| §13.6.5 | Split-stage ATS skipping stage 1 | ☐ | ☐ | | |
| §13.7 | PCle permission attribute interpretation | ⚠️ | ⚠️ | BUG-QA-6 | E_PAGE_REQUEST perm bits |
| §13.7.1 | Permission attributes granted in ATS Translation Completions | ☐ | ☐ | | |
| §13.8 | Attributes for SMMU-originated accesses | ☐ | ☐ | | PTW MemAttr |

---

## Chapter 14 — External Interfaces

| Section | Title | C++ | Rust | Notes |
|---------|-------|-----|------|-------|
| §14.1 | Data path ingress/egress ports | N/A | N/A | Hardware interface — not modeled |
| §14.2 | ATS Interface, packets, protocol | N/A | N/A | Hardware interface |
| §14.3 | SMMU-originated transactions | ☐ | ☐ | PTW transaction properties |

---

## Chapter 15 — Translation Procedure

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §15.1 | Translation procedure charts | ⚠️ | ⚠️ | | Cross-check flowcharts with §3/5 audits |
| §15.2 | Notes on translation procedure charts | N/A | N/A | | Informational |

---

## Chapter 16 — System and Implementation Considerations

| Section | Title | C++ | Rust | Bugs | Notes |
|---------|-------|-----|------|------|-------|
| §16.1 | Stages | N/A | N/A | | Informational |
| §16.2 | Caching | ☐ | ☐ | | TLB and config cache behavior |
| §16.2.1 | Caching combined structures | ☐ | ☐ | | L1CD+CD combined caching |
| §16.2.2 | Data dependencies between structures | ☐ | ☐ | | |
| §16.3 | Programming implications of bus address sizing | ☐ | ☐ | | |
| §16.4 | System integration | N/A | N/A | | Informational |
| §16.4.1 | System integration for SMMU with RME DA | 🚫 | 🚫 | | Realm out of scope |
| §16.5 | System software | N/A | N/A | | Software guidance |
| §16.6 | IMPLEMENTATION DEFINED features | N/A | N/A | | Informational |
| §16.6.1 | Configuration and translation cache locking | ☐ | ☐ | | |
| §16.7 | Interconnect-specific features | ☐ | ☐ | | |
| §16.7.1 | Reporting of Unsupported Client Transactions | ☐ | ☐ | | F_UUT |
| §16.7.2 | Non-data transfer transactions (CMO) | ☐ | ☐ | | |
| §16.7.2.1 | Control of Cache Maintenance Operations | ☐ | ☐ | | |
| §16.7.2.2 | Permissions model for CMOs | ☐ | ☐ | | |
| §16.7.2.3 | Memory types and Shareability for CMOs | ☐ | ☐ | | |
| §16.7.3 | Treatment of AMBA Exclusives | ☐ | ☐ | | |
| §16.7.4 | Treatment of downstream aborts | ☐ | ☐ | | |
| §16.7.5 | SMMU and AMBA attribute differences | ☐ | ☐ | | |
| §16.7.5.1 | Conversion of AMBA attributes to Armv8 on input | ☐ | ☐ | | |
| §16.7.5.1.1 | Input attribute conversion table | ☐ | ☐ | | |
| §16.7.5.2 | Conversion of Armv8 attributes to AMBA on output | ☐ | ☐ | | |
| §16.7.5.2.1 | Output attribute conversion table | ☐ | ☐ | | |
| §16.7.5.3 | Common interpretation between SMMU and PE | ☐ | ☐ | | |
| §16.7.6 | Far Atomic operations | ☐ | ☐ | | |
| §16.7.7 | AMBA DVM messages with CD.ASET==1 | ☐ | ☐ | | |
| §16.8 | Summary of SMMU transactions | N/A | N/A | | Reference table |

---

## Chapter 17 — Memory System Resource Partitioning and Monitoring (MPAM)

| Section | Title | C++ | Rust | Notes |
|---------|-------|-----|------|-------|
| §17.1–17.7 | All MPAM sections | 🚫 | 🚫 | MPAM out of scope |

---

## Chapter 18 — Support for Memory Encryption Contexts

| Section | Title | C++ | Rust | Notes |
|---------|-------|-----|------|-------|
| §18 | Memory Encryption Contexts | 🚫 | 🚫 | MEC/MECID — out of scope |

---

## Summary of Out-of-Scope Areas

The following areas are intentionally not implemented and will not be audited:

| Area | Reason |
|------|--------|
| Secure programming interface (SMMU_S_*) | Out of scope for non-secure model |
| Root Control Page (SMMU_ROOT_*) | RME root management — out of scope |
| Realm Register Pages (SMMU_R_*) | RME Realm state — out of scope |
| VMS (Virtual Machine Structures) | §5.6 — not modeled |
| MPAM (Ch. 17, SMMU_MPAMIDR, GMPAM, GBPMPAM) | Not modeled |
| DPT (Device Permission Table) §3.24, §4.6 | Not modeled |
| GPC (Granule Protection Checks) §3.25 | GPT — not modeled |
| Performance Monitors Extension (Ch. 10) | PMCG — not modeled |
| Debug/Trace (Ch. 11) | Not modeled |
| Memory Encryption Contexts (Ch. 18) | MECID — not modeled |
| F_TLB_CONFLICT / F_CFG_CONFLICT | IMPLEMENTATION DEFINED |
| CXL interactions (§3.9.3) | Not modeled |

---

## Audit Progress Tracker

| Chapter | Total Sections | ✅ Verified (N/A+✅) | ⚠️ Audited+Fixed | ☐ Not Started | 🚫 Out of Scope |
|---------|---------------|---------------------|-----------------|--------------|----------------|
| Ch. 1 About | 3 | 3 (3 N/A) | 0 | 0 | 0 |
| Ch. 2 Introduction | 10 | 4 (3 N/A + 1 ✅) | 4 | 0 | 2 |
| Ch. 3 Operation | 120 | 88 (48 N/A + 40 ✅) | 10 | 0 | 22 |
| Ch. 4 Commands | 52 | 3 (3 N/A) | 47 | 0 | 2 |
| Ch. 5 Data Structures | 19 | 3 (2 N/A + 1 ✅) | 13 | 0 | 3 |
| Ch. 6 Registers | 87 | 41 (19 N/A + 22 ✅) | 22 | 0 | 24 |
| Ch. 7 Faults/Events | 31 | 6 (1 N/A + 5 ✅) | 21 | 0 | 4 |
| Ch. 8 PRI | 4 | 3 (3 ✅) | 1 | 0 | 0 |
| Ch. 9 ATOS | 7 | 6 (6 ✅) | 0 | 0 | 1 |
| Ch. 10 PMCG | 1 | 0 | 0 | 0 | 1 |
| Ch. 11 Debug | 1 | 0 | 0 | 0 | 1 |
| Ch. 12 RAS | 13 | 12 (10 N/A + 2 ✅) | 0 | 0 | 1 |
| Ch. 13 Attrs | 28 | 4 (4 N/A) | 13 | 11 | 0 |
| Ch. 14 External | 3 | 2 (2 N/A) | 0 | 1 | 0 |
| Ch. 15 Trans. Proc. | 2 | 1 (1 N/A) | 1 | 0 | 0 |
| Ch. 16 System | 27 | 5 (5 N/A) | 0 | 21 | 1 |
| Ch. 17 MPAM | 1 | 0 | 0 | 0 | 1 |
| Ch. 18 MEC | 1 | 0 | 0 | 0 | 1 |
| **TOTAL** | **410** | **181 (N/A+✅)** | **132** | **33** | **64** |

**Notes on status symbols**: All ⚠️ rows indicate sections that have been audited; bugs found were filed
and fixed. No currently OPEN bugs remain. The ⚠️ symbol means "audited with bugs found and fixed —
re-audit recommended to confirm full coverage." ✅ is reserved for sections with zero bugs ever found
and confirmed clean.

**Last updated**: 2026-04-10 (Session: §13.1.4 Replace — BUG-13.1.4-A ATOS ignores INSTCFG/PRIVCFG + BUG-13.1.4-D Device/NC forces OSH; CURRENT_SECTION→13.1.5)
**Current bug count**: BUG-AUDIT-01 through BUG-AUDIT-127 — all fixed ✅; BUG-12.3-A, BUG-13.1.2-A/B, BUG-13.1.4-A/D — all fixed ✅
**Additional named batches fixed**: CONF-GAP series, BUG-QA series, BUG-NEW series, BUG-CPP/RUST series
**Test status**: C++ 185/185 | Rust 214/214 (all suites green) | 0 clippy warnings
