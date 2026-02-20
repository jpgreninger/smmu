//! Test serde serialization for all public types

#[cfg(feature = "serde")]
#[cfg(test)]
mod serde_tests {
    use smmu::types::*;

    #[test]
    fn test_stream_id_serde() {
        let stream_id = StreamID::new(42).unwrap();
        let serialized = serde_json::to_string(&stream_id).unwrap();
        let deserialized: StreamID = serde_json::from_str(&serialized).unwrap();
        assert_eq!(stream_id, deserialized);
    }

    #[test]
    fn test_pasid_serde() {
        let pasid = PASID::new(1234).unwrap();
        let serialized = serde_json::to_string(&pasid).unwrap();
        let deserialized: PASID = serde_json::from_str(&serialized).unwrap();
        assert_eq!(pasid, deserialized);
    }

    #[test]
    fn test_access_type_serde() {
        let access = AccessType::ReadWrite;
        let serialized = serde_json::to_string(&access).unwrap();
        let deserialized: AccessType = serde_json::from_str(&serialized).unwrap();
        assert_eq!(access, deserialized);
    }

    #[test]
    fn test_security_state_serde() {
        let state = SecurityState::Secure;
        let serialized = serde_json::to_string(&state).unwrap();
        let deserialized: SecurityState = serde_json::from_str(&serialized).unwrap();
        assert_eq!(state, deserialized);
    }

    #[test]
    fn test_iova_serde() {
        let iova = IOVA::new(0x1000).unwrap();
        let serialized = serde_json::to_string(&iova).unwrap();
        let deserialized: IOVA = serde_json::from_str(&serialized).unwrap();
        assert_eq!(iova, deserialized);
    }

    #[test]
    fn test_page_permissions_serde() {
        let perms = PagePermissions::read_write();
        let serialized = serde_json::to_string(&perms).unwrap();
        let deserialized: PagePermissions = serde_json::from_str(&serialized).unwrap();
        assert_eq!(perms, deserialized);
    }

    #[test]
    fn test_translation_data_serde() {
        let pa = PA::new(0x2000).unwrap();
        let data = TranslationData::new(pa, PagePermissions::read_only(), SecurityState::NonSecure);
        let serialized = serde_json::to_string(&data).unwrap();
        let deserialized: TranslationData = serde_json::from_str(&serialized).unwrap();
        assert_eq!(data, deserialized);
    }

    #[test]
    fn test_fault_type_serde() {
        let fault = FaultType::TranslationFault;
        let serialized = serde_json::to_string(&fault).unwrap();
        let deserialized: FaultType = serde_json::from_str(&serialized).unwrap();
        assert_eq!(fault, deserialized);
    }

    #[test]
    fn test_fault_record_serde() {
        let record = FaultRecord::new(
            StreamID::new(1).unwrap(),
            PASID::new(0).unwrap(),
            IOVA::new(0x1000).unwrap(),
            FaultType::PermissionFault,
            AccessType::Write,
            SecurityState::NonSecure,
        );
        let serialized = serde_json::to_string(&record).unwrap();
        let deserialized: FaultRecord = serde_json::from_str(&serialized).unwrap();
        assert_eq!(record, deserialized);
    }

    #[test]
    fn test_translation_stage_serde() {
        let stage = TranslationStage::Stage1And2;
        let serialized = serde_json::to_string(&stage).unwrap();
        let deserialized: TranslationStage = serde_json::from_str(&serialized).unwrap();
        assert_eq!(stage, deserialized);
    }

    #[test]
    fn test_event_entry_serde() {
        let event = EventEntry::new(EventType::FTranslation, 42, 1, 0x1000);
        let serialized = serde_json::to_string(&event).unwrap();
        let deserialized: EventEntry = serde_json::from_str(&serialized).unwrap();
        assert_eq!(event, deserialized);
    }

    #[test]
    fn test_command_entry_serde() {
        let cmd = CommandEntry::new(CommandType::Sync, 42, 1);
        let serialized = serde_json::to_string(&cmd).unwrap();
        let deserialized: CommandEntry = serde_json::from_str(&serialized).unwrap();
        assert_eq!(cmd, deserialized);
    }

    #[test]
    fn test_queue_statistics_serde() {
        let stats = QueueStatistics::new(10, 5, 2, 512, 256, 128);
        let serialized = serde_json::to_string(&stats).unwrap();
        let deserialized: QueueStatistics = serde_json::from_str(&serialized).unwrap();
        assert_eq!(stats.event_queue_size(), deserialized.event_queue_size());
    }

    #[test]
    fn test_stream_config_serde() {
        let config = StreamConfig::stage1_only();
        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: StreamConfig = serde_json::from_str(&serialized).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_validation_error_serde() {
        let error = ValidationError::OutOfRange {
            field: "test".to_string(),
            value: 100,
            max: 50,
        };
        let serialized = serde_json::to_string(&error).unwrap();
        let deserialized: ValidationError = serde_json::from_str(&serialized).unwrap();
        assert_eq!(error, deserialized);
    }
}
