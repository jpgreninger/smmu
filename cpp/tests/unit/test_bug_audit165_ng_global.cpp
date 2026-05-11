// TDD tests for BUG-AUDIT-165-CPP
//
// ARM §3.17.1: TLB entries must track the nG (not-Global) bit from the page
// table descriptor.
//
//   Global entries    (nG=0, StreamConfig::globalTranslations==true):
//     Tagged with ASID=0.  ASID-targeted TLBIs must evict global entries
//     regardless of the ASID operand — global entries match all ASIDs.
//
//   Non-global entries (nG=1, StreamConfig::globalTranslations==false, DEFAULT):
//     ASID-scoped.  ASID-targeted TLBIs only evict entries whose ASID matches
//     the operand.
//
// BUG: TLBEntry had no nonGlobal field.  invalidateByVMIDAndASID(),
// invalidateByVMIDAndVAAndASID(), and invalidateByVMIDAndVARange() compared
// ASID operand only, so global entries were NOT evicted when the ASID operand
// did not match — a spec violation.
//
// Test pattern (TLBI_NH_ASID with mismatched ASID):
//   Stream A: asid=0x10, vmid=0x01, globalTranslations=true  (nG=0, global)
//   Stream B: asid=0x20, vmid=0x01, globalTranslations=false (nG=1, non-global)
//   Issue TLBI_NH_ASID(vmid=0x01, asid=0x99) — ASID does NOT match either stream.
//
//   Before fix: neither entry evicted (ASID mismatch for both) → re-translate
//               of stream A succeeds (stale cache hit) → WRONG.
//   After  fix: stream A's global entry (nG=0) IS evicted → re-translate faults.
//               stream B's non-global entry (nG=1) NOT evicted → re-translate
//               succeeds (cache hit still valid).

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <cstdint>

using namespace smmu;

namespace {

static constexpr PASID   PASID_ZERO   = 0;
static constexpr IOVA    IOVA_A       = 0x1000;
static constexpr IOVA    IOVA_B       = 0x2000;
static constexpr PA      PA_A         = 0xA000;
static constexpr PA      PA_B         = 0xB000;

// Stream A: global translations (nG=0), asid=0x10
static constexpr StreamID SID_A       = 0x10;
static constexpr uint16_t ASID_A      = 0x10;

// Stream B: non-global translations (nG=1, default), asid=0x20
static constexpr StreamID SID_B       = 0x20;
static constexpr uint16_t ASID_B      = 0x20;

// Both share VMID=0x01
static constexpr uint16_t VMID_SHARED = 0x01;

// ASID operand for TLBI — deliberately does not match ASID_A or ASID_B
static constexpr uint16_t ASID_MISMATCH = 0x99;

static void setupStream(SMMU& smmu, StreamID sid, uint16_t asid, uint16_t vmid,
                        bool globalTrans) {
    StreamConfig cfg;
    cfg.translationEnabled  = true;
    cfg.stage1Enabled       = true;
    cfg.stage2Enabled       = false;
    cfg.faultMode           = FaultMode::Terminate;
    cfg.strw                = StreamWorld::EL1_EL0;
    cfg.asid                = asid;
    cfg.vmid                = vmid;
    cfg.securityState       = SecurityState::NonSecure;
    cfg.globalTranslations  = globalTrans;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, PASID_ZERO);
}

class BugAudit165NgGlobalTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
        // Stream A: global translations (CD.nG=0)
        setupStream(smmu_, SID_A, ASID_A, VMID_SHARED, /*globalTranslations=*/true);
        // Stream B: non-global translations (CD.nG=1, default)
        setupStream(smmu_, SID_B, ASID_B, VMID_SHARED, /*globalTranslations=*/false);
    }

    SMMU smmu_;
};

} // anonymous namespace

// ============================================================================
// Test 1: TLBI_NH_ASID with mismatched ASID evicts global entries (nG=0)
// ============================================================================
//
// ARM §3.17.1: Global TLB entries (nG=0) are ASID-independent — they must be
// evicted by any ASID-targeted TLBI that matches the VMID, regardless of the
// ASID operand.
//
// Before fix: global entry for stream A survives → re-translate returns stale
//             hit → FAIL.
// After  fix: global entry for stream A is evicted → re-translate faults → PASS.
TEST_F(BugAudit165NgGlobalTest, TlbiNhAsidEvictsGlobalEntryOnAnyAsid) {
    PagePermissions perms(true, false, false);
    smmu_.mapPage(SID_A, PASID_ZERO, IOVA_A, PA_A, perms);

    // Warm the TLB for stream A.
    auto warm = smmu_.translate(SID_A, PASID_ZERO, IOVA_A,
                                AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(warm.isOk()) << "Initial translate for stream A must succeed.";

    // Unmap the page so the backing store no longer resolves this IOVA.
    smmu_.unmapPage(SID_A, PASID_ZERO, IOVA_A);

    // Issue TLBI_NH_ASID(vmid=0x01, asid=0x99).
    // VMID matches stream A.  ASID does NOT match stream A's asid (0x10).
    // With fix: global entry (nG=0) must be evicted — ASID operand irrelevant.
    CommandEntry cmd;
    cmd.type = CommandType::TLBI_NH_ASID;
    cmd.vmid = VMID_SHARED;
    cmd.asid = ASID_MISMATCH;
    smmu_.submitCommand(cmd);
    smmu_.processCommandQueue();

    // Re-translate stream A: backing store is unmapped → should fault.
    auto result = smmu_.translate(SID_A, PASID_ZERO, IOVA_A,
                                  AccessType::Read, SecurityState::NonSecure);

    EXPECT_FALSE(result.isOk())
        << "BUG-AUDIT-165-CPP: TLBI_NH_ASID must evict global entries (nG=0) "
           "regardless of ASID operand (§3.17.1). Stream A's global TLB entry "
           "was not evicted — re-translate returned a stale cache hit instead of "
           "faulting on the unmapped page.";
}

// ============================================================================
// Test 2: TLBI_NH_ASID with mismatched ASID preserves non-global entries (nG=1)
// ============================================================================
//
// ARM §3.17.1: Non-global TLB entries (nG=1) are ASID-scoped — they must NOT
// be evicted by a TLBI whose ASID operand does not match their ASID tag.
//
// Both before and after fix this should pass (non-global entry not evicted),
// but we include it to guard against regressions that over-evict.
TEST_F(BugAudit165NgGlobalTest, TlbiNhAsidPreservesNonGlobalEntryOnAnyAsid) {
    PagePermissions perms(true, false, false);
    smmu_.mapPage(SID_B, PASID_ZERO, IOVA_B, PA_B, perms);

    // Warm the TLB for stream B.
    auto warm = smmu_.translate(SID_B, PASID_ZERO, IOVA_B,
                                AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(warm.isOk()) << "Initial translate for stream B must succeed.";

    // Unmap the page so re-translate faults on a cache miss.
    smmu_.unmapPage(SID_B, PASID_ZERO, IOVA_B);

    // Issue TLBI_NH_ASID(vmid=0x01, asid=0x99).
    // VMID matches stream B.  ASID does NOT match stream B's asid (0x20).
    // Non-global entry (nG=1): must NOT be evicted by this ASID mismatch.
    CommandEntry cmd;
    cmd.type = CommandType::TLBI_NH_ASID;
    cmd.vmid = VMID_SHARED;
    cmd.asid = ASID_MISMATCH;
    smmu_.submitCommand(cmd);
    smmu_.processCommandQueue();

    // Re-translate stream B: TLB entry should still be present → succeeds
    // (page is unmapped in backing store, but the TLB entry is valid).
    auto result = smmu_.translate(SID_B, PASID_ZERO, IOVA_B,
                                  AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk())
        << "BUG-AUDIT-165-CPP regression: TLBI_NH_ASID with mismatched ASID must "
           "NOT evict non-global entries (nG=1, §3.17.1). Stream B's entry was "
           "incorrectly evicted — re-translate faulted on an unmapped page instead "
           "of hitting the valid TLB entry.";
}

// ============================================================================
// Test 3: TLBI_NH_ASID with matching ASID evicts non-global entries (nG=1)
// ============================================================================
//
// Sanity check: ASID-matched TLBI still evicts non-global entries.
TEST_F(BugAudit165NgGlobalTest, TlbiNhAsidWithMatchingAsidEvictsNonGlobalEntry) {
    PagePermissions perms(true, false, false);
    smmu_.mapPage(SID_B, PASID_ZERO, IOVA_B, PA_B, perms);

    auto warm = smmu_.translate(SID_B, PASID_ZERO, IOVA_B,
                                AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(warm.isOk()) << "Initial translate for stream B must succeed.";

    smmu_.unmapPage(SID_B, PASID_ZERO, IOVA_B);

    // TLBI_NH_ASID(vmid=0x01, asid=0x20) — ASID matches stream B's asid exactly.
    CommandEntry cmd;
    cmd.type = CommandType::TLBI_NH_ASID;
    cmd.vmid = VMID_SHARED;
    cmd.asid = ASID_B;
    smmu_.submitCommand(cmd);
    smmu_.processCommandQueue();

    auto result = smmu_.translate(SID_B, PASID_ZERO, IOVA_B,
                                  AccessType::Read, SecurityState::NonSecure);

    EXPECT_FALSE(result.isOk())
        << "Sanity: TLBI_NH_ASID with matching ASID (0x20) must evict non-global "
           "entry (nG=1). Re-translate should fault on unmapped page.";
}

// ============================================================================
// Test 4: Default StreamConfig uses non-global (nG=1) — backward compatibility
// ============================================================================
//
// Ensure that streams created without setting globalTranslations (default=false)
// behave as non-global: ASID-mismatched TLBI must NOT evict their entries.
TEST_F(BugAudit165NgGlobalTest, DefaultStreamIsNonGlobal) {
    // Use a fresh stream with default StreamConfig (globalTranslations not set).
    static constexpr StreamID SID_DEFAULT = 0x30;
    static constexpr uint16_t ASID_DEFAULT = 0x30;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.strw               = StreamWorld::EL1_EL0;
    cfg.asid               = ASID_DEFAULT;
    cfg.vmid               = VMID_SHARED;
    cfg.securityState      = SecurityState::NonSecure;
    // globalTranslations intentionally NOT set — must default to false (nG=1).
    smmu_.configureStream(SID_DEFAULT, cfg);
    smmu_.enableStream(SID_DEFAULT);
    smmu_.createStreamPASID(SID_DEFAULT, PASID_ZERO);

    static constexpr IOVA IOVA_DEF = 0x3000;
    static constexpr PA   PA_DEF   = 0xC000;
    PagePermissions perms(true, false, false);
    smmu_.mapPage(SID_DEFAULT, PASID_ZERO, IOVA_DEF, PA_DEF, perms);

    auto warm = smmu_.translate(SID_DEFAULT, PASID_ZERO, IOVA_DEF,
                                AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(warm.isOk()) << "Initial translate must succeed.";

    smmu_.unmapPage(SID_DEFAULT, PASID_ZERO, IOVA_DEF);

    // ASID mismatch — default stream is non-global, entry must survive.
    CommandEntry cmd;
    cmd.type = CommandType::TLBI_NH_ASID;
    cmd.vmid = VMID_SHARED;
    cmd.asid = ASID_MISMATCH;
    smmu_.submitCommand(cmd);
    smmu_.processCommandQueue();

    auto result = smmu_.translate(SID_DEFAULT, PASID_ZERO, IOVA_DEF,
                                  AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk())
        << "Default StreamConfig must use non-global (nG=1). ASID-mismatched TLBI "
           "must NOT evict the entry. Re-translate should hit the TLB.";
}
