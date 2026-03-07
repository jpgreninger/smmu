// ARM SMMU v3 BUG-CPP-NEW-6: Dirty bit skipped for Privileged/Execute write variants
// Copyright (c) 2024 John Greninger
//
// TDD failing test — written BEFORE the fix.
// ARM IHI0070G.b §3.13: Hardware dirty-bit tracking (hd=true) must apply to ALL
// write access types, including WritePrivileged and ReadWritePrivileged.
// The bug: address_space.cpp updateAccessFlags() line ~137 only tested
// AccessType::Write and AccessType::ReadWrite, missing WritePrivileged and
// ReadWritePrivileged.

#include <gtest/gtest.h>
#include "smmu/address_space.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

class DirtyBitPrivilegedTest : public ::testing::Test {
protected:
    void SetUp() override {
        as = std::unique_ptr<AddressSpace>(new AddressSpace());
        iova = 0x10000ULL;
        pa   = 0x80000000ULL;
        // Map a page with read+write permissions.
        PagePermissions perms(true, true, false);
        as->mapPage(iova, pa, perms);
    }

    std::unique_ptr<AddressSpace> as;
    IOVA iova;
    PA   pa;
};

// Baseline: existing Write variant must still set dirty bit (regression guard).
TEST_F(DirtyBitPrivilegedTest, Write_SetsDirtyBit) {
    bool changed = as->updateAccessFlags(iova, true, true, AccessType::Write);
    EXPECT_TRUE(changed) << "updateAccessFlags should return true when dirty bit is newly set";
    EXPECT_TRUE(as->getPageDirty(iova)) << "Dirty bit must be set after Write";
}

// Baseline: ReadWrite must still set dirty bit (regression guard).
TEST_F(DirtyBitPrivilegedTest, ReadWrite_SetsDirtyBit) {
    bool changed = as->updateAccessFlags(iova, true, true, AccessType::ReadWrite);
    EXPECT_TRUE(changed);
    EXPECT_TRUE(as->getPageDirty(iova)) << "Dirty bit must be set after ReadWrite";
}

// BUG-CPP-NEW-6: WritePrivileged must set dirty bit — fails before fix.
TEST_F(DirtyBitPrivilegedTest, WritePrivileged_SetsDirtyBit) {
    bool changed = as->updateAccessFlags(iova, true, true, AccessType::WritePrivileged);
    EXPECT_TRUE(changed) << "updateAccessFlags should report a change for WritePrivileged";
    EXPECT_TRUE(as->getPageDirty(iova))
        << "Dirty bit must be set after WritePrivileged (BUG-CPP-NEW-6)";
}

// BUG-CPP-NEW-6: ReadWritePrivileged must set dirty bit — fails before fix.
TEST_F(DirtyBitPrivilegedTest, ReadWritePrivileged_SetsDirtyBit) {
    bool changed = as->updateAccessFlags(iova, true, true, AccessType::ReadWritePrivileged);
    EXPECT_TRUE(changed) << "updateAccessFlags should report a change for ReadWritePrivileged";
    EXPECT_TRUE(as->getPageDirty(iova))
        << "Dirty bit must be set after ReadWritePrivileged (BUG-CPP-NEW-6)";
}

// Execute must NOT set dirty bit (regression guard).
TEST_F(DirtyBitPrivilegedTest, Execute_DoesNotSetDirtyBit) {
    // Map execute-permitted page separately
    AddressSpace exas;
    IOVA exiova = 0x20000ULL;
    exas.mapPage(exiova, 0x90000000ULL, PagePermissions(true, false, true));
    bool changed = exas.updateAccessFlags(exiova, true, true, AccessType::Execute);
    // AF should be set (ha=true), but dirty must not be set
    EXPECT_FALSE(exas.getPageDirty(exiova)) << "Dirty bit must NOT be set after Execute";
    (void)changed;
}

// ExecutePrivileged must NOT set dirty bit.
TEST_F(DirtyBitPrivilegedTest, ExecutePrivileged_DoesNotSetDirtyBit) {
    AddressSpace exas;
    IOVA exiova = 0x20000ULL;
    exas.mapPage(exiova, 0x90000000ULL, PagePermissions(true, false, true));
    bool changed = exas.updateAccessFlags(exiova, true, true, AccessType::ExecutePrivileged);
    EXPECT_FALSE(exas.getPageDirty(exiova)) << "Dirty bit must NOT be set after ExecutePrivileged";
    (void)changed;
}

} // namespace test
} // namespace smmu
