// ARM SMMU v3 FINDING-M-10: Address Size Fault Checking
// Copyright (c) 2024 John Greninger
//
// TDD spec tests for per-context IOVA address-size validation.
//
// Spec: ARM IHI0070G.b §3.4 (Address sizes), §3.4.1 (Input address size),
//       §7.3.14 (F_ADDR_SIZE event, opcode 0x11)
//
// Requirements:
// - AddressSpace must store a per-context inputAddressSizeBits (default: 52).
// - SMMU::setStreamInputAddressSize(streamID, pasid, bits) propagates the
//   size to the relevant AddressSpace.
// - translatePage() checks IOVA against the configured limit BEFORE the page
//   table lookup.  If IOVA >= (1ULL << bits), return AddressSizeFault
//   (SMMUError::InvalidAddress) instead of the generic TranslationFault.
// - handleTranslationFailure() generates EventType::F_ADDR_SIZE when the
//   fault type is AddressSizeFault.
// - The default 52-bit limit must NOT break any existing behaviour.
// - Only the affected (stream, PASID) context is constrained; other contexts
//   with the default 52-bit limit are unaffected.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// ── Constants ──────────────────────────────────────────────────────────────

static constexpr StreamID STREAM_A = 0x10;
static constexpr StreamID STREAM_B = 0x20;
static constexpr PASID    PASID_0  = 0;
static constexpr PASID    PASID_1  = 1;

// One byte beyond each power-of-2 boundary
static constexpr IOVA JUST_ABOVE_32BIT = (1ULL << 32);   // 4 GiB
static constexpr IOVA JUST_ABOVE_48BIT = (1ULL << 48);   // 256 TiB

// A PA safely within 52-bit range for all test mappings
static constexpr PA TEST_PA = 0x8000'0000ULL;

// ── Fixture ────────────────────────────────────────────────────────────────

class AddrSizeFaultTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu = std::make_unique<SMMU>();
    }

    void TearDown() override {
        smmu.reset();
    }

    // Configure a Stage-1 stream with a single PASID.
    void configureStream(StreamID sid, PASID pasid) {
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = false;
        ASSERT_TRUE(smmu->configureStream(sid, cfg).isOk());
        ASSERT_TRUE(smmu->enableStream(sid).isOk());
        ASSERT_TRUE(smmu->createStreamPASID(sid, pasid).isOk());
    }

    // Map a page at iova for (sid, pasid); PA is always TEST_PA.
    void mapPage(StreamID sid, PASID pasid, IOVA iova) {
        PagePermissions perms(true, true, false);
        ASSERT_TRUE(smmu->mapPage(sid, pasid, iova, TEST_PA, perms).isOk());
    }

    // Translate and return the result.
    TranslationResult translate(StreamID sid, PASID pasid, IOVA iova) {
        return smmu->translate(sid, pasid, iova, AccessType::Read);
    }

    std::unique_ptr<SMMU> smmu;
};

// ── Test 1: default 52-bit limit — existing behaviour unchanged ───────────

// With the default limit (52-bit), any valid IOVA succeeds.
TEST_F(AddrSizeFaultTest, Default52Bit_NormalIOVA_Succeeds) {
    configureStream(STREAM_A, PASID_0);
    IOVA iova = 0x1000;
    mapPage(STREAM_A, PASID_0, iova);
    TranslationResult r = translate(STREAM_A, PASID_0, iova);
    EXPECT_TRUE(r.isOk())
        << "Normal IOVA with default 52-bit limit must succeed";
}

// With the default 52-bit limit, a large-but-valid IOVA succeeds.
TEST_F(AddrSizeFaultTest, Default52Bit_LargeIOVA_BelowLimit_Succeeds) {
    configureStream(STREAM_A, PASID_0);
    // Map at an address well beyond 48-bit but inside 52-bit
    IOVA iova = 0x000F'0000'0000'0000ULL;
    mapPage(STREAM_A, PASID_0, iova);
    TranslationResult r = translate(STREAM_A, PASID_0, iova);
    EXPECT_TRUE(r.isOk())
        << "IOVA within 52-bit default limit must succeed";
}

// ── Test 2: 32-bit limit — IOVA within range succeeds ────────────────────

TEST_F(AddrSizeFaultTest, Set32Bit_IOVAWithinRange_Succeeds) {
    configureStream(STREAM_A, PASID_0);

    // Map a page at a 32-bit IOVA
    IOVA iova = 0x0000'0000'8000'0000ULL;  // 2 GiB — within 32-bit
    mapPage(STREAM_A, PASID_0, iova);

    // Constrain the context to 32-bit
    EXPECT_TRUE(smmu->setStreamInputAddressSize(STREAM_A, PASID_0, 32).isOk());

    TranslationResult r = translate(STREAM_A, PASID_0, iova);
    EXPECT_TRUE(r.isOk())
        << "IOVA within the 32-bit configured limit must succeed";
}

// ── Test 3: 32-bit limit — IOVA exceeding range → AddressSizeFault ───────

// An IOVA >= 2^32 must produce SMMUError::InvalidAddress (AddressSizeFault)
// when the context is configured for 32-bit, even if the page is mapped.
TEST_F(AddrSizeFaultTest, Set32Bit_IOVAAbove32Bit_ReturnsAddressSizeFault) {
    configureStream(STREAM_A, PASID_0);

    // Map a page at the offending IOVA (valid in 52-bit space, so mapPage succeeds)
    mapPage(STREAM_A, PASID_0, JUST_ABOVE_32BIT);

    // Now restrict the context to 32-bit input address size
    ASSERT_TRUE(smmu->setStreamInputAddressSize(STREAM_A, PASID_0, 32).isOk());

    TranslationResult r = translate(STREAM_A, PASID_0, JUST_ABOVE_32BIT);
    ASSERT_TRUE(r.isError())
        << "IOVA exceeding 32-bit limit must fail";
    EXPECT_EQ(r.getError(), SMMUError::InvalidAddress)
        << "Expected SMMUError::InvalidAddress (AddressSizeFault), got "
        << static_cast<int>(r.getError());
}

// ── Test 4: 48-bit limit — IOVA exceeding range → AddressSizeFault ───────

TEST_F(AddrSizeFaultTest, Set48Bit_IOVAAbove48Bit_ReturnsAddressSizeFault) {
    configureStream(STREAM_A, PASID_0);

    // JUST_ABOVE_48BIT is 2^48 — valid in 52-bit global space
    mapPage(STREAM_A, PASID_0, JUST_ABOVE_48BIT);

    ASSERT_TRUE(smmu->setStreamInputAddressSize(STREAM_A, PASID_0, 48).isOk());

    TranslationResult r = translate(STREAM_A, PASID_0, JUST_ABOVE_48BIT);
    ASSERT_TRUE(r.isError())
        << "IOVA exceeding 48-bit limit must fail";
    EXPECT_EQ(r.getError(), SMMUError::InvalidAddress)
        << "Expected SMMUError::InvalidAddress (AddressSizeFault) for 48-bit overflow";
}

// ── Test 5: F_ADDR_SIZE event generated ──────────────────────────────────

// After an address-size fault the event queue must contain an F_ADDR_SIZE entry.
TEST_F(AddrSizeFaultTest, AddrSizeFault_GeneratesF_ADDR_SIZE_Event) {
    configureStream(STREAM_A, PASID_0);
    mapPage(STREAM_A, PASID_0, JUST_ABOVE_32BIT);
    ASSERT_TRUE(smmu->setStreamInputAddressSize(STREAM_A, PASID_0, 32).isOk());

    // Trigger the fault
    smmu->translate(STREAM_A, PASID_0, JUST_ABOVE_32BIT, AccessType::Read);

    // Inspect event queue for F_ADDR_SIZE
    std::vector<EventEntry> events = smmu->getEventQueue();
    bool found = false;
    for (const auto& e : events) {
        if (e.type == EventType::F_ADDR_SIZE && e.streamID == STREAM_A) {
            found = true;
            break;
        }
    }
    EXPECT_TRUE(found)
        << "F_ADDR_SIZE event (0x11) must be generated after an address size fault";
}

// ── Test 6: Other stream/PASID not affected ───────────────────────────────

// Stream B (with default 52-bit) must be unaffected when Stream A is 32-bit.
TEST_F(AddrSizeFaultTest, OtherStream_NotAffectedByAddressSizeLimit) {
    configureStream(STREAM_A, PASID_0);
    configureStream(STREAM_B, PASID_0);

    // Map large IOVA in both streams
    IOVA large_iova = JUST_ABOVE_32BIT;
    mapPage(STREAM_A, PASID_0, large_iova);
    mapPage(STREAM_B, PASID_0, large_iova);

    // Restrict only Stream A to 32-bit
    ASSERT_TRUE(smmu->setStreamInputAddressSize(STREAM_A, PASID_0, 32).isOk());

    // Stream A must fault
    TranslationResult rA = translate(STREAM_A, PASID_0, large_iova);
    EXPECT_TRUE(rA.isError())
        << "Stream A (32-bit limit) must fault on large IOVA";
    EXPECT_EQ(rA.getError(), SMMUError::InvalidAddress);

    // Stream B (default 52-bit) must succeed
    TranslationResult rB = translate(STREAM_B, PASID_0, large_iova);
    EXPECT_TRUE(rB.isOk())
        << "Stream B (default 52-bit) must still succeed with the same IOVA";
}

// ── Test 7: Different PASIDs are independent ──────────────────────────────

// Only the restricted PASID faults; the other PASID on the same stream succeeds.
TEST_F(AddrSizeFaultTest, OtherPASID_NotAffectedByAddressSizeLimit) {
    configureStream(STREAM_A, PASID_0);
    ASSERT_TRUE(smmu->createStreamPASID(STREAM_A, PASID_1).isOk());

    IOVA large_iova = JUST_ABOVE_32BIT;
    mapPage(STREAM_A, PASID_0, large_iova);
    mapPage(STREAM_A, PASID_1, large_iova);

    // Restrict only PASID_0 to 32-bit
    ASSERT_TRUE(smmu->setStreamInputAddressSize(STREAM_A, PASID_0, 32).isOk());

    // PASID_0 must fault
    TranslationResult r0 = translate(STREAM_A, PASID_0, large_iova);
    EXPECT_TRUE(r0.isError()) << "PASID_0 (32-bit) must fault";
    EXPECT_EQ(r0.getError(), SMMUError::InvalidAddress);

    // PASID_1 (default 52-bit) must succeed
    TranslationResult r1 = translate(STREAM_A, PASID_1, large_iova);
    EXPECT_TRUE(r1.isOk()) << "PASID_1 (default 52-bit) must succeed";
}

// ── Test 8: exact boundary — one page before the limit succeeds ──────────

TEST_F(AddrSizeFaultTest, Set32Bit_ExactBoundaryMinus1_Succeeds) {
    configureStream(STREAM_A, PASID_0);

    // 2^32 - 1 page (last page within 32-bit space)
    IOVA last_valid_page = (JUST_ABOVE_32BIT - 0x1000ULL);  // 0xFFFF_F000
    mapPage(STREAM_A, PASID_0, last_valid_page);
    ASSERT_TRUE(smmu->setStreamInputAddressSize(STREAM_A, PASID_0, 32).isOk());

    TranslationResult r = translate(STREAM_A, PASID_0, last_valid_page);
    EXPECT_TRUE(r.isOk())
        << "Last page within 32-bit limit must succeed";
}

// ── Test 9: setStreamInputAddressSize validates bits ─────────────────────

// Bits below 32 or above 52 are invalid per ARM spec.
TEST_F(AddrSizeFaultTest, SetInputAddressSize_InvalidBits_ReturnsError) {
    configureStream(STREAM_A, PASID_0);

    // Too small
    EXPECT_TRUE(smmu->setStreamInputAddressSize(STREAM_A, PASID_0, 24).isError())
        << "inputAddressSize < 32 must be rejected";

    // Too large
    EXPECT_TRUE(smmu->setStreamInputAddressSize(STREAM_A, PASID_0, 56).isError())
        << "inputAddressSize > 52 must be rejected";
}

} // namespace test
} // namespace smmu
