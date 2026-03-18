// Tests for bugs found in newBugs17Mar2026_9pm.md
// TDD: tests written before fixes, then verified green after.
//
// Bug 3: setStrtabLog2Size() accepts values > 24 — max valid LOG2SIZE is 24 per ARM §6.3.25.
// Bug 6: ReadExecute/WriteExecute missing from C++ AccessType enum and generateEvent() switch.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"

using namespace smmu;

// ===== Bug 3: LOG2SIZE clamped to 24 =====

TEST(StrtabLog2SizeTest, Log2SizeClampedTo24WhenPassedMax32) {
    SMMU smmu;
    smmu.setStrtabLog2Size(32);
    // Bug 3 fix: values > 24 are architecturally undefined; must be clamped to 24.
    EXPECT_EQ(smmu.getStrtabLog2Size(), 24u);
}

TEST(StrtabLog2SizeTest, Log2Size25ClampedTo24) {
    SMMU smmu;
    smmu.setStrtabLog2Size(25);
    EXPECT_EQ(smmu.getStrtabLog2Size(), 24u);
}

TEST(StrtabLog2SizeTest, Log2Size24Accepted) {
    SMMU smmu;
    smmu.setStrtabLog2Size(24);
    EXPECT_EQ(smmu.getStrtabLog2Size(), 24u);
}

TEST(StrtabLog2SizeTest, Log2Size0Accepted) {
    SMMU smmu;
    smmu.setStrtabLog2Size(0);
    EXPECT_EQ(smmu.getStrtabLog2Size(), 0u);
}

TEST(StrtabLog2SizeTest, Log2Size16Accepted) {
    SMMU smmu;
    smmu.setStrtabLog2Size(16);
    EXPECT_EQ(smmu.getStrtabLog2Size(), 16u);
}

// ===== Bug 6: ReadExecute/WriteExecute EventEntry wire-format =====

// Helper: set up an enabled SMMU with a stage-1-only stream that has no
// address space mapped, then translate to force an F_TRANSLATION fault.
// The returned EventEntry captures the rnw/ind/pnu wire-format fields.
static EventEntry getEventForAccessType(AccessType at) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
    const StreamID sid = 0xE0u;
    const PASID pid = 0u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.stage2Enabled = false;
    cfg.faultMode = FaultMode::Terminate;
    cfg.t0sz = 0; // no VA range restriction
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, pid);
    // No page mapped → F_TRANSLATION fault generated.
    smmu.translate(sid, pid, 0x1000u, at);
    auto events = smmu.getEventQueue();
    if (events.empty()) {
        return EventEntry{};
    }
    return events.front();
}

TEST(AccessTypeEventWireFormatTest, ReadExecuteRnwFalseIndTrue) {
    // ReadExecute: has execute component → ind=true; no write → rnw=false; unprivileged → pnu=false
    EventEntry ev = getEventForAccessType(AccessType::ReadExecute);
    EXPECT_FALSE(ev.rnw) << "ReadExecute: rnw must be false (no write component)";
    EXPECT_TRUE(ev.ind) << "ReadExecute: ind must be true (has execute component)";
    EXPECT_FALSE(ev.pnu) << "ReadExecute: pnu must be false (unprivileged)";
}

TEST(AccessTypeEventWireFormatTest, WriteExecuteRnwTrueIndTrue) {
    // WriteExecute: has write + execute → rnw=true, ind=true; unprivileged → pnu=false
    EventEntry ev = getEventForAccessType(AccessType::WriteExecute);
    EXPECT_TRUE(ev.rnw) << "WriteExecute: rnw must be true (has write component)";
    EXPECT_TRUE(ev.ind) << "WriteExecute: ind must be true (has execute component)";
    EXPECT_FALSE(ev.pnu) << "WriteExecute: pnu must be false (unprivileged)";
}

// Regression: verify existing access types remain correct after adding new ones
TEST(AccessTypeEventWireFormatTest, ReadRnwFalseIndFalsePnuFalse) {
    EventEntry ev = getEventForAccessType(AccessType::Read);
    EXPECT_FALSE(ev.rnw);
    EXPECT_FALSE(ev.ind);
    EXPECT_FALSE(ev.pnu);
}

TEST(AccessTypeEventWireFormatTest, WriteRnwTrueIndFalsePnuFalse) {
    EventEntry ev = getEventForAccessType(AccessType::Write);
    EXPECT_TRUE(ev.rnw);
    EXPECT_FALSE(ev.ind);
    EXPECT_FALSE(ev.pnu);
}

TEST(AccessTypeEventWireFormatTest, ExecuteRnwFalseIndTruePnuFalse) {
    EventEntry ev = getEventForAccessType(AccessType::Execute);
    EXPECT_FALSE(ev.rnw);
    EXPECT_TRUE(ev.ind);
    EXPECT_FALSE(ev.pnu);
}
