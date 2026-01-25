// SPDX-License-Identifier: MIT OR Apache-2.0
//! Transformation metrics and memory profiling
//!
//! This module provides detailed metrics collection for tape-to-tape
//! transformations, including timing, throughput, and memory allocation tracking.

use fionn_core::format::FormatKind;
use std::time::{Duration, Instant};

/// Comprehensive transformation metrics
#[derive(Debug, Clone)]
pub struct TransformMetrics {
    /// Source format
    pub source_format: FormatKind,
    /// Target format
    pub target_format: FormatKind,

    // Timing
    /// Total transformation time
    pub total_time: Duration,
    /// Parse time (source -> tape)
    pub parse_time: Duration,
    /// Emit time (tape -> target)
    pub emit_time: Duration,

    // Sizes
    /// Input size in bytes
    pub input_bytes: usize,
    /// Output size in bytes
    pub output_bytes: usize,

    // Throughput (calculated)
    /// Parse throughput in bytes/sec
    pub parse_throughput_bps: f64,
    /// Emit throughput in bytes/sec
    pub emit_throughput_bps: f64,
    /// Total throughput in bytes/sec
    pub total_throughput_bps: f64,

    // Memory
    /// Memory metrics
    pub memory: MemoryMetrics,

    // Internal timing state
    start_time: Option<Instant>,
    parse_start: Option<Instant>,
    emit_start: Option<Instant>,
}

/// Memory allocation metrics
#[derive(Debug, Clone, Default)]
pub struct MemoryMetrics {
    /// Peak heap allocation during transformation
    pub peak_bytes: usize,
    /// Total bytes allocated
    pub total_allocated: usize,
    /// Number of allocations
    pub allocation_count: usize,
    /// Tape node count
    pub tape_nodes: usize,
    /// String bytes in tape
    pub string_bytes: usize,
    /// Output buffer capacity
    pub output_capacity: usize,
}

impl TransformMetrics {
    /// Create new metrics for a transformation
    #[must_use]
    pub fn new(source: FormatKind, target: FormatKind) -> Self {
        Self {
            source_format: source,
            target_format: target,
            total_time: Duration::ZERO,
            parse_time: Duration::ZERO,
            emit_time: Duration::ZERO,
            input_bytes: 0,
            output_bytes: 0,
            parse_throughput_bps: 0.0,
            emit_throughput_bps: 0.0,
            total_throughput_bps: 0.0,
            memory: MemoryMetrics::default(),
            start_time: None,
            parse_start: None,
            emit_start: None,
        }
    }

    /// Start timing
    pub fn start(&mut self) {
        self.start_time = Some(Instant::now());
        self.parse_start = Some(Instant::now());
    }

    /// Record parse completion
    pub fn record_parse(&mut self, input_bytes: usize) {
        if let Some(start) = self.parse_start.take() {
            self.parse_time = start.elapsed();
        }
        self.input_bytes = input_bytes;
        self.emit_start = Some(Instant::now());
    }

    /// Record emit completion
    pub fn record_emit(&mut self, output_bytes: usize) {
        if let Some(start) = self.emit_start.take() {
            self.emit_time = start.elapsed();
        }
        self.output_bytes = output_bytes;
    }

    /// Finish timing and calculate derived metrics
    pub fn finish(&mut self) {
        if let Some(start) = self.start_time.take() {
            self.total_time = start.elapsed();
        }

        // Calculate throughput
        let parse_secs = self.parse_time.as_secs_f64();
        let emit_secs = self.emit_time.as_secs_f64();
        let total_secs = self.total_time.as_secs_f64();

        if parse_secs > 0.0 {
            self.parse_throughput_bps = self.input_bytes as f64 / parse_secs;
        }

        if emit_secs > 0.0 {
            self.emit_throughput_bps = self.output_bytes as f64 / emit_secs;
        }

        if total_secs > 0.0 {
            self.total_throughput_bps = self.input_bytes as f64 / total_secs;
        }
    }

    /// Record memory metrics from tape stats
    #[allow(clippy::missing_const_for_fn)] // Arithmetic on usize is not const
    pub fn record_memory(&mut self, tape_nodes: usize, string_bytes: usize, output_cap: usize) {
        self.memory.tape_nodes = tape_nodes;
        self.memory.string_bytes = string_bytes;
        self.memory.output_capacity = output_cap;

        // Estimate allocations (rough)
        self.memory.total_allocated = tape_nodes * 48 + string_bytes + output_cap;
        self.memory.peak_bytes = self.memory.total_allocated;
        self.memory.allocation_count = 3; // tape vec, strings, output
    }

    /// Get parse latency in nanoseconds
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Duration::as_nanos() is not const
    pub fn parse_latency_ns(&self) -> u128 {
        self.parse_time.as_nanos()
    }

    /// Get emit latency in nanoseconds
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Duration::as_nanos() is not const
    pub fn emit_latency_ns(&self) -> u128 {
        self.emit_time.as_nanos()
    }

    /// Get total latency in nanoseconds
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Duration::as_nanos() is not const
    pub fn total_latency_ns(&self) -> u128 {
        self.total_time.as_nanos()
    }

    /// Get size expansion ratio (output/input)
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Float division is not const
    pub fn expansion_ratio(&self) -> f64 {
        if self.input_bytes > 0 {
            self.output_bytes as f64 / self.input_bytes as f64
        } else {
            1.0
        }
    }

    /// Get parse throughput in MiB/s
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Float division is not const
    pub fn parse_throughput_mibs(&self) -> f64 {
        self.parse_throughput_bps / (1024.0 * 1024.0)
    }

    /// Get emit throughput in MiB/s
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Float division is not const
    pub fn emit_throughput_mibs(&self) -> f64 {
        self.emit_throughput_bps / (1024.0 * 1024.0)
    }

    /// Get total throughput in MiB/s
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Float division is not const
    pub fn total_throughput_mibs(&self) -> f64 {
        self.total_throughput_bps / (1024.0 * 1024.0)
    }

    /// Format as summary string
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} -> {}: {:.2}µs total ({:.2}µs parse + {:.2}µs emit), \
             {}B -> {}B ({:.2}x), {:.2} MiB/s",
            self.source_format,
            self.target_format,
            self.total_time.as_micros() as f64,
            self.parse_time.as_micros() as f64,
            self.emit_time.as_micros() as f64,
            self.input_bytes,
            self.output_bytes,
            self.expansion_ratio(),
            self.total_throughput_mibs()
        )
    }
}

impl std::fmt::Display for TransformMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.summary())
    }
}

/// Metrics aggregator for multiple samples
#[derive(Debug, Clone, Default)]
pub struct MetricsAggregator {
    samples: Vec<TransformMetrics>,
}

impl MetricsAggregator {
    /// Create new aggregator
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a sample
    pub fn add(&mut self, metrics: TransformMetrics) {
        self.samples.push(metrics);
    }

    /// Get sample count
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Vec::len() is not const
    pub fn count(&self) -> usize {
        self.samples.len()
    }

    /// Calculate mean total time
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // u128 to u64 truncation acceptable for durations
    pub fn mean_total_time(&self) -> Duration {
        if self.samples.is_empty() {
            return Duration::ZERO;
        }
        let total: Duration = self.samples.iter().map(|m| m.total_time).sum();
        total / self.samples.len() as u32
    }

    /// Calculate mean throughput
    #[must_use]
    pub fn mean_throughput_mibs(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let sum: f64 = self
            .samples
            .iter()
            .map(TransformMetrics::total_throughput_mibs)
            .sum();
        sum / self.samples.len() as f64
    }

    /// Calculate p50 latency
    #[must_use]
    pub fn p50_latency(&self) -> Duration {
        self.percentile_latency(50)
    }

    /// Calculate p95 latency
    #[must_use]
    pub fn p95_latency(&self) -> Duration {
        self.percentile_latency(95)
    }

    /// Calculate p99 latency
    #[must_use]
    pub fn p99_latency(&self) -> Duration {
        self.percentile_latency(99)
    }

    /// Calculate percentile latency
    fn percentile_latency(&self, percentile: usize) -> Duration {
        if self.samples.is_empty() {
            return Duration::ZERO;
        }

        let mut times: Vec<Duration> = self.samples.iter().map(|m| m.total_time).collect();
        times.sort();

        let idx = (times.len() * percentile / 100).min(times.len() - 1);
        times[idx]
    }

    /// Get summary statistics
    #[must_use]
    pub fn summary(&self) -> AggregatedStats {
        AggregatedStats {
            count: self.count(),
            mean_latency: self.mean_total_time(),
            p50_latency: self.p50_latency(),
            p95_latency: self.p95_latency(),
            p99_latency: self.p99_latency(),
            mean_throughput_mibs: self.mean_throughput_mibs(),
        }
    }
}

/// Aggregated statistics
#[derive(Debug, Clone)]
pub struct AggregatedStats {
    /// Sample count
    pub count: usize,
    /// Mean latency
    pub mean_latency: Duration,
    /// p50 latency
    pub p50_latency: Duration,
    /// p95 latency
    pub p95_latency: Duration,
    /// p99 latency
    pub p99_latency: Duration,
    /// Mean throughput in MiB/s
    pub mean_throughput_mibs: f64,
}

impl std::fmt::Display for AggregatedStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "n={}: mean={:.2}µs, p50={:.2}µs, p95={:.2}µs, p99={:.2}µs, {:.2} MiB/s",
            self.count,
            self.mean_latency.as_micros() as f64,
            self.p50_latency.as_micros() as f64,
            self.p95_latency.as_micros() as f64,
            self.p99_latency.as_micros() as f64,
            self.mean_throughput_mibs
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_basic() {
        let mut metrics = TransformMetrics::new(FormatKind::Json, FormatKind::Json);
        metrics.start();
        std::thread::sleep(Duration::from_micros(100));
        metrics.record_parse(1000);
        std::thread::sleep(Duration::from_micros(100));
        metrics.record_emit(1000);
        metrics.finish();

        assert!(metrics.total_time > Duration::ZERO);
        assert!(metrics.parse_time > Duration::ZERO);
        assert!(metrics.emit_time > Duration::ZERO);
    }

    #[test]
    fn test_aggregator() {
        let mut agg = MetricsAggregator::new();

        for _ in 0..10 {
            let mut m = TransformMetrics::new(FormatKind::Json, FormatKind::Json);
            m.total_time = Duration::from_micros(100);
            m.input_bytes = 1000;
            agg.add(m);
        }

        assert_eq!(agg.count(), 10);
        assert!(agg.mean_total_time() > Duration::ZERO);
    }
}
