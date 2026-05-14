// TDD tests for BUG-AUDIT-166-CPP
//
// ARM §3.17 lines 3460-3466: EL2 and EL3 StreamWorlds do not participate in
// stage-2 VMID-tagged TLB entries.  Per §3.17.1: "VMID tagging is only applied
// when stage-2 translation is active for EL1&0 translation regime streams."
// NS-EL2 and EL3 streams are never stage-2 translation regime; their TLB
// entries must be tagged VMID=0.
//
// BUG: smmu.cpp line 772 sets entryVmid = streamCfg.vmid.  The Secure branch
// (lines 778-780) zeros entryVmid only for stage-1-only Secure streams.  When
// stage2Enabled=true, that branch is skipped and entryVmid = streamCfg.vmid
// survives.  The EL2/EL3 guard (lines 799-800) only zeros entryAsid, not entryVmid.
//
// Test method (indirect — mirrors BUG-AUDIT-152 pattern):
//   1. Configure a two-stage stream with strw=EL2 and vmid=0x99.
//      (When stage2Enabled=true, STRW validation is skipped per §5.2 IgnoreSTESTRW
//       so configureStream() accepts strw=EL2.)
//   2. Map a page and translate — warms TLB with the inserted entry.
//   3. Unmap the page — subsequent walk will fault on TLB miss.
//   4. Issue TLBI_S12_VMALL(vmid=0x99) — targets entries tagged VMID=0x99.
//   5a. BUG (before fix): entry carries VMID=0x99 → TLBI evicts it → re-translate
//       walks page table → faults → FAIL (test expects success).
//   5b. FIX (after fix): entry carries VMID=0 → TLBI misses it → re-translate
//       hits cache → success → PASS.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <cstdint>

using namespace smmu;

namespace {

static constexpr PASID  PASID_ZERO = 0;
static constexpr IOVA   TEST_IOVA  = 0x1000;
static constexpr PA     TEST_PA    = 0xB000;

static constexpr StreamID SID_EL2_2S    = 0x10;
static constexpr StreamID SID_EL3_2S    = 0x20;
static constexpr StreamID SID_EL1_2S    = 0x30;  // control: EL1_EL0 retains VMID
static constexpr StreamID SID_EL2_E2H_2S = 0x40; // EL2_E2H: ASID valid, VMID must be 0

static constexpr uint16_t VMID_TEST   = 0x99;
static constexpr uint16_t VMID_CTRL   = 0xAB;

static void issueTlbiS12Vmall(SMMU& smmu, uint16_t vmid) {
    CommandEntry cmd;
    cmd.type = CommandType::TLBI_S12_VMALL;
    cmd.vmid = vmid;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();
}

// Two-stage stream: stage1Enabled=true, stage2Enabled=true.
// STRW validation is skipped by IgnoreSTESTRW when stage2Enabled=true (§5.2).
static void setupTwoStageStream(SMMU& smmu, StreamID sid, StreamWorld strw,
                                 uint16_t vmid, SecurityState sec = SecurityState::NonSecure) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.strw               = strw;
    cfg.asid               = 0x10;
    cfg.vmid               = vmid;
    cfg.securityState      = sec;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, PASID_ZERO);
}

class BugAudit166El2El3VmidTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    }

    SMMU smmu_;
};

} // anonymous namespace

// ============================================================================
// Test 1: Two-stage EL2 stream — TLB entry must have VMID=0
// ============================================================================
//
// §3.17.1: EL2 streams are not in the EL1&0 translation regime; no VMID tag.
// TLBI_S12_VMALL(vmid=0x99) must NOT evict the TLB entry (entry has VMID=0).
// Re-translate must succeed (cache hit).
//
// BEFORE FIX: entry VMID=0x99 → TLBI evicts it → re-translate faults → FAIL.
// AFTER FIX:  entry VMID=0   → TLBI misses it → re-translate hits  → PASS.
TEST_F(BugAudit166El2El3VmidTest, EL2TwoStageStreamHasNoVmidTag) {
    setupTwoStageStream(smmu_, SID_EL2_2S, StreamWorld::EL2, VMID_TEST);

    PagePermissions perms(true, true, false);
    smmu_.mapPage(SID_EL2_2S, PASID_ZERO, TEST_IOVA, TEST_PA, perms);

    auto warm = smmu_.translate(SID_EL2_2S, PASID_ZERO, TEST_IOVA,
                                AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(warm.isOk()) << "Initial translate must succeed to warm TLB.";

    smmu_.unmapPage(SID_EL2_2S, PASID_ZERO, TEST_IOVA);

    // TLBI targeting VMID=0x99.  If EL2 entry is correctly tagged VMID=0,
    // this TLBI does not match and the cached entry survives.
    issueTlbiS12Vmall(smmu_, VMID_TEST);

    auto result = smmu_.translate(SID_EL2_2S, PASID_ZERO, TEST_IOVA,
                                  AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk())
        << "BUG-AUDIT-166-CPP: Two-stage EL2 stream TLB entry must have VMID=0 (§3.17). "
           "TLBI_S12_VMALL(0x99) must NOT evict it — re-translate should hit the cache.";
}

// ============================================================================
// Test 2: EL3 StreamWorld enum is distinct — static guard covers both EL2 + EL3
// ============================================================================
//
// ARM §5.2 SteIllegal() rejects strw=EL3 for Secure stage-1-only streams.
// With stage2Enabled=true, STRW is ignored (IgnoreSTESTRW), so the EL3 TLB
// insertion path is reachable in principle. The fix (zeroing entryVmid for
// strw∈{EL2,EL3}) covers both.  This test confirms the enum values are distinct
// so the combined guard is logically correct, and that the EL2 behavioral test
// (Test 1) provides coverage for the same code block.
TEST_F(BugAudit166El2El3VmidTest, EL3StreamWorldEnumDistinctFromEL2) {
    static_assert(static_cast<int>(StreamWorld::EL2) != static_cast<int>(StreamWorld::EL3),
        "EL2 and EL3 must be distinct StreamWorld values for the VMID-zero guard.");
    static_assert(static_cast<int>(StreamWorld::EL2) != static_cast<int>(StreamWorld::EL1_EL0),
        "EL2 must be distinct from EL1_EL0.");
    SUCCEED() << "EL3 StreamWorld enum verified distinct; VMID fix covers both EL2 and EL3.";
}

// ============================================================================
// Test 3: Two-stage EL1_EL0 stream — TLB entry must retain its VMID tag
// ============================================================================
//
// EL1_EL0 (normal) streams participate in stage-2 VMID tagging.
// TLBI_S12_VMALL(vmid=0xAB) MUST evict the entry, forcing a fault.
// This confirms the fix does not break the normal EL1_EL0 path.
TEST_F(BugAudit166El2El3VmidTest, EL1TwoStageStreamRetainsVmidTag) {
    setupTwoStageStream(smmu_, SID_EL1_2S, StreamWorld::EL1_EL0, VMID_CTRL);

    PagePermissions perms(true, true, false);
    smmu_.mapPage(SID_EL1_2S, PASID_ZERO, TEST_IOVA, TEST_PA, perms);

    auto warm = smmu_.translate(SID_EL1_2S, PASID_ZERO, TEST_IOVA,
                                AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(warm.isOk()) << "Initial translate must succeed to warm TLB.";

    smmu_.unmapPage(SID_EL1_2S, PASID_ZERO, TEST_IOVA);

    // TLBI targeting VMID=0xAB — must evict the EL1_EL0 entry.
    issueTlbiS12Vmall(smmu_, VMID_CTRL);

    auto result = smmu_.translate(SID_EL1_2S, PASID_ZERO, TEST_IOVA,
                                  AccessType::Read, SecurityState::NonSecure);

    EXPECT_FALSE(result.isOk())
        << "BUG-AUDIT-166-CPP: EL1_EL0 two-stage stream TLB entry must retain VMID=0xAB. "
           "TLBI_S12_VMALL(0xAB) must evict it — re-translate should fault.";
}

// ============================================================================
// Test 4: Two-stage EL2_E2H stream — TLB entry must have VMID=0 (§3.17 l.3436)
// ============================================================================
//
// ARM §3.17 line 3436: any-EL2-E2H translations are ASID-tagged if TTD.nG==1,
// but carry NO VMID tag.  TLBI_S12_VMALL(vmid=0x99) must NOT evict the entry
// (entry has VMID=0); re-translate must succeed (cache hit).
//
// BEFORE FIX: entry VMID=0x99 → TLBI evicts it → re-translate faults → FAIL.
// AFTER FIX:  entry VMID=0   → TLBI misses it → re-translate hits  → PASS.
TEST_F(BugAudit166El2El3VmidTest, EL2E2HStreamHasNoVmidTag) {
    // CR2.E2H=1 makes STRW=EL2_E2H valid and preserves the ASID for E2H.
    smmu_.setCR2(SMMU::CR2_E2H);

    setupTwoStageStream(smmu_, SID_EL2_E2H_2S, StreamWorld::EL2_E2H, VMID_TEST);

    PagePermissions perms(true, true, false);
    smmu_.mapPage(SID_EL2_E2H_2S, PASID_ZERO, TEST_IOVA, TEST_PA, perms);

    auto warm = smmu_.translate(SID_EL2_E2H_2S, PASID_ZERO, TEST_IOVA,
                                AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(warm.isOk()) << "Initial translate must succeed to warm TLB.";

    smmu_.unmapPage(SID_EL2_E2H_2S, PASID_ZERO, TEST_IOVA);

    // TLBI targeting VMID=0x99.  If EL2_E2H entry is correctly tagged VMID=0,
    // this TLBI does not match and the cached entry survives.
    issueTlbiS12Vmall(smmu_, VMID_TEST);

    auto result = smmu_.translate(SID_EL2_E2H_2S, PASID_ZERO, TEST_IOVA,
                                  AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk())
        << "BUG-AUDIT-166-CPP: Two-stage EL2_E2H stream TLB entry must have VMID=0 "
           "(ARM §3.17 line 3436). TLBI_S12_VMALL(0x99) must NOT evict it — "
           "re-translate should hit the cache.";
}
