//! ARM SMMU v3 §13.6.3 Split-stage ATS conformance tests
//! BUG-13.6.3-A: AtsTranslated + ATSCHK=1 + EATS=0b10 must run stage-2-only
//! on the incoming IPA, not full stage-1+stage-2 translate.

#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]

#[cfg(test)]
mod tests_13_6_3 {
    use smmu::smmu::SMMU;
    use smmu::types::{
        AccessType, FaultMode, PagePermissions, SecurityState, StreamConfig, StreamID,
        TransactionType, IOVA, PA, PASID,
    };

    fn sid(n: u32) -> StreamID {
        StreamID::new(n).unwrap()
    }

    fn pasid(n: u32) -> PASID {
        PASID::new(n).unwrap()
    }

    fn iova(addr: u64) -> IOVA {
        IOVA::new(addr).unwrap()
    }

    fn pa(addr: u64) -> PA {
        PA::new(addr).unwrap()
    }

    fn make_smmu_atschk() -> SMMU {
        let smmu = SMMU::new();
        smmu.enable().unwrap();
        // Enable ATSCHK (bit 4 of CR0)
        let cr0 = smmu.get_cr0();
        smmu.set_cr0(cr0 | SMMU::CR0_ATSCHK | SMMU::CR0_EVENTQEN);
        smmu
    }

    /// §13.6.3: Set up split-stage (EATS=0b10) two-stage stream.
    ///
    /// Stage-1: VA 0x1000 → IPA 0x2000 (only this VA is mapped in stage-1)
    /// Stage-2: IPA 0x2000 → PA 0x3000
    ///
    /// The ATS TR would return IPA 0x2000 to the device ATC.
    /// A subsequent AtsTranslated transaction uses 0x2000 as input (IPA).
    fn setup_split_stage_stream(smmu: &SMMU, stream_n: u32) {
        let mut cfg = StreamConfig::builder()
            .stage1_enabled(true)
            .stage2_enabled(true)
            .translation_enabled(true)
            .fault_mode(FaultMode::Terminate)
            .build()
            .unwrap();
        cfg.eats = 2; // EATS=0b10: split-stage ATS
        smmu.configure_stream(sid(stream_n), cfg).unwrap();
        smmu.create_pasid(sid(stream_n), pasid(0)).unwrap();

        // Stage-1: VA 0x1000 → IPA 0x2000 (read-write)
        smmu.map_page(
            sid(stream_n),
            pasid(0),
            iova(0x1000),
            pa(0x2000),
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .unwrap();

        // Stage-2: IPA 0x2000 → PA 0x3000 (read-write)
        smmu.create_stage2_address_space(sid(stream_n)).unwrap();
        smmu.map_stage2_page(
            sid(stream_n),
            iova(0x2000),
            pa(0x3000),
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .unwrap();
    }

    /// §13.6.3 BUG-13.6.3-A: AtsTranslated + ATSCHK=1 + EATS=0b10.
    ///
    /// The device received IPA 0x2000 from the split-stage ATS TR completion.
    /// When it resubmits as AtsTranslated(0x2000), the SMMU must run stage-2-only
    /// on the IPA 0x2000, yielding PA 0x3000.
    ///
    /// BUG: Currently ATSCHK=1 path calls self.translate(0x2000) which runs
    /// stage-1+stage-2. Address 0x2000 is NOT mapped in stage-1 tables
    /// (only 0x1000 is), so the full translate fails with PageNotMapped.
    /// The correct behavior is to run translate_stage2_only(0x2000) → PA 0x3000.
    #[test]
    fn test_13_6_3_a_atschk_eats2_ats_translated_runs_stage2_only() {
        let smmu = make_smmu_atschk();
        let stream_n = 0xD6_30u32;
        setup_split_stage_stream(&smmu, stream_n);

        // Device sends AtsTranslated with IPA 0x2000 (what it cached from ATS TR).
        // ATSCHK=1, EATS=0b10 → must run stage-2-only on the IPA.
        let result = smmu.translate_with_type(
            sid(stream_n),
            pasid(0),
            iova(0x2000), // IPA from split-stage ATS completion
            AccessType::Read,
            SecurityState::NonSecure,
            TransactionType::AtsTranslated,
        );

        assert!(
            result.is_ok(),
            "§13.6.3: AtsTranslated + ATSCHK=1 + EATS=0b10 should run \
             stage-2-only on IPA 0x2000 → PA 0x3000; got: {:?}",
            result
        );

        let data = result.unwrap();
        assert_eq!(
            data.physical_address().as_u64(),
            0x3000,
            "§13.6.3: AtsTranslated IPA 0x2000 must map to PA 0x3000 via stage-2-only"
        );
    }

    /// §13.6.3 sanity: ordinary full translate of VA 0x1000 yields PA 0x3000 (two-stage).
    /// This confirms the mapping setup is correct (S1: 0x1000→0x2000, S2: 0x2000→0x3000).
    #[test]
    fn test_13_6_3_sanity_two_stage_ordinary_translate() {
        let smmu = make_smmu_atschk();
        let stream_n = 0xD6_31u32;
        setup_split_stage_stream(&smmu, stream_n);

        let result = smmu.translate(
            sid(stream_n),
            pasid(0),
            iova(0x1000),
            AccessType::Read,
            SecurityState::NonSecure,
        );

        assert!(
            result.is_ok(),
            "§13.6.3 sanity: two-stage translate VA 0x1000 must succeed; got: {:?}",
            result
        );
        assert_eq!(
            result.unwrap().physical_address().as_u64(),
            0x3000,
            "§13.6.3 sanity: VA 0x1000 must translate to PA 0x3000 via two stages"
        );
    }
}
