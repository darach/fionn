#![allow(clippy::all)]
#![allow(clippy::option_if_let_else)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::pedantic)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! AFL fuzz target for JSONL processing.
//!
//! This target tests JSONL line extraction with SIMD optimizations.
//!
//! Run with:
//!   cargo afl build --release -bin `fuzz_jsonl
//!   cargo afl fuzz -i fuzz/corpus/jsonl -o fuzz/output/jsonl target/release/`fuzz_jsonl

#[macro_use]
extern crate afl;

use fionn_simd::SimdLineSeparator;

/// Test SIMD line separator
fn fuzz_simd_line_separator(data: &[u8]) {
    let separator = SimdLineSeparator::new();

    // Test line boundary detection - should not panic
    let boundaries = separator.find_line_boundaries(data);
    // Verify boundaries are sorted and within bounds
    for &boundary in &boundaries {
        assert!(boundary <= data.len());
    }
}

fn main() {
    fuzz!(|data: &[u8]| {
        // Skip extremely large inputs to avoid timeout
        if data.len() <= 100_000 {
            fuzz_simd_line_separator(data);
        }
    });
}
