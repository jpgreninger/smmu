// ARM SMMU v3 CT-30, CT-33 spec compliance tests
// CT-30: Missing command opcodes (§4.1.1)
// CT-33: CR0.CMDQEN/EVENTQEN/PRIQEN queue enable gates (§4.1.2, §7.2.1)
// Copyright (c) 2024 John Greninger

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// ─── CT-30: All spec command opcodes present ───────────────────────────────

TEST(Ct30CommandOpcodes, AllSpecCommandOpcodesArePresent) {
    // §4.1.1 complete command opcode table verification
    EXPECT_EQ(static_cast<int>(CommandType::CFGI_VMS_PIDM),    0x07);
    EXPECT_EQ(static_cast<int>(CommandType::TLBI_EL3_ALL),     0x18);
    EXPECT_EQ(static_cast<int>(CommandType::TLBI_EL3_VA),      0x1A);
    EXPECT_EQ(static_cast<int>(CommandType::TLBI_S_EL2_ALL),   0x50);
    EXPECT_EQ(static_cast<int>(CommandType::TLBI_S_EL2_ASID),  0x51);
    EXPECT_EQ(static_cast<int>(CommandType::TLBI_S_EL2_VA),    0x52);
    EXPECT_EQ(static_cast<int>(CommandType::TLBI_S_EL2_VAA),   0x53);
    EXPECT_EQ(static_cast<int>(CommandType::TLBI_S_S12_VMALL), 0x58);
    EXPECT_EQ(static_cast<int>(CommandType::TLBI_S_S2_IPA),    0x5A);
    EXPECT_EQ(static_cast<int>(CommandType::TLBI_SNH_ALL),     0x60);
    EXPECT_EQ(static_cast<int>(CommandType::DPTI_ALL),         0x70);
    EXPECT_EQ(static_cast<int>(CommandType::DPTI_PA),          0x73);
}

// Existing opcodes must retain their values after extension
TEST(Ct30CommandOpcodes, ExistingOpcodesUnchanged) {
    EXPECT_EQ(static_cast<int>(CommandType::PREFETCH_CONFIG), 0x01);
    EXPECT_EQ(static_cast<int>(CommandType::PREFETCH_ADDR),   0x02);
    EXPECT_EQ(static_cast<int>(CommandType::CFGI_STE),        0x03);
    EXPECT_EQ(static_cast<int>(CommandType::CFGI_ALL),        0x04);
    EXPECT_EQ(static_cast<int>(CommandType::CFGI_CD),         0x05);
    EXPECT_EQ(static_cast<int>(CommandType::CFGI_CD_ALL),     0x06);
    EXPECT_EQ(static_cast<int>(CommandType::TLBI_NH_ALL),     0x10);
    EXPECT_EQ(static_cast<int>(CommandType::TLBI_NH_ASID),    0x11);
    EXPECT_EQ(static_cast<int>(CommandType::TLBI_NH_VA),      0x12);
    EXPECT_EQ(static_cast<int>(CommandType::TLBI_NH_VAA),     0x13);
    EXPECT_EQ(static_cast<int>(CommandType::TLBI_EL2_ALL),    0x20);
    EXPECT_EQ(static_cast<int>(CommandType::TLBI_EL2_ASID),   0x21);
    EXPECT_EQ(static_cast<int>(CommandType::TLBI_EL2_VA),     0x22);
    EXPECT_EQ(static_cast<int>(CommandType::TLBI_EL2_VAA),    0x23);
    EXPECT_EQ(static_cast<int>(CommandType::TLBI_S12_VMALL),  0x28);
    EXPECT_EQ(static_cast<int>(CommandType::TLBI_S2_IPA),     0x2A);
    EXPECT_EQ(static_cast<int>(CommandType::TLBI_NSNH_ALL),   0x30);
    EXPECT_EQ(static_cast<int>(CommandType::ATC_INV),         0x40);
    EXPECT_EQ(static_cast<int>(CommandType::PRI_RESP),        0x41);
    EXPECT_EQ(static_cast<int>(CommandType::RESUME),          0x44);
    EXPECT_EQ(static_cast<int>(CommandType::STALL_TERM),      0x45);
    EXPECT_EQ(static_cast<int>(CommandType::SYNC),            0x46);
}

// New commands can be submitted and processed without error
TEST(Ct30CommandOpcodes, NewCommandsCanBeSubmittedAndProcessed) {
    SMMU smmu;
    smmu.enable();

    // CFGI_VMS_PIDM — should be accepted as a no-op
    {
        CommandEntry cmd;
        cmd.type = CommandType::CFGI_VMS_PIDM;
        cmd.streamID = 0;
        cmd.pasid = 0;
        EXPECT_TRUE(smmu.submitCommand(cmd).isOk());
    }
    // TLBI_EL3_ALL — should be accepted as a TLB flush
    {
        CommandEntry cmd;
        cmd.type = CommandType::TLBI_EL3_ALL;
        EXPECT_TRUE(smmu.submitCommand(cmd).isOk());
    }
    // DPTI_ALL — should be accepted as a no-op
    {
        CommandEntry cmd;
        cmd.type = CommandType::DPTI_ALL;
        EXPECT_TRUE(smmu.submitCommand(cmd).isOk());
    }
    // DPTI_PA — should be accepted as a no-op
    {
        CommandEntry cmd;
        cmd.type = CommandType::DPTI_PA;
        EXPECT_TRUE(smmu.submitCommand(cmd).isOk());
    }
    // Processing all should not crash
    smmu.processCommandQueue();
}

// ─── CT-33: CR0 register queue enable gates ───────────────────────────────

TEST(Ct33Cr0, SetAndGetCR0) {
    SMMU smmu;
    smmu.setCR0(0u);
    EXPECT_EQ(smmu.getCR0(), 0u);

    smmu.setCR0(0xFFFFFFFFu);
    EXPECT_NE(smmu.getCR0(), 0u);
}

TEST(Ct33Cr0, SMMUENBitMapsToEnableDisable) {
    SMMU smmu;
    // CR0.SMMUEN = bit 0
    smmu.setCR0(0u);
    EXPECT_FALSE(smmu.isEnabled());

    smmu.setCR0(1u); // set SMMUEN
    EXPECT_TRUE(smmu.isEnabled());
}

TEST(Ct33Cr0, EventNotRecordedWhenEventqenNotSet) {
    SMMU smmu;
    // CR0.EVENTQEN=0 (bit 2 cleared), SMMUEN=0 so bypass is active
    smmu.setCR0(0u);

    // Trigger a stream lookup event (C_BAD_STREAMID) when SMMUEN is set but EVENTQEN=0
    // Enable SMMUEN only, not EVENTQEN (bit 0=1, bit 2=0)
    smmu.setCR0(1u); // SMMUEN=1, EVENTQEN=0

    // Translate to non-existent stream to trigger C_BAD_STREAMID
    smmu.translate(9999, 0, 0x1000, AccessType::Read, SecurityState::NonSecure);

    // Event should NOT be recorded when EVENTQEN=0
    EXPECT_TRUE(smmu.getEventQueue().empty())
        << "CT-33: Events must not be recorded when CR0.EVENTQEN=0";
}

TEST(Ct33Cr0, EventRecordedWhenEventqenSet) {
    SMMU smmu;
    // CR0.SMMUEN=1 (bit 0), CR0.EVENTQEN=1 (bit 2) → value = 0x05 = 0b00000101
    smmu.setCR0(0x05u);
    EXPECT_TRUE(smmu.isEnabled());

    // Translate to non-existent stream to trigger C_BAD_STREAMID
    smmu.translate(9999, 0, 0x1000, AccessType::Read, SecurityState::NonSecure);

    // Event SHOULD be recorded when EVENTQEN=1
    EXPECT_FALSE(smmu.getEventQueue().empty())
        << "CT-33: Events must be recorded when CR0.EVENTQEN=1";
}

TEST(Ct33Cr0, CommandQueueDisabledWhenCmdqenNotSet) {
    SMMU smmu;
    // CR0.SMMUEN=1 (bit 0), CR0.EVENTQEN=1 (bit 2), but CMDQEN=0 (bit 3 cleared)
    // bit 0 = SMMUEN, bit 2 = EVENTQEN → 0x05; CMDQEN = bit 3 → not set
    smmu.setCR0(0x05u); // SMMUEN=1, EVENTQEN=1, CMDQEN=0

    StreamConfig cfg;
    cfg.translationEnabled = false;
    cfg.stage1Enabled = false;
    cfg.stage2Enabled = false;
    smmu.configureStream(1, cfg);

    CommandEntry cmd;
    cmd.type = CommandType::CFGI_ALL;
    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());

    // processCommandQueue should be a no-op when CMDQEN=0
    size_t sizeBefore = smmu.getCommandQueueSize();
    smmu.processCommandQueue();
    size_t sizeAfter = smmu.getCommandQueueSize();

    // Queue should still have items (not processed) when CMDQEN=0
    EXPECT_EQ(sizeBefore, sizeAfter)
        << "CT-33: Command queue must not be processed when CR0.CMDQEN=0";
}

TEST(Ct33Cr0, CommandQueueProcessedWhenCmdqenSet) {
    SMMU smmu;
    // bit 0=SMMUEN, bit 2=EVENTQEN, bit 3=CMDQEN → 0x0D = 0b00001101
    smmu.setCR0(0x0Du); // SMMUEN=1, EVENTQEN=1, CMDQEN=1

    CommandEntry cmd;
    cmd.type = CommandType::SYNC;
    cmd.cs = 0u; // SIG_NONE — no completion event
    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());

    EXPECT_EQ(smmu.getCommandQueueSize(), 1u);
    smmu.processCommandQueue();
    EXPECT_EQ(smmu.getCommandQueueSize(), 0u)
        << "CT-33: Command queue must be processed when CR0.CMDQEN=1";
}

} // namespace test
} // namespace smmu
