// ARM SMMU v3 — QA Conformance Gaps: NEW-1, NEW-2, NEW-5
// Copyright (c) 2024 John Greninger
//
// TDD spec tests for three newly identified conformance gaps:
//
//   GAP NEW-1  §7.3: CLASS field encoding in EventEntry
//     - C_* (config) events must have eventClass == 0 (field not defined for them)
//     - F_TRANSLATION / F_ADDR_SIZE / F_PERMISSION faults must have eventClass == 2 (IN)
//
//   GAP NEW-2  §7.3.13: S2 and IPA fields for two-stage faults
//     - Stage-2 translation fault must have event.s2 == true
//     - Stage-2 translation fault must carry event.ipa == stage-1 output address
//     - Stage-1-only faults must have event.s2 == false, event.ipa == 0
//
//   GAP NEW-5  §3.4: Stage-2-bypass OAS — truncate, not abort
//     - When stage-1 translates and stage-2 is bypassed, a PA > OAS must be
//       silently truncated to OAS width (no F_ADDR_SIZE event, result is OK)

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include "smmu/configuration.h"
#include <memory>

using namespace smmu;

// ─────────────────────────────────────────────────────────────────────────────
// Shared helpers
// ─────────────────────────────────────────────────────────────────────────────

namespace {

static constexpr StreamID SID  = 0x42;
static constexpr PASID    PID  = 0;
static constexpr IOVA     BASE_IOVA = 0x1000;
static constexpr PA       BASE_PA   = 0x8000;

// Build and enable a stage-1-only stream with a PASID-0 context.
void setupStage1Stream(SMMU& smmu, StreamID sid = SID, PASID pasid = PID) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, pasid).isOk());
}

// Build and enable a two-stage stream (stage1 + stage2).
// The caller must still call setStreamStage2AddressSpace() for stage-2 mappings.
void setupTwoStageStream(SMMU& smmu, StreamID sid = SID, PASID pasid = PID) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, pasid).isOk());
}

// Return the first event from the queue, or a default-constructed entry.
EventEntry firstEvent(SMMU& smmu) {
    auto ev = smmu.getEventQueue();
    if (ev.empty()) {
        return EventEntry{};
    }
    return ev.front();
}

} // anonymous namespace

// ═════════════════════════════════════════════════════════════════════════════
// GAP NEW-1: CLASS field encoding (ARM IHI0070G.b §7.3)
// ═════════════════════════════════════════════════════════════════════════════

class NewQAFindings : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_ = std::make_unique<SMMU>();
        smmu_->enable();
    }

    void TearDown() override {
        smmu_.reset();
    }

    std::unique_ptr<SMMU> smmu_;
};

// C_BAD_STE event must have eventClass == 0 (CLASS field is not defined for
// configuration events; SW model leaves it as 0).
// ARM IHI0070G.b §7.3: "The CLASS field is defined only for F_* translation events."
TEST_F(NewQAFindings, New1_CConfigEventClassIsZero) {
    // Trigger a C_BAD_STE by configuring a Non-Secure stream with STRW=EL3
    // (§5.2 Table 3-2: EL3 is forbidden for NS streams → C_BAD_STE).
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.strw               = StreamWorld::EL3; // forbidden for NS

    smmu_->setCR0(smmu_->getCR0() | SMMU::CR0_EVENTQEN);
    auto r = smmu_->configureStream(SID, cfg);
    EXPECT_TRUE(r.isError())
        << "C_BAD_STE config validation must fail for NS+STRW=EL3";

    auto ev = smmu_->getEventQueue();
    ASSERT_FALSE(ev.empty())
        << "C_BAD_STE event must be recorded in the event queue";

    const EventEntry& e = ev.front();
    EXPECT_EQ(e.type, EventType::C_BAD_STE)
        << "Event type must be C_BAD_STE";
    EXPECT_EQ(e.eventClass, 0u)
        << "GAP NEW-1: C_* events must have eventClass==0 (CLASS field undefined for config faults)";
}

// F_TRANSLATION event must have eventClass == 2 (0b10 = IN = fault on input address).
// ARM IHI0070G.b §7.3: CLASS 0b10 = IN means the fault is on the input address itself.
TEST_F(NewQAFindings, New1_FTranslationEventClassIsIN) {
    setupStage1Stream(*smmu_);
    smmu_->setCR0(smmu_->getCR0() | SMMU::CR0_EVENTQEN);

    // Translate an unmapped IOVA — triggers F_TRANSLATION
    auto r = smmu_->translate(SID, PID, BASE_IOVA, AccessType::Read);
    EXPECT_TRUE(r.isError())
        << "Unmapped IOVA must produce a translation error";

    auto ev = smmu_->getEventQueue();
    ASSERT_FALSE(ev.empty())
        << "F_TRANSLATION event must be recorded";

    const EventEntry& e = ev.front();
    EXPECT_EQ(e.type, EventType::F_TRANSLATION)
        << "Event type must be F_TRANSLATION";
    EXPECT_EQ(e.eventClass, 2u)
        << "GAP NEW-1: F_TRANSLATION must have eventClass==2 (0b10=IN, fault on input address)";
}

// ═════════════════════════════════════════════════════════════════════════════
// GAP NEW-2: S2 and IPA fields for two-stage faults (ARM IHI0070G.b §7.3.13)
// ═════════════════════════════════════════════════════════════════════════════

// Two-stage stream: stage-1 maps IOVA→IPA but stage-2 has no mapping for IPA.
// The resulting F_TRANSLATION event must have s2==true and ipa==stage-1 output IPA.
TEST_F(NewQAFindings, New2_Stage2FaultHasS2True) {
    setupTwoStageStream(*smmu_);
    smmu_->setCR0(smmu_->getCR0() | SMMU::CR0_EVENTQEN);

    // Create a separate stage-2 address space (no mappings — stage-2 will fault).
    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu_->setStreamStage2AddressSpace(SID, s2as).isOk());

    // Map IOVA -> IPA in stage-1
    static constexpr IPA IPA_ADDR = 0x20000;
    PagePermissions perms(true, true, false);
    ASSERT_TRUE(smmu_->mapPage(SID, PID, BASE_IOVA, IPA_ADDR, perms).isOk());

    // Translate: stage-1 succeeds (IOVA -> IPA_ADDR), stage-2 fails (IPA not mapped)
    auto r = smmu_->translate(SID, PID, BASE_IOVA, AccessType::Read);
    EXPECT_TRUE(r.isError())
        << "Two-stage fault must produce a translation error";

    auto ev = smmu_->getEventQueue();
    ASSERT_FALSE(ev.empty())
        << "F_TRANSLATION event must be recorded for two-stage fault";

    const EventEntry& e = ev.front();
    EXPECT_EQ(e.type, EventType::F_TRANSLATION)
        << "Event type must be F_TRANSLATION";
    EXPECT_TRUE(e.s2)
        << "GAP NEW-2: stage-2 fault must have s2==true in the event record";
}

TEST_F(NewQAFindings, New2_Stage2FaultHasCorrectIPA) {
    setupTwoStageStream(*smmu_);
    smmu_->setCR0(smmu_->getCR0() | SMMU::CR0_EVENTQEN);

    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu_->setStreamStage2AddressSpace(SID, s2as).isOk());

    static constexpr IPA IPA_ADDR = 0x20000;
    PagePermissions perms(true, true, false);
    ASSERT_TRUE(smmu_->mapPage(SID, PID, BASE_IOVA, IPA_ADDR, perms).isOk());

    auto r = smmu_->translate(SID, PID, BASE_IOVA, AccessType::Read);
    EXPECT_TRUE(r.isError());

    auto ev = smmu_->getEventQueue();
    ASSERT_FALSE(ev.empty());

    const EventEntry& e = ev.front();
    EXPECT_EQ(e.type, EventType::F_TRANSLATION);
    // event.ipa must carry the IPA (stage-1 output address), page-aligned
    static constexpr IPA IPA_PAGE_ALIGNED = IPA_ADDR & ~static_cast<uint64_t>(0xFFF);
    EXPECT_EQ(e.ipa, IPA_PAGE_ALIGNED)
        << "GAP NEW-2: event.ipa must equal stage-1 output IPA (0x" << std::hex << IPA_PAGE_ALIGNED
        << "), got 0x" << e.ipa;
}

// Stage-1-only fault: F_TRANSLATION must have s2==false and ipa==0.
TEST_F(NewQAFindings, New2_Stage1FaultHasS2False) {
    setupStage1Stream(*smmu_);
    smmu_->setCR0(smmu_->getCR0() | SMMU::CR0_EVENTQEN);

    // Translate unmapped IOVA in stage-1-only stream
    auto r = smmu_->translate(SID, PID, BASE_IOVA, AccessType::Read);
    EXPECT_TRUE(r.isError());

    auto ev = smmu_->getEventQueue();
    ASSERT_FALSE(ev.empty());

    const EventEntry& e = ev.front();
    EXPECT_EQ(e.type, EventType::F_TRANSLATION);
    EXPECT_FALSE(e.s2)
        << "GAP NEW-2: stage-1-only fault must have s2==false";
    EXPECT_EQ(e.ipa, 0u)
        << "GAP NEW-2: stage-1-only fault must have ipa==0";
}

// ═════════════════════════════════════════════════════════════════════════════
// GAP NEW-5: Stage-2-bypass OAS — truncate, not abort (ARM IHI0070G.b §3.4)
// ═════════════════════════════════════════════════════════════════════════════

// Stage-1-only stream (stage-2 bypassed): a stage-1 mapping that produces a
// PA above OAS must be silently truncated to OAS width.  The translation must
// succeed (not abort), and no F_ADDR_SIZE event must be generated.
//
// Setup:
//   - Configure SMMU with OAS = 48 bits explicitly
//   - Map IOVA -> PA where PA = 0x1_0000_1000 (above 48-bit OAS, below 52-bit MAX)
//   - Translate -> result must be OK, PA truncated to 48-bit, no F_ADDR_SIZE

TEST_F(NewQAFindings, New5_Stage2BypassOAS_TruncatesNotAborts) {
    // Configure SMMU with a 48-bit OAS so that PAs in [2^48, 2^52) are above OAS
    // but below MAX_PHYSICAL_ADDRESS (52-bit).  This lets mapPage() succeed while
    // the OAS check in translateUnlocked() currently generates F_ADDR_SIZE (bug).
    SMMUConfiguration smmuCfg;
    AddressConfiguration addrCfg;
    addrCfg.maxPASize = 48; // 48-bit OAS
    smmuCfg.setAddressConfiguration(addrCfg);
    SMMU smmu48(smmuCfg);
    smmu48.enable();
    smmu48.setCR0(smmu48.getCR0() | SMMU::CR0_EVENTQEN);

    // Configure a stage-1-only stream (stage-2 bypassed)
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    ASSERT_TRUE(smmu48.configureStream(SID, cfg).isOk());
    ASSERT_TRUE(smmu48.enableStream(SID).isOk());
    ASSERT_TRUE(smmu48.createStreamPASID(SID, PID).isOk());

    // PA just above the 48-bit OAS boundary: 2^48 + page_offset.
    // This is within 52-bit MAX_PHYSICAL_ADDRESS space, so mapPage() must accept it.
    static constexpr uint64_t OAS_BITS = 48u;
    static constexpr PA ABOVE_OAS_PA = (static_cast<PA>(1) << OAS_BITS) | 0x1000u; // above OAS

    PagePermissions perms(true, true, false);
    auto map_r = smmu48.mapPage(SID, PID, BASE_IOVA, ABOVE_OAS_PA, perms);
    ASSERT_TRUE(map_r.isOk())
        << "mapPage must accept PA=0x" << std::hex << ABOVE_OAS_PA
        << " (within 52-bit MAX_PHYSICAL_ADDRESS range)";

    // Translate: stage-1 should produce ABOVE_OAS_PA, stage-2 is bypassed.
    // Per ARM IHI0070G.b §3.4: when stage-2 is bypassed (STE.Config=0b10x),
    // PA above OAS must be silently TRUNCATED to OAS width — NOT aborted.
    auto r = smmu48.translate(SID, PID, BASE_IOVA, AccessType::Read);

    EXPECT_TRUE(r.isOk())
        << "GAP NEW-5: §3.4 stage-2-bypass — PA above OAS must be truncated silently, "
        << "not aborted with F_ADDR_SIZE";

    // No F_ADDR_SIZE event must be generated
    auto ev = smmu48.getEventQueue();
    bool has_addr_size_event = false;
    for (const auto& e : ev) {
        if (e.type == EventType::F_ADDR_SIZE) {
            has_addr_size_event = true;
            break;
        }
    }
    EXPECT_FALSE(has_addr_size_event)
        << "GAP NEW-5: §3.4 stage-2-bypass — no F_ADDR_SIZE event must be generated";

    if (r.isOk()) {
        // Verify the returned PA is masked to OAS width
        static constexpr PA OAS_MASK = (static_cast<PA>(1) << OAS_BITS) - 1u;
        PA expected_pa = ABOVE_OAS_PA & OAS_MASK;
        PA actual_pa   = r.getValue().physicalAddress;
        EXPECT_EQ(actual_pa, expected_pa)
            << "GAP NEW-5: truncated PA must equal PA & ((1<<OAS)-1). "
            << "Expected 0x" << std::hex << expected_pa
            << ", got 0x" << actual_pa;
    }
}
