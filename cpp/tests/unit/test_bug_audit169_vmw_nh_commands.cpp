// TDD tests for BUG-AUDIT-169-CPP
//
// ARM §3.17.6: VMW (VMID wildcard) field in CR0 specifies how many low bits of the
// VMID operand are ignored during VMID-targeted TLB invalidation.  VMW applies to
// ALL VMID-targeted commands, including TLBI_NH_ALL, TLBI_NH_VA, TLBI_NH_VAA,
// and TLBI_NH_ASID.
//
// BUG: smmu.cpp executeTLBInvalidationCommand() applies getVmidMask() only to
// TLBI_S12_VMALL and TLBI_S2_IPA.  The four NH commands use exact VMID matching
// (no wildcard), violating §3.17.6.
//
// Test method for TLBI_NH_ALL (representative; other commands follow same pattern):
//   1. Set CR0.VMW=1 (wildcard bit 0: VMIDs 0x00 and 0x01 are treated as identical).
//   2. Configure two EL1_EL0 streams: vmid=0x00 (base) and vmid=0x01 (wildcard sibling).
//   3. Translate both streams to warm TLB.
//   4. Unmap both pages.
//   5. Issue TLBI_NH_ALL(vmid=0x00).
//      Without fix: exact match — only VMID=0x00 evicted; VMID=0x01 entry survives.
//      With fix: wildcard match — both VMID=0x00 and VMID=0x01 evicted.
//   6. Re-translate vmid=0x01 stream — expects fault (evicted by wildcard).
//      Before fix: succeeds (cache hit) → FAIL.
//      After fix:  faults (cache miss) → PASS.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <cstdint>

using namespace smmu;

namespace {

static constexpr PASID  PASID_ZERO  = 0;
static constexpr IOVA   TEST_IOVA   = 0x1000;
static constexpr IOVA   TEST_IOVA2  = 0x2000;
static constexpr PA     TEST_PA     = 0xD000;
static constexpr PA     TEST_PA2    = 0xE000;

// VMIDs that differ only in bit 0 — identical under VMW=1.
static constexpr uint16_t VMID_BASE    = 0x00;
static constexpr uint16_t VMID_SIBLING = 0x01;
static constexpr uint16_t VMID_UNRELATED = 0x10;

static constexpr StreamID SID_BASE     = 0x10;
static constexpr StreamID SID_SIBLING  = 0x20;
static constexpr StreamID SID_UNREL    = 0x30;

// CR0_VMW_SHIFT from smmu.h — VMW is bits [8:6] of CR0.
// VMW=1: wildcard bit 0 of VMID (mask = 0xFFFE).
static constexpr uint32_t VMW1 = (1u << 6);  // CR0.VMW=1

static void setupStream(SMMU& smmu, StreamID sid, uint16_t vmid) {
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

class BugAudit169VmwNhCommandsTest : public ::testing::Test {
protected:
    void SetUp() override {
        // CR0.VMW=1 — wildcard bit 0 of VMID.
        smmu_.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | VMW1);
    }

    SMMU smmu_;
};

} // anonymous namespace

// ============================================================================
// Test 1: TLBI_NH_ALL with VMW=1 evicts wildcard-matching VMID entries
// ============================================================================
//
// §3.17.6: VMW=1 wildcards bit 0 of VMID.
// TLBI_NH_ALL(vmid=0x00) must evict VMID=0x01 entries (bit 0 wildcarded).
// After fix: VMID=0x01 entry evicted → fault.
// Before fix: VMID=0x01 entry not evicted (exact match only) → cache hit → FAIL.
TEST_F(BugAudit169VmwNhCommandsTest, TlbiNhAllAppliesVmwWildcard) {
    setupStream(smmu_, SID_SIBLING, VMID_SIBLING);

    PagePermissions perms(true, true, false);
    smmu_.mapPage(SID_SIBLING, PASID_ZERO, TEST_IOVA, TEST_PA, perms);

    auto warm = smmu_.translate(SID_SIBLING, PASID_ZERO, TEST_IOVA,
                                AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(warm.isOk()) << "Initial translate must succeed to warm TLB.";

    smmu_.unmapPage(SID_SIBLING, PASID_ZERO, TEST_IOVA);

    // TLBI_NH_ALL(vmid=0x00): with VMW=1, wildcard bit 0 → matches VMID=0x01.
    CommandEntry cmd;
    cmd.type = CommandType::TLBI_NH_ALL;
    cmd.vmid = VMID_BASE;
    smmu_.submitCommand(cmd);
    smmu_.processCommandQueue();

    auto result = smmu_.translate(SID_SIBLING, PASID_ZERO, TEST_IOVA,
                                  AccessType::Read, SecurityState::NonSecure);

    EXPECT_FALSE(result.isOk())
        << "BUG-AUDIT-169-CPP: TLBI_NH_ALL with VMW=1 must evict wildcard-matching "
           "VMID entries (§3.17.6). VMID=0x01 entry (bit-0 matches VMID=0x00) "
           "must be evicted — re-translate should fault.";
}

// ============================================================================
// Test 2: TLBI_NH_ASID with VMW=1 evicts wildcard-matching VMID entries
// ============================================================================
TEST_F(BugAudit169VmwNhCommandsTest, TlbiNhAsidAppliesVmwWildcard) {
    setupStream(smmu_, SID_SIBLING, VMID_SIBLING);

    PagePermissions perms(true, true, false);
    smmu_.mapPage(SID_SIBLING, PASID_ZERO, TEST_IOVA, TEST_PA, perms);

    auto warm = smmu_.translate(SID_SIBLING, PASID_ZERO, TEST_IOVA,
                                AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(warm.isOk()) << "Initial translate must succeed to warm TLB.";

    smmu_.unmapPage(SID_SIBLING, PASID_ZERO, TEST_IOVA);

    // TLBI_NH_ASID(vmid=0x00, asid=0x10): VMW=1 → bit-0 wildcarded → matches VMID=0x01.
    CommandEntry cmd;
    cmd.type = CommandType::TLBI_NH_ASID;
    cmd.vmid = VMID_BASE;
    cmd.asid = 0x10;  // matches the stream's asid
    smmu_.submitCommand(cmd);
    smmu_.processCommandQueue();

    auto result = smmu_.translate(SID_SIBLING, PASID_ZERO, TEST_IOVA,
                                  AccessType::Read, SecurityState::NonSecure);

    EXPECT_FALSE(result.isOk())
        << "BUG-AUDIT-169-CPP: TLBI_NH_ASID with VMW=1 must evict wildcard-matching "
           "VMID entries (§3.17.6). Re-translate should fault.";
}

// ============================================================================
// Test 3: VMW=0 (default exact match) — unrelated VMID NOT evicted
// ============================================================================
//
// With VMW=0 (default), TLBI_NH_ALL(vmid=0x00) uses exact matching.
// VMID=0x10 (unrelated) must NOT be evicted.
TEST_F(BugAudit169VmwNhCommandsTest, TlbiNhAllVmwZeroUsesExactMatch) {
    // Override with VMW=0 (no wildcarding) — need to recreate SMMU with no VMW
    SMMU smmu2;
    smmu2.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    setupStream(smmu2, SID_UNREL, VMID_UNRELATED);

    PagePermissions perms(true, true, false);
    smmu2.mapPage(SID_UNREL, PASID_ZERO, TEST_IOVA, TEST_PA, perms);

    auto warm = smmu2.translate(SID_UNREL, PASID_ZERO, TEST_IOVA,
                                AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(warm.isOk()) << "Initial translate must succeed to warm TLB.";

    smmu2.unmapPage(SID_UNREL, PASID_ZERO, TEST_IOVA);

    // TLBI_NH_ALL(vmid=0x00): VMW=0 → exact match → VMID=0x10 not evicted.
    CommandEntry cmd;
    cmd.type = CommandType::TLBI_NH_ALL;
    cmd.vmid = VMID_BASE;
    smmu2.submitCommand(cmd);
    smmu2.processCommandQueue();

    auto result = smmu2.translate(SID_UNREL, PASID_ZERO, TEST_IOVA,
                                  AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk())
        << "BUG-AUDIT-169-CPP: With VMW=0, TLBI_NH_ALL(vmid=0x00) must NOT evict "
           "VMID=0x10 entries (no wildcard). Re-translate should hit cache.";
}
