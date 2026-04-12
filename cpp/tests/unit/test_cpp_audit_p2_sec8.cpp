// §8.2 Secure stream PRI discard, PRIQ_ABT_ERR gate, and §8.3 Resp=0b11 tests
//
// §8.2 — Secure stream PRI discard (BUG-SEC8-SECURE-PRI-CPP):
//   ARM §8.2: PRI requests from Secure streams must be discarded with
//   ResponseCode=0b1111 (Failure).  The C++ submitPageRequest() has no check
//   for request.securityState == SecurityState::Secure.
//   Fix: Add a Secure-stream check after the PRIQEN/SMMUEN check.
//
// §8.2 — PRIQ_ABT_ERR gate (BUG-SEC8-PRIQ-ABT-CPP):
//   ARM §8.2: When GERROR.PRIQ_ABT_ERR is active (bit 3), new PPRs must be
//   inhibited.  The C++ implementation does not check PRIQ_ABT_ERR before
//   accepting a PPR.  signalGerror() is private so the test cannot activate
//   PRIQ_ABT_ERR directly; constant presence and initial state are verified.
//
// §8.3 — PRG Response Resp=0b11 → CERROR_ILL (already implemented):
//   ARM §8.3 / §4.5.2: CMD_PRI_RESP with Resp=0b11 raises CERROR_ILL +
//   GERROR_CMDQ_ERR.  Already fixed (BUG-NEW-A).  Tests act as regression guard.
//
// TDD:
//   - SecureStreamPriDiscarded MUST FAIL before the Secure-stream fix.
//   - PriRespResp0b11RaisesCerrorIll MUST PASS immediately (already implemented).

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>
#include <cstdint>

namespace smmu {
namespace test {

// ── Helpers ──────────────────────────────────────────────────────────────────

static constexpr StreamID kSecureSid = 0x0300u;
static constexpr StreamID kNsSid     = 0x0301u;
static constexpr PASID    kPasid     = 0u;

static void configureStreamPri(SMMU& smmu, StreamID sid,
                                SecurityState ss = SecurityState::NonSecure)
{
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = ss;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, kPasid);
}

static PRIEntry makePriEntry(StreamID sid, SecurityState ss,
                              bool isLast = true)
{
    PRIEntry e;
    e.streamID         = sid;
    e.pasid            = kPasid;
    e.requestedAddress = 0x8000u;
    e.accessType       = AccessType::Read;
    e.isLastRequest    = isLast;
    e.prgIndex         = 0u;
    e.securityState    = ss;
    return e;
}

// ── Fixture ───────────────────────────────────────────────────────────────────

class Sec8PriTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_ = std::make_unique<SMMU>();
        smmu_->enable();
        smmu_->setCR0(smmu_->getCR0() | SMMU::CR0_EVENTQEN
                                      | SMMU::CR0_PRIQEN
                                      | SMMU::CR0_CMDQEN);
    }
    std::unique_ptr<SMMU> smmu_;
};

// ── §8.2 Secure stream PRI discard ───────────────────────────────────────────

// A PRI request whose securityState == Secure must be discarded (not enqueued).
// ARM §8.2: Secure PPRs are discarded with ResponseCode=0b1111 (PRIF).
// Before the fix: submitPageRequest() enqueues it; queue size grows to 1.
// After  the fix: submitPageRequest() discards it; queue size stays at 0.
TEST_F(Sec8PriTest, SecureStreamPriDiscarded) {
    configureStreamPri(*smmu_, kSecureSid, SecurityState::Secure);

    ASSERT_EQ(smmu_->getPRIQueueSize(), 0u)
        << "Pre-condition: PRI queue must be empty";

    PRIEntry secureReq = makePriEntry(kSecureSid, SecurityState::Secure, true);
    smmu_->submitPageRequest(secureReq);

    // §8.2: Secure PPR must be discarded — queue must remain empty.
    EXPECT_EQ(smmu_->getPRIQueueSize(), 0u)
        << "§8.2: Secure stream PRI request must be discarded (not enqueued)";
}

// Non-secure PRI request must still be enqueued correctly (regression guard).
TEST_F(Sec8PriTest, NonSecureStreamPriAccepted) {
    configureStreamPri(*smmu_, kNsSid, SecurityState::NonSecure);

    PRIEntry nsReq = makePriEntry(kNsSid, SecurityState::NonSecure, true);
    smmu_->submitPageRequest(nsReq);

    EXPECT_EQ(smmu_->getPRIQueueSize(), 1u)
        << "§8.2: Non-secure stream PRI request must be enqueued normally";
}

// ── §8.2 PRIQ_ABT_ERR gate ───────────────────────────────────────────────────

// Verify PRIQ_ABT_ERR starts inactive (structural test).
TEST_F(Sec8PriTest, PriqAbtErrStartsInactive) {
    EXPECT_EQ(smmu_->getGerror() & GERROR_PRIQ_ABT_ERR, 0u)
        << "§8.2: GERROR.PRIQ_ABT_ERR must be inactive (0) at startup";
}

// Verify GERROR_PRIQ_ABT_ERR constant is at the correct bit position (bit 3).
TEST_F(Sec8PriTest, PriqAbtErrConstantIsBit3) {
    EXPECT_EQ(GERROR_PRIQ_ABT_ERR, static_cast<uint32_t>(1u << 3))
        << "§8.2: GERROR_PRIQ_ABT_ERR must be bit 3 per ARM §6.3.17";
}

// ── §8.3 PRG Response Resp=0b11 → CERROR_ILL ─────────────────────────────────

// ARM §8.3 / §4.5.2: CMD_PRI_RESP with Resp=0b11 is Reserved/ILLEGAL.
// Must raise CERROR_ILL and signal GERROR_CMDQ_ERR.
// Already implemented (BUG-NEW-A). This is a regression guard.
TEST_F(Sec8PriTest, PriRespResp0b11RaisesCerrorIll) {
    configureStreamPri(*smmu_, kNsSid, SecurityState::NonSecure);

    // Enqueue a valid PPR first so the PRI queue is non-empty.
    PRIEntry ppr = makePriEntry(kNsSid, SecurityState::NonSecure, true);
    smmu_->submitPageRequest(ppr);
    ASSERT_EQ(smmu_->getPRIQueueSize(), 1u)
        << "Pre-condition: PPR must be enqueued before sending PRI_RESP";

    // Submit CMD_PRI_RESP with Resp=0b11 (Reserved/ILLEGAL).
    // The 2-bit Resp field is in bits[1:0] of the flags field per ARM §4.5.2.
    CommandEntry cmd;
    cmd.type         = CommandType::PRI_RESP;
    cmd.streamID     = kNsSid;
    cmd.pasid        = kPasid;
    cmd.prgIndex     = 0u;
    cmd.flags        = 0x03u; // Resp = 0b11 (ILLEGAL)
    cmd.timestamp    = 0u;
    cmd.startAddress = 0u;
    cmd.endAddress   = 0u;
    smmu_->submitCommand(cmd);
    smmu_->processCommandQueue();

    // §8.3 / §4.5.2: GERROR.CMDQ_ERR must be active after Resp=0b11.
    EXPECT_NE(smmu_->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "§8.3 / §4.5.2: CMD_PRI_RESP with Resp=0b11 must set GERROR.CMDQ_ERR";
}

// Verify that a valid Resp value (0b00 = Success) does NOT raise CERROR_ILL.
TEST_F(Sec8PriTest, PriRespResp0b00IsValid) {
    configureStreamPri(*smmu_, kNsSid, SecurityState::NonSecure);

    PRIEntry ppr = makePriEntry(kNsSid, SecurityState::NonSecure, true);
    smmu_->submitPageRequest(ppr);

    CommandEntry cmd;
    cmd.type         = CommandType::PRI_RESP;
    cmd.streamID     = kNsSid;
    cmd.pasid        = kPasid;
    cmd.prgIndex     = 0u;
    cmd.flags        = 0x00u; // Resp = 0b00 (Success — valid)
    cmd.timestamp    = 0u;
    cmd.startAddress = 0u;
    cmd.endAddress   = 0u;
    smmu_->submitCommand(cmd);
    smmu_->processCommandQueue();

    // §8.3: Valid Resp=0b00 must NOT set GERROR.CMDQ_ERR.
    EXPECT_EQ(smmu_->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "§8.3: CMD_PRI_RESP with valid Resp=0b00 must not raise GERROR.CMDQ_ERR";
}

} // namespace test
} // namespace smmu
