// Tests for bugs found in newBugs17Mar2026_9pm.md
// TDD: tests written before fixes, then verified green after.
//
// Bug 3 (corrected): LOG2SIZE clamp is 32 (max SIDSIZE per ARM §6.3.4 IDR1).
//   Per §6.3.25 the field is 6 bits; effective range = MIN(LOG2SIZE, SIDSIZE),
//   SIDSIZE max = 32. Values 0–32 are valid; values > 32 are clamped to 32.
// Bug 6: ReadExecute/WriteExecute missing from C++ AccessType enum and generateEvent() switch.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"

using namespace smmu;

// ===== Bug 3 (corrected): LOG2SIZE clamp is 32, not 24 =====

TEST(StrtabLog2SizeTest, Log2Size32Accepted) {
    // 32 is the maximum valid value (matches max SIDSIZE per ARM §6.3.4 IDR1).
    SMMU smmu;
    smmu.setStrtabLog2Size(32);
    EXPECT_EQ(smmu.getStrtabLog2Size(), 32u);
}

TEST(StrtabLog2SizeTest, Log2Size33ClampedTo32) {
    // Values > 32 exceed max SIDSIZE and must be clamped to 32.
    SMMU smmu;
    smmu.setStrtabLog2Size(33);
    EXPECT_EQ(smmu.getStrtabLog2Size(), 32u);
}

TEST(StrtabLog2SizeTest, Log2Size64ClampedTo32) {
    SMMU smmu;
    smmu.setStrtabLog2Size(64);
    EXPECT_EQ(smmu.getStrtabLog2Size(), 32u);
}

TEST(StrtabLog2SizeTest, Log2Size25Accepted) {
    // 25 is a valid value (25 <= 32).
    SMMU smmu;
    smmu.setStrtabLog2Size(25);
    EXPECT_EQ(smmu.getStrtabLog2Size(), 25u);
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
    // RnW-GAP fix: ARM §7.3 RnW=1=Read.
    // ReadExecute: has execute component → ind=true; no write → rnw=true (read); unprivileged → pnu=false
    EventEntry ev = getEventForAccessType(AccessType::ReadExecute);
    EXPECT_TRUE(ev.rnw) << "ReadExecute: rnw must be true (ARM §7.3 RnW=1=Read, no write component)";
    EXPECT_TRUE(ev.ind) << "ReadExecute: ind must be true (has execute component)";
    EXPECT_FALSE(ev.pnu) << "ReadExecute: pnu must be false (unprivileged)";
}

// WriteExecute is architecturally impossible per ARM §7.3: "InD==0 when RnW==0 (write)".
// Instruction fetches can never be writes. WriteExecute was removed from AccessType.

// Regression: verify existing access types remain correct after adding new ones
// RnW-GAP fix: ARM §7.3 RnW=1=Read, RnW=0=Write. Updated all rnw expectations.
TEST(AccessTypeEventWireFormatTest, ReadRnwFalseIndFalsePnuFalse) {
    EventEntry ev = getEventForAccessType(AccessType::Read);
    EXPECT_TRUE(ev.rnw);    // RnW-GAP: Read → rnw=true (ARM §7.3 RnW=1=Read)
    EXPECT_FALSE(ev.ind);
    EXPECT_FALSE(ev.pnu);
}

TEST(AccessTypeEventWireFormatTest, WriteRnwTrueIndFalsePnuFalse) {
    EventEntry ev = getEventForAccessType(AccessType::Write);
    EXPECT_FALSE(ev.rnw);   // RnW-GAP: Write → rnw=false (ARM §7.3 RnW=0=Write)
    EXPECT_FALSE(ev.ind);
    EXPECT_FALSE(ev.pnu);
}

TEST(AccessTypeEventWireFormatTest, ExecuteRnwFalseIndTruePnuFalse) {
    EventEntry ev = getEventForAccessType(AccessType::Execute);
    EXPECT_TRUE(ev.rnw);    // RnW-GAP: Execute is a read; rnw=true (ARM §7.3 RnW=1=Read)
    EXPECT_TRUE(ev.ind);
    EXPECT_FALSE(ev.pnu);
}
