// ARM SMMU v3 FINDING-NEW-21, NEW-22, NEW-23 spec tests
// Copyright (c) 2024 John Greninger
//
// TDD tests for:
// - FINDING-NEW-21: ATC_INVALIDATE_COMPLETION only generated for CMD_ATC_INV (§4.5.1)
// - FINDING-NEW-22: GERROR_CMDQ_ABT_ERR set (not F_TLB_CONFLICT) when queue is full (§6.3.17)
// - FINDING-NEW-23: F_PERMISSION event generated on TLB cache-hit permission fault (§7.3.16)

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// ─── Test fixtures ────────────────────────────────────────────────────────────

static constexpr StreamID N21_STREAM = 0xA0;
static constexpr StreamID N22_STREAM = 0xA1;
static constexpr StreamID N23_STREAM = 0xA2;
static constexpr PASID    TEST_PASID = 0;
static constexpr IOVA     TEST_IOVA  = 0x4000ULL;
static constexpr PA       TEST_PA    = 0x9000'0000ULL;

static void basicStream(SMMU& smmu, StreamID sid, bool stage1 = true) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = stage1;
    cfg.stage2Enabled = false;
    cfg.faultMode = FaultMode::Terminate;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, TEST_PASID).isOk());
}

// helper: count events of a given type in the queue
static int countEvents(SMMU& smmu, EventType type) {
    int n = 0;
    for (const auto& ev : smmu.getEventQueue()) {
        if (ev.type == type) {
            ++n;
        }
    }
    return n;
}

// ─── FINDING-NEW-21: ATC_INVALIDATE_COMPLETION must ONLY be generated for CMD_ATC_INV ───

// A CMD_CFGI_STE command must NOT generate ATC_INVALIDATE_COMPLETION (0xE1).
TEST(New21Spec, CFGI_STE_DoesNotEmitATC_INV_Completion) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();
    basicStream(*smmu, N21_STREAM);

    CommandEntry cmd;
    cmd.type = CommandType::CFGI_STE;
    cmd.streamID = N21_STREAM;
    ASSERT_TRUE(smmu->submitCommand(cmd).isOk());
    smmu->processCommandQueue();

    EXPECT_EQ(countEvents(*smmu, EventType::ATC_INVALIDATE_COMPLETION), 0)
        << "CMD_CFGI_STE must not emit ATC_INVALIDATE_COMPLETION (§4.5.1)";
}

// A CMD_CFGI_ALL command must NOT generate ATC_INVALIDATE_COMPLETION.
TEST(New21Spec, CFGI_ALL_DoesNotEmitATC_INV_Completion) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    CommandEntry cmd;
    cmd.type = CommandType::CFGI_ALL;
    ASSERT_TRUE(smmu->submitCommand(cmd).isOk());
    smmu->processCommandQueue();

    EXPECT_EQ(countEvents(*smmu, EventType::ATC_INVALIDATE_COMPLETION), 0)
        << "CMD_CFGI_ALL must not emit ATC_INVALIDATE_COMPLETION (§4.5.1)";
}

// A CMD_TLBI_NH_ALL command must NOT generate ATC_INVALIDATE_COMPLETION.
TEST(New21Spec, TLBI_NH_ALL_DoesNotEmitATC_INV_Completion) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    CommandEntry cmd;
    cmd.type = CommandType::TLBI_NH_ALL;
    ASSERT_TRUE(smmu->submitCommand(cmd).isOk());
    smmu->processCommandQueue();

    EXPECT_EQ(countEvents(*smmu, EventType::ATC_INVALIDATE_COMPLETION), 0)
        << "CMD_TLBI_NH_ALL must not emit ATC_INVALIDATE_COMPLETION (§4.5.1)";
}

// CMD_ATC_INV MUST generate exactly one ATC_INVALIDATE_COMPLETION event.
TEST(New21Spec, ATC_INV_EmitsExactlyOneCompletion) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();
    basicStream(*smmu, N21_STREAM);

    CommandEntry cmd;
    cmd.type = CommandType::ATC_INV;
    cmd.streamID = N21_STREAM;
    cmd.pasid = TEST_PASID;
    cmd.startAddress = 0;
    cmd.endAddress   = 0xFFFF;
    ASSERT_TRUE(smmu->submitCommand(cmd).isOk());
    smmu->processCommandQueue();

    EXPECT_EQ(countEvents(*smmu, EventType::ATC_INVALIDATE_COMPLETION), 1)
        << "CMD_ATC_INV must emit exactly one ATC_INVALIDATE_COMPLETION (§4.5.1)";
}

// ─── FINDING-NEW-22 / BUG-03: Command queue full — no GERROR bit, no F_TLB_CONFLICT ───

// When the command queue is full, no F_TLB_CONFLICT event must be generated.
TEST(New22Spec, QueueFull_DoesNotEmit_F_TLB_CONFLICT) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    CommandEntry sync;
    sync.type = CommandType::SYNC;

    VoidResult r;
    for (int i = 0; i < 2000; ++i) {
        r = smmu->submitCommand(sync);
        if (r.isError()) {
            ASSERT_EQ(r.getError(), SMMUError::CommandQueueFull);
            break;
        }
    }
    ASSERT_TRUE(r.isError()) << "Queue must eventually become full";

    EXPECT_EQ(countEvents(*smmu, EventType::F_TLB_CONFLICT), 0)
        << "Queue full must not emit F_TLB_CONFLICT (§6.3.17)";
}

// BUG-03: command queue full must NOT set any GERROR bit.
// ARM §6.3.17: GERROR_MSI_CMDQ_ABT_ERR (bit[4]) is for CMD_SYNC MSI write
// aborts; GERROR_CMDQ_ERR (bit[0]) is for hardware command consumption errors.
// Queue fullness is a software producer concern — no GERROR bit is defined.
TEST(New22Spec, QueueFull_DoesNotSetAnyGerrorBit) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    CommandEntry sync;
    sync.type = CommandType::SYNC;

    VoidResult r;
    for (int i = 0; i < 2000; ++i) {
        r = smmu->submitCommand(sync);
        if (r.isError()) {
            break;
        }
    }
    ASSERT_TRUE(r.isError()) << "Queue must eventually become full";

    uint32_t gerror = smmu->getGerror();
    EXPECT_EQ(gerror, 0u)
        << "BUG-03: no GERROR bit must be set when command queue is full (§6.3.17, §3.5.1)";
}

// ─── FINDING-NEW-23: F_PERMISSION must be generated on TLB cache-hit permission fault ───

// After a read-only mapping is cached via a successful read, a write through the
// TLB fast-path must generate an F_PERMISSION event (§7.3.16) not just record
// a fault internally.
TEST(New23Spec, TLBCacheHit_PermissionFault_GeneratesF_PERMISSION) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();
    smmu->enableCaching(true);
    basicStream(*smmu, N23_STREAM);

    // Map read-only page
    PagePermissions roPerms(true, false, false);  // read=true, write=false, exec=false
    ASSERT_TRUE(smmu->mapPage(N23_STREAM, TEST_PASID, TEST_IOVA, TEST_PA, roPerms).isOk());

    // Warm the TLB with a successful read
    auto r1 = smmu->translate(N23_STREAM, TEST_PASID, TEST_IOVA,
                               AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(r1.isOk()) << "Initial read must succeed to populate TLB";

    // Clear event queue so we can observe only the new events
    smmu->clearEventQueue();

    // Now attempt a write — TLB hit but permission denied → must generate F_PERMISSION
    auto r2 = smmu->translate(N23_STREAM, TEST_PASID, TEST_IOVA,
                               AccessType::Write, SecurityState::NonSecure);
    EXPECT_TRUE(r2.isError())
        << "Write to read-only page must fail";

    // §7.3.16: F_PERMISSION (0x13) must appear in the event queue
    EXPECT_GT(countEvents(*smmu, EventType::F_PERMISSION), 0)
        << "TLB cache-hit permission fault must generate F_PERMISSION event (§7.3.16)";
}

// Execute-only scenario: a read after a cached exec-only mapping must fault
// with F_PERMISSION.
TEST(New23Spec, TLBCacheHit_ExecOnlyReadFault_GeneratesF_PERMISSION) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();
    smmu->enableCaching(true);
    basicStream(*smmu, N23_STREAM + 1);

    // Map exec-only page
    PagePermissions xPerms(false, false, true);  // read=false, write=false, exec=true
    ASSERT_TRUE(smmu->mapPage(N23_STREAM + 1, TEST_PASID, TEST_IOVA, TEST_PA, xPerms).isOk());

    // Warm TLB with an exec translation
    auto r1 = smmu->translate(N23_STREAM + 1, TEST_PASID, TEST_IOVA,
                               AccessType::Execute, SecurityState::NonSecure);
    ASSERT_TRUE(r1.isOk()) << "Initial exec must succeed";

    smmu->clearEventQueue();

    // Read attempt — permission denied via TLB fast-path
    auto r2 = smmu->translate(N23_STREAM + 1, TEST_PASID, TEST_IOVA,
                               AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(r2.isError());
    EXPECT_GT(countEvents(*smmu, EventType::F_PERMISSION), 0)
        << "Exec-only TLB hit read must generate F_PERMISSION (§7.3.16)";
}

} // namespace test
} // namespace smmu
