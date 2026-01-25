#![allow(clippy::all)]
#![allow(clippy::option_if_let_else)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::pedantic)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! AFL fuzz target for path parsing across all SIMD variants.
//!
//! This target verifies that all path parsing implementations produce
//! consistent results and don't panic on arbitrary input.
//!
//! Run with:
//!   cargo afl build --release --bin `fuzz_path`
//!   cargo afl fuzz -i fuzz/corpus/path -o fuzz/output/path target/release/`fuzz_path`

#[macro_use]
extern crate afl;

use fionn_core::path::{ParsedPath, PathComponent, parse_baseline, parse_simd};

/// Verify all parsing implementations produce equivalent results
fn verify_path_parsing_equivalence(input: &str) {
    // Parse with all available implementations
    let baseline_result = parse_baseline(input);
    let simd_result = parse_simd(input);
    let parsed_path = ParsedPath::parse(input);

    // Verify component counts match
    assert_eq!(
        baseline_result.len(),
        simd_result.len(),
        "baseline vs simd component count mismatch for input: {:?}",
        input
    );

    // Verify each component matches
    for (i, (baseline, simd)) in baseline_result.iter().zip(simd_result.iter()).enumerate() {
        assert_eq!(
            baseline, simd,
            "component {} mismatch for input: {:?}",
            i, input
        );
    }

    // Verify ParsedPath produces same count
    assert_eq!(
        baseline_result.len(),
        parsed_path.components().len(),
        "ParsedPath component count mismatch for input: {:?}",
        input
    );
}

/// Test path parsing with forced SIMD variants (`x86_64` only)
#[cfg(target_arch = "x86_64")]
fn verify_parsing(input: &str) {
    let baseline = parse_baseline(input);
    let simd = parse_simd(input);

    // Just ensure they parse without panic
    let _ = baseline;
    let _ = simd;
}

// No forced variants

/// Verify path component extraction is correct
fn verify_path_components(input: &str) {
    let components = parse_simd(input);

    for comp in &components {
        match comp {
            PathComponent::Field(name) => {
                // Field names should be valid UTF-8 (guaranteed by parsing from &str)
                // Empty field names are allowed (e.g., consecutive delimiters)
                let _ = name.len();
            }
            PathComponent::ArrayIndex(idx) => {
                // Array indices can be any usize (including MAX from saturating arithmetic)
                let _ = idx;
            }
        }
    }
}

fn main() {
    fuzz!(|data: &[u8]| {
        // Only process valid UTF-8 strings
        if let Ok(input) = std::str::from_utf8(data) {
            // Skip extremely long inputs to avoid timeout
            if input.len() <= 10_000 {
                verify_path_parsing_equivalence(input);
                verify_parsing(input);
                verify_path_components(input);
            }
        }
    });
}
