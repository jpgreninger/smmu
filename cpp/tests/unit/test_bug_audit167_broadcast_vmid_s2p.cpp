// TDD tests for BUG-AUDIT-167-CPP
//
// ARM §3.17 line 3466: "If the SMMU does not support stage 2 (IDR0.S2P==0),
// a VMID operand is treated as 0."
//
// BUG: receiveBroadcastTLBI() (smmu.cpp:5806-5820) passes the caller-supplied vmid
// directly to executeTLBInvalidationCommand() without substituting 0 when S2P==0.
// This means CMD_TLBI_NH_ALL(vmid=X) when X!=0 misses TLB entries tagged VMID=0
// instead of correctly treating X as 0 and evicting them.
//
// Test method:
//   1. Set S2P=false (S2 not supported).
//   2. Configure a stage-1-only EL1_EL0 stream (VMID=0 since no stage-2).
//   3. Translate to warm TLB — entry tagged VMID=0.
//   4. Unmap — subsequent walk will fault.
//   5. Issue receiveBroadcastTLBI(TLBI_NH_ALL, asid=0, vmid=0x99).
//   5a. BUG (before fix): vmid=0x99 passed to invalidation — does NOT match
//       VMID=0 entry → entry survives → re-translate hits cache → isOk() → FAIL.
//   5b. FIX (after fix): S2P==0 → substitute vmid=0 → matches VMID=0 entry →
//       eviction occurs → re-translate faults → !isOk() → PASS.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <cstdint>

using namespace smmu;

namespace {

static constexpr PASID  PASID_ZERO = 0;
static constexpr IOVA   TEST_IOVA  = 0x1000;
static constexpr PA     TEST_PA    = 0xC000;
static constexpr StreamID SID_EL1  = 0x10;
static constexpr StreamID SID_EL1B = 0x20;  // second stream for control test

class BugAudit167BroadcastVmidS2PTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    }

    SMMU smmu_;
};

static void setupStage1Stream(SMMU& smmu, StreamID sid, uint16_t vmid = 0) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.strw               = StreamWorld::EL1_EL0;
    cfg.asid               = 0x10;
    cfg.vmid               = vmid;
    cfg.securityState      = SecurityState::NonSecure;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, PASID_ZERO);
}

} // anonymous namespace

// ============================================================================
// Test 1: S2P==0 — TLBI_NH_ALL(vmid=0x99) must treat vmid as 0 and evict entry
// ============================================================================
//
// §3.17 line 3466: VMID operand treated as 0 when S2P=0.
// TLBI_NH_ALL(vmid=0x99) with S2P=0 must match and evict all VMID=0 entries.
// After fix: eviction occurs → re-translate faults.
// Before fix: eviction missed → re-translate hits → FAIL.
TEST_F(BugAudit167BroadcastVmidS2PTest, S2PZeroSubstitutesVmidZeroInBroadcastTLBI) {
    smmu_.setS2PSupported(false);

    setupStage1Stream(smmu_, SID_EL1);

    PagePermissions perms(true, true, false);
    smmu_.mapPage(SID_EL1, PASID_ZERO, TEST_IOVA, TEST_PA, perms);

    auto warm = smmu_.translate(SID_EL1, PASID_ZERO, TEST_IOVA,
                                AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(warm.isOk()) << "Initial translate must succeed to warm TLB.";

    smmu_.unmapPage(SID_EL1, PASID_ZERO, TEST_IOVA);

    // With S2P=0, vmid=0x99 must be substituted with 0, matching the VMID=0 entry.
    smmu_.receiveBroadcastTLBI(CommandType::TLBI_NH_ALL, /*asid=*/0, /*vmid=*/0x99);

    auto result = smmu_.translate(SID_EL1, PASID_ZERO, TEST_IOVA,
                                  AccessType::Read, SecurityState::NonSecure);

    EXPECT_FALSE(result.isOk())
        << "BUG-AUDIT-167-CPP: S2P==0 requires VMID operand treated as 0 (§3.17 line 3466). "
           "TLBI_NH_ALL(vmid=0x99) must evict VMID=0 entry — re-translate should fault.";
}

// ============================================================================
// Test 2: S2P==1 — TLBI_NH_ALL(vmid=0x99) must NOT evict VMID=0 entries
// ============================================================================
//
// When S2P=1, the VMID operand is used as-is. TLBI_NH_ALL(vmid=0x99) must NOT
// match entries tagged VMID=0 — those entries survive.
// This confirms the fix does not break the S2P=1 path.
TEST_F(BugAudit167BroadcastVmidS2PTest, S2PSupportedUsesActualVmid) {
    // S2P defaults to true; explicit to make intent clear
    smmu_.setS2PSupported(true);

    setupStage1Stream(smmu_, SID_EL1B);

    PagePermissions perms(true, true, false);
    smmu_.mapPage(SID_EL1B, PASID_ZERO, TEST_IOVA, TEST_PA, perms);

    auto warm = smmu_.translate(SID_EL1B, PASID_ZERO, TEST_IOVA,
                                AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(warm.isOk()) << "Initial translate must succeed to warm TLB.";

    smmu_.unmapPage(SID_EL1B, PASID_ZERO, TEST_IOVA);

    // S2P=1: vmid=0x99 is used as-is. Entry has VMID=0. TLBI should NOT evict it.
    smmu_.receiveBroadcastTLBI(CommandType::TLBI_NH_ALL, /*asid=*/0, /*vmid=*/0x99);

    auto result = smmu_.translate(SID_EL1B, PASID_ZERO, TEST_IOVA,
                                  AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk())
        << "BUG-AUDIT-167-CPP: With S2P=1, VMID=0x99 must NOT match VMID=0 entry. "
           "Re-translate should hit cache (entry not evicted).";
}
