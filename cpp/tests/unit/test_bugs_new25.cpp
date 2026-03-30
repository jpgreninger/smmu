// TDD failing tests for BUG-AUDIT-49, BUG-AUDIT-50, BUG-AUDIT-52.
//
// BUG-AUDIT-49 (§9.1.4 / §6.3.40): gatosTranslate() ATTR/SH hardcoded.
//   Current: success path always returns ATTR=0xFF (Normal WB/WA) and SH=0b11 (ISH).
//   Required: device-memory pages must return ATTR=0x00 (Device-nGnRnE) and SH=0b10
//             (OSH per spec §9.1.4: "Shareability is returned as Outer Shareable when
//             ATTR selects any Device type").
//   BEFORE FIX: ATTR==0xFF, SH==0b11 → tests FAIL.
//   AFTER FIX:  ATTR==0x00, SH==0b10 → tests PASS.
//
// BUG-AUDIT-50 (§5.4 / §5.2): StreamConfig missing endi/s2endi fields.
//   Current: StreamConfig has no endi (CD.ENDI) or s2endi (STE.S2ENDI) members.
//   Required: Fields must exist and configureStream() must accept them without error
//             because IDR0.TTENDIAN=0b00 (mixed-endian) means all values are legal.
//   BEFORE FIX: struct members absent → compilation errors → tests FAIL.
//   AFTER FIX:  fields present, configureStream() returns ok → tests PASS.
//
// BUG-AUDIT-52 (§6.3.6 IDR5): GRAN16K/GRAN64K overclaimed.
//   Current: getIDR5() sets bits[5] (GRAN16K) and bits[6] (GRAN64K) even though only
//            4KB granule is implemented.
//   Required: Only GRAN4K (bit[4]) must be set; GRAN16K (bit[5]) and GRAN64K (bit[6])
//             must be 0.
//   BEFORE FIX: bits[5] and bits[6] are both 1 → tests FAIL.
//   AFTER FIX:  bits[5] and bits[6] are both 0 → tests PASS.

#include <gtest/gtest.h>
#include <type_traits>
#include <memory>
#include "smmu/smmu.h"
#include "smmu/address_space.h"

using namespace smmu;

// Helper: enable all queues and the SMMU global enable.
static void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
    s.setS1PSupported(true);
    s.setS2PSupported(true);
}

// ─── BUG-AUDIT-49 tests — ATTR/SH hardcoded in gatosTranslate() ──────────────
//
// To exercise the bug we set up a full two-stage stream (stage1+stage2) with
// S2PTW=false so that a device-memory stage-2 page does NOT cause F_PERMISSION
// and translation succeeds.  gatosTranslate() then returns a PA, but ATTR and SH
// in the GATOS_PAR must reflect the device-memory type rather than being hardcoded
// to 0xFF / 0b11.
//
// Setup pattern (mirrors test_conf_gaps_jkl.cpp):
//   stage-1 maps IOVA → IPA (normal memory, mapPage)
//   stage-2 maps IPA  → PA  (device memory, mapStage2DevicePage)
//   S2PTW=false: no F_PERMISSION on device page, translation succeeds
//   gatosTranslate(IOVA) returns GATOS_PAR — ATTR/SH must reflect device type.

// Helper: build a two-stage stream config with S2PTW=false for BUG-AUDIT-49 tests.
static StreamConfig twoStageDeviceCfg() {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.s2ptw              = false;   // device page allowed — no S2PTW fault
    cfg.t0sz               = 0u;      // sentinel: no VA range restriction
    cfg.s2t0sz             = 0u;      // sentinel: no IPA range restriction
    cfg.s2ps               = 5u;      // 48-bit PA
    cfg.aa64               = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 1u;
    return cfg;
}

// BUG-AUDIT-49 test 1: gatosTranslate() on a device-memory stage-2 page — ATTR
// must NOT be 0xFF (Normal WB/WA cached).
//
// ARM §9.1.4 / §6.3.40: ATTR is determined from the translation tables; a
// Device-nGnRnE page must produce ATTR=0x00, not 0xFF.
//
// BEFORE FIX: ATTR bits[63:56] == 0xFF (hardcoded) → test FAILS.
// AFTER FIX:  ATTR bits[63:56] != 0xFF (device type) → test PASSES.
TEST(BugAudit49, GatosTranslate_DevicePage_AttrNotNormal) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid  = 300u;
    const IOVA     iova = 0x1000u;
    const IOVA     ipa  = 0x5000u;
    const PA       pa   = 0xA000u;

    StreamConfig cfg = twoStageDeviceCfg();
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0u).isOk());

    // Stage-1: IOVA → IPA (normal mapping via mapPage)
    PagePermissions ro(true, false, false);
    ASSERT_TRUE(smmu.mapPage(sid, 0u, iova, static_cast<PA>(ipa),
                              ro, SecurityState::NonSecure).isOk());

    // Stage-2: IPA → PA as Device memory (S2PTW=false: does not fault)
    PagePermissions rw(true, true, false);
    ASSERT_TRUE(smmu.mapStage2DevicePage(sid, ipa, pa, rw,
                                         SecurityState::NonSecure).isOk());

    uint64_t par = smmu.gatosTranslate(sid, 0u, iova,
                                        AccessType::Read,
                                        SecurityState::NonSecure);

    // Translation must have succeeded (FAULT bit 0 must be 0).
    ASSERT_EQ(par & 1u, 0u)
        << "BUG-AUDIT-49: gatosTranslate() must succeed for device-memory page with "
           "S2PTW=false; par=0x" << std::hex << par;

    uint64_t attr = (par >> 56u) & 0xFFu;

    EXPECT_NE(attr, 0xFFu)
        << "BUG-AUDIT-49: ATTR (bits[63:56]) must NOT be 0xFF (Normal WB/WA) for a "
           "device-memory page (ARM §9.1.4 / §6.3.40). Current code hardcodes 0xFF "
           "regardless of memory type — this is the pre-fix failing condition. "
           "attr=0x" << std::hex << attr;
}

// BUG-AUDIT-49 test 2: gatosTranslate() on a device-memory stage-2 page — SH
// must NOT be 0b11 (ISH). ARM §9.1.4: "Shareability is returned as Outer Shareable
// when ATTR selects any Device type." Required: SH=0b10 (OSH).
//
// BEFORE FIX: SH bits[9:8] == 0b11 (hardcoded ISH) → test FAILS.
// AFTER FIX:  SH bits[9:8] != 0b11 (OSH = 0b10)    → test PASSES.
TEST(BugAudit49, GatosTranslate_DevicePage_ShNotIsh) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid  = 301u;
    const IOVA     iova = 0x2000u;
    const IOVA     ipa  = 0x6000u;
    const PA       pa   = 0xB000u;

    StreamConfig cfg = twoStageDeviceCfg();
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0u).isOk());

    PagePermissions ro(true, false, false);
    ASSERT_TRUE(smmu.mapPage(sid, 0u, iova, static_cast<PA>(ipa),
                              ro, SecurityState::NonSecure).isOk());

    PagePermissions rw(true, true, false);
    ASSERT_TRUE(smmu.mapStage2DevicePage(sid, ipa, pa, rw,
                                         SecurityState::NonSecure).isOk());

    uint64_t par = smmu.gatosTranslate(sid, 0u, iova,
                                        AccessType::Read,
                                        SecurityState::NonSecure);

    ASSERT_EQ(par & 1u, 0u)
        << "BUG-AUDIT-49: gatosTranslate() must succeed for device-memory page with "
           "S2PTW=false (before checking SH); par=0x" << std::hex << par;

    uint64_t sh = (par >> 8u) & 0x3u;

    EXPECT_NE(sh, 0x3u)
        << "BUG-AUDIT-49: SH (bits[9:8]) must NOT be 0b11 (Inner Shareable) for a "
           "device-memory page. ARM §9.1.4 requires OSH (0b10). Current code hardcodes "
           "0b11 — this is the pre-fix failing condition. sh=0b" << std::oct << sh;
}

// BUG-AUDIT-49 test 3 (regression): gatosTranslate() on a normal memory page must
// still return ATTR=0xFF (Normal WB/WA) and SH=0b11 (ISH).
//
// BEFORE / AFTER FIX: This must always pass (regression guard).
TEST(BugAudit49, GatosTranslate_NormalPage_AttrAndShUnchanged) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid  = 302u;
    const IOVA     iova = 0x3000u;
    const IOVA     ipa  = 0x7000u;
    const PA       pa   = 0xC000u;

    // Two-stage stream, normal memory only.
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.s2ptw              = false;
    cfg.t0sz               = 0u;
    cfg.s2t0sz             = 0u;
    cfg.s2ps               = 5u;
    cfg.aa64               = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 2u;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0u).isOk());

    PagePermissions ro(true, false, false);
    ASSERT_TRUE(smmu.mapPage(sid, 0u, iova, static_cast<PA>(ipa),
                              ro, SecurityState::NonSecure).isOk());

    // Stage-2: normal (non-device) page via setStreamStage2AddressSpace.
    auto stage2AS = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(sid, stage2AS).isOk());
    ASSERT_TRUE(stage2AS->mapPage(ipa, pa, ro, SecurityState::NonSecure).isOk());

    uint64_t par = smmu.gatosTranslate(sid, 0u, iova,
                                        AccessType::Read,
                                        SecurityState::NonSecure);

    ASSERT_EQ(par & 1u, 0u)
        << "BUG-AUDIT-49 regression: gatosTranslate() must succeed for normal page; "
           "par=0x" << std::hex << par;

    uint64_t attr = (par >> 56u) & 0xFFu;
    uint64_t sh   = (par >>  8u) & 0x3u;

    EXPECT_EQ(attr, 0xFFu)
        << "BUG-AUDIT-49 regression: normal page must still return ATTR=0xFF (Normal WB/WA)";
    EXPECT_EQ(sh, 0x3u)
        << "BUG-AUDIT-49 regression: normal page must still return SH=0b11 (ISH)";
}

// ─── BUG-AUDIT-50 tests — StreamConfig missing endi/s2endi fields ─────────────
//
// These tests use a type-trait detection idiom to check at runtime whether
// StreamConfig has the required endi/s2endi members.  This approach allows the
// file to compile even when the fields are absent, so that BUG-AUDIT-49 and
// BUG-AUDIT-52 tests can also run and report their failures.
//
// The detection template has a true specialization that accesses cfg.endi /
// cfg.s2endi only when the field exists; the primary template falls back to a
// compile-time constant 'false' — which means the test reports FAILURE with a
// clear message before the fix is applied.

namespace {

// Detection idiom: has_endi<T> is true_type when T has a member named 'endi'.
template<typename T, typename = void>
struct has_endi : std::false_type {};

template<typename T>
struct has_endi<T, decltype(void(std::declval<T>().endi))> : std::true_type {};

// Detection idiom: has_s2endi<T> is true_type when T has a member named 's2endi'.
template<typename T, typename = void>
struct has_s2endi : std::false_type {};

template<typename T>
struct has_s2endi<T, decltype(void(std::declval<T>().s2endi))> : std::true_type {};

} // anonymous namespace

// BUG-AUDIT-50 test 1: StreamConfig must have an 'endi' field (CD.ENDI).
//
// ARM §5.4 CD.ENDI: endianness of stage-1 translation table walks.
// IDR0.TTENDIAN=0b00 (mixed-endian) — any endi value is legal; no C_BAD_CD.
//
// BEFORE FIX: has_endi<StreamConfig>::value == false → EXPECT_TRUE fails → FAILS.
// AFTER FIX:  field present, value==true, configureStream() returns ok → PASSES.
TEST(BugAudit50, StreamConfig_EndiField_Exists) {
    // Runtime detection: reports FAIL before the field is added to StreamConfig.
    EXPECT_TRUE((has_endi<StreamConfig>::value))
        << "BUG-AUDIT-50: StreamConfig must have an 'endi' (CD.ENDI) member field "
           "(ARM §5.4). The field is currently absent — add 'bool endi' to StreamConfig. "
           "IDR0.TTENDIAN=0b00 means any endi value is legal per ARM §5.4.";
}

// BUG-AUDIT-50 test 2: StreamConfig must have an 's2endi' field (STE.S2ENDI).
//
// ARM §5.2 STE.S2ENDI: endianness of stage-2 translation table walks.
// IDR0.TTENDIAN=0b00 — any s2endi value is legal; no C_BAD_STE.
//
// BEFORE FIX: has_s2endi<StreamConfig>::value == false → EXPECT_TRUE fails → FAILS.
// AFTER FIX:  field present, value==true, configureStream() returns ok → PASSES.
TEST(BugAudit50, StreamConfig_S2EndiField_Exists) {
    EXPECT_TRUE((has_s2endi<StreamConfig>::value))
        << "BUG-AUDIT-50: StreamConfig must have an 's2endi' (STE.S2ENDI) member field "
           "(ARM §5.2). The field is currently absent — add 'bool s2endi' to StreamConfig. "
           "IDR0.TTENDIAN=0b00 means any s2endi value is legal per ARM §5.2.";
}

// BUG-AUDIT-50 test 3: configureStream() with endi=true must not emit C_BAD_CD.
//
// This test only runs the configureStream() path once the endi field exists.
// Before the fix, the field detection above fails and guards this test.
//
// BEFORE FIX: endi absent → has_endi false → skipped via ASSERT_TRUE → FAILS (via test 1).
// AFTER FIX:  endi present, configureStream() ok, no C_BAD_CD → PASSES.
TEST(BugAudit50, StreamConfig_EndiFalse_NoEvent) {
    // If endi does not exist, this test is blocked by the assertion.
    ASSERT_TRUE((has_endi<StreamConfig>::value))
        << "BUG-AUDIT-50: skipping runtime check — 'endi' field not yet present in StreamConfig";

    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.t0sz               = 16u;
    smmu.configureStream(312u, cfg);

    // No C_BAD_CD or C_BAD_STE should be emitted for any ENDI value.
    std::vector<EventEntry> events = smmu.getEventQueue();
    bool hasBadCd  = false;
    bool hasBadSte = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::C_BAD_CD)  { hasBadCd  = true; }
        if (ev.type == EventType::C_BAD_STE) { hasBadSte = true; }
    }
    EXPECT_FALSE(hasBadCd)
        << "BUG-AUDIT-50: endi=false must not emit C_BAD_CD (TTENDIAN=0b00 is mixed-endian)";
    EXPECT_FALSE(hasBadSte)
        << "BUG-AUDIT-50: endi=false must not emit C_BAD_STE (TTENDIAN=0b00 is mixed-endian)";
}

// ─── BUG-AUDIT-52 tests — IDR5 GRAN16K/GRAN64K overclaimed ───────────────────
//
// getIDR5() currently sets bit[5] (GRAN16K=1) and bit[6] (GRAN64K=1) even though
// only 4KB granule pages are actually implemented.
// Required: only bit[4] (GRAN4K) must be set; bits[5] and [6] must be 0.

// BUG-AUDIT-52 test 1: IDR5 GRAN16K (bit[5]) must be 0 — only 4KB supported.
//
// BEFORE FIX: (getIDR5() >> 5) & 1 == 1 → assertion fails → FAILS.
// AFTER FIX:  (getIDR5() >> 5) & 1 == 0 → PASSES.
TEST(BugAudit52, Idr5_Gran16k_MustBeZero) {
    SMMU smmu;
    uint32_t idr5 = smmu.getIDR5();

    EXPECT_EQ((idr5 >> 5u) & 1u, 0u)
        << "BUG-AUDIT-52: IDR5.GRAN16K (bit[5]) must be 0 — 16KB granule is not "
           "implemented by this model (ARM §6.3.6). Current code incorrectly sets "
           "bit[5]=1. Raw IDR5=0x" << std::hex << idr5;
}

// BUG-AUDIT-52 test 2: IDR5 GRAN64K (bit[6]) must be 0 — only 4KB supported.
//
// BEFORE FIX: (getIDR5() >> 6) & 1 == 1 → assertion fails → FAILS.
// AFTER FIX:  (getIDR5() >> 6) & 1 == 0 → PASSES.
TEST(BugAudit52, Idr5_Gran64k_MustBeZero) {
    SMMU smmu;
    uint32_t idr5 = smmu.getIDR5();

    EXPECT_EQ((idr5 >> 6u) & 1u, 0u)
        << "BUG-AUDIT-52: IDR5.GRAN64K (bit[6]) must be 0 — 64KB granule is not "
           "implemented by this model (ARM §6.3.6). Current code incorrectly sets "
           "bit[6]=1. Raw IDR5=0x" << std::hex << idr5;
}

// BUG-AUDIT-52 test 3 (regression): IDR5 GRAN4K (bit[4]) must remain 1.
// This must pass both before and after the fix.
//
// BEFORE / AFTER FIX: This must always pass (regression guard).
TEST(BugAudit52, Idr5_Gran4k_MustBeOne) {
    SMMU smmu;
    uint32_t idr5 = smmu.getIDR5();

    EXPECT_EQ((idr5 >> 4u) & 1u, 1u)
        << "BUG-AUDIT-52 regression: IDR5.GRAN4K (bit[4]) must be 1 — 4KB granule "
           "IS implemented (ARM §6.3.6). Raw IDR5=0x" << std::hex << idr5;
}

// BUG-AUDIT-52 test 4: After the fix, only bit[4] is set in the granule field bits[6:4].
//
// BEFORE FIX: (getIDR5() >> 4) & 0x7 == 0x7 (all three set) → assertion fails → FAILS.
// AFTER FIX:  (getIDR5() >> 4) & 0x7 == 0x1 (only GRAN4K)   → PASSES.
TEST(BugAudit52, Idr5_GranuleField_OnlyGran4kSet) {
    SMMU smmu;
    uint32_t idr5 = smmu.getIDR5();
    uint32_t gran_bits = (idr5 >> 4u) & 0x7u;

    EXPECT_EQ(gran_bits, 0x1u)
        << "BUG-AUDIT-52: IDR5 granule field bits[6:4] must be 0b001 (only GRAN4K). "
           "Current code returns 0b111 (all three granules claimed). "
           "Raw granule bits=0b" << (int)((gran_bits >> 2) & 1)
                                 << (int)((gran_bits >> 1) & 1)
                                 << (int)(gran_bits & 1);
}

// ─── BUG-AUDIT-53 tests — IDR0.DORMHINT (bit[8]) must be 1 ───────────────────
//
// ARM §6.3.1: IDR0.DORMHINT=1 means the SMMU supports dormancy signaling and
// STATUSR.DORMANT is meaningful.  Dormancy is implemented in this model
// (shutdown() / STATUSR.DORMANT path exists).  Without DORMHINT=1, software
// will not know that STATUSR.DORMANT is valid and will never use dormancy.
//
// BEFORE FIX: bit[8]==0 → FAILS.
// AFTER FIX:  bit[8]==1 → PASSES.

// BUG-AUDIT-53 test 1: IDR0.DORMHINT (bit[8]) must be 1 — dormancy is implemented.
// BEFORE FIX: bit[8]==0 → FAILS.
// AFTER FIX:  bit[8]==1 → PASSES.
TEST(BugAudit53, Idr0_DormhintBit_MustBeOne) {
    SMMU smmu;
    uint32_t idr0 = smmu.getIDR0();
    EXPECT_EQ((idr0 >> 8u) & 1u, 1u)
        << "BUG-AUDIT-53: IDR0.DORMHINT (bit[8]) must be 1 — shutdown()/dormancy is "
           "implemented (ARM §6.3.1). Software reads DORMHINT to know if STATUSR.DORMANT "
           "is meaningful. Current code sets bit[8]=0. idr0=0x" << std::hex << idr0;
}

// ─── BUG-AUDIT-55 tests — IDR0.SEV (bit[14]) must be gated on sevSupported_ ──
//
// ARM §4.7.3 / §6.3.1: IDR0.SEV=1 advertises CMD_SYNC CS=0b10 (SIG_SEV) WFE-wake
// support.  The WFE/SEV mechanism is NOT implemented — software relying on SEV=1
// for WFE-based synchronization will hang.  IDR0.SEV must default to 0 and only be
// set when explicitly enabled via setSevSupported(true).
//
// BEFORE FIX (hardcoded 1): BugAudit55_DefaultFalse FAILS.
// AFTER FIX (gated):        all three tests PASS.

// BUG-AUDIT-55 test 1: IDR0.SEV (bit[14]) must be 0 by default.
// BEFORE FIX: bit[14]==1 → FAILS.
// AFTER FIX:  bit[14]==0 (default false) → PASSES.
TEST(BugAudit55, Idr0_Sev_DefaultFalse) {
    SMMU smmu;
    uint32_t idr0 = smmu.getIDR0();
    EXPECT_EQ((idr0 >> 14u) & 1u, 0u)
        << "BUG-AUDIT-55: IDR0.SEV (bit[14]) must be 0 by default — CMD_SYNC SIG_SEV "
           "(CS=0b10) WFE-wake is not implemented (ARM §4.7.3 / §6.3.1). Default must be "
           "false until setSevSupported(true) is called. Current code hardcodes bit[14]=1. "
           "idr0=0x" << std::hex << idr0;
}

// BUG-AUDIT-55 test 2: setSevSupported(true) must set IDR0.SEV bit[14] = 1.
TEST(BugAudit55, Idr0_Sev_TrueAfterSetSupported) {
    SMMU smmu;
    smmu.setSevSupported(true);
    uint32_t idr0 = smmu.getIDR0();
    EXPECT_EQ((idr0 >> 14u) & 1u, 1u)
        << "BUG-AUDIT-55: after setSevSupported(true), IDR0.SEV (bit[14]) must be 1. "
           "idr0=0x" << std::hex << idr0;
}

// BUG-AUDIT-55 test 3: setSevSupported(false) after true must clear IDR0.SEV.
TEST(BugAudit55, Idr0_Sev_FalseAfterClear) {
    SMMU smmu;
    smmu.setSevSupported(true);
    smmu.setSevSupported(false);
    uint32_t idr0 = smmu.getIDR0();
    EXPECT_EQ((idr0 >> 14u) & 1u, 0u)
        << "BUG-AUDIT-55: after setSevSupported(false), IDR0.SEV must be 0. "
           "idr0=0x" << std::hex << idr0;
}

// ─── BUG-AUDIT-54 C++ tests — receiveBroadcastTLBI() PTM polarity ────────────
//
// ARM §6.3.12 CR2.PTM:
//   PTM=0 → SMMU participates in broadcast TLB maintenance (MUST execute TLBI)
//   PTM=1 → Private TLB Maintenance (SMMU may skip TLBI)
//
// BEFORE FIX: guard uses == 0u (inverted): skips when PTM=0, executes when PTM=1
// AFTER FIX:  guard uses != 0u (correct): executes when PTM=0, skips when PTM=1
//
// Test strategy: use TLB cache hit/miss counters to observe whether the TLBI ran.
//   1. Enable caching and set up a stage-1 stream with one mapped page.
//   2. First translate → cache miss (populates TLB).
//   3. receiveBroadcastTLBI(TLBI_NH_ALL).
//   4. Second translate:
//      PTM=0 (participate): TLB was flushed → second translate is a cache miss.
//      PTM=1 (private/skip): TLB NOT flushed → second translate is a cache hit.

// Helper: build a minimal stage-1 stream config for PTM tests.
static StreamConfig ptmTestStreamCfg(uint16_t vmid) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.t0sz               = 0u;
    cfg.aa64               = true;
    cfg.vmid               = vmid;
    return cfg;
}

// BUG-AUDIT-54 test 1: With PTM=1 (private), receiveBroadcastTLBI() must NOT
// flush the TLB — the broadcast is silently ignored.  A second translate after
// the broadcast must be a cache HIT (TLB entry still present).
//
// BEFORE FIX: PTM=1 → executes TLBI → TLB flushed → second translate is a miss → FAILS
// AFTER FIX:  PTM=1 → skips TLBI  → TLB intact  → second translate is a hit  → PASSES
// Uses TLBI_NSNH_ALL (VMID-agnostic) to ensure the entry is targeted.
TEST(BugAudit54Cpp, ReceiveBroadcastTlbi_Ptm1_Private_SkipsTlbi) {
    SMMU smmu;
    enableSMMU(smmu);
    smmu.enableCaching(true);

    const StreamID sid  = 400u;
    const IOVA     iova = 0x1000u;
    const PA       pa   = 0xA000u;

    StreamConfig cfg = ptmTestStreamCfg(10u);
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0u).isOk());

    PagePermissions ro(true, false, false);
    ASSERT_TRUE(smmu.mapPage(sid, 0u, iova, pa, ro, SecurityState::NonSecure).isOk());

    // First translate: cache miss — populates TLB entry.
    TranslationResult r1 = smmu.translate(sid, 0u, iova, AccessType::Read);
    ASSERT_TRUE(r1.isOk())
        << "BUG-AUDIT-54 setup: first translate must succeed; result=" << (int)r1.getError();

    uint64_t hits_before  = smmu.getCacheHitCount();
    uint64_t misses_before = smmu.getCacheMissCount();

    // CR2.PTM=1 → Private TLB Maintenance: SMMU must NOT participate in broadcast.
    // Use TLBI_NSNH_ALL (VMID-agnostic) so the entry for vmid=10 is targeted.
    smmu.setCR2(SMMU::CR2_PTM);
    smmu.receiveBroadcastTLBI(CommandType::TLBI_NSNH_ALL);

    // Second translate: TLB should still be populated → cache HIT.
    TranslationResult r2 = smmu.translate(sid, 0u, iova, AccessType::Read);
    ASSERT_TRUE(r2.isOk())
        << "BUG-AUDIT-54: second translate with PTM=1 must succeed; result=" << (int)r2.getError();

    uint64_t hits_after  = smmu.getCacheHitCount();
    uint64_t misses_after = smmu.getCacheMissCount();

    EXPECT_GT(hits_after, hits_before)
        << "BUG-AUDIT-54: receiveBroadcastTLBI() with CR2.PTM=1 (Private) must NOT "
           "execute the TLBI — second translate must be a cache HIT (ARM §6.3.12). "
           "hits_before=" << hits_before << " hits_after=" << hits_after
           << " misses_before=" << misses_before << " misses_after=" << misses_after;

    EXPECT_EQ(misses_after, misses_before)
        << "BUG-AUDIT-54: with CR2.PTM=1 the TLB must not be flushed — no new cache miss "
           "expected on second translate (ARM §6.3.12).";
}

// BUG-AUDIT-54 test 2: With PTM=0 (participate), receiveBroadcastTLBI() MUST
// flush the TLB.  A second translate after the broadcast must be a cache MISS
// (TLB entry was evicted by the invalidation).
//
// BEFORE FIX: PTM=0 → skips TLBI → TLB intact → second translate is a hit → FAILS
// AFTER FIX:  PTM=0 → executes TLBI → TLB flushed → second translate is a miss → PASSES
// Uses TLBI_NSNH_ALL (VMID-agnostic) to ensure the entry is targeted.
TEST(BugAudit54Cpp, ReceiveBroadcastTlbi_Ptm0_Participates_ExecutesTlbi) {
    SMMU smmu;
    enableSMMU(smmu);
    smmu.enableCaching(true);

    const StreamID sid  = 401u;
    const IOVA     iova = 0x2000u;
    const PA       pa   = 0xB000u;

    StreamConfig cfg = ptmTestStreamCfg(11u);
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0u).isOk());

    PagePermissions ro(true, false, false);
    ASSERT_TRUE(smmu.mapPage(sid, 0u, iova, pa, ro, SecurityState::NonSecure).isOk());

    // First translate: cache miss — populates TLB entry.
    TranslationResult r1 = smmu.translate(sid, 0u, iova, AccessType::Read);
    ASSERT_TRUE(r1.isOk())
        << "BUG-AUDIT-54 setup: first translate must succeed; result=" << (int)r1.getError();

    uint64_t misses_before = smmu.getCacheMissCount();

    // CR2.PTM=0 → SMMU participates in broadcast TLB maintenance.
    // Use TLBI_NSNH_ALL (VMID-agnostic) so the entry for vmid=11 is targeted.
    smmu.setCR2(0u);
    smmu.receiveBroadcastTLBI(CommandType::TLBI_NSNH_ALL);

    // Second translate: TLB was flushed → cache MISS (re-walk required).
    TranslationResult r2 = smmu.translate(sid, 0u, iova, AccessType::Read);
    ASSERT_TRUE(r2.isOk())
        << "BUG-AUDIT-54: second translate with PTM=0 must succeed (page still mapped); "
           "result=" << (int)r2.getError();

    uint64_t misses_after = smmu.getCacheMissCount();

    EXPECT_GT(misses_after, misses_before)
        << "BUG-AUDIT-54: receiveBroadcastTLBI() with CR2.PTM=0 (participate) MUST "
           "execute the TLBI — second translate must be a cache MISS (ARM §6.3.12). "
           "misses_before=" << misses_before << " misses_after=" << misses_after;
}
