//! Tests for timestamp optimization in fault recording

#[cfg(test)]
mod timestamp_tests {
    use smmu::prelude::*;

    #[test]
    fn test_fault_timestamps_monotonic() {
        let smmu = SMMU::new();
        smmu.enable().unwrap(); // SMMUEN=1 required to reach fault path (§6.3.9)
        let stream_id = StreamID::new(1).unwrap();
        smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();

        let pasid = PASID::new(0).unwrap();
        smmu.create_pasid(stream_id, pasid).unwrap();

        // Trigger multiple faults
        for i in 0..10 {
            let iova = IOVA::new(0x1000 * (i + 1)).unwrap();
            let _ = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
        }

        // Verify timestamps are monotonically increasing
        let faults = smmu.get_faults();
        assert_eq!(faults.len(), 10);

        for i in 1..faults.len() {
            assert!(
                faults[i].timestamp() > faults[i - 1].timestamp(),
                "Timestamp {} ({}) should be > timestamp {} ({})",
                i,
                faults[i].timestamp(),
                i - 1,
                faults[i - 1].timestamp()
            );
        }
    }

    #[test]
    fn test_no_systemtime_overhead() {
        use std::time::Instant;

        let smmu = SMMU::new();
        let stream_id = StreamID::new(1).unwrap();
        smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();

        let pasid = PASID::new(0).unwrap();
        smmu.create_pasid(stream_id, pasid).unwrap();

        // Measure time to generate 1000 faults
        let iova = IOVA::new(0x1000).unwrap();
        let start = Instant::now();

        for _ in 0..1000 {
            let _ = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
        }

        let duration = start.elapsed();
        println!("1000 faults generated in {duration:?}");

        // With atomic counter-based timestamps, this should be very fast
        // Each fault now involves:
        // - StreamContext-level recording (atomic increment + try-lock + builder)
        // - SMMU-level recording (atomic increment + mutex + builder + event queue)
        // Total overhead per fault: ~200-300ns with double recording
        // So 1000 faults should complete in < 3ms (with margin for CI/slow machines)
        //
        // Note: If we had SystemTime::now() calls, each would add 20-50ns syscall overhead
        // which would push total time to 5-10ms+ due to repeated syscalls.
        assert!(duration.as_micros() < 3000, "Fault recording took too long: {duration:?}");
    }
}
