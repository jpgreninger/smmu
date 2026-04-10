//! Conformance tests for ARM SMMU v3 §7.5 "Global error recording" and
//! §7.5.1 "GERROR interrupt notification".
//!
//! BUG-GERROR-01: DPT_ERR constant (bit 10) missing.
//! BUG-GERROR-02: signal_gerror() never raises GERROR interrupt notification.
//! BUG-GERROR-03: IRQ_CTRL.GERROR_IRQEN (bit 0) gate not enforced.
#![allow(missing_docs)]
#![allow(clippy::doc_markdown)]
use smmu::types::{CommandEntry, CommandType};
use smmu::SMMU;

fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    smmu
}

/// Trigger a CMDQ_ERR by submitting a CMD_SYNC with CS=3 (Reserved → CERROR_ILL).
fn trigger_cmdq_err(smmu: &SMMU) {
    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 3; // CS=0b11 is Reserved → CERROR_ILL per §4.7.3
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();
}

// ============================================================================
// BUG-GERROR-01: DPT_ERR constant
// ============================================================================

/// §6.3.17: GERROR bit 10 = DPT_ERR (Data Prefetch Table lookup fault).
/// The constant must exist and equal 1 << 10 = 0x400.
#[test]
fn test_7_5_gerror_dpt_err_constant_defined() {
    assert_eq!(
        SMMU::GERROR_DPT_ERR,
        1u32 << 10,
        "§6.3.17: GERROR_DPT_ERR must be bit 10 (0x400)"
    );
}

// ============================================================================
// BUG-GERROR-02/03: §7.5.1 GERROR interrupt notification
// ============================================================================

/// §7.5.1: When GERROR_IRQEN==1 and an error activates, GERROR interrupt
/// must be raised (observable via get_gerror_irq_pending()).
#[test]
fn test_7_5_1_gerror_irq_raised_when_irqen_set() {
    let smmu = make_smmu();

    // Enable GERROR interrupt (bit 0 of IRQ_CTRL).
    smmu.set_irq_ctrl(SMMU::IRQ_CTRL_GERROR_IRQEN);

    // Trigger CMDQ_ERR to activate a GERROR bit.
    trigger_cmdq_err(&smmu);

    assert_ne!(
        smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR, 0,
        "precondition: CMDQ_ERR must be active"
    );

    assert!(
        smmu.get_gerror_irq_pending(),
        "§7.5.1: GERROR interrupt must be pending when GERROR_IRQEN==1 and error activates"
    );
}

/// §7.5.1: When GERROR_IRQEN==0, activating a GERROR error must NOT raise
/// the interrupt.
#[test]
fn test_7_5_1_gerror_irq_not_raised_when_irqen_clear() {
    let smmu = make_smmu();

    // GERROR_IRQEN == 0 (IRQ_CTRL default is 0).
    smmu.set_irq_ctrl(0);

    trigger_cmdq_err(&smmu);

    assert_ne!(
        smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR, 0,
        "precondition: CMDQ_ERR must be active"
    );

    assert!(
        !smmu.get_gerror_irq_pending(),
        "§7.5.1: GERROR interrupt must NOT be pending when GERROR_IRQEN==0"
    );
}

/// §7.5.1: If the error is already active (GERROR_IRQEN==1 but bit already
/// toggled), signal_gerror() must NOT raise a second interrupt.
#[test]
fn test_7_5_1_gerror_irq_not_re_raised_for_already_active_error() {
    let smmu = make_smmu();
    smmu.set_irq_ctrl(SMMU::IRQ_CTRL_GERROR_IRQEN);

    // First signal — activates CMDQ_ERR and raises IRQ.
    trigger_cmdq_err(&smmu);
    assert!(smmu.get_gerror_irq_pending(), "precondition: first signal must raise IRQ");

    // Clear the IRQ pending flag (software ISR would do this).
    smmu.clear_gerror_irq_pending();
    assert!(!smmu.get_gerror_irq_pending(), "precondition: IRQ pending must be clear after clear");

    // Second signal with error still active — must NOT raise IRQ again.
    trigger_cmdq_err(&smmu);

    assert!(
        !smmu.get_gerror_irq_pending(),
        "§7.5: SMMU must not toggle GERROR (and must not raise IRQ) if error already active"
    );
}

/// §7.5.1: After acknowledging the error (clear_gerror) and a new error occurs,
/// the interrupt must be raised again.
#[test]
fn test_7_5_1_gerror_irq_re_raised_after_acknowledgment() {
    let smmu = make_smmu();
    smmu.set_irq_ctrl(SMMU::IRQ_CTRL_GERROR_IRQEN);

    // First error cycle.
    trigger_cmdq_err(&smmu);
    assert!(smmu.get_gerror_irq_pending(), "precondition: first cycle must raise IRQ");
    smmu.clear_gerror_irq_pending();

    // Software acknowledges the GERROR error.
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);
    assert_eq!(
        smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR,
        smmu.get_gerrorn() & SMMU::GERROR_CMDQ_ERR,
        "precondition: after acknowledge, GERROR and GERRORN bits must be equal (inactive)"
    );

    // Second error cycle — error is now inactive, so signal_gerror must re-activate.
    trigger_cmdq_err(&smmu);

    assert!(
        smmu.get_gerror_irq_pending(),
        "§7.5.1: after acknowledgment, a new error must raise the GERROR interrupt again"
    );
}

/// §7.5.1: The IRQ pending flag must be clear when no GERROR error has activated.
/// (Baseline: no spurious interrupt at reset.)
#[test]
fn test_7_5_1_no_spurious_irq_at_reset() {
    let smmu = make_smmu();
    smmu.set_irq_ctrl(SMMU::IRQ_CTRL_GERROR_IRQEN);
    assert!(
        !smmu.get_gerror_irq_pending(),
        "§7.5.1: GERROR IRQ pending must be 0 at reset with no active errors"
    );
}
