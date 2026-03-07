// BUG-CPP-DBGR-8: bulk mapPages path missing SecurityState (§3.10)
// TDD: failing test written BEFORE the fix.
// AddressSpace::mapPages() lacks a SecurityState parameter; all bulk-mapped
// pages silently use the default SecurityState (NonSecure).
// The fix adds a SecurityState parameter (with default = NonSecure for backward
// compatibility) and passes it through to mapPage() internally.

#include <gtest/gtest.h>
#include "smmu/address_space.h"
#include "smmu/types.h"
#include <utility>
#include <vector>

namespace smmu {
namespace test {

// -----------------------------------------------------------------------
// mapPages() with default (NonSecure) SecurityState:
// mapped pages must be translatable with NonSecure security state
// -----------------------------------------------------------------------
TEST(BulkMapSpec, MapPages_DefaultNonSecure_TranslatesWithNonSecure) {
    AddressSpace as;

    std::vector<std::pair<IOVA, PA>> mappings = {
        {0x1000ULL, 0xA000ULL},
        {0x2000ULL, 0xB000ULL},
    };
    PagePermissions perms(true, true, false);

    VoidResult result = as.mapPages(mappings, perms);
    ASSERT_TRUE(result.isOk()) << "mapPages() must succeed";

    // Translate with NonSecure — must succeed
    TranslationResult tr1 = as.translatePage(0x1000ULL, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(tr1.isOk()) << "Page mapped with default NS must translate with NonSecure";
    if (tr1.isOk()) {
        EXPECT_EQ(tr1.getValue().physicalAddress, 0xA000ULL);
    }

    TranslationResult tr2 = as.translatePage(0x2000ULL, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(tr2.isOk()) << "Second bulk-mapped page must also translate with NonSecure";
    if (tr2.isOk()) {
        EXPECT_EQ(tr2.getValue().physicalAddress, 0xB000ULL);
    }
}

// -----------------------------------------------------------------------
// mapPages() with SecurityState parameter:
// This tests the new signature mapPages(mappings, perms, securityState).
// After the fix, mapPages must accept and pass through the SecurityState.
// -----------------------------------------------------------------------
TEST(BulkMapSpec, MapPages_WithSecurityState_TranslatesCorrectly) {
    AddressSpace as;

    std::vector<std::pair<IOVA, PA>> mappings = {
        {0x3000ULL, 0xC000ULL},
        {0x4000ULL, 0xD000ULL},
    };
    PagePermissions perms(true, false, false);

    // Use the new SecurityState-aware overload
    VoidResult result = as.mapPages(mappings, perms, SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk()) << "mapPages() with explicit SecurityState must succeed";

    TranslationResult tr = as.translatePage(0x3000ULL, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(tr.isOk()) << "Bulk-mapped page must translate with matching NonSecure state";
    if (tr.isOk()) {
        EXPECT_EQ(tr.getValue().physicalAddress, 0xC000ULL);
    }
}

// -----------------------------------------------------------------------
// mapPages() with wrong count — basic validation still works
// -----------------------------------------------------------------------
TEST(BulkMapSpec, MapPages_EmptyMappings_Succeeds) {
    AddressSpace as;
    std::vector<std::pair<IOVA, PA>> empty;
    PagePermissions perms(true, false, false);
    VoidResult result = as.mapPages(empty, perms);
    EXPECT_TRUE(result.isOk()) << "mapPages() with empty list must succeed (no-op)";
}

// -----------------------------------------------------------------------
// mapPages() with invalid permissions must fail
// -----------------------------------------------------------------------
TEST(BulkMapSpec, MapPages_NoPermissions_Fails) {
    AddressSpace as;
    std::vector<std::pair<IOVA, PA>> mappings = {{0x5000ULL, 0xE000ULL}};
    PagePermissions noPerms(false, false, false);
    VoidResult result = as.mapPages(mappings, noPerms);
    EXPECT_TRUE(result.isError()) << "mapPages() with no permissions must fail";
}

} // namespace test
} // namespace smmu
