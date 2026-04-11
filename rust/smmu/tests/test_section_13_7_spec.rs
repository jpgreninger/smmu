//! ARM SMMU v3 §13.7 PCIe permission attribute interpretation conformance tests
//! BUG-13.7-A: EATS=0b10 AtsTranslated + ATSCHK=1 skips INSTCFG/PRIVCFG overrides

#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]

#[cfg(test)]
mod tests_13_7 {
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

    /// Build an SMMU with SMMUEN=1, ATSCHK=1, EVENTQEN=1.
    fn make_smmu_atschk() -> SMMU {
        let smmu = SMMU::new();
        smmu.enable().unwrap();
        let cr0 = smmu.get_cr0();
        smmu.set_cr0(cr0 | SMMU::CR0_ATSCHK | SMMU::CR0_EVENTQEN);
        smmu
    }

    /// Set up split-stage (EATS=0b10) two-stage stream with INSTCFG=Force-Data (2).
    ///
    /// STE.INSTCFG=Force-Data means all accesses should be treated as Data
    /// (Execute → Read; ReadExecute → Read).
    ///
    /// Stage-1: VA 0x1000 → IPA 0x2000 (execute permission granted)
    /// Stage-2: IPA 0x2000 → PA 0x3000 (execute permission denied — execute-only
    ///          privilege enforcement doesn't matter; the INSTCFG override should
    ///          demote Execute→Data before stage-2 permission check)
    ///
    /// With INSTCFG=Force-Data, an Execute AtsTranslated request must be treated
    /// as a Read access for stage-2 permission checking. Since stage-2 grants
    /// read+write, a Read access must succeed.
    fn setup_split_stage_instcfg_force_data(smmu: &SMMU, stream_n: u32) {
        let mut cfg = StreamConfig::builder()
            .stage1_enabled(true)
            .stage2_enabled(true)
            .translation_enabled(true)
            .fault_mode(FaultMode::Terminate)
            .build()
            .unwrap();
        cfg.eats = 2; // EATS=0b10: split-stage ATS
        cfg.inst_cfg = 2; // INSTCFG=Force-Data: Execute→Read
        smmu.configure_stream(sid(stream_n), cfg).unwrap();
        smmu.create_pasid(sid(stream_n), pasid(0)).unwrap();

        // Stage-1: VA 0x1000 → IPA 0x2000 (read+write+execute)
        smmu.map_page(
            sid(stream_n),
            pasid(0),
            iova(0x1000),
            pa(0x2000),
            PagePermissions::all(),
            SecurityState::NonSecure,
        )
        .unwrap();

        // Stage-2: IPA 0x2000 → PA 0x3000 (read+write, NO execute)
        smmu.create_stage2_address_space(sid(stream_n)).unwrap();
        smmu.map_stage2_page(
            sid(stream_n),
            iova(0x2000),
            pa(0x3000),
            PagePermissions::read_write(), // no execute at stage-2
            SecurityState::NonSecure,
        )
        .unwrap();
    }

    /// §13.7 BUG-13.7-A: EATS=0b10 AtsTranslated + ATSCHK=1 must apply INSTCFG
    /// overrides BEFORE stage-2 translation.
    ///
    /// Scenario: STE.INSTCFG=Force-Data. Device submits AtsTranslated(0x2000)
    /// with Execute access. Without override: Execute hits stage-2 which has
    /// no execute permission → PermissionViolation. With correct override:
    /// Execute is first demoted to Read by INSTCFG=Force-Data, then stage-2
    /// is checked for Read → OK (stage-2 has read-write).
    ///
    /// BUG: The current EATS=2 ATSCHK path calls translate_stage2_only(access)
    /// with the raw Execute access, bypassing effective_access_type(). So Execute
    /// hits stage-2 which has no execute permission and returns PermissionViolation.
    #[test]
    fn test_13_7_a_eats2_atschk_applies_instcfg_before_stage2() {
        let smmu = make_smmu_atschk();
        let stream_n = 0xD7_00u32;
        setup_split_stage_instcfg_force_data(&smmu, stream_n);

        // Device sends AtsTranslated with Execute access and IPA 0x2000.
        // INSTCFG=Force-Data must demote Execute→Read before stage-2 check.
        // Stage-2 has read-write (no execute), so after override Read succeeds.
        let result = smmu.translate_with_type(
            sid(stream_n),
            pasid(0),
            iova(0x2000), // IPA from split-stage ATS completion
            AccessType::Execute, // raw execute access from device
            SecurityState::NonSecure,
            TransactionType::AtsTranslated,
        );

        assert!(
            result.is_ok(),
            "§13.7: AtsTranslated EATS=0b10 ATSCHK=1 must apply INSTCFG=Force-Data \
             (Execute→Read) before stage-2 permission check; stage-2 has read-write \
             so Read must succeed. Got: {:?}",
            result
        );

        let data = result.unwrap();
        assert_eq!(
            data.physical_address().as_u64(),
            0x3000,
            "§13.7: IPA 0x2000 must map to PA 0x3000 via stage-2-only after override"
        );
    }

    /// §13.7 BUG-13.7-A negative: without INSTCFG override, Execute on a
    /// read-write-only stage-2 page must fail (confirming test setup is correct).
    #[test]
    fn test_13_7_a_without_instcfg_execute_fails_on_rw_stage2() {
        let smmu = make_smmu_atschk();
        let stream_n = 0xD7_01u32;

        // Same two-stage setup but INSTCFG=Use-Incoming (0) — no override.
        let mut cfg = StreamConfig::builder()
            .stage1_enabled(true)
            .stage2_enabled(true)
            .translation_enabled(true)
            .fault_mode(FaultMode::Terminate)
            .build()
            .unwrap();
        cfg.eats = 2;
        cfg.inst_cfg = 0; // Use-Incoming — no override
        smmu.configure_stream(sid(stream_n), cfg).unwrap();
        smmu.create_pasid(sid(stream_n), pasid(0)).unwrap();

        smmu.map_page(
            sid(stream_n),
            pasid(0),
            iova(0x1000),
            pa(0x2000),
            PagePermissions::all(),
            SecurityState::NonSecure,
        )
        .unwrap();

        smmu.create_stage2_address_space(sid(stream_n)).unwrap();
        smmu.map_stage2_page(
            sid(stream_n),
            iova(0x2000),
            pa(0x3000),
            PagePermissions::read_write(), // no execute
            SecurityState::NonSecure,
        )
        .unwrap();

        // Execute access to IPA 0x2000 without INSTCFG override:
        // stage-2 has no execute permission → must fail.
        let result = smmu.translate_with_type(
            sid(stream_n),
            pasid(0),
            iova(0x2000),
            AccessType::Execute,
            SecurityState::NonSecure,
            TransactionType::AtsTranslated,
        );

        assert!(
            result.is_err(),
            "§13.7 negative: Execute on read-write-only stage-2 page without \
             INSTCFG override must fail; got Ok"
        );
    }
}
