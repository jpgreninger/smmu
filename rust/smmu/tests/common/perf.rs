//! Performance testing utilities
//!
//! Provides helpers for measuring and validating performance characteristics
//! in tests and benchmarks.

use std::time::{Duration, Instant};

/// Performance measurement result
#[derive(Debug, Clone)]
pub struct PerfResult {
    pub duration: Duration,
    pub iterations: usize,
    pub avg_ns: u64,
    pub min_ns: u64,
    pub max_ns: u64,
}

impl PerfResult {
    /// Check if performance meets target
    #[must_use]
    pub fn meets_target(&self, target_ns: u64, tolerance_percent: f64) -> bool {
        let tolerance = (target_ns as f64 * tolerance_percent / 100.0) as u64;
        self.avg_ns <= target_ns + tolerance
    }

    /// Get throughput in operations per second
    #[must_use]
    pub fn throughput(&self) -> f64 {
        if self.avg_ns == 0 {
            return 0.0;
        }
        1_000_000_000.0 / self.avg_ns as f64
    }
}

/// Simple performance measurement helper
pub struct PerfTimer {
    start: Instant,
    iterations: Vec<Duration>,
}

impl PerfTimer {
    /// Create a new performance timer
    #[must_use]
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            iterations: Vec::new(),
        }
    }

    /// Start timing a new iteration
    pub fn start_iteration(&mut self) {
        self.start = Instant::now();
    }

    /// End current iteration and record time
    pub fn end_iteration(&mut self) {
        let elapsed = self.start.elapsed();
        self.iterations.push(elapsed);
    }

    /// Time a closure and record the duration
    pub fn time_operation<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        self.start_iteration();
        let result = f();
        self.end_iteration();
        result
    }

    /// Get performance result
    #[must_use]
    pub fn result(&self) -> PerfResult {
        if self.iterations.is_empty() {
            return PerfResult {
                duration: Duration::ZERO,
                iterations: 0,
                avg_ns: 0,
                min_ns: 0,
                max_ns: 0,
            };
        }

        let total: Duration = self.iterations.iter().sum();
        let avg_ns = (total.as_nanos() / self.iterations.len() as u128) as u64;

        let min_ns = self
            .iterations
            .iter()
            .map(|d| d.as_nanos() as u64)
            .min()
            .unwrap_or(0);

        let max_ns = self
            .iterations
            .iter()
            .map(|d| d.as_nanos() as u64)
            .max()
            .unwrap_or(0);

        PerfResult {
            duration: total,
            iterations: self.iterations.len(),
            avg_ns,
            min_ns,
            max_ns,
        }
    }

    /// Reset timer
    pub fn reset(&mut self) {
        self.iterations.clear();
    }
}

impl Default for PerfTimer {
    fn default() -> Self {
        Self::new()
    }
}

/// Warmup helper - runs operations to warm up caches
pub fn warmup<F>(iterations: usize, mut f: F)
where
    F: FnMut(),
{
    for _ in 0..iterations {
        f();
    }
}

/// Measure average time of repeated operation
#[must_use]
pub fn measure_average<F, R>(iterations: usize, mut f: F) -> PerfResult
where
    F: FnMut() -> R,
{
    let mut timer = PerfTimer::new();

    for _ in 0..iterations {
        timer.time_operation(&mut f);
    }

    timer.result()
}

/// Measure operation with warmup period
#[must_use]
pub fn measure_with_warmup<F, R>(warmup_iters: usize, measure_iters: usize, mut f: F) -> PerfResult
where
    F: FnMut() -> R,
{
    // Warmup phase
    warmup(warmup_iters, || {
        f();
    });

    // Measurement phase
    measure_average(measure_iters, f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_perf_timer_basic() {
        let mut timer = PerfTimer::new();

        timer.time_operation(|| {
            // Simulate some work
            std::thread::sleep(Duration::from_micros(10));
        });

        let result = timer.result();
        assert_eq!(result.iterations, 1);
        assert!(result.avg_ns > 0);
    }

    #[test]
    fn test_perf_timer_multiple_iterations() {
        let mut timer = PerfTimer::new();

        for _ in 0..5 {
            timer.time_operation(|| {
                // Minimal work
            });
        }

        let result = timer.result();
        assert_eq!(result.iterations, 5);
        assert!(result.min_ns <= result.avg_ns);
        assert!(result.max_ns >= result.avg_ns);
    }

    #[test]
    fn test_perf_result_meets_target() {
        let result = PerfResult {
            duration: Duration::from_nanos(100),
            iterations: 1,
            avg_ns: 100,
            min_ns: 100,
            max_ns: 100,
        };

        assert!(result.meets_target(135, 10.0));
        assert!(!result.meets_target(90, 10.0));
    }

    #[test]
    fn test_perf_result_throughput() {
        let result = PerfResult {
            duration: Duration::from_nanos(100),
            iterations: 1,
            avg_ns: 100,
            min_ns: 100,
            max_ns: 100,
        };

        let throughput = result.throughput();
        assert_eq!(throughput, 10_000_000.0); // 10M ops/sec for 100ns
    }

    #[test]
    fn test_measure_average() {
        let result = measure_average(10, || {
            // Minimal operation
            42
        });

        assert_eq!(result.iterations, 10);
    }

    #[test]
    fn test_measure_with_warmup() {
        let result = measure_with_warmup(5, 10, || {
            // Minimal operation
            42
        });

        assert_eq!(result.iterations, 10);
    }
}
