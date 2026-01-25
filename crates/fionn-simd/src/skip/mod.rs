// SPDX-License-Identifier: MIT OR Apache-2.0
//! Skip strategies for JSON container traversal
//!
//! Provides multiple implementations for skipping JSON values, objects, and arrays.
//! See `docs/research/skip-strategies.md` for algorithm attribution.
//!
//! Scalar implementations are in this module. SIMD implementations (AVX2, NEON)
//! are in the unified `simd` module for architecture-specific acceleration.
//!
//! # Parallel Processing
//! For processing multiple documents, use `skip_batch_parallel` which distributes
//! work across all available CPU cores using rayon.

mod arena_jsonski;
mod jsonski;
mod langdale;
mod scalar;

use rayon::prelude::*;

pub use arena_jsonski::ArenaJsonSkiSkip;
pub use jsonski::JsonSkiSkip;
pub use langdale::LangdaleSkip;
pub use scalar::ScalarSkip;

// Re-export SIMD implementations from the x86 module
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub use crate::x86::skip::Avx2Skip;

/// Result of a skip operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkipResult {
    /// Bytes consumed (offset past closing delimiter)
    pub consumed: usize,
    /// Whether escape sequences were encountered
    pub has_escapes: bool,
}

/// Strategy for skipping JSON values
pub trait Skip {
    /// Skip an object starting after the opening `{`
    fn skip_object(&self, input: &[u8]) -> Option<SkipResult>;

    /// Skip an array starting after the opening `[`
    fn skip_array(&self, input: &[u8]) -> Option<SkipResult>;

    /// Skip a string starting after the opening `"`
    fn skip_string(&self, input: &[u8]) -> Option<SkipResult>;

    /// Skip any JSON value (auto-detects type from first non-whitespace byte)
    fn skip_value(&self, input: &[u8]) -> Option<SkipResult>;
}

/// Runtime selection of skip strategy
#[derive(Debug, Clone, Copy, Default)]
pub enum SkipStrategy {
    /// Scalar byte-by-byte (baseline)
    Scalar,
    /// Langdale-Lemire XOR prefix with branchless escape
    Langdale,
    /// `JSONSki` bracket counting (default - best general performance)
    #[default]
    JsonSki,
    /// AVX2 SIMD (`x86_64` only)
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    Avx2,
}

impl std::fmt::Display for SkipStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Scalar => "Scalar",
            Self::Langdale => "Langdale",
            Self::JsonSki => "JsonSki",
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            Self::Avx2 => "Avx2",
        };
        write!(f, "{name}")
    }
}

impl SkipStrategy {
    /// Create a skipper for this strategy
    #[must_use]
    pub fn skipper(&self) -> Box<dyn Skip + Send + Sync> {
        match self {
            Self::Scalar => Box::new(ScalarSkip),
            Self::Langdale => Box::new(LangdaleSkip::new()),
            Self::JsonSki => Box::new(JsonSkiSkip::new()),
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            Self::Avx2 => Box::new(Avx2Skip::new()),
        }
    }

    /// Get the best available SIMD strategy for this platform
    #[must_use]
    pub fn best_simd() -> Self {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            if Avx2Skip::is_available() {
                return Self::Avx2;
            }
        }
        Self::JsonSki
    }

    /// Get all available strategies for benchmarking
    #[must_use]
    pub fn all_strategies() -> Vec<Self> {
        // Mutable on x86/x86_64 where Avx2 may be pushed
        #[allow(unused_mut)] // Mutated conditionally on x86 targets
        let mut strategies = vec![Self::Scalar, Self::Langdale, Self::JsonSki];
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            if Avx2Skip::is_available() {
                strategies.push(Self::Avx2);
            }
        }
        strategies
    }
}

/// Skip multiple JSON documents in parallel using all available CPU cores
///
/// Each document is processed independently using the best available SIMD strategy.
/// This is ideal for batch processing of JSON lines (JSONL) or arrays of documents.
///
/// # Arguments
/// * `documents` - Slice of byte slices, each containing a JSON document starting with `{` or `[`
///
/// # Returns
/// Vector of skip results in the same order as input documents
///
/// # Example
/// ```
/// use fionn_simd::skip::skip_objects_parallel;
///
/// let docs: Vec<&[u8]> = vec![
///     br#""key": "value"}"#,
///     br#""name": "test"}"#,
/// ];
/// let results = skip_objects_parallel(&docs);
/// ```
#[must_use]
pub fn skip_objects_parallel(documents: &[&[u8]]) -> Vec<Option<SkipResult>> {
    documents
        .par_iter()
        .map(|doc| {
            let skipper = SkipStrategy::best_simd().skipper();
            skipper.skip_object(doc)
        })
        .collect()
}

/// Skip multiple JSON arrays in parallel using all available CPU cores
#[must_use]
pub fn skip_arrays_parallel(documents: &[&[u8]]) -> Vec<Option<SkipResult>> {
    documents
        .par_iter()
        .map(|doc| {
            let skipper = SkipStrategy::best_simd().skipper();
            skipper.skip_array(doc)
        })
        .collect()
}

/// Skip multiple JSON values in parallel using all available CPU cores
/// Auto-detects type (object, array, string, etc.) for each document
#[must_use]
pub fn skip_values_parallel(documents: &[&[u8]]) -> Vec<Option<SkipResult>> {
    documents
        .par_iter()
        .map(|doc| {
            let skipper = SkipStrategy::best_simd().skipper();
            skipper.skip_value(doc)
        })
        .collect()
}

/// Parallel batch skipper with configurable strategy
///
/// Use this when you need more control over the skip strategy or
/// want to reuse the same skipper across multiple batches.
#[derive(Debug, Clone, Copy)]
pub struct ParallelSkipper {
    strategy: SkipStrategy,
}

impl std::fmt::Display for ParallelSkipper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ParallelSkipper(strategy={})", self.strategy)
    }
}

impl ParallelSkipper {
    /// Create a new parallel skipper with the given strategy
    #[must_use]
    pub const fn new(strategy: SkipStrategy) -> Self {
        Self { strategy }
    }

    /// Create a parallel skipper using the best available SIMD strategy
    #[must_use]
    pub fn best_simd() -> Self {
        Self::new(SkipStrategy::best_simd())
    }

    /// Skip objects in parallel
    #[must_use]
    pub fn skip_objects(&self, documents: &[&[u8]]) -> Vec<Option<SkipResult>> {
        let strategy = self.strategy;
        documents
            .par_iter()
            .map(|doc| {
                let skipper = strategy.skipper();
                skipper.skip_object(doc)
            })
            .collect()
    }

    /// Skip arrays in parallel
    #[must_use]
    pub fn skip_arrays(&self, documents: &[&[u8]]) -> Vec<Option<SkipResult>> {
        let strategy = self.strategy;
        documents
            .par_iter()
            .map(|doc| {
                let skipper = strategy.skipper();
                skipper.skip_array(doc)
            })
            .collect()
    }

    /// Skip values in parallel (auto-detects type)
    #[must_use]
    pub fn skip_values(&self, documents: &[&[u8]]) -> Vec<Option<SkipResult>> {
        let strategy = self.strategy;
        documents
            .par_iter()
            .map(|doc| {
                let skipper = strategy.skipper();
                skipper.skip_value(doc)
            })
            .collect()
    }

    /// Benchmark skip operations and return timing data.
    ///
    /// Returns `(total_bytes_processed, elapsed_duration)`.
    /// Caller can compute throughput as: `total_bytes as f64 / elapsed.as_secs_f64()`
    #[must_use]
    pub fn benchmark(
        &self,
        documents: &[&[u8]],
        iterations: usize,
    ) -> (usize, std::time::Duration) {
        let total_bytes: usize = documents.iter().map(|d| d.len()).sum();
        let start = std::time::Instant::now();

        for _ in 0..iterations {
            let _ = self.skip_objects(documents);
        }

        (total_bytes.saturating_mul(iterations), start.elapsed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // SkipResult Tests
    // =========================================================================

    #[test]
    fn test_skip_result_debug() {
        let result = SkipResult {
            consumed: 10,
            has_escapes: true,
        };
        let debug_str = format!("{result:?}");
        assert!(debug_str.contains("consumed"));
        assert!(debug_str.contains("10"));
        assert!(debug_str.contains("has_escapes"));
    }

    #[test]
    fn test_skip_result_clone() {
        let result = SkipResult {
            consumed: 42,
            has_escapes: false,
        };
        let cloned = result;
        assert_eq!(result, cloned);
    }

    #[test]
    fn test_skip_result_copy() {
        let result = SkipResult {
            consumed: 100,
            has_escapes: true,
        };
        let copied = result;
        assert_eq!(result.consumed, copied.consumed);
        assert_eq!(result.has_escapes, copied.has_escapes);
    }

    #[test]
    fn test_skip_result_equality() {
        let a = SkipResult {
            consumed: 5,
            has_escapes: false,
        };
        let b = SkipResult {
            consumed: 5,
            has_escapes: false,
        };
        let c = SkipResult {
            consumed: 5,
            has_escapes: true,
        };
        let d = SkipResult {
            consumed: 6,
            has_escapes: false,
        };

        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    // =========================================================================
    // SkipStrategy Tests
    // =========================================================================

    #[test]
    fn test_skip_strategy_default() {
        let strategy = SkipStrategy::default();
        matches!(strategy, SkipStrategy::JsonSki);
    }

    #[test]
    fn test_skip_strategy_debug() {
        let strategy = SkipStrategy::Scalar;
        let debug_str = format!("{strategy}");
        assert!(debug_str.contains("Scalar"));
    }

    #[test]
    fn test_skip_strategy_clone() {
        let strategy = SkipStrategy::Langdale;
        let cloned = strategy;
        matches!(cloned, SkipStrategy::Langdale);
    }

    #[test]
    fn test_skip_strategy_skipper_scalar() {
        let strategy = SkipStrategy::Scalar;
        let skipper = strategy.skipper();
        let result = skipper.skip_object(b"}");
        assert!(result.is_some());
    }

    #[test]
    fn test_skip_strategy_skipper_langdale() {
        let strategy = SkipStrategy::Langdale;
        let skipper = strategy.skipper();
        let result = skipper.skip_object(b"}");
        assert!(result.is_some());
    }

    #[test]
    fn test_skip_strategy_skipper_jsonski() {
        let strategy = SkipStrategy::JsonSki;
        let skipper = strategy.skipper();
        let result = skipper.skip_object(b"}");
        assert!(result.is_some());
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[test]
    fn test_skip_strategy_skipper_avx2() {
        let strategy = SkipStrategy::Avx2;
        let skipper = strategy.skipper();
        let result = skipper.skip_object(b"}");
        assert!(result.is_some());
    }

    #[test]
    fn test_skip_strategy_best_simd() {
        let strategy = SkipStrategy::best_simd();
        // Should return a valid strategy for the current platform
        let skipper = strategy.skipper();
        let result = skipper.skip_object(b"}");
        assert!(result.is_some());
    }

    #[test]
    fn test_skip_strategy_all_strategies() {
        let strategies = SkipStrategy::all_strategies();
        // Should have at least 3 strategies (Scalar, Langdale, JsonSki)
        assert!(strategies.len() >= 3);
        assert!(strategies.iter().any(|s| matches!(s, SkipStrategy::Scalar)));
        assert!(
            strategies
                .iter()
                .any(|s| matches!(s, SkipStrategy::Langdale))
        );
        assert!(
            strategies
                .iter()
                .any(|s| matches!(s, SkipStrategy::JsonSki))
        );
    }

    // =========================================================================
    // Parallel Skip Functions Tests
    // =========================================================================

    #[test]
    fn test_skip_objects_parallel_empty() {
        let documents: Vec<&[u8]> = vec![];
        let results = skip_objects_parallel(&documents);
        assert!(results.is_empty());
    }

    #[test]
    fn test_skip_objects_parallel_single() {
        let documents: Vec<&[u8]> = vec![b"}"];
        let results = skip_objects_parallel(&documents);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_some());
    }

    #[test]
    fn test_skip_objects_parallel_multiple() {
        let documents: Vec<&[u8]> = vec![
            br#""key": "value"}"#,
            br#""name": "test"}"#,
            br#""a": 1, "b": 2}"#,
        ];
        let results = skip_objects_parallel(&documents);
        assert_eq!(results.len(), 3);
        for result in &results {
            assert!(result.is_some());
        }
    }

    #[test]
    fn test_skip_arrays_parallel_empty() {
        let documents: Vec<&[u8]> = vec![];
        let results = skip_arrays_parallel(&documents);
        assert!(results.is_empty());
    }

    #[test]
    fn test_skip_arrays_parallel_single() {
        let documents: Vec<&[u8]> = vec![b"]"];
        let results = skip_arrays_parallel(&documents);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_some());
    }

    #[test]
    fn test_skip_arrays_parallel_multiple() {
        let documents: Vec<&[u8]> = vec![b"1, 2, 3]", b"\"a\", \"b\"]", b"]"];
        let results = skip_arrays_parallel(&documents);
        assert_eq!(results.len(), 3);
        for result in &results {
            assert!(result.is_some());
        }
    }

    #[test]
    fn test_skip_values_parallel_empty() {
        let documents: Vec<&[u8]> = vec![];
        let results = skip_values_parallel(&documents);
        assert!(results.is_empty());
    }

    #[test]
    fn test_skip_values_parallel_mixed() {
        let documents: Vec<&[u8]> = vec![
            b"{}",      // object
            b"[]",      // array
            b"\"str\"", // string
            b"123",     // number
            b"true",    // boolean
            b"null",    // null
        ];
        let results = skip_values_parallel(&documents);
        assert_eq!(results.len(), 6);
    }

    // =========================================================================
    // ParallelSkipper Tests
    // =========================================================================

    #[test]
    fn test_parallel_skipper_new() {
        let skipper = ParallelSkipper::new(SkipStrategy::Scalar);
        let docs: Vec<&[u8]> = vec![b"}"];
        let results = skipper.skip_objects(&docs);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_parallel_skipper_best_simd() {
        let skipper = ParallelSkipper::best_simd();
        let docs: Vec<&[u8]> = vec![b"}"];
        let results = skipper.skip_objects(&docs);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_parallel_skipper_debug() {
        let skipper = ParallelSkipper::new(SkipStrategy::JsonSki);
        let debug_str = format!("{skipper}");
        assert!(debug_str.contains("ParallelSkipper"));
    }

    #[test]
    fn test_parallel_skipper_clone() {
        let skipper = ParallelSkipper::new(SkipStrategy::Langdale);
        let cloned = skipper;
        let docs: Vec<&[u8]> = vec![b"}"];
        let results1 = skipper.skip_objects(&docs);
        let results2 = cloned.skip_objects(&docs);
        assert_eq!(results1.len(), results2.len());
    }

    #[test]
    fn test_parallel_skipper_skip_objects() {
        let skipper = ParallelSkipper::new(SkipStrategy::JsonSki);
        let docs: Vec<&[u8]> = vec![br#""a": 1}"#, br#""b": 2}"#];
        let results = skipper.skip_objects(&docs);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(std::option::Option::is_some));
    }

    #[test]
    fn test_parallel_skipper_skip_arrays() {
        let skipper = ParallelSkipper::new(SkipStrategy::JsonSki);
        let docs: Vec<&[u8]> = vec![b"1, 2]", b"3, 4]"];
        let results = skipper.skip_arrays(&docs);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(std::option::Option::is_some));
    }

    #[test]
    fn test_parallel_skipper_skip_values() {
        let skipper = ParallelSkipper::new(SkipStrategy::JsonSki);
        let docs: Vec<&[u8]> = vec![b"{}", b"[]", b"42"];
        let results = skipper.skip_values(&docs);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_parallel_skipper_benchmark() {
        let skipper = ParallelSkipper::new(SkipStrategy::Scalar);
        let docs: Vec<&[u8]> = vec![br#""key": "value"}"#];
        let (bytes, duration) = skipper.benchmark(&docs, 1);
        // Should have processed some bytes
        assert!(bytes > 0);
        // Duration should be non-zero (or at least not panic)
        let _ = duration;
    }

    // =========================================================================
    // Cross-Strategy Equivalence Tests
    // =========================================================================

    #[test]
    fn test_all_strategies_produce_same_results() {
        let test_cases: Vec<&[u8]> = vec![
            b"}",                  // empty object
            br#""key": "value"}"#, // simple object
            br#""a": {"b": 1}}"#,  // nested object
            b"]",                  // empty array
            b"1, 2, 3]",           // simple array
        ];

        let strategies = SkipStrategy::all_strategies();

        for input in &test_cases {
            let results: Vec<Option<SkipResult>> = strategies
                .iter()
                .map(|s| s.skipper().skip_object(input))
                .collect();

            // All strategies should return same consumed count
            let first = results[0].as_ref();
            for (i, result) in results.iter().enumerate().skip(1) {
                assert_eq!(
                    first.map(|r| r.consumed),
                    result.as_ref().map(|r| r.consumed),
                    "Strategy {i} differs from first for input {input:?}"
                );
            }
        }
    }
}
