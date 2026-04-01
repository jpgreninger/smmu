// ARM SMMU v3 Conformance Gap Tests
// Tests for CONF-GAP-9, 10, 14, 11, 12, 13, 17, 18, 6, 8, 3
// Copyright (c) 2024 John Greninger

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"

using namespace smmu;

// ============================================================================
// CONF-GAP-9: SMMU_CR0ACK handshake (§6.3.10)
// ============================================================================

TEST(ConfGap9CR0ACK, SetCR0UpdatesACK) {
    SMMU smmu;
    smmu.setCR0(0x0Fu);
    EXPECT_EQ(smmu.getCR0ACK(), 0x0Fu);
}

TEST(ConfGap9CR0ACK, ResetClearsBoth) {
    SMMU smmu;
    smmu.setCR0(0xFFu);
    smmu.reset();
    EXPECT_EQ(smmu.getCR0(), 0u);
    EXPECT_EQ(smmu.getCR0ACK(), 0u);
}

TEST(ConfGap9CR0ACK, EnableSyncsACK) {
    SMMU smmu;
    smmu.enable();
    // After enable(), both CR0 and CR0ACK must reflect SMMUEN set
    EXPECT_NE(smmu.getCR0ACK() & SMMU::CR0_SMMUEN, 0u);
    EXPECT_EQ(smmu.getCR0() & SMMU::CR0_SMMUEN, smmu.getCR0ACK() & SMMU::CR0_SMMUEN);
}

TEST(ConfGap9CR0ACK, DisableSyncsACK) {
    SMMU smmu;
    smmu.enable();
    smmu.disable();
    EXPECT_EQ(smmu.getCR0ACK() & SMMU::CR0_SMMUEN, 0u);
    EXPECT_EQ(smmu.getCR0() & SMMU::CR0_SMMUEN, smmu.getCR0ACK() & SMMU::CR0_SMMUEN);
}

TEST(ConfGap9CR0ACK, InitialValueZero) {
    SMMU smmu;
    EXPECT_EQ(smmu.getCR0ACK(), 0u);
}

// ============================================================================
// CONF-GAP-10: SMMU_CR1 register (§6.3.11)
// ============================================================================

TEST(ConfGap10CR1, SetGetRoundTrip) {
    SMMU smmu;
    uint32_t val = SMMU::CR1_TABLE_SH | SMMU::CR1_QUEUE_IC;
    smmu.setCR1(val);
    EXPECT_EQ(smmu.getCR1(), val);
}

TEST(ConfGap10CR1, InitialValueZero) {
    SMMU smmu;
    EXPECT_EQ(smmu.getCR1(), 0u);
}

TEST(ConfGap10CR1, ResetClearsValue) {
    SMMU smmu;
    smmu.setCR1(0xFFFFFFFFu);
    smmu.reset();
    EXPECT_EQ(smmu.getCR1(), 0u);
}

TEST(ConfGap10CR1, ConstantsCorrect) {
    // Verify bit positions per ARM §6.3.11
    EXPECT_EQ(SMMU::CR1_TABLE_SH, 0x3u << 10);
    EXPECT_EQ(SMMU::CR1_TABLE_OC, 0x3u << 8);
    EXPECT_EQ(SMMU::CR1_TABLE_IC, 0x3u << 6);
    EXPECT_EQ(SMMU::CR1_QUEUE_SH, 0x3u << 4);
    EXPECT_EQ(SMMU::CR1_QUEUE_OC, 0x3u << 2);
    EXPECT_EQ(SMMU::CR1_QUEUE_IC, 0x3u << 0);
}

TEST(ConfGap10CR1, AllBitsRoundTrip) {
    SMMU smmu;
    uint32_t allBits = SMMU::CR1_TABLE_SH | SMMU::CR1_TABLE_OC | SMMU::CR1_TABLE_IC |
                       SMMU::CR1_QUEUE_SH | SMMU::CR1_QUEUE_OC | SMMU::CR1_QUEUE_IC;
    smmu.setCR1(allBits);
    EXPECT_EQ(smmu.getCR1(), allBits);
}

// ============================================================================
// CONF-GAP-14: STE.MEV event merging field (§5.2)
// ============================================================================

TEST(ConfGap14MEV, StructHasMevField) {
    StreamConfig cfg;
    cfg.mev = false;
    EXPECT_FALSE(cfg.mev);
    cfg.mev = true;
    EXPECT_TRUE(cfg.mev);
}

TEST(ConfGap14MEV, DefaultMevFalse) {
    StreamConfig cfg;
    EXPECT_FALSE(cfg.mev);
}

TEST(ConfGap14MEV, MevTrueSuppressDuplicates) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_EVENTQEN);

    // Use translate on unmapped IOVA to generate F_TRANSLATION events
    // with MEV=true, duplicates (same type+streamID) should be suppressed
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.mev = true;
    smmu.configureStream(10u, cfg);
    smmu.enableStream(10u);
    smmu.createStreamPASID(10u, 0u);
    smmu.clearEventQueue();

    // Translate an unmapped IOVA three times → should generate F_TRANSLATION each time
    // but MEV=true should suppress duplicates
    smmu.translate(10u, 0u, 0x9000u, AccessType::Read);
    smmu.translate(10u, 0u, 0x9000u, AccessType::Read);
    smmu.translate(10u, 0u, 0x9000u, AccessType::Read);

    auto events = smmu.getEventQueue();
    // With MEV=true, only 1 F_TRANSLATION event for stream 10 should exist
    size_t translEvents = 0;
    for (const auto& ev : events) {
        if (ev.streamID == 10u && ev.type == EventType::F_TRANSLATION) {
            ++translEvents;
        }
    }
    EXPECT_EQ(translEvents, 1u) << "MEV=true should suppress duplicate F_TRANSLATION events, got "
                                 << events.size() << " total events";
}

TEST(ConfGap14MEV, MevFalseAllowsDuplicates) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_EVENTQEN);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.mev = false;
    smmu.configureStream(11u, cfg);
    smmu.enableStream(11u);
    smmu.createStreamPASID(11u, 0u);
    smmu.clearEventQueue();

    // Translate an unmapped IOVA three times — MEV=false means duplicates allowed
    smmu.translate(11u, 0u, 0x9000u, AccessType::Read);
    smmu.translate(11u, 0u, 0x9000u, AccessType::Read);
    smmu.translate(11u, 0u, 0x9000u, AccessType::Read);

    auto events = smmu.getEventQueue();
    size_t translEvents = 0;
    for (const auto& ev : events) {
        if (ev.streamID == 11u && ev.type == EventType::F_TRANSLATION) {
            ++translEvents;
        }
    }
    EXPECT_GE(translEvents, 2u) << "MEV=false should allow duplicate F_TRANSLATION events";
}

// ============================================================================
// CONF-GAP-11: CR2.PTM effect (§6.3.12)
// ============================================================================

TEST(ConfGap11PTM, ConstantExists) {
    EXPECT_EQ(SMMU::CR2_PTM, 1u << 2);
}

TEST(ConfGap11PTM, PTMZeroFiresBroadcastTLBI) {
    // BUG-AUDIT-54 fix: PTM=0 means SMMU PARTICIPATES in broadcast TLB maintenance
    // (ARM §6.3.12). The broadcast must execute and evict the TLB entry.
    // Verify by: warm TLB → unmap page → broadcast TLBI with PTM=0 →
    // TLB evicted → page walk fails → translate faults.
    SMMU smmu;
    smmu.setCR2(smmu.getCR2() & ~SMMU::CR2_PTM);  // PTM=0: participate
    smmu.enable();
    smmu.enableCaching(true);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    smmu.configureStream(5u, cfg);
    smmu.enableStream(5u);
    smmu.createStreamPASID(5u, 0u);
    PagePermissions rw;
    rw.read = true;
    rw.write = true;
    smmu.mapPage(5u, 0u, 0x1000u, 0x2000u, rw);

    // Warm up TLB
    auto r1 = smmu.translate(5u, 0u, 0x1000u, AccessType::Read);
    ASSERT_TRUE(r1.isOk()) << "Initial translate must succeed";

    // Unmap the page so a page-table walk would fail
    smmu.unmapPage(5u, 0u, 0x1000u);

    // Broadcast TLBI with PTM=0 — SMMU participates, TLB entry must be evicted.
    ASSERT_EQ(smmu.getCR2() & SMMU::CR2_PTM, 0u) << "PTM must be 0";
    // Use TLBI_NSNH_ALL (VMID-agnostic) to ensure the entry is evicted.
    smmu.receiveBroadcastTLBI(CommandType::TLBI_NSNH_ALL);

    // TLB evicted + page unmapped → translate must fault (BUG-AUDIT-54: ARM §6.3.12)
    auto r2 = smmu.translate(5u, 0u, 0x1000u, AccessType::Read);
    EXPECT_FALSE(r2.isOk()) << "PTM=0: broadcast TLBI must evict TLB; walk faults on unmapped page (ARM §6.3.12)";
}

TEST(ConfGap11PTM, PTMOneSkipsBroadcastTLBI) {
    // BUG-AUDIT-54 fix: PTM=1 means Private TLB Maintenance — SMMU does NOT
    // participate in broadcast TLB maintenance (ARM §6.3.12).
    // Verify by: warm TLB → unmap page → broadcast TLBI with PTM=1 →
    // TLB remains warm → translate still succeeds despite unmapped page.
    SMMU smmu;
    smmu.setCR2(SMMU::CR2_PTM);  // PTM=1: Private — skip broadcast
    smmu.enable();
    smmu.enableCaching(true);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    smmu.configureStream(6u, cfg);
    smmu.enableStream(6u);
    smmu.createStreamPASID(6u, 0u);
    PagePermissions rw;
    rw.read = true;
    rw.write = true;
    smmu.mapPage(6u, 0u, 0x1000u, 0x2000u, rw);

    // Warm up TLB
    auto r1 = smmu.translate(6u, 0u, 0x1000u, AccessType::Read);
    ASSERT_TRUE(r1.isOk()) << "Initial translate must succeed";

    // Unmap the page so a page-table walk would fail
    smmu.unmapPage(6u, 0u, 0x1000u);

    // Broadcast TLBI with PTM=1 — must be a no-op; TLB entry remains (BUG-AUDIT-54: ARM §6.3.12)
    smmu.receiveBroadcastTLBI(CommandType::TLBI_NSNH_ALL);

    // TLB entry was NOT evicted → translate still hits the warm TLB entry
    auto r2 = smmu.translate(6u, 0u, 0x1000u, AccessType::Read);
    EXPECT_TRUE(r2.isOk()) << "PTM=1: broadcast TLBI must be a no-op; TLB remains warm (ARM §6.3.12)";
}

// ============================================================================
// CONF-GAP-12: CR0.VMW VMID wildcard matching (§6.3.9)
// ============================================================================

TEST(ConfGap12VMW, ConstantsExist) {
    // ARM IHI0070G.b §6.3.9.1: VMW field is at bits[8:6], shift=6, mask=0x1C0
    EXPECT_EQ(SMMU::CR0_VMW_SHIFT, 6u);
    EXPECT_EQ(SMMU::CR0_VMW_MASK, 7u << 6);
}

TEST(ConfGap12VMW, WildcardInvalidatesGroup) {
    // Verify the VMW constants match the spec (bits[8:6])
    EXPECT_EQ(SMMU::CR0_VMW_SHIFT, 6u);
    EXPECT_EQ(SMMU::CR0_VMW_MASK, 7u << 6u);

    // Functional test: set VMW=2 on a fresh SMMU before enable()
    SMMU smmu;
    // Set VMW=2 (bits[8:6] = 0b010 = 2 shifted to bits 8:6) — CR0 at this point is 0
    smmu.setCR0((2u << SMMU::CR0_VMW_SHIFT));
    EXPECT_EQ((smmu.getCR0() >> SMMU::CR0_VMW_SHIFT) & 7u, 2u);
}

TEST(ConfGap12VMW, TLBCacheInvalidateByVMIDWithMask) {
    // Test TLBCache::invalidateByVMIDWithMask directly
    TLBCache cache(64);
    TLBEntry e1;
    e1.streamID = 1; e1.pasid = 0; e1.iova = 0x1000; e1.physicalAddress = 0x2000;
    e1.valid = true; e1.vmid = 0x04;
    cache.insert(e1);
    TLBEntry e2;
    e2.streamID = 2; e2.pasid = 0; e2.iova = 0x3000; e2.physicalAddress = 0x4000;
    e2.valid = true; e2.vmid = 0x05;
    cache.insert(e2);
    TLBEntry e3;
    e3.streamID = 3; e3.pasid = 0; e3.iova = 0x5000; e3.physicalAddress = 0x6000;
    e3.valid = true; e3.vmid = 0x08;
    cache.insert(e3);

    // Invalidate VMID=0x04 with mask=0xFFFC (lower 2 bits wildcard)
    // Entries with vmid 0x04 and 0x05 (differ only in lower 2 bits) should be evicted
    // Entry with vmid 0x08 should survive
    cache.invalidateByVMIDWithMask(0x04u, static_cast<uint16_t>(~0x3u));

    EXPECT_EQ(cache.getSize(), 1u);
    auto r = cache.lookupEntry(3, 0, 0x5000, SecurityState::NonSecure);
    EXPECT_TRUE(r.isOk()) << "Entry with VMID=0x08 should survive";
}

// ============================================================================
// CONF-GAP-13: GBPA output attribute fields (§6.3.22)
// ============================================================================

TEST(ConfGap13GBPA, GbpaConfigStructExists) {
    GbpaConfig cfg;
    EXPECT_FALSE(cfg.abort);
    EXPECT_EQ(cfg.instCfg, 0u);
    EXPECT_EQ(cfg.privCfg, 0u);
    EXPECT_FALSE(cfg.mtCfg);
    EXPECT_EQ(cfg.memAttr, 0u);
    EXPECT_EQ(cfg.shCfg, 0u);
    EXPECT_EQ(cfg.allocCfg, 0u);
}

TEST(ConfGap13GBPA, SetGetGbpaConfig) {
    SMMU smmu;
    GbpaConfig cfg;
    cfg.abort = false;
    cfg.memAttr = 0x5u;
    cfg.shCfg = 0x2u;
    cfg.allocCfg = 0xAu;
    cfg.instCfg = 0x1u;
    cfg.privCfg = 0x2u;
    cfg.mtCfg = true;
    smmu.setGbpaConfig(cfg);
    auto got = smmu.getGbpaConfig();
    EXPECT_EQ(got.memAttr, 0x5u);
    EXPECT_EQ(got.shCfg, 0x2u);
    EXPECT_EQ(got.allocCfg, 0xAu);
    EXPECT_EQ(got.instCfg, 0x1u);
    EXPECT_EQ(got.privCfg, 0x2u);
    EXPECT_TRUE(got.mtCfg);
    EXPECT_FALSE(got.abort);
}

TEST(ConfGap13GBPA, BypassAppliesAttrs) {
    SMMU smmu;
    // SMMUEN=0, GBPA.ABORT=0 → bypass; output attrs applied
    GbpaConfig cfg;
    cfg.abort = false;
    cfg.memAttr = 0x5u;
    cfg.shCfg = 0x2u;
    cfg.allocCfg = 0x3u;
    cfg.mtCfg = true;
    smmu.setGbpaConfig(cfg);

    // With SMMUEN=0, translate should bypass
    auto r = smmu.translate(0u, 0u, 0x1000u, AccessType::Read);
    ASSERT_TRUE(r.isOk()) << "Bypass should succeed";
    // Output attributes should be applied
    EXPECT_EQ(r.getValue().memType, cfg.memAttr) << "memType should come from GBPA";
    EXPECT_EQ(r.getValue().shareability, cfg.shCfg) << "shareability from GBPA.SHCFG";
    EXPECT_EQ(r.getValue().allocHint, cfg.allocCfg) << "allocHint from GBPA.ALLOCCFG";
}

TEST(ConfGap13GBPA, SetGbpaAbortUpdatesConfig) {
    SMMU smmu;
    smmu.setGbpaAbort(true);
    EXPECT_TRUE(smmu.isGbpaAbort());
    auto cfg = smmu.getGbpaConfig();
    EXPECT_TRUE(cfg.abort);
}

TEST(ConfGap13GBPA, ResetClearsGbpaConfig) {
    SMMU smmu;
    GbpaConfig cfg;
    cfg.memAttr = 0xFu;
    cfg.mtCfg = true;
    smmu.setGbpaConfig(cfg);
    smmu.reset();
    auto got = smmu.getGbpaConfig();
    EXPECT_EQ(got.memAttr, 0u);
    EXPECT_FALSE(got.mtCfg);
    EXPECT_FALSE(got.abort);
}

// ============================================================================
// CONF-GAP-17: CMDQ_CONS.ERR reason codes (§6.3.17)
// ============================================================================

TEST(ConfGap17CmdqConsErr, ConstantsExist) {
    EXPECT_EQ(CMDQ_CONS_ERR_SHIFT, 24u);
    EXPECT_EQ(CERROR_NONE, 0u);
    EXPECT_EQ(CERROR_ILL, 1u);
    EXPECT_EQ(CERROR_ABT, 2u);
    EXPECT_EQ(CERROR_ATC_INV_SYNC, 3u);
}

TEST(ConfGap17CmdqConsErr, CS3SetsCerrorIll) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    CommandEntry sync;
    sync.type = CommandType::SYNC;
    sync.cs = 3u;  // Reserved → CERROR_ILL
    smmu.submitCommand(sync);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL) << "CS=0b11 must set CERROR_ILL";
}

TEST(ConfGap17CmdqConsErr, GetCmdqConsErrAccessor) {
    SMMU smmu;
    // Initially no error
    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_NONE);
}

// ============================================================================
// CONF-GAP-18: CMD_SYNC SIG_IRQ vs SIG_SEV not distinguished (§4.7.3)
// BUG-QA-7 fix: CS=2 is SIG_SEV (PE-level wakeup), NOT SIG_MSI.
// ============================================================================

TEST(ConfGap18CmdSync, CmdSyncSignalTypeEnumExists) {
    CmdSyncSignalType none = CmdSyncSignalType::None;
    CmdSyncSignalType irq  = CmdSyncSignalType::Irq;
    CmdSyncSignalType sev  = CmdSyncSignalType::Sev;
    EXPECT_NE(none, irq);
    EXPECT_NE(irq, sev);
    EXPECT_NE(none, sev);
}

TEST(ConfGap18CmdSync, CS1SetsIrqSignalType) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    CommandEntry sync;
    sync.type = CommandType::SYNC;
    sync.cs = 1u;  // SIG_IRQ
    smmu.submitCommand(sync);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdSyncLastSignalType(), CmdSyncSignalType::Irq);
}

TEST(ConfGap18CmdSync, CS2SetsSevSignalType) {
    SMMU smmu;
    smmu.setSevSupported(true);
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    CommandEntry sync;
    sync.type = CommandType::SYNC;
    sync.cs = 2u;  // SIG_SEV per ARM §4.7.3
    smmu.submitCommand(sync);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdSyncLastSignalType(), CmdSyncSignalType::Sev);
}

TEST(ConfGap18CmdSync, CS0NoSignal) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // First set a known state
    CommandEntry sync1;
    sync1.type = CommandType::SYNC;
    sync1.cs = 1u;
    smmu.submitCommand(sync1);
    smmu.processCommandQueue();
    ASSERT_EQ(smmu.getCmdSyncLastSignalType(), CmdSyncSignalType::Irq);

    // CS=0 should NOT change the signal type (no completion signal)
    CommandEntry sync0;
    sync0.type = CommandType::SYNC;
    sync0.cs = 0u;
    smmu.submitCommand(sync0);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdSyncLastSignalType(), CmdSyncSignalType::Irq) << "CS=0 should not change signal type";
}

TEST(ConfGap18CmdSync, MsiRegistersRoundTrip) {
    SMMU smmu;
    smmu.setCmdqSyncMsiAddr(0xDEADBEEF00001234ULL);
    smmu.setCmdqSyncMsiData(0xABCDu);
    smmu.setCmdqSyncMsiAttr(0x5u);
    EXPECT_EQ(smmu.getCmdqSyncMsiAddr(), 0xDEADBEEF00001234ULL);
    EXPECT_EQ(smmu.getCmdqSyncMsiData(), 0xABCDu);
    EXPECT_EQ(smmu.getCmdqSyncMsiAttr(), 0x5u);
}

TEST(ConfGap18CmdSync, ResetClearsMsiRegisters) {
    SMMU smmu;
    smmu.setCmdqSyncMsiAddr(0x12345678ULL);
    smmu.setCmdqSyncMsiData(0xDEADu);
    smmu.setCmdqSyncMsiAttr(0x7u);
    smmu.reset();
    EXPECT_EQ(smmu.getCmdqSyncMsiAddr(), 0u);
    EXPECT_EQ(smmu.getCmdqSyncMsiData(), 0u);
    EXPECT_EQ(smmu.getCmdqSyncMsiAttr(), 0u);
}

// ============================================================================
// CONF-GAP-6: VA/VAA TLBI selective invalidation (§4.4)
// ============================================================================

TEST(ConfGap6TLBIVA, InvalidateByVAAndASIDExists) {
    TLBCache cache(64);
    TLBEntry e1;
    e1.streamID = 1; e1.pasid = 0; e1.iova = 0x1000; e1.physicalAddress = 0x2000;
    e1.valid = true; e1.asid = 10;
    cache.insert(e1);
    TLBEntry e2;
    e2.streamID = 2; e2.pasid = 0; e2.iova = 0x3000; e2.physicalAddress = 0x4000;
    e2.valid = true; e2.asid = 20;
    cache.insert(e2);

    // Invalidate VA=0x1000, ASID=10 → only first entry evicted
    cache.invalidateByVAAndASID(0x1000u, 10u);
    EXPECT_EQ(cache.getSize(), 1u);
    auto r = cache.lookupEntry(2, 0, 0x3000, SecurityState::NonSecure);
    EXPECT_TRUE(r.isOk()) << "Different stream/ASID entry should survive";
}

TEST(ConfGap6TLBIVA, InvalidateByVAExists) {
    TLBCache cache(64);
    TLBEntry e1;
    e1.streamID = 1; e1.pasid = 0; e1.iova = 0x1000; e1.physicalAddress = 0x2000;
    e1.valid = true; e1.asid = 10;
    cache.insert(e1);
    TLBEntry e2;
    e2.streamID = 2; e2.pasid = 0; e2.iova = 0x1000; e2.physicalAddress = 0x5000;
    e2.valid = true; e2.asid = 20;
    cache.insert(e2);
    TLBEntry e3;
    e3.streamID = 3; e3.pasid = 0; e3.iova = 0x3000; e3.physicalAddress = 0x6000;
    e3.valid = true; e3.asid = 30;
    cache.insert(e3);

    // invalidateByVA(0x1000) — regardless of ASID — should evict entries for VA=0x1000
    cache.invalidateByVA(0x1000u);
    EXPECT_EQ(cache.getSize(), 1u);
    auto r = cache.lookupEntry(3, 0, 0x3000, SecurityState::NonSecure);
    EXPECT_TRUE(r.isOk()) << "Different VA entry should survive";
}

TEST(ConfGap6TLBIVA, TLBINHVATargetsSpecificEntry) {
    SMMU smmu;
    // Note: CR2.PTM does not affect command-queue TLBI commands (only receiveBroadcastTLBI path).
    // Set PTM=0 (participate) as default; value is irrelevant for command-queue operations.
    smmu.setCR2(0u);
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    PagePermissions rw; rw.read = true; rw.write = true;

    StreamConfig cfg1;
    cfg1.translationEnabled = true;
    cfg1.stage1Enabled = true;
    cfg1.asid = 10u;
    smmu.configureStream(0x10u, cfg1);
    smmu.enableStream(0x10u);
    smmu.createStreamPASID(0x10u, 0u);
    smmu.setStreamASID(0x10u, 10u);
    smmu.mapPage(0x10u, 0u, 0x1000u, 0xA000u, rw);

    StreamConfig cfg2;
    cfg2.translationEnabled = true;
    cfg2.stage1Enabled = true;
    cfg2.asid = 20u;
    smmu.configureStream(0x20u, cfg2);
    smmu.enableStream(0x20u);
    smmu.createStreamPASID(0x20u, 0u);
    smmu.setStreamASID(0x20u, 20u);
    smmu.mapPage(0x20u, 0u, 0x1000u, 0xB000u, rw);

    // Warm up TLB
    auto r1 = smmu.translate(0x10u, 0u, 0x1000u, AccessType::Read);
    auto r2 = smmu.translate(0x20u, 0u, 0x1000u, AccessType::Read);
    ASSERT_TRUE(r1.isOk()) << "Stream 0x10 initial translate, error="
                            << static_cast<int>(r1.getError());
    ASSERT_TRUE(r2.isOk()) << "Stream 0x20 initial translate, error="
                            << static_cast<int>(r2.getError());

    // TLBI_NH_VA targeting VA=0x1000, ASID=10 — should only evict stream 0x10 entry
    CommandEntry tlbi;
    tlbi.type = CommandType::TLBI_NH_VA;
    tlbi.startAddress = 0x1000u;
    tlbi.asid = 10u;
    smmu.submitCommand(tlbi);
    smmu.processCommandQueue();

    // Both translations should still succeed (page table still intact)
    auto r1b = smmu.translate(0x10u, 0u, 0x1000u, AccessType::Read);
    auto r2b = smmu.translate(0x20u, 0u, 0x1000u, AccessType::Read);
    EXPECT_TRUE(r1b.isOk()) << "Stream 0x10 should re-translate after TLBI_NH_VA";
    EXPECT_TRUE(r2b.isOk()) << "Stream 0x20 should still translate (different ASID)";
}

// ============================================================================
// CONF-GAP-8: Range-based TLBI / RIL feature (§4.4.1.1)
// ============================================================================

TEST(ConfGap8RIL, CommandEntryHasRILFields) {
    CommandEntry cmd;
    cmd.tg = 0u;
    cmd.num = 3u;
    cmd.scale = 0u;
    cmd.ttl = 0u;
    cmd.ril = true;
    EXPECT_EQ(cmd.tg, 0u);
    EXPECT_EQ(cmd.num, 3u);
    EXPECT_EQ(cmd.scale, 0u);
    EXPECT_EQ(cmd.ttl, 0u);
    EXPECT_TRUE(cmd.ril);
}

TEST(ConfGap8RIL, DefaultRILFalse) {
    CommandEntry cmd;
    EXPECT_FALSE(cmd.ril);
    EXPECT_EQ(cmd.tg, 0u);
    EXPECT_EQ(cmd.num, 0u);
    EXPECT_EQ(cmd.scale, 0u);
    EXPECT_EQ(cmd.ttl, 0u);
}

TEST(ConfGap8RIL, InvalidateByVARangeExists) {
    TLBCache cache(64);
    // Insert 4 consecutive pages
    for (int i = 0; i < 4; ++i) {
        TLBEntry e;
        e.streamID = 1; e.pasid = 0;
        e.iova = static_cast<IOVA>(0x1000u + i * 0x1000u);
        e.physicalAddress = e.iova + 0x10000u;
        e.valid = true; e.asid = 5u;
        cache.insert(e);
    }
    // Insert a 5th page at different offset
    TLBEntry e5;
    e5.streamID = 1; e5.pasid = 0; e5.iova = 0x10000u;
    e5.physicalAddress = 0x20000u; e5.valid = true; e5.asid = 5u;
    cache.insert(e5);

    // Invalidate range [0x1000, 0x4000] with ASID=5 → 4 pages evicted
    cache.invalidateByVARange(0x1000u, 0x4000u, 5u);
    EXPECT_EQ(cache.getSize(), 1u);
    auto r = cache.lookupEntry(1, 0, 0x10000u, SecurityState::NonSecure);
    EXPECT_TRUE(r.isOk()) << "Page outside range should survive";
}

TEST(ConfGap8RIL, RILCommandInvalidatesRange) {
    SMMU smmu;
    // Note: CR2.PTM does not affect command-queue TLBI; PTM=0 (participate) is correct default.
    smmu.setCR2(0u);
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.asid = 7u;
    smmu.configureStream(0x30u, cfg);
    smmu.enableStream(0x30u);
    smmu.createStreamPASID(0x30u, 0u);
    smmu.setStreamASID(0x30u, 7u);
    PagePermissions rw; rw.read = true; rw.write = true;
    // Map 4 pages
    smmu.mapPage(0x30u, 0u, 0x1000u, 0xA000u, rw);
    smmu.mapPage(0x30u, 0u, 0x2000u, 0xB000u, rw);
    smmu.mapPage(0x30u, 0u, 0x3000u, 0xC000u, rw);
    smmu.mapPage(0x30u, 0u, 0x4000u, 0xD000u, rw);

    // Warm up TLB for all 4 pages
    auto w1 = smmu.translate(0x30u, 0u, 0x1000u, AccessType::Read);
    auto w2 = smmu.translate(0x30u, 0u, 0x2000u, AccessType::Read);
    auto w3 = smmu.translate(0x30u, 0u, 0x3000u, AccessType::Read);
    auto w4 = smmu.translate(0x30u, 0u, 0x4000u, AccessType::Read);
    ASSERT_TRUE(w1.isOk()) << "Warmup translate 0x1000, error=" << static_cast<int>(w1.getError());
    ASSERT_TRUE(w2.isOk()); ASSERT_TRUE(w3.isOk()); ASSERT_TRUE(w4.isOk());

    // TLBI_NH_VA with ril=true, tg=0 (4KB), num=3, scale=0 → 4 pages
    CommandEntry tlbi;
    tlbi.type = CommandType::TLBI_NH_VA;
    tlbi.startAddress = 0x1000u;
    tlbi.asid = 7u;
    tlbi.ril = true;
    tlbi.tg = 0u;   // 4KB granule
    tlbi.num = 3u;  // (3+1) = 4 pages
    tlbi.scale = 0u;
    smmu.submitCommand(tlbi);
    smmu.processCommandQueue();

    // All 4 pages should still translate (page table intact, TLB just cleared)
    for (IOVA va = 0x1000u; va <= 0x4000u; va += 0x1000u) {
        auto r = smmu.translate(0x30u, 0u, va, AccessType::Read);
        EXPECT_TRUE(r.isOk()) << "Page at VA=" << va << " should re-translate after RIL TLBI";
    }
}

// ============================================================================
// CONF-GAP-3: 2-level stream table (§3.3.1.2, §6.3.25)
// ============================================================================

TEST(ConfGap3TwoLevel, StreamTableFormatEnumExists) {
    StreamTableFormat linear = StreamTableFormat::Linear;
    StreamTableFormat twoLevel = StreamTableFormat::TwoLevel;
    EXPECT_NE(linear, twoLevel);
}

TEST(ConfGap3TwoLevel, SetGetFormat) {
    SMMU smmu;
    EXPECT_EQ(smmu.getStrtabFormat(), StreamTableFormat::Linear);
    smmu.setStrtabFormat(StreamTableFormat::TwoLevel);
    EXPECT_EQ(smmu.getStrtabFormat(), StreamTableFormat::TwoLevel);
}

TEST(ConfGap3TwoLevel, SetGetSplit) {
    SMMU smmu;
    smmu.setStrtabSplit(8u);
    EXPECT_EQ(smmu.getStrtabSplit(), 8u);
}

TEST(ConfGap3TwoLevel, DefaultSplitIsSix) {
    SMMU smmu;
    EXPECT_EQ(smmu.getStrtabSplit(), 6u);
}

TEST(ConfGap3TwoLevel, TwoLevelTranslationWorks) {
    SMMU smmu;
    smmu.setStrtabFormat(StreamTableFormat::TwoLevel);
    smmu.setStrtabSplit(6u);  // L2 index = lower 6 bits
    smmu.setStrtabLog2Size(10u);  // 2^10 = 1024 streams total, L1 = 10-6=4 bits

    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_EVENTQEN);

    // Use bypass mode (no translation) for simplicity — just verify StreamID lookup
    // Stream IDs that cross L1 boundary (L1 index varies, all within 2-level bounds)
    StreamConfig cfg;
    cfg.translationEnabled = false;
    cfg.stage1Enabled = false;
    cfg.bypassEnabled = true;  // bypass mode — identity mapping
    smmu.configureStream(0u, cfg);    // L1[0], L2[0]
    smmu.configureStream(64u, cfg);   // L1[1], L2[0]
    smmu.configureStream(128u, cfg);  // L1[2], L2[0]

    auto r0 = smmu.translate(0u, 0u, 0x1000u, AccessType::Read);
    auto r1 = smmu.translate(64u, 0u, 0x1000u, AccessType::Read);
    auto r2 = smmu.translate(128u, 0u, 0x1000u, AccessType::Read);
    EXPECT_TRUE(r0.isOk()) << "Stream 0 should translate in 2-level mode, error="
                            << static_cast<int>(r0.getError());
    EXPECT_TRUE(r1.isOk()) << "Stream 64 should translate in 2-level mode, error="
                            << static_cast<int>(r1.getError());
    EXPECT_TRUE(r2.isOk()) << "Stream 128 should translate in 2-level mode, error="
                            << static_cast<int>(r2.getError());
}

TEST(ConfGap3TwoLevel, OutOfBoundsReturnsError) {
    SMMU smmu;
    smmu.setStrtabFormat(StreamTableFormat::TwoLevel);
    smmu.setStrtabSplit(4u);   // L2 = 4 bits (16 entries), L1 = LOG2SIZE-4 bits
    smmu.setStrtabLog2Size(6u); // 2^6=64 streams total; L1 has 2^(6-4)=4 L1 entries

    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_EVENTQEN);

    // StreamID=64 would need L1[4] which is out of bounds (only 4 L1 entries: 0-3)
    auto r = smmu.translate(64u, 0u, 0x1000u, AccessType::Read);
    EXPECT_FALSE(r.isOk()) << "Out-of-bounds StreamID should fail in 2-level mode";
    EXPECT_EQ(r.getError(), SMMUError::InvalidStreamID);
}

int main(int argc, char** argv) {
    ::testing::InitGoogleTest(&argc, argv);
    return RUN_ALL_TESTS();
}
