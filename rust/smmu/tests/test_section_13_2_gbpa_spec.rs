//! ARM SMMU v3 §13.2 GBPA bypass conformance tests
//! BUG-13.2-A: GBPA bypass missing Device/NC→OSH enforcement per §13.1.7

#[cfg(test)]
mod tests_13_2 {
    use smmu::smmu::{GbpaConfig, SMMU};
    use smmu::types::{AccessType, SecurityState, StreamID, IOVA, PASID};

    fn make_smmu() -> SMMU {
        SMMU::new()
    }

    /// §13.1.7 / §13.2: When GBPA.MTCFG=1 with Device `MemAttr` and SHCFG != OSH,
    /// the bypass result must still output OSH (`ensure_consistent_attrs`).
    #[test]
    fn test_13_2_a_device_memattr_forces_osh_over_gbpa_shcfg() {
        let smmu = make_smmu();
        // SMMUEN=0 → bypass mode
        smmu.set_gbpa_config(GbpaConfig {
            abort: false,
            inst_cfg: 0,
            priv_cfg: 0,
            mt_cfg: true,
            mem_attr: 0x00, // Device-nGnRnE
            sh_cfg: 0b00,   // NSH (weakest) — should be overridden to OSH
            alloc_cfg: 0,
        });

        let sid = StreamID::new(1).unwrap();
        let pasid = PASID::new(0).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let result = smmu
            .translate(sid, pasid, iova, AccessType::Read, SecurityState::NonSecure)
            .unwrap();

        assert_eq!(
            result.shareability(),
            0b10,
            "§13.2/§13.1.7: Device GBPA bypass must force OSH (0b10), got {:#04b}",
            result.shareability()
        );
    }

    /// §13.2: When GBPA.MTCFG=1 with Normal-iNC-oNC (0x44), must also force OSH.
    #[test]
    fn test_13_2_a_nc_memattr_forces_osh_over_gbpa_shcfg() {
        let smmu = make_smmu();
        smmu.set_gbpa_config(GbpaConfig {
            abort: false,
            inst_cfg: 0,
            priv_cfg: 0,
            mt_cfg: true,
            mem_attr: 0x44, // Normal-iNC-oNC
            sh_cfg: 0b01,   // ISH — should be overridden to OSH
            alloc_cfg: 0,
        });

        let sid = StreamID::new(1).unwrap();
        let pasid = PASID::new(0).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let result = smmu
            .translate(sid, pasid, iova, AccessType::Read, SecurityState::NonSecure)
            .unwrap();

        assert_eq!(
            result.shareability(),
            0b10,
            "§13.2/§13.1.7: Normal-NC GBPA bypass must force OSH (0b10), got {:#04b}",
            result.shareability()
        );
    }

    /// §13.2: Normal-WB (0xFF) with SHCFG=ISH should NOT be forced to OSH.
    #[test]
    fn test_13_2_normal_wb_preserves_gbpa_shcfg() {
        let smmu = make_smmu();
        smmu.set_gbpa_config(GbpaConfig {
            abort: false,
            inst_cfg: 0,
            priv_cfg: 0,
            mt_cfg: true,
            mem_attr: 0xFF, // Normal-WB (not device, not NC)
            sh_cfg: 0b01,   // ISH — should be preserved
            alloc_cfg: 0,
        });

        let sid = StreamID::new(1).unwrap();
        let pasid = PASID::new(0).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let result = smmu
            .translate(sid, pasid, iova, AccessType::Read, SecurityState::NonSecure)
            .unwrap();

        assert_eq!(
            result.shareability(),
            0b01,
            "§13.2: Normal-WB GBPA bypass should preserve SHCFG=ISH (0b01), got {:#04b}",
            result.shareability()
        );
    }

    /// §13.2: When GBPA.MTCFG=0 (no mem type override), `sh_cfg` passes through unchanged.
    #[test]
    fn test_13_2_no_mtcfg_preserves_shcfg() {
        let smmu = make_smmu();
        smmu.set_gbpa_config(GbpaConfig {
            abort: false,
            inst_cfg: 0,
            priv_cfg: 0,
            mt_cfg: false,  // no mem type override
            mem_attr: 0x00, // irrelevant when mt_cfg=false
            sh_cfg: 0b01,   // ISH — should pass through
            alloc_cfg: 0,
        });

        let sid = StreamID::new(1).unwrap();
        let pasid = PASID::new(0).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let result = smmu
            .translate(sid, pasid, iova, AccessType::Read, SecurityState::NonSecure)
            .unwrap();

        assert_eq!(
            result.shareability(),
            0b01,
            "§13.2: mt_cfg=false should preserve SHCFG=ISH (0b01), got {:#04b}",
            result.shareability()
        );
    }
}
