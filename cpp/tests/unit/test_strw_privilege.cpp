// TDD: Tests for GAP-2 (STE.STRW privilege check suppression)
// These tests MUST FAIL before implementation.
#include <gtest/gtest.h>
#include "smmu/types.h"
#include "smmu/stream_context.h"
#include "smmu/address_space.h"
#include "smmu/smmu.h"

using namespace smmu;

// ===== GAP-2: PagePermissions.privilegedOnly =====

TEST(PagePermissionsPrivilegedTest, DefaultPrivilegedOnlyFalse) {
    PagePermissions perms;
    EXPECT_FALSE(perms.privilegedOnly);
}

TEST(PagePermissionsPrivilegedTest, ConstructorSetsPrivilegedOnly) {
    PagePermissions perms(true, false, false, true); // privilegedOnly=true
    EXPECT_TRUE(perms.privilegedOnly);
}

// ===== GAP-2: New AccessType variants =====

TEST(AccessTypePrivilegedTest, ReadPrivilegedExists) {
    AccessType at = AccessType::ReadPrivileged;
    // Must compile and be distinct from Read
    EXPECT_NE(at, AccessType::Read);
}

TEST(AccessTypePrivilegedTest, WritePrivilegedExists) {
    AccessType at = AccessType::WritePrivileged;
    EXPECT_NE(at, AccessType::Write);
}

TEST(AccessTypePrivilegedTest, ExecutePrivilegedExists) {
    AccessType at = AccessType::ExecutePrivileged;
    EXPECT_NE(at, AccessType::Execute);
}

TEST(AccessTypePrivilegedTest, ReadWritePrivilegedExists) {
    AccessType at = AccessType::ReadWritePrivileged;
    EXPECT_NE(at, AccessType::ReadWrite);
}

// ===== GAP-2: STRW=EL1/EL0 enforces privilegedOnly =====

TEST(StrwPrivilegeTest, El1El0UnprivAccessDeniedOnPrivPage) {
    StreamContext ctx;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.strw = StreamWorld::EL1_EL0; // Normal: privilege checks apply
    ctx.updateConfiguration(cfg);
    ctx.enableStream();

    auto as = std::make_shared<AddressSpace>();
    // Map page as privileged-only
    PagePermissions privPerms(true, false, false, true); // read=true, privilegedOnly=true
    as->mapPage(0x1000, 0x2000, privPerms);
    ctx.addPASID(0, as);
    ctx.setStage1Enabled(true);

    // Unprivileged read should be denied (privilegedOnly=true, STRW=EL1/EL0)
    TranslationResult result = ctx.translate(0, 0x1000, AccessType::Read);
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PagePermissionViolation);
}

TEST(StrwPrivilegeTest, El1El0PrivAccessAllowedOnPrivPage) {
    StreamContext ctx;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.strw = StreamWorld::EL1_EL0;
    ctx.updateConfiguration(cfg);
    ctx.enableStream();

    auto as = std::make_shared<AddressSpace>();
    PagePermissions privPerms(true, false, false, true); // privilegedOnly=true
    as->mapPage(0x1000, 0x2000, privPerms);
    ctx.addPASID(0, as);
    ctx.setStage1Enabled(true);

    // Privileged read should succeed
    TranslationResult result = ctx.translate(0, 0x1000, AccessType::ReadPrivileged);
    EXPECT_TRUE(result.isOk());
}

// ===== GAP-2: STRW=EL2 suppresses privilegedOnly =====

TEST(StrwPrivilegeTest, El2SuppressesPrivilegedOnly) {
    StreamContext ctx;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.strw = StreamWorld::EL2; // EL2: privilege checks suppressed
    ctx.updateConfiguration(cfg);
    ctx.enableStream();

    auto as = std::make_shared<AddressSpace>();
    PagePermissions privPerms(true, false, false, true); // privilegedOnly=true
    as->mapPage(0x1000, 0x2000, privPerms);
    ctx.addPASID(0, as);
    ctx.setStage1Enabled(true);

    // Unprivileged read should succeed on EL2 stream (STRW suppresses privilege check)
    TranslationResult result = ctx.translate(0, 0x1000, AccessType::Read);
    EXPECT_TRUE(result.isOk());
}

TEST(StrwPrivilegeTest, El3SuppressesPrivilegedOnly) {
    StreamContext ctx;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.strw = StreamWorld::EL3; // EL3: privilege checks suppressed
    ctx.updateConfiguration(cfg);
    ctx.enableStream();

    auto as = std::make_shared<AddressSpace>();
    PagePermissions privPerms(true, false, false, true); // privilegedOnly=true
    as->mapPage(0x1000, 0x2000, privPerms);
    ctx.addPASID(0, as);
    ctx.setStage1Enabled(true);

    TranslationResult result = ctx.translate(0, 0x1000, AccessType::Read);
    EXPECT_TRUE(result.isOk());
}

TEST(StrwPrivilegeTest, El2E2hDoesNotSuppressPrivilege) {
    // EL2_E2H (VHE) uses normal privilege checking
    StreamContext ctx;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.strw = StreamWorld::EL2_E2H;
    ctx.updateConfiguration(cfg);
    ctx.enableStream();

    auto as = std::make_shared<AddressSpace>();
    PagePermissions privPerms(true, false, false, true);
    as->mapPage(0x1000, 0x2000, privPerms);
    ctx.addPASID(0, as);
    ctx.setStage1Enabled(true);

    // EL2_E2H: privilege checks apply, unprivileged should be denied
    TranslationResult result = ctx.translate(0, 0x1000, AccessType::Read);
    EXPECT_TRUE(result.isError());
}

TEST(StrwPrivilegeTest, NormalPageAllowedUnprivilegedAccess) {
    StreamContext ctx;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.strw = StreamWorld::EL1_EL0;
    ctx.updateConfiguration(cfg);
    ctx.enableStream();

    auto as = std::make_shared<AddressSpace>();
    PagePermissions normalPerms(true, false, false, false); // privilegedOnly=false
    as->mapPage(0x1000, 0x2000, normalPerms);
    ctx.addPASID(0, as);
    ctx.setStage1Enabled(true);

    // Normal page: unprivileged read should succeed
    TranslationResult result = ctx.translate(0, 0x1000, AccessType::Read);
    EXPECT_TRUE(result.isOk());
}

// ===== GAP-A: TLB fast-path STRW-aware privilege promotion =====
//
// The Bug 1 fix added a STRW-aware re-check in smmu.cpp after the lockless
// early TLB probe.  The early probe uses the raw AccessType and always fails
// validateAccessPermissions() for privilegedOnly pages (since the access type
// is e.g. Read, not ReadPrivileged).  After the StreamContext is fetched the
// fix promotes the AccessType to the Privileged variant for EL2/EL3 streams
// and performs a second TLB lookup.  These tests exercise that code path by
// priming the TLB on the first translate() call (slow path) and confirming
// that the second translate() call still succeeds (fast path) for EL2/EL3.
// EL2_E2H (VHE) must NOT be promoted, so the first translate() call should
// already fail for VHE.
//
// ARM §3.3.4 / §13.4.1: EL2 and EL3 treat AP[1] as 1 (always privileged).
// ARM §D5.1.3: VHE (EL2_E2H) retains normal privilege separation.

// Helper: configure a full SMMU instance with an EL2 or EL3 stream, enable
// the SMMU and TLB caching, map a privilegedOnly page, and return the SMMU.
// Callers own the returned object.
static std::unique_ptr<SMMU> makeSmmuWithStrwStream(
        StreamID sid, StreamWorld strw, PASID pasid,
        IOVA iova, PA pa) {
    std::unique_ptr<SMMU> smmu(new SMMU());
    smmu->enable();
    smmu->enableCaching(true);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.strw               = strw;
    smmu->configureStream(sid, cfg);
    smmu->enableStream(sid);
    smmu->createStreamPASID(sid, pasid);

    // Map the page as privilegedOnly=true so that an unprivileged AccessType
    // (Read) normally fails validateAccessPermissions().
    PagePermissions privPerms(true, false, false, true); // read=true, privilegedOnly=true
    smmu->mapPage(sid, pasid, iova, pa, privPerms);

    return smmu;
}

// Test 1 (GAP-A):
// EL2 stream with a privilegedOnly page.
// First translate() — slow path — should succeed (STRW=EL2 suppresses priv check).
// Second translate() same IOVA — should hit the STRW-aware TLB fast-path and
// still succeed, demonstrating Bug 1 fix is active in the re-check branch.
// The cache-hit counter must increase on the second call.
TEST(TlbFastPathPrivilegeTest, El2TlbFastPathPrivilegePromotion) {
    static constexpr StreamID SID   = 0xEA20;
    static constexpr PASID    PID   = 0;
    static constexpr IOVA     IOVA_VAL = 0x3000ULL;
    static constexpr PA       PA_VAL   = 0x8000'1000ULL;

    std::unique_ptr<SMMU> smmu = makeSmmuWithStrwStream(
            SID, StreamWorld::EL2, PID, IOVA_VAL, PA_VAL);

    smmu->resetStatistics();

    // First translate: slow path; STRW=EL2 converts Read -> ReadPrivileged
    // inside StreamContext so the page-table walk allows the access.
    TranslationResult first = smmu->translate(SID, PID, IOVA_VAL, AccessType::Read);
    ASSERT_TRUE(first.isOk())
        << "First translate (slow path, EL2 stream, privilegedOnly page) must succeed";

    uint64_t hitsAfterFirst = smmu->getCacheHitCount();

    // Second translate same IOVA: the STRW-aware re-check in translateUnlocked()
    // should find the TLB entry, promote Read -> ReadPrivileged, pass
    // validateAccessPermissions(), and return success from the fast path.
    TranslationResult second = smmu->translate(SID, PID, IOVA_VAL, AccessType::Read);
    EXPECT_TRUE(second.isOk())
        << "Second translate (TLB fast-path, EL2 stream, privilegedOnly page) must succeed";

    // The cache-hit counter must have increased, confirming the second call
    // was served from the TLB rather than the page-table walk.
    uint64_t hitsAfterSecond = smmu->getCacheHitCount();
    EXPECT_GT(hitsAfterSecond, hitsAfterFirst)
        << "Cache-hit counter must increase on second translate, confirming TLB fast-path";

    // Physical address must be identical on both calls.
    EXPECT_EQ(first.getValue().physicalAddress, second.getValue().physicalAddress)
        << "Both translations must resolve to the same physical address";
}

// Test 2 (GAP-A):
// EL2_E2H (VHE) stream with a privilegedOnly page.
// EL2_E2H must NOT suppress privilege checks (ARM §D5.1.3).  The Bug 1 fix
// explicitly excludes EL2_E2H from the promotion branch.  Therefore even the
// first translate() call must fail with a permission error, ensuring VHE is
// not accidentally folded into the EL2/EL3 suppression logic.
TEST(TlbFastPathPrivilegeTest, El2E2HTlbFastPathNoPrivilegeSuppression) {
    static constexpr StreamID SID    = 0xEA21;
    static constexpr PASID    PID    = 0;
    static constexpr IOVA     IOVA_VAL = 0x4000ULL;
    static constexpr PA       PA_VAL   = 0x8000'2000ULL;

    std::unique_ptr<SMMU> smmu = makeSmmuWithStrwStream(
            SID, StreamWorld::EL2_E2H, PID, IOVA_VAL, PA_VAL);

    // For EL2_E2H the AccessType::Read is not promoted; privilegedOnly=true
    // means the permission check rejects the access.
    TranslationResult first = smmu->translate(SID, PID, IOVA_VAL, AccessType::Read);
    EXPECT_TRUE(first.isError())
        << "EL2_E2H stream must NOT suppress privilege check; Read on privilegedOnly page must fail";
    EXPECT_EQ(first.getError(), SMMUError::PagePermissionViolation)
        << "Expected PagePermissionViolation for EL2_E2H stream on privilegedOnly page";
}
