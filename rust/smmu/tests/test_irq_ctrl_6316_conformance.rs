//! Tests for §6.3.16 `SMMU_IRQ_CTRL` conformance (BUG-IRQ-01, BUG-IRQ-02)

use smmu::SMMU;

/// BUG-IRQ-01: Bits [31:3] are RES0 — writes must be ignored (masked to bits [2:0]).
#[test]
fn test_irq_ctrl_res0_bits_masked_on_write() {
    let smmu = SMMU::new();
    smmu.set_irq_ctrl(0xFFFF_FFFF);
    assert_eq!(
        smmu.get_irq_ctrlack(),
        0x7,
        "bits [31:3] are RES0 and must be masked to 0 on write; only bits [2:0] survive"
    );
}

/// Sanity: all three valid enable bits survive a write of 0x7.
#[test]
fn test_irq_ctrl_valid_bits_preserved() {
    let smmu = SMMU::new();
    smmu.set_irq_ctrl(0x7);
    assert_eq!(
        smmu.get_irq_ctrlack(),
        0x7,
        "all three enable bits [2:0] must be preserved when written"
    );
}

/// BUG-IRQ-02: `PRIQ_IRQEN` (bit 1) is RES0 when IDR0.PRI == 0.
#[test]
fn test_irq_ctrl_priq_irqen_res0_when_pri_disabled() {
    let smmu = SMMU::new();
    smmu.set_pri_supported(false);
    smmu.set_irq_ctrl(0x7);
    let ctrlack = smmu.get_irq_ctrlack();
    assert_eq!(
        ctrlack & 0x2,
        0,
        "PRIQ_IRQEN (bit 1) must be forced to 0 when IDR0.PRI == 0"
    );
    // bits 0 and 2 must still be stored
    assert_eq!(
        ctrlack & 0x5,
        0x5,
        "EVTQ_IRQEN (bit 0) and GERROR_IRQEN (bit 2) must still be preserved when PRI is disabled"
    );
}

/// BUG-IRQ-02: `PRIQ_IRQEN` (bit 1) is settable when IDR0.PRI == 1 (default).
#[test]
fn test_irq_ctrl_priq_irqen_settable_when_pri_enabled() {
    let smmu = SMMU::new(); // pri_supported defaults to true
    smmu.set_pri_supported(true);
    smmu.set_irq_ctrl(0x2);
    assert_eq!(
        smmu.get_irq_ctrlack(),
        0x2,
        "PRIQ_IRQEN (bit 1) must be settable when IDR0.PRI == 1"
    );
}

/// After `SMMU::new()` the reset value of `IRQ_CTRLACK` must be 0.
#[test]
fn test_irq_ctrl_reset_value_zero() {
    let smmu = SMMU::new();
    assert_eq!(
        smmu.get_irq_ctrlack(),
        0,
        "IRQ_CTRLACK reset value must be 0 per §6.3.16"
    );
}

/// In the synchronous SW model `IRQ_CTRLACK` mirrors `IRQ_CTRL` after each write.
#[test]
fn test_irq_ctrl_ctrlack_mirrors_ctrl() {
    let smmu = SMMU::new();
    smmu.set_irq_ctrl(0x5);
    assert_eq!(
        smmu.get_irq_ctrlack(),
        0x5,
        "IRQ_CTRLACK must mirror IRQ_CTRL immediately in the synchronous model"
    );
}
