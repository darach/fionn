// SPDX-License-Identifier: MIT OR Apache-2.0
//! SIMD-accelerated utilities for fionn
//!
//! This crate provides SIMD-accelerated utilities for JSON processing:
//! - Skip strategies for fast JSON value skipping (Scalar, Langdale, `JsonSki`, AVX2)
//! - Line boundary detection for JSONL files
//! - Character classification
//!
//! # Skip Strategies
//!
//! The [`skip`] module provides multiple implementations for skipping JSON values:
//!
//! - [`ScalarSkip`] - Byte-by-byte baseline
//! - [`LangdaleSkip`] - Langdale-Lemire XOR prefix algorithm
//! - [`JsonSkiSkip`] - `JSONSki` bracket counting (default)
//! - [`Avx2Skip`] - AVX2 SIMD acceleration (`x86_64`)
//!
//! Use [`SkipStrategy`] for runtime selection of the best strategy.

pub mod skip;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod x86;

/// Format-specific SIMD parsers for multi-format support
#[cfg(any(
    feature = "yaml",
    feature = "toml",
    feature = "csv",
    feature = "ison",
    feature = "toon"
))]
pub mod formats;

/// Tape-to-tape transformation engine
#[cfg(any(
    feature = "yaml",
    feature = "toml",
    feature = "csv",
    feature = "ison",
    feature = "toon"
))]
pub mod transform;

// Re-export key types from skip module
pub use skip::{
    JsonSkiSkip, LangdaleSkip, ParallelSkipper, ScalarSkip, Skip, SkipResult, SkipStrategy,
    skip_arrays_parallel, skip_objects_parallel, skip_values_parallel,
};

// Re-export SIMD skip implementations
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub use x86::skip::Avx2Skip;

/// SIMD-accelerated line separator detection for JSONL
#[derive(Debug, Clone, Default)]
pub struct SimdLineSeparator;

impl SimdLineSeparator {
    /// Create a new SIMD line separator detector
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }

    /// Detect line boundaries in a data chunk using SIMD
    ///
    /// Returns a vector of positions marking the end of each line (position after newline).
    /// If the data doesn't end with a newline, the final position is the end of the data.
    #[must_use]
    pub fn find_line_boundaries(&self, data: &[u8]) -> Vec<usize> {
        // Use memchr iterator which exploits SIMD for finding byte occurrences
        let mut boundaries: Vec<usize> = memchr::memchr_iter(b'\n', data)
            .map(|pos| pos + 1) // Position after the \n
            .collect();

        // If data doesn't end with \n, add the end position
        if !data.is_empty() && data[data.len() - 1] != b'\n' {
            boundaries.push(data.len());
        }

        boundaries
    }
}

/// SIMD-accelerated structural filtering for JSONL documents
#[derive(Debug, Clone, Default)]
pub struct SimdStructuralFilter;

impl SimdStructuralFilter {
    /// Create a new SIMD structural filter
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }

    /// Check if a JSON line contains required schema fields using SIMD
    #[must_use]
    pub fn matches_schema(&self, line: &[u8], required_fields: &[String]) -> bool {
        if line.is_empty() {
            return false;
        }

        // Fast pre-filter using memchr::memmem to check for required fields in the raw bytes
        for field in required_fields {
            let needle = format!("\"{field}\"");
            if memchr::memmem::find(line, needle.as_bytes()).is_none() {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_separator_empty() {
        let sep = SimdLineSeparator::new();
        let boundaries = sep.find_line_boundaries(b"");
        assert!(boundaries.is_empty());
    }

    #[test]
    fn test_line_separator_single_line_no_newline() {
        let sep = SimdLineSeparator::new();
        let boundaries = sep.find_line_boundaries(b"hello");
        assert_eq!(boundaries, vec![5]);
    }

    #[test]
    fn test_line_separator_single_line_with_newline() {
        let sep = SimdLineSeparator::new();
        let boundaries = sep.find_line_boundaries(b"hello\n");
        assert_eq!(boundaries, vec![6]);
    }

    #[test]
    fn test_line_separator_multiple_lines() {
        let sep = SimdLineSeparator::new();
        let boundaries = sep.find_line_boundaries(b"line1\nline2\nline3\n");
        assert_eq!(boundaries, vec![6, 12, 18]);
    }

    #[test]
    fn test_line_separator_multiple_lines_no_trailing() {
        let sep = SimdLineSeparator::new();
        let boundaries = sep.find_line_boundaries(b"line1\nline2\nline3");
        assert_eq!(boundaries, vec![6, 12, 17]);
    }

    #[test]
    fn test_structural_filter_empty() {
        let filter = SimdStructuralFilter::new();
        assert!(!filter.matches_schema(b"", &[]));
    }

    #[test]
    fn test_structural_filter_match() {
        let filter = SimdStructuralFilter::new();
        let line = br#"{"name": "Alice", "age": 30}"#;
        assert!(filter.matches_schema(line, &["name".to_string()]));
        assert!(filter.matches_schema(line, &["age".to_string()]));
        assert!(filter.matches_schema(line, &["name".to_string(), "age".to_string()]));
    }

    #[test]
    fn test_structural_filter_no_match() {
        let filter = SimdStructuralFilter::new();
        let line = br#"{"name": "Alice"}"#;
        assert!(!filter.matches_schema(line, &["missing".to_string()]));
    }

    #[test]
    fn test_skip_strategy_default() {
        let strategy = SkipStrategy::default();
        assert!(matches!(strategy, SkipStrategy::JsonSki));
    }

    #[test]
    fn test_skip_trait_object() {
        let skipper = ScalarSkip;
        let result = skipper.skip_object(b"}");
        assert!(result.is_some());
    }
}
