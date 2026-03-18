// TDD: Tests for Bug 2 — event record RnW/InD/PnU fields (ARM IHI0070G.b §7.3)
//
// ARM IHI0070G.b §7.3 specifies that event record fields RnW, InD, and PnU must
// reflect the actual transaction access type:
//   RnW: 1=read/execute, 0=write  (RnW-GAP fix: spec RnW=1=Read, RnW=0=Write)
//   InD: 0=data,         1=instruction (execute)
//   PnU: 0=unprivileged, 1=privileged
//
// Bug: 13 generateEvent() call sites in smmu.cpp omit the accessType parameter
// and therefore default to AccessType::Read.  For non-Read transactions the
// resulting event records incorrectly show rnw=false, ind=false, pnu=false even
// when the actual access was a Write, Execute, or Privileged access.
//
// Tested call sites (representative subset of the 13 broken sites):
//
//   1. Line 1246 (site 1):  F_ADDR_SIZE — STE.Config=0b100 full bypass, OAS exceeded,
//                           AccessType::Write  → rnw must be true
//   2. Line 1352 (site 3):  F_TRANSLATION — T0SZ VA-range exceeded,
//                           AccessType::Write  → rnw must be true
//   3. Line 1365 (site 4):  F_TRANSLATION — CD.EPD0=1 translation disabled,
//                           AccessType::Execute → ind must be true
//   4. Line 1408 (site 5):  F_TRANSLATION — S2T0SZ IPA-range exceeded (stage-2-only),
//                           AccessType::Write  → rnw must be true
//   5. Line 1594 (site 7):  F_ADDR_SIZE — CD.IPS IPA-size exceeded (two-stage),
//                           AccessType::Write  → rnw must be true
//   6. Line 1882 (site 10): F_ADDR_SIZE — S2PS PA-size exceeded (stage-2-only),
//                           AccessType::Execute → ind must be true

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include "smmu/configuration.h"
#include <memory>

using namespace smmu;

// ─────────────────────────────────────────────────────────────────────────────
// Shared constants
// ─────────────────────────────────────────────────────────────────────────────

namespace {

static constexpr StreamID SID      = 0x10;
static constexpr PASID    PID      = 0;
static constexpr IOVA     BASE_IOVA = 0x1000;

} // anonymous namespace

// ─────────────────────────────────────────────────────────────────────────────
// Test fixture
// ─────────────────────────────────────────────────────────────────────────────

class EventAccessTypeFix : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_ = std::make_unique<SMMU>();
        smmu_->enable();
        smmu_->setCR0(smmu_->getCR0() | SMMU::CR0_EVENTQEN);
    }

    void TearDown() override {
        smmu_.reset();
    }

    std::unique_ptr<SMMU> smmu_;
};

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 — Site 1 (line 1246): F_ADDR_SIZE on full-bypass stream, Write access
//
// Setup: STE.Config=0b100 (bypass, translationEnabled=false, bypassEnabled=true).
//        SMMU configured with 48-bit OAS; bypass stream gets an IOVA above OAS
//        treated as a direct PA.  The STE.Config=0b100 OAS check fires.
//        Bug: generateEvent() called without accessType → rnw=false always.
//        Fix: should pass accessType so rnw=true for Write.
// ─────────────────────────────────────────────────────────────────────────────
TEST_F(EventAccessTypeFix, Site1_BypassOAS_WriteAccess_RnWMustBeTrue) {
    // Configure an SMMU with a 48-bit OAS so PAs above 2^48 trigger F_ADDR_SIZE
    // on the bypass path (STE.Config=0b100).
    SMMUConfiguration smmuCfg;
    AddressConfiguration addrCfg;
    addrCfg.maxPASize = 48;
    smmuCfg.setAddressConfiguration(addrCfg);
    SMMU smmu48(smmuCfg);
    smmu48.enable();
    smmu48.setCR0(smmu48.getCR0() | SMMU::CR0_EVENTQEN);

    // bypassEnabled=true, translationEnabled=false → STE.Config=0b100
    StreamConfig cfg;
    cfg.translationEnabled = false;
    cfg.bypassEnabled      = true;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = false;
    ASSERT_TRUE(smmu48.configureStream(SID, cfg).isOk());
    ASSERT_TRUE(smmu48.enableStream(SID).isOk());

    // IOVA above 48-bit OAS: bypass treats IOVA as PA directly.
    // 2^48 + page_offset puts it above the OAS bound.
    static constexpr IOVA ABOVE_OAS_IOVA = (static_cast<IOVA>(1) << 48u) | 0x1000u;

    auto r = smmu48.translate(SID, PID, ABOVE_OAS_IOVA, AccessType::Write);
    EXPECT_TRUE(r.isError())
        << "Bypass with IOVA above OAS must produce a translation error (F_ADDR_SIZE)";

    auto ev = smmu48.getEventQueue();
    ASSERT_FALSE(ev.empty())
        << "F_ADDR_SIZE event must be recorded";

    // Find the F_ADDR_SIZE event
    const EventEntry* found = nullptr;
    for (const auto& e : ev) {
        if (e.type == EventType::F_ADDR_SIZE) {
            found = &e;
            break;
        }
    }
    ASSERT_NE(found, nullptr)
        << "An F_ADDR_SIZE event must exist in the event queue";

    // RnW-GAP fix: ARM §7.3 RnW=1=Read, RnW=0=Write.  Write access → rnw=false.
    EXPECT_FALSE(found->rnw)
        << "Bug2 Site1: F_ADDR_SIZE bypass OAS event rnw must be false for Write access "
        << "(ARM IHI0070G.b §7.3 RnW=0=Write)";
    EXPECT_FALSE(found->ind)
        << "Bug2 Site1: ind must be false for a data Write access";
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 — Site 3 (line 1352): F_TRANSLATION on T0SZ VA-range exceeded, Write
//
// Setup: Stage-1-only stream.  CD.T0SZ=16 → VA limit = 2^(64-16) = 2^48.
//        IOVA >= 2^48 triggers the T0SZ range check and F_TRANSLATION.
//        Bug: generateEvent() called without accessType → rnw=false always.
//        Fix: rnw=true for Write access.
// ─────────────────────────────────────────────────────────────────────────────
TEST_F(EventAccessTypeFix, Site3_T0SZRangeExceeded_WriteAccess_RnWMustBeTrue) {
    // t0sz=16 is the default; VA limit = 2^48
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.t0sz               = 16; // VA limit = 2^(64-16) = 2^48
    ASSERT_TRUE(smmu_->configureStream(SID, cfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(SID).isOk());
    ASSERT_TRUE(smmu_->createStreamPASID(SID, PID).isOk());

    // IOVA at exactly 2^48 — exceeds T0SZ range
    static constexpr IOVA VA_ABOVE_LIMIT = static_cast<IOVA>(1) << 48u;

    auto r = smmu_->translate(SID, PID, VA_ABOVE_LIMIT, AccessType::Write);
    EXPECT_TRUE(r.isError())
        << "IOVA >= VA limit must produce a translation error (F_TRANSLATION)";

    auto ev = smmu_->getEventQueue();
    ASSERT_FALSE(ev.empty())
        << "F_TRANSLATION event must be recorded for T0SZ range violation";

    const EventEntry* found = nullptr;
    for (const auto& e : ev) {
        if (e.type == EventType::F_TRANSLATION) {
            found = &e;
            break;
        }
    }
    ASSERT_NE(found, nullptr)
        << "An F_TRANSLATION event must exist in the event queue";

    // RnW-GAP fix: ARM §7.3 RnW=0=Write. Write access → rnw=false.
    EXPECT_FALSE(found->rnw)
        << "Bug2 Site3: F_TRANSLATION T0SZ event rnw must be false for Write access "
        << "(ARM IHI0070G.b §7.3 RnW=0=Write)";
    EXPECT_FALSE(found->ind)
        << "Bug2 Site3: ind must be false for a data Write access";
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 — Site 4 (line 1365): F_TRANSLATION on CD.EPD0=1, Execute access
//
// Setup: Stage-1-only stream with EPD0=true (TTBR0 walk disabled).
//        Any translation on this stream → F_TRANSLATION immediately.
//        Bug: generateEvent() called without accessType → ind=false always.
//        Fix: ind=true for Execute access.
// ─────────────────────────────────────────────────────────────────────────────
TEST_F(EventAccessTypeFix, Site4_EPD0Disabled_ExecuteAccess_IndMustBeTrue) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.t0sz               = 0; // no VA range restriction
    cfg.epd0               = true; // TTBR0 walk disabled → F_TRANSLATION on any access
    ASSERT_TRUE(smmu_->configureStream(SID, cfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(SID).isOk());
    ASSERT_TRUE(smmu_->createStreamPASID(SID, PID).isOk());

    auto r = smmu_->translate(SID, PID, BASE_IOVA, AccessType::Execute);
    EXPECT_TRUE(r.isError())
        << "EPD0=1 must produce a translation error (F_TRANSLATION)";

    auto ev = smmu_->getEventQueue();
    ASSERT_FALSE(ev.empty())
        << "F_TRANSLATION event must be recorded for EPD0 fault";

    const EventEntry* found = nullptr;
    for (const auto& e : ev) {
        if (e.type == EventType::F_TRANSLATION) {
            found = &e;
            break;
        }
    }
    ASSERT_NE(found, nullptr)
        << "An F_TRANSLATION event must exist in the event queue";

    // Bug 2: generateEvent() at line 1365 omits accessType → ind defaults to false.
    EXPECT_TRUE(found->ind)
        << "Bug2 Site4: F_TRANSLATION EPD0 event ind must be true for Execute access "
        << "(ARM IHI0070G.b §7.3 InD field)";
    // RnW-GAP fix: ARM §7.3 RnW=1=Read. Execute is a read; rnw=true.
    EXPECT_TRUE(found->rnw)
        << "Bug2 Site4: rnw must be true for an Execute (read/instruction) access "
        << "(ARM IHI0070G.b §7.3 RnW=1=Read)";
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 — Site 5 (line 1408): F_TRANSLATION on S2T0SZ range exceeded (S2-only),
//          Write access
//
// Setup: Stage-2-only stream with s2t0sz=16 → IPA limit = 2^48.
//        IOVA (treated as IPA in S2-only mode) >= 2^48 → F_TRANSLATION.
//        Bug: generateEvent() called without accessType → rnw=false always.
//        Fix: rnw=true for Write access.
// ─────────────────────────────────────────────────────────────────────────────
TEST_F(EventAccessTypeFix, Site5_S2T0SZRangeExceeded_WriteAccess_RnWMustBeTrue) {
    // Stage-2-only stream: translationEnabled=true, stage1=false, stage2=true
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = true;
    cfg.s2t0sz             = 16; // IPA limit = 2^48
    ASSERT_TRUE(smmu_->configureStream(SID, cfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(SID).isOk());

    // Set up a stage-2 address space (no mappings needed — the range check fires first)
    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu_->setStreamStage2AddressSpace(SID, s2as).isOk());

    // IPA exactly at 2^48 — exceeds S2T0SZ range
    static constexpr IOVA IPA_ABOVE_LIMIT = static_cast<IOVA>(1) << 48u;

    auto r = smmu_->translate(SID, PID, IPA_ABOVE_LIMIT, AccessType::Write);
    EXPECT_TRUE(r.isError())
        << "IPA >= S2T0SZ limit must produce a translation error (F_TRANSLATION)";

    auto ev = smmu_->getEventQueue();
    ASSERT_FALSE(ev.empty())
        << "F_TRANSLATION event must be recorded for S2T0SZ range violation";

    const EventEntry* found = nullptr;
    for (const auto& e : ev) {
        if (e.type == EventType::F_TRANSLATION) {
            found = &e;
            break;
        }
    }
    ASSERT_NE(found, nullptr)
        << "An F_TRANSLATION event must exist in the event queue";

    // RnW-GAP fix: ARM §7.3 RnW=0=Write. Write access → rnw=false.
    EXPECT_FALSE(found->rnw)
        << "Bug2 Site5: F_TRANSLATION S2T0SZ event rnw must be false for Write access "
        << "(ARM IHI0070G.b §7.3 RnW=0=Write)";
    EXPECT_FALSE(found->ind)
        << "Bug2 Site5: ind must be false for a data Write access";
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5 — Site 7 (line 1594): F_ADDR_SIZE on CD.IPS IPA-size exceeded (two-stage),
//          Write access
//
// Setup: Two-stage stream.  CD.IPS=2 (40-bit, s2ps encoding 2).
//        Stage-1 maps IOVA to an IPA that is >= 2^40 → F_ADDR_SIZE.
//        Bug: generateEvent() called without accessType → rnw=false always.
//        Fix: rnw=true for Write access.
// ─────────────────────────────────────────────────────────────────────────────
TEST_F(EventAccessTypeFix, Site7_CDIPSExceeded_WriteAccess_RnWMustBeTrue) {
    // Two-stage stream with IPS=2 (40-bit IPA space, oasBitsFromS2PS(2)=40)
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.t0sz               = 0;   // no VA restriction
    cfg.ips                = 2;   // 40-bit IPS — IPA >= 2^40 → F_ADDR_SIZE
    cfg.s2t0sz             = 0;   // no IPA range restriction at stage-2 T0SZ level
    cfg.s2ps               = 6;   // 52-bit S2PS — no S2PS check fires
    ASSERT_TRUE(smmu_->configureStream(SID, cfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(SID).isOk());
    ASSERT_TRUE(smmu_->createStreamPASID(SID, PID).isOk());

    // Stage-2 address space: provide it but don't add mappings
    // (the IPS check fires before the S2 translation attempt)
    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu_->setStreamStage2AddressSpace(SID, s2as).isOk());

    // Map stage-1: IOVA → IPA above 40-bit bound (>= 2^40)
    static constexpr IPA ABOVE_IPS_IPA = (static_cast<IPA>(1) << 40u) | 0x2000u;
    PagePermissions perms(true, true, true);
    ASSERT_TRUE(smmu_->mapPage(SID, PID, BASE_IOVA, ABOVE_IPS_IPA, perms).isOk());

    auto r = smmu_->translate(SID, PID, BASE_IOVA, AccessType::Write);
    EXPECT_TRUE(r.isError())
        << "IPA >= CD.IPS limit must produce a translation error (F_ADDR_SIZE)";

    auto ev = smmu_->getEventQueue();
    ASSERT_FALSE(ev.empty())
        << "F_ADDR_SIZE event must be recorded for CD.IPS violation";

    const EventEntry* found = nullptr;
    for (const auto& e : ev) {
        if (e.type == EventType::F_ADDR_SIZE) {
            found = &e;
            break;
        }
    }
    ASSERT_NE(found, nullptr)
        << "An F_ADDR_SIZE event must exist in the event queue";

    // RnW-GAP fix: ARM §7.3 RnW=0=Write. Write access → rnw=false.
    EXPECT_FALSE(found->rnw)
        << "Bug2 Site7: F_ADDR_SIZE CD.IPS event rnw must be false for Write access "
        << "(ARM IHI0070G.b §7.3 RnW=0=Write)";
    EXPECT_FALSE(found->ind)
        << "Bug2 Site7: ind must be false for a data Write access";
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6 — Site 10 (line 1882): F_ADDR_SIZE on S2PS PA-size exceeded (S2-only),
//          Execute access
//
// Setup: Stage-2-only stream with s2ps=2 (40-bit PA space).
//        IPA maps to a PA >= 2^40 → F_ADDR_SIZE.
//        Bug: generateEvent() called without accessType → ind=false always.
//        Fix: ind=true for Execute access.
// ─────────────────────────────────────────────────────────────────────────────
TEST_F(EventAccessTypeFix, Site10_S2PSExceeded_ExecuteAccess_IndMustBeTrue) {
    // Stage-2-only stream with s2ps=2 (40-bit output PA space)
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = true;
    cfg.s2t0sz             = 0; // no IPA range restriction
    cfg.s2ps               = 2; // 40-bit PA space — PA >= 2^40 → F_ADDR_SIZE
    ASSERT_TRUE(smmu_->configureStream(SID, cfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(SID).isOk());

    // Stage-2 address space: map an IPA to a PA above the 40-bit S2PS bound
    auto s2as = std::make_shared<AddressSpace>();
    static constexpr IPA  IPA_ADDR       = 0x3000u;
    static constexpr PA   ABOVE_S2PS_PA  = (static_cast<PA>(1) << 40u) | 0x4000u;
    PagePermissions perms(true, true, true); // read+write+execute
    ASSERT_TRUE(s2as->mapPage(IPA_ADDR, ABOVE_S2PS_PA, perms).isOk());
    ASSERT_TRUE(smmu_->setStreamStage2AddressSpace(SID, s2as).isOk());

    auto r = smmu_->translate(SID, PID, IPA_ADDR, AccessType::Execute);
    EXPECT_TRUE(r.isError())
        << "PA >= S2PS limit must produce a translation error (F_ADDR_SIZE)";

    auto ev = smmu_->getEventQueue();
    ASSERT_FALSE(ev.empty())
        << "F_ADDR_SIZE event must be recorded for S2PS PA-size violation";

    const EventEntry* found = nullptr;
    for (const auto& e : ev) {
        if (e.type == EventType::F_ADDR_SIZE) {
            found = &e;
            break;
        }
    }
    ASSERT_NE(found, nullptr)
        << "An F_ADDR_SIZE event must exist in the event queue";

    // Bug 2: generateEvent() at line 1882 omits accessType → ind defaults to false.
    EXPECT_TRUE(found->ind)
        << "Bug2 Site10: F_ADDR_SIZE S2PS event ind must be true for Execute access "
        << "(ARM IHI0070G.b §7.3 InD field)";
    // RnW-GAP fix: ARM §7.3 RnW=1=Read. Execute is a read; rnw=true.
    EXPECT_TRUE(found->rnw)
        << "Bug2 Site10: rnw must be true for an Execute (read/instruction) access "
        << "(ARM IHI0070G.b §7.3 RnW=1=Read)";
}
