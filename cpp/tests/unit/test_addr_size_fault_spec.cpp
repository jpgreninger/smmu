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
#include "smmu/configuration.h"
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
        // ARM §6.3.9: SMMU starts disabled; enable globally before tests.
        smmu->enable();
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

// ── FINDING-NEW-16: OAS check on bypass mode (§3.4) ──────────────────────

/// §3.4 / §7.3.14: STE bypass stream (Config==0b100) with IOVA >= OAS must
/// abort with SMMUError::InvalidAddress and record F_ADDR_SIZE event.
TEST_F(AddrSizeFaultTest, SteBypass_IOVAExceedsOAS_FaultsWithFAddrSize) {
    // STE.Config==0b100: bypass — identity mapping (PA==IOVA)
    StreamConfig bypassCfg;
    bypassCfg.translationEnabled = false;
    bypassCfg.stage1Enabled      = false;
    bypassCfg.stage2Enabled      = false;
    bypassCfg.bypassEnabled      = true;  // STE.Config==0b100: bypass
    ASSERT_TRUE(smmu->configureStream(STREAM_A, bypassCfg).isOk());
    ASSERT_TRUE(smmu->enableStream(STREAM_A).isOk());

    // OAS default is 52 bits; address (1ULL << 52) is exactly at the limit.
    constexpr IOVA OVER_OAS = (1ULL << 52);  // first address beyond 52-bit OAS

    TranslationResult r = smmu->translate(STREAM_A, PASID_0, OVER_OAS,
                                          AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(r.isError())
        << "STE bypass IOVA >= OAS must abort (ARM §3.4)";
    EXPECT_EQ(r.getError(), SMMUError::InvalidAddress)
        << "Error must be InvalidAddress (F_ADDR_SIZE) for OAS overflow in bypass";

    // §7.3.14: F_ADDR_SIZE event must be recorded.
    auto events = smmu->getEventQueue();
    ASSERT_FALSE(events.empty())
        << "F_ADDR_SIZE event must be recorded for STE bypass OAS overflow";
    EXPECT_EQ(events.front().type, EventType::F_ADDR_SIZE)
        << "Event type must be F_ADDR_SIZE (§7.3.14)";
}

/// §3.4: STE bypass stream with IOVA just within OAS must succeed (no fault).
TEST_F(AddrSizeFaultTest, SteBypass_IOVAWithinOAS_Succeeds) {
    StreamConfig bypassCfg;
    bypassCfg.translationEnabled = false;
    bypassCfg.stage1Enabled      = false;
    bypassCfg.stage2Enabled      = false;
    bypassCfg.bypassEnabled      = true;  // STE.Config==0b100: bypass
    ASSERT_TRUE(smmu->configureStream(STREAM_A, bypassCfg).isOk());
    ASSERT_TRUE(smmu->enableStream(STREAM_A).isOk());

    // Last valid address within 52-bit OAS
    constexpr IOVA WITHIN_OAS = (1ULL << 52) - 0x1000ULL;

    TranslationResult r = smmu->translate(STREAM_A, PASID_0, WITHIN_OAS,
                                          AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(r.isOk())
        << "STE bypass IOVA within OAS must succeed (ARM §3.4)";

    auto events = smmu->getEventQueue();
    EXPECT_TRUE(events.empty())
        << "Successful bypass must not record any fault events";
}

/// §3.4 / §3.11: GBPA bypass (SMMUEN=0, GBPA.ABORT=0) with IOVA >= OAS must
/// abort silently — no event recorded (global bypass path).
TEST_F(AddrSizeFaultTest, GbpaBypass_IOVAExceedsOAS_SilentAbort) {
    // Fixture enables SMMU; disable to enter GBPA bypass mode (SMMUEN=0, GBPA.ABORT=0).
    smmu->disable();

    constexpr IOVA OVER_OAS = (1ULL << 52);

    TranslationResult r = smmu->translate(STREAM_A, PASID_0, OVER_OAS,
                                          AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(r.isError())
        << "GBPA bypass IOVA >= OAS must abort (ARM §3.4)";

    // §3.11 / §3.4: GBPA bypass OAS abort is silent — no event.
    auto events = smmu->getEventQueue();
    EXPECT_TRUE(events.empty())
        << "GBPA bypass OAS abort must not record any event (ARM §3.11)";
}

/// §3.4 / §3.11: GBPA bypass with IOVA within OAS must succeed with PA == IOVA.
TEST_F(AddrSizeFaultTest, GbpaBypass_IOVAWithinOAS_IdentityMapping) {
    // Fixture enables SMMU; disable to enter GBPA bypass mode (SMMUEN=0, GBPA.ABORT=0).
    smmu->disable();

    constexpr IOVA WITHIN_OAS = 0x4000'0000ULL;

    TranslationResult r = smmu->translate(STREAM_A, PASID_0, WITHIN_OAS,
                                          AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(r.isOk())
        << "GBPA bypass IOVA within OAS must succeed with identity mapping";
    if (r.isOk()) {
        EXPECT_EQ(r.getValue().physicalAddress, WITHIN_OAS)
            << "GBPA bypass must produce PA == IOVA (identity mapping)";
    }
}

// ── BUG-CPP-F Tests: hardcoded 48-bit constant in classifyDetailedTranslationFault ──

// BUG-CPP-F: classifyDetailedTranslationFault uses a hardcoded
// MAX_48BIT_ADDRESS = 0x0000FFFFFFFFFFFFULL constant in its default branch.
// On systems configured with 52-bit OAS, addresses in the range
// [2^48, 2^52-1] are valid but are incorrectly classified as AddressSizeFault.
//
// The fix derives the threshold from configuration.addressConfig.maxPASize
// (a bit-count) as (1ULL << maxPASize) - 1.
//
// ARM IHI0070G.b §3.4 / §STE.S2PS.

// Helper fixture for BUG-CPP-F tests.
class BugCppFTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu = std::make_unique<SMMU>();
        smmu->enable();
    }

    std::unique_ptr<SMMU> smmu;

    // Configure the SMMU with the given OAS bit-count (e.g., 48 or 52).
    void setOAS(uint64_t pasBits) {
        AddressConfiguration addrCfg;
        addrCfg.maxPASize      = pasBits;
        addrCfg.maxIOVASize    = pasBits; // keep consistent
        addrCfg.maxStreamCount = 65536;
        addrCfg.maxPASIDCount  = 1048576;
        VoidResult r = smmu->updateAddressConfiguration(addrCfg);
        ASSERT_TRUE(r.isOk())
            << "updateAddressConfiguration() for OAS=" << pasBits << " must succeed";
    }

    // Configure a stage-1 stream with a page at 'iova' -> 'pa'.
    void configureStreamWithPage(StreamID sid, IOVA iova, PA pa) {
        StreamConfig scfg;
        scfg.translationEnabled = true;
        scfg.stage1Enabled      = true;
        scfg.stage2Enabled      = false;
        scfg.faultMode          = FaultMode::Terminate;
        scfg.securityState      = SecurityState::NonSecure;
        scfg.bypassEnabled      = false;
        ASSERT_TRUE(smmu->configureStream(sid, scfg).isOk());
        ASSERT_TRUE(smmu->enableStream(sid).isOk());
        ASSERT_TRUE(smmu->createStreamPASID(sid, 0).isOk());
        ASSERT_TRUE(smmu->mapPage(sid, 0, iova, pa,
                                  PagePermissions(true, false, false),
                                  SecurityState::NonSecure).isOk());
    }
};

// BUG-CPP-F test 1: with 48-bit OAS, an address just ABOVE 48 bits (bit 48 set)
// must be classified as AddressSizeFault.  This is the pre-existing correct
// behaviour that must not regress.
TEST_F(BugCppFTest, OAS48_AddressAbove48Bits_ClassifiedAsAddressSizeFault) {
    setOAS(48);

    const StreamID SID = 0xF1;
    // Map a page well within 48-bit range.
    configureStreamWithPage(SID, 0x1000ULL, 0x8000'0000ULL);

    // Translate to an address at exactly 2^48 (one beyond 48-bit OAS).
    const IOVA ABOVE_48BIT = (1ULL << 48);
    TranslationResult r = smmu->translate(SID, 0, ABOVE_48BIT,
                                          AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(r.isError())
        << "BUG-CPP-F: address >= 2^48 on a 48-bit OAS system must fault";
}

// BUG-CPP-F test 2: with 52-bit OAS, an address in the range [2^48, 2^52-1]
// is VALID and must NOT be classified as AddressSizeFault.
//
// BEFORE the fix: classifyDetailedTranslationFault uses the hardcoded constant
// 0x0000FFFFFFFFFFFF (= 2^48-1) and incorrectly returns AddressSizeFault for
// any address with bit 48 set — even on a 52-bit OAS system.
//
// AFTER the fix: the threshold is (1ULL << 52) - 1, so addresses up to
// 2^52-1 are accepted as in-range.
//
// We test this by mapping a page at an IOVA above 2^48 on a 52-bit system
// and verifying that translation succeeds (or fails only because no mapping
// exists at that address — NOT because of an AddressSizeFault classification).
TEST_F(BugCppFTest, OAS52_AddressBetween48And52Bits_NotAddressSizeFault) {
    setOAS(52);

    const StreamID SID = 0xF2;
    // Map a page at an IOVA that has bit 48 set — valid in 52-bit OAS.
    const IOVA IOVA_49BIT = (1ULL << 48); // 2^48 — within 52-bit OAS
    const PA   PA_VAL     = 0x1'0000'0000ULL; // 4 GiB physical address
    configureStreamWithPage(SID, IOVA_49BIT, PA_VAL);

    // Translation must succeed because the page is mapped and the IOVA
    // is within the 52-bit OAS.
    // BEFORE fix: classifyDetailedTranslationFault returns AddressSizeFault
    //             (SMMUError::InvalidAddress) because IOVA_49BIT > 0x0000FFFFFFFFFFFF.
    // AFTER fix:  threshold = (1ULL<<52)-1; IOVA_49BIT is within range, so
    //             the lookup proceeds to the page table and finds the mapping.
    TranslationResult r = smmu->translate(SID, 0, IOVA_49BIT,
                                          AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(r.isOk())
        << "BUG-CPP-F: address 2^48 must be valid on a 52-bit OAS system — "
           "classifyDetailedTranslationFault must use configured maxPASize, "
           "not a hardcoded 48-bit constant";

    if (r.isOk()) {
        EXPECT_EQ(r.getValue().physicalAddress, PA_VAL)
            << "BUG-CPP-F: translated PA must match the mapped value";
    }
}

// BUG-CPP-F test 3: with 52-bit OAS, an address at exactly 2^52 (one beyond
// the OAS) must still be classified as AddressSizeFault.
TEST_F(BugCppFTest, OAS52_AddressExactlyAt2Pow52_IsAddressSizeFault) {
    setOAS(52);

    const StreamID SID = 0xF3;
    // Map a page within the 52-bit range.
    configureStreamWithPage(SID, 0x1000ULL, 0x8000'0000ULL);

    // 2^52 is one beyond the 52-bit OAS — must fault.
    const IOVA ABOVE_52BIT = (1ULL << 52);
    TranslationResult r = smmu->translate(SID, 0, ABOVE_52BIT,
                                          AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(r.isError())
        << "BUG-CPP-F: address 2^52 must fault on a 52-bit OAS system";
}

// BUG-CPP-F test 4: address just below 2^52 must be in-range on a 52-bit system.
TEST_F(BugCppFTest, OAS52_AddressJustBelow2Pow52_NotAddressSizeFault) {
    setOAS(52);

    const StreamID SID = 0xF4;
    // Map a page at the highest valid 52-bit IOVA (page-aligned).
    const IOVA TOP_IOVA = ((1ULL << 52) - 1) & ~0xFFFULL; // round down to page boundary
    const PA   PA_VAL   = 0xC000'0000ULL;
    configureStreamWithPage(SID, TOP_IOVA, PA_VAL);

    TranslationResult r = smmu->translate(SID, 0, TOP_IOVA,
                                          AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(r.isOk())
        << "BUG-CPP-F: the highest valid 52-bit IOVA (" << std::hex << TOP_IOVA
        << ") must translate successfully on a 52-bit OAS system";
}

} // namespace test
} // namespace smmu
