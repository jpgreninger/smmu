// ARM SMMU v3 Conformance: GATOS_PAR FAULTCODE mapping + IDR0.HTTU advertisement
// GAP-1: mapEventTypeToGatosFaultCode() — corrected per ARM IHI0070G.b §9.1.5
// GAP-3: IDR0.HTTU[7:6] must advertise HA/HD support (§6.3.1)

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <memory>

using namespace smmu;

namespace {

static constexpr StreamID SID = 0x30;
static constexpr PASID    PID = 0;

void enableSmmuWithEvents(SMMU& s) {
    s.enable();
    s.setCR0(s.getCR0() | SMMU::CR0_EVENTQEN);
}

// Extracts FAULTCODE bits[11:4] from a GATOS_PAR fault value
uint64_t extractFaultCode(uint64_t par) {
    return (par >> 4) & 0xFFu;
}

// ─────────────────────────────────────────────────────────────────────────────
// GAP-1 Test 1: C_BAD_SUBSTREAMID → FAULTCODE must be 0x08
//
// Trigger: stage-1 stream with s1cdMax=4 (limit=16), translate with PASID=16.
// C_BAD_SUBSTREAMID is generated when PASID >= 2^s1cdMax.
// ─────────────────────────────────────────────────────────────────────────────
TEST(GatosFaultCodeTest, gap1_c_bad_substreamid_faultcode_is_0x08) {
    SMMU smmu;
    enableSmmuWithEvents(smmu);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.s1cdMax            = 4;   // valid PASIDs: 0..15
    cfg.s1dss              = 2;   // use CD[0] for PASID=0
    cfg.t0sz               = 0;
    ASSERT_TRUE(smmu.configureStream(SID, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(SID).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(SID, 0).isOk());

    // PASID=16 is out-of-range (>= 2^4 = 16) → C_BAD_SUBSTREAMID
    uint64_t par = smmu.gatosTranslate(SID, 16, 0x1000, AccessType::Read);

    EXPECT_EQ(par & 0x1u, 1u) << "bit 0 must be 1 (FAULT)";
    EXPECT_EQ(extractFaultCode(par), 0x08u)
        << "FAULTCODE must be 0x08 (C_BAD_SUBSTREAMID) per ARM §9.1.5; got 0x"
        << std::hex << extractFaultCode(par);
}

// ─────────────────────────────────────────────────────────────────────────────
// GAP-1 Test 2: F_STREAM_DISABLED → FAULTCODE must be 0x06
//
// Trigger: stage-1 stream with s1cdMax=4, s1dss=0, translate with PASID=0.
// S1DSS=0b00 → F_STREAM_DISABLED (§7.3.7) when non-substream on substream-capable stream.
// ─────────────────────────────────────────────────────────────────────────────
TEST(GatosFaultCodeTest, gap1_f_stream_disabled_faultcode_is_0x06) {
    SMMU smmu;
    enableSmmuWithEvents(smmu);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.s1cdMax            = 4;   // substream-capable
    cfg.s1dss              = 0;   // 0b00 = abort with F_STREAM_DISABLED when PASID=0
    cfg.t0sz               = 0;
    ASSERT_TRUE(smmu.configureStream(SID, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(SID).isOk());
    // No PASID=0 CD needed — S1DSS=0 aborts before CD lookup

    // PASID=0 + s1cdMax>0 + s1dss=0 → F_STREAM_DISABLED
    uint64_t par = smmu.gatosTranslate(SID, PID, 0x1000, AccessType::Read);

    EXPECT_EQ(par & 0x1u, 1u) << "bit 0 must be 1 (FAULT)";
    EXPECT_EQ(extractFaultCode(par), 0x06u)
        << "FAULTCODE must be 0x06 (F_STREAM_DISABLED) per ARM §9.1.5; got 0x"
        << std::hex << extractFaultCode(par);
}

// ─────────────────────────────────────────────────────────────────────────────
// GAP-1 Test 3: C_BAD_STREAMID → FAULTCODE must be 0x02
//
// SPEC-20: C_BAD_STREAMID fires only when StreamID is OUTSIDE 2^LOG2SIZE (§7.3.3).
// Use LOG2SIZE=8 (range 0-255) and StreamID=0x200 (512) to trigger it.
// C_BAD_STE (0x04) fires for in-range unconfigured streams (§7.3.5).
// ─────────────────────────────────────────────────────────────────────────────
TEST(GatosFaultCodeTest, gap1_c_bad_streamid_faultcode_is_0x02) {
    SMMU smmu;
    // RECINVSID must be set before enable() — setCR2() is ignored when SMMUEN=1.
    smmu.setCR2(SMMU::CR2_RECINVSID);
    // BUG-AUDIT-63: STRTAB_BASE_CFG is RO when SMMUEN=1; set log2size before enable().
    // LOG2SIZE=8 → valid range 0-255; SID=0x200 (512) is out of range → C_BAD_STREAMID
    smmu.setStrtabLog2Size(8u);
    enableSmmuWithEvents(smmu);

    static constexpr StreamID OUT_OF_RANGE_SID = 0x200u; // 512, outside [0,255]
    uint64_t par = smmu.gatosTranslate(OUT_OF_RANGE_SID, PID, 0x1000, AccessType::Read);

    EXPECT_EQ(par & 0x1u, 1u) << "bit 0 must be 1 (FAULT)";
    EXPECT_EQ(extractFaultCode(par), 0x02u)
        << "FAULTCODE must be 0x02 (C_BAD_STREAMID) per ARM §9.1.5; got 0x"
        << std::hex << extractFaultCode(par);
}

// ─────────────────────────────────────────────────────────────────────────────
// GAP-3 Test: IDR0.HTTU[7:6] must be 0b01 (access flag only — dirty state not
// implemented).
// BUG-AUDIT-35 fix: changed expected value from 0b10 to 0b01.  The model only
// implements access-flag hardware update (AF), not dirty state (HD).  §6.3.1:
//   0b00 = no h/w update; 0b01 = access flag only; 0b10 = access flag + dirty.
// ─────────────────────────────────────────────────────────────────────────────
TEST(Idr0HttuTest, gap3_idr0_httu_bits_are_0b01) {
    SMMU smmu;
    uint32_t idr0 = smmu.getIDR0();
    uint32_t httu = (idr0 >> 6) & 0x3u;
    EXPECT_EQ(httu, 1u)
        << "IDR0.HTTU[7:6] must be 0b01 (=1, access flag only, dirty state not implemented, §6.3.1); "
        << "got " << httu;
}

} // namespace
