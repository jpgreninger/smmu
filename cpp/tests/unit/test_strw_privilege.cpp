// TDD: Tests for GAP-2 (STE.STRW privilege check suppression)
// These tests MUST FAIL before implementation.
#include <gtest/gtest.h>
#include "smmu/types.h"
#include "smmu/stream_context.h"
#include "smmu/address_space.h"

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
