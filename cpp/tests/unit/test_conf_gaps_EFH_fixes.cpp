// ARM SMMU v3 Conformance Gap Tests: GAP-E, GAP-F, GAP-H
// Copyright (c) 2024 John Greninger
//
// GAP-E (§3.4.1/§5.4, Low): CD.TBI — top-byte-ignore for VA range check
// GAP-F (§5.4/§3.4, Low):   CD.IPS — stage-1 output IPA size bound → F_ADDR_SIZE
// GAP-H (§3.13.6, Low):     PRIQ overflow auto-PRG_RESPONSE(FAILURE)

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include "smmu/configuration.h"
#include <memory>

using namespace smmu;

// ─────────────────────────────────────────────────────────────────────────────
// Shared constants and helpers
// ─────────────────────────────────────────────────────────────────────────────

namespace {

static constexpr StreamID SID   = 0x10;
static constexpr PASID    PID   = 0;

// Stage-1-only stream with a PASID-0 address space.
void setupStage1Stream(SMMU& smmu, StreamID sid = SID, PASID pasid = PID) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, pasid).isOk());
}

// Two-stage stream (stage-1 + stage-2).
// The caller must still set up address spaces and call setStreamStage2AddressSpace.
void setupTwoStageStream(SMMU& smmu, StreamID sid = SID, PASID pasid = PID,
                         uint8_t ips = 6) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.ips                = ips;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, pasid).isOk());
}

// Return the first event from the queue.
EventEntry firstEvent(SMMU& smmu) {
    auto ev = smmu.getEventQueue();
    return ev.empty() ? EventEntry{} : ev.front();
}

} // anonymous namespace

// ═════════════════════════════════════════════════════════════════════════════
// GAP-E: CD.TBI — top-byte-ignore (ARM IHI0070G.b §3.4.1)
// ═════════════════════════════════════════════════════════════════════════════

class GapETest : public ::testing::Test {
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

// TBI=1, T0SZ=16 (48-bit VA range: [0, 2^48)).
// IOVA=0xFF00_1000_0000_0000: top byte=0xFF, effective=0x0000_1000_0000_0000
// which is within 2^48 → translation should proceed (fault only on unmapped page,
// not on the range check).
//
// We map the effective page so the translation succeeds end-to-end.
TEST_F(GapETest, gap_e_tbi_1_masks_top_byte_range_check) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.t0sz               = 16;   // 48-bit VA
    cfg.tbi                = true; // mask top byte before range check
    ASSERT_TRUE(smmu_->configureStream(SID, cfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(SID).isOk());
    ASSERT_TRUE(smmu_->createStreamPASID(SID, PID).isOk());

    // IOVA with top byte 0xFF: 0xFF00_1000_0000_0000
    // After TBI masking:        0x0000_1000_0000_0000  (within 48-bit range [0, 2^48))
    //
    // Map the page at the effective (top-byte-stripped) address.  The SW model applies
    // TBI masking to both the range check AND the page-table lookup, so the mapping must
    // be at the masked address for translate() to find it.
    static constexpr IOVA TAGGED_IOVA    = UINT64_C(0xFF00100000000000);
    static constexpr IOVA EFFECTIVE_PAGE = UINT64_C(0x0000100000000000); // tagged IOVA & 0x00FF...
    static constexpr PA   OUTPUT_PA      = UINT64_C(0x00000000DEAD0000);

    PagePermissions perms(true, true, false);
    ASSERT_TRUE(smmu_->mapPage(SID, PID, EFFECTIVE_PAGE, OUTPUT_PA, perms).isOk());

    // Translate with tagged IOVA — TBI must strip the top byte for both the range
    // check and the page-table lookup, so the mapping at EFFECTIVE_PAGE is found.
    auto result = smmu_->translate(SID, PID, TAGGED_IOVA, AccessType::Read);

    // The event queue must be empty (no F_TRANSLATION from a range-check failure).
    auto events = smmu_->getEventQueue();
    EXPECT_TRUE(events.empty())
        << "GAP-E: TBI=1 must mask top byte; no F_TRANSLATION range-check event expected";
    EXPECT_TRUE(result.isOk())
        << "GAP-E: TBI=1 with tagged IOVA inside effective 48-bit range must succeed";
}

// TBI=0, same tagged IOVA — top byte is NOT masked, so the full IOVA 0xFF00...
// is >= 2^48 and must generate F_TRANSLATION.
TEST_F(GapETest, gap_e_tbi_0_top_byte_not_masked) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.t0sz               = 16;    // 48-bit VA range
    cfg.tbi                = false; // top byte participates in range check (default)
    ASSERT_TRUE(smmu_->configureStream(SID, cfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(SID).isOk());
    ASSERT_TRUE(smmu_->createStreamPASID(SID, PID).isOk());

    // IOVA with top byte 0xFF: 0xFF00_1000_0000_0000 >= 2^48 → range-check fault.
    static constexpr IOVA TAGGED_IOVA = UINT64_C(0xFF00100000000000);

    auto result = smmu_->translate(SID, PID, TAGGED_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isError())
        << "GAP-E: TBI=0, IOVA with top byte >= 2^48 must produce an error";

    EventEntry e = firstEvent(*smmu_);
    EXPECT_EQ(e.type, EventType::F_TRANSLATION)
        << "GAP-E: TBI=0 out-of-range IOVA must generate F_TRANSLATION";
}

// TBI=1, T0SZ=16 (48-bit), IOVA=0xFF01_0000_0000_0000.
// After masking top byte: effective = 0x0001_0000_0000_0000 = 2^48 = vaLimit.
// This equals the limit (not strictly below), so it is still out of range
// → F_TRANSLATION.
TEST_F(GapETest, gap_e_tbi_1_effective_iova_still_exceeds_range) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.t0sz               = 16;    // 48-bit VA range: [0, 2^48)
    cfg.tbi                = true;  // mask top byte
    ASSERT_TRUE(smmu_->configureStream(SID, cfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(SID).isOk());
    ASSERT_TRUE(smmu_->createStreamPASID(SID, PID).isOk());

    // 0xFF01_0000_0000_0000: strip top byte → 0x0001_0000_0000_0000 == 2^48 (at limit)
    static constexpr IOVA IOVA_AT_LIMIT = UINT64_C(0xFF01000000000000);

    auto result = smmu_->translate(SID, PID, IOVA_AT_LIMIT, AccessType::Read);
    EXPECT_TRUE(result.isError())
        << "GAP-E: effective IOVA == 2^48 is at the limit and must still fault";

    EventEntry e = firstEvent(*smmu_);
    EXPECT_EQ(e.type, EventType::F_TRANSLATION)
        << "GAP-E: effective IOVA at vaLimit must generate F_TRANSLATION";
}

// ═════════════════════════════════════════════════════════════════════════════
// GAP-F: CD.IPS — stage-1 output IPA size (ARM IHI0070G.b §5.4/§3.4)
// ═════════════════════════════════════════════════════════════════════════════

class GapFTest : public ::testing::Test {
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

// IPS=0 (32-bit output): stage-1 maps IOVA→IPA where IPA = 2^32.
// The IPA is at the limit (>= 2^32) → F_ADDR_SIZE must be generated.
TEST_F(GapFTest, gap_f_ips_32bit_ipa_exceeds_generates_f_addr_size) {
    // IPA just beyond 32-bit range
    static constexpr IPA  TOO_LARGE_IPA = UINT64_C(1) << 32; // == 2^32
    static constexpr IOVA TEST_IOVA     = UINT64_C(0x4000);

    // Set up two-stage stream with IPS=0 (32-bit IPA limit)
    setupTwoStageStream(*smmu_, SID, PID, /*ips=*/0);

    // Provide a stage-2 address space.  Even if stage-2 has the IPA mapped,
    // the IPS check happens before stage-2 is consulted.
    auto s2as = std::make_shared<AddressSpace>();
    PagePermissions s2perms(true, true, false);
    s2as->mapPage(TOO_LARGE_IPA, UINT64_C(0x80000000), s2perms);
    ASSERT_TRUE(smmu_->setStreamStage2AddressSpace(SID, s2as).isOk());

    // Map IOVA -> TOO_LARGE_IPA in stage-1
    PagePermissions s1perms(true, true, false);
    ASSERT_TRUE(smmu_->mapPage(SID, PID, TEST_IOVA, TOO_LARGE_IPA, s1perms).isOk());

    auto result = smmu_->translate(SID, PID, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isError())
        << "GAP-F: IPS=0 (32-bit), IPA=2^32 must produce an error";

    EventEntry e = firstEvent(*smmu_);
    EXPECT_EQ(e.type, EventType::F_ADDR_SIZE)
        << "GAP-F: IPA exceeding IPS=0 (32-bit) must generate F_ADDR_SIZE";
}

// IPS=5 (48-bit): IPA = 0x0000_FFFF_FFFF_F000 is within 2^48 → translation succeeds.
TEST_F(GapFTest, gap_f_ips_48bit_ipa_within_succeeds) {
    // 0x0000_FFFF_FFFF_F000 < 2^48 (2^48 = 0x0001_0000_0000_0000)
    static constexpr IPA  LARGE_IPA = UINT64_C(0x0000FFFFFFFFFFF0); // 16 hex = 64-bit, within 52b
    static constexpr IOVA TEST_IOVA = UINT64_C(0x5000);
    static constexpr PA   OUTPUT_PA = UINT64_C(0x0000100000000000);

    // Use IPS=5 (48-bit limit, same as default S2PS=5)
    setupTwoStageStream(*smmu_, SID, PID, /*ips=*/5);

    // Stage-2 maps LARGE_IPA -> OUTPUT_PA
    auto s2as = std::make_shared<AddressSpace>();
    PagePermissions perms(true, true, false);
    s2as->mapPage(LARGE_IPA, OUTPUT_PA, perms);
    ASSERT_TRUE(smmu_->setStreamStage2AddressSpace(SID, s2as).isOk());

    // Stage-1 maps TEST_IOVA -> LARGE_IPA
    ASSERT_TRUE(smmu_->mapPage(SID, PID, TEST_IOVA, LARGE_IPA, perms).isOk());

    auto result = smmu_->translate(SID, PID, TEST_IOVA, AccessType::Read);
    auto events = smmu_->getEventQueue();
    EXPECT_TRUE(events.empty())
        << "GAP-F: IPS=5 (48-bit), IPA within range — no F_ADDR_SIZE expected";
    EXPECT_TRUE(result.isOk())
        << "GAP-F: IPS=5 (48-bit), IPA within 2^48 must succeed";
}

// IPS=6 (52-bit, default — no effective restriction): IPA within 48-bit range
// is also within 52-bit range, so IPS=6 imposes no additional restriction.
// Use S2PS=6 as well so the stage-2 OAS check (NEW-8) does not interfere.
TEST_F(GapFTest, gap_f_ips_default_52bit_no_restriction) {
    // Use an IPA within 48-bit space (satisfies both IPS=6 and default S2PS checks).
    static constexpr IPA  NORMAL_IPA = UINT64_C(0x000000FFFFFFF000); // within 48b
    static constexpr IOVA TEST_IOVA  = UINT64_C(0x6000);
    static constexpr PA   OUTPUT_PA  = UINT64_C(0x0000000080000000); // within 48b

    // Configure with IPS=6 and S2PS=6 (52-bit) to avoid any address-size fault.
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.ips                = 6;  // 52-bit stage-1 IPA output: no effective restriction
    cfg.s2ps               = 6;  // 52-bit stage-2 PA output: no effective restriction
    ASSERT_TRUE(smmu_->configureStream(SID, cfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(SID).isOk());
    ASSERT_TRUE(smmu_->createStreamPASID(SID, PID).isOk());

    auto s2as = std::make_shared<AddressSpace>();
    PagePermissions perms(true, true, false);
    s2as->mapPage(NORMAL_IPA, OUTPUT_PA, perms);
    ASSERT_TRUE(smmu_->setStreamStage2AddressSpace(SID, s2as).isOk());

    ASSERT_TRUE(smmu_->mapPage(SID, PID, TEST_IOVA, NORMAL_IPA, perms).isOk());

    auto result = smmu_->translate(SID, PID, TEST_IOVA, AccessType::Read);
    auto events = smmu_->getEventQueue();
    EXPECT_TRUE(events.empty())
        << "GAP-F: IPS=6 (52-bit default), IPA within range — no F_ADDR_SIZE expected";
    EXPECT_TRUE(result.isOk())
        << "GAP-F: IPS=6 (52-bit), IPA within 2^52 must succeed";
}

// ═════════════════════════════════════════════════════════════════════════════
// GAP-H: PRIQ overflow auto-PRG_RESPONSE(FAILURE) (ARM IHI0070G.b §3.13.6)
// ═════════════════════════════════════════════════════════════════════════════

class GapHTest : public ::testing::Test {
protected:
    void SetUp() override {
        // Use the minimum PRIQ capacity (16 entries, the allowed minimum) so that
        // overflow is easy to trigger without needing hundreds of entries.
        SMMUConfiguration config = SMMUConfiguration::createDefault();
        // QueueConfiguration(eventSize, commandSize, priSize): minimum is 16.
        QueueConfiguration qcfg(512, 256, 16);
        config.setQueueConfiguration(qcfg);
        smmu_ = std::make_unique<SMMU>(config);
        smmu_->enable();
        priCapacity_ = 16;
    }

    void TearDown() override {
        smmu_.reset();
    }

    // Fill the PRIQ to capacity with distinct prgIndex values.
    void fillPRIQ(size_t count) {
        for (size_t i = 0; i < count; ++i) {
            PRIEntry req;
            req.streamID        = SID;
            req.pasid           = PID;
            req.requestedAddress = static_cast<IOVA>(i << 12);
            req.accessType      = AccessType::Read;
            req.isLastRequest   = false;
            req.prgIndex        = static_cast<uint16_t>(i);
            smmu_->submitPageRequest(req);
        }
    }

    std::unique_ptr<SMMU> smmu_;
    size_t priCapacity_;
};

// Fill PRIQ to capacity, then submit one more request.
// The overflow entry must appear as an auto-failure response.
TEST_F(GapHTest, gap_h_priq_overflow_auto_failure_response) {
    // Fill to capacity
    fillPRIQ(priCapacity_);
    EXPECT_EQ(smmu_->getPRIQueueSize(), priCapacity_)
        << "PRIQ must be at capacity before overflow";

    // No auto-failures yet
    EXPECT_TRUE(smmu_->getPRIAutoFailures().empty())
        << "No auto-failures expected before overflow";

    // Submit the overflow request with a distinct prgIndex
    PRIEntry overflow;
    overflow.streamID        = SID;
    overflow.pasid           = PID;
    overflow.requestedAddress = UINT64_C(0x9000);
    overflow.accessType      = AccessType::Read;
    overflow.isLastRequest   = true;
    overflow.prgIndex        = 99;
    smmu_->submitPageRequest(overflow);

    // The overflow entry must NOT be enqueued in the PRIQ
    EXPECT_EQ(smmu_->getPRIQueueSize(), priCapacity_)
        << "GAP-H: PRIQ must stay at capacity; overflow entry must not be enqueued";

    // An auto-failure response must be recorded
    auto failures = smmu_->getPRIAutoFailures();
    ASSERT_EQ(failures.size(), 1u)
        << "GAP-H: §3.13.6 — exactly one auto-failure response must be generated on PRIQ overflow";

    const PRIAutoFailure& f = failures.front();
    EXPECT_EQ(f.streamID, SID)
        << "GAP-H: auto-failure streamID must match the overflow request";
    EXPECT_EQ(f.pasid, PID)
        << "GAP-H: auto-failure pasid must match the overflow request";
    EXPECT_EQ(f.prgIndex, static_cast<uint16_t>(99))
        << "GAP-H: auto-failure prgIndex must match the overflow request's prgIndex";
}

// When PRIQ is not full, normal enqueue must succeed and no auto-failure generated.
TEST_F(GapHTest, gap_h_priq_normal_enqueue_no_auto_response) {
    EXPECT_EQ(smmu_->getPRIQueueSize(), 0u);
    EXPECT_TRUE(smmu_->getPRIAutoFailures().empty());

    // Submit a single request — PRIQ has capacity 4, so it fits
    PRIEntry req;
    req.streamID        = SID;
    req.pasid           = PID;
    req.requestedAddress = UINT64_C(0x1000);
    req.accessType      = AccessType::Read;
    req.isLastRequest   = false;
    req.prgIndex        = 7;
    smmu_->submitPageRequest(req);

    EXPECT_EQ(smmu_->getPRIQueueSize(), 1u)
        << "GAP-H: request must be enqueued when PRIQ is not full";
    EXPECT_TRUE(smmu_->getPRIAutoFailures().empty())
        << "GAP-H: no auto-failure must be generated when PRIQ is not full";
}
