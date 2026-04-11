//! Transaction type for ATS-capable devices (ARM IHI0070G.b §3.9).

/// ARM IHI0070G.b §3.9 — Transaction type for ATS-capable devices.
///
/// Distinguishes ordinary DMA transactions from ATS Translation Requests (TR)
/// and ATS Translated transactions (TT) so the SMMU can apply the correct
/// checks per §3.9.
#[repr(u8)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum TransactionType {
    /// §3.9 OT: Normal DMA transaction (default; preserves existing behaviour).
    #[default]
    Ordinary = 0,
    /// §3.9 TR: ATS Translation Request — device requesting a cached TLP entry.
    AtsTranslationRequest = 1,
    /// §3.9 TT: ATS Translated transaction — device presenting a pre-translated PA.
    AtsTranslated = 2,
    /// §13.1.4 ATOS: Address Translation Operation — INSTCFG/PRIVCFG/NSCFG/MTCFG/SHCFG/ALLOCCFG
    /// overrides are all ignored; InD and PnU are taken directly from ATOS_ADDR.
    Atos = 3,
}
