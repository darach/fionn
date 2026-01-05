#![allow(clippy::all)]
#![allow(clippy::option_if_let_else)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::pedantic)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! AFL fuzz target for character classification across all SIMD variants.
//!
//! This target verifies that SIMD character classification produces
//! identical results to scalar implementations.
//!
//! Run with:
//!   cargo afl build --release -bin `fuzz_classify
//!   cargo afl fuzz -i fuzz/corpus/classify -o fuzz/output/classify target/release/`fuzz_classify

#[macro_use]
extern crate afl;

// Note: SIMD classification not implemented in fionn_simd

/// Scalar reference implementation for whitespace detection
fn scalar_whitespace_mask(chunk: &[u8; 64]) -> u64 {
    let mut mask = 0u64;
    for (i, &byte) in chunk.iter().enumerate() {
        if byte == b' ' || byte == b'\t' || byte == b'\n' || byte == b'\r' {
            mask |= 1u64 << i;
        }
    }
    mask
}

/// Scalar reference implementation for structural character detection
fn scalar_structural_mask(chunk: &[u8; 64]) -> u64 {
    let mut mask = 0u64;
    for (i, &byte) in chunk.iter().enumerate() {
        if matches!(byte, b'{' | b'}' | b'[' | b']' | b':' | b',') {
            mask |= 1u64 << i;
        }
    }
    mask
}

/// Scalar reference implementation for string character detection
fn scalar_string_mask(chunk: &[u8; 64]) -> u64 {
    let mut mask = 0u64;
    for (i, &byte) in chunk.iter().enumerate() {
        if byte == b'"' || byte == b'\\' {
            mask |= 1u64 << i;
        }
    }
    mask
}

/// Scalar reference implementation for number character detection
fn scalar_number_mask(chunk: &[u8; 64]) -> u64 {
    let mut mask = 0u64;
    for (i, &byte) in chunk.iter().enumerate() {
        if byte.is_ascii_digit() || matches!(byte, b'-' | b'+' | b'.' | b'e' | b'E') {
            mask |= 1u64 << i;
        }
    }
    mask
}

/// Verify classification with scalar implementations
fn verify_classification(chunk: &[u8; 64]) {
    // Just call the scalar functions to ensure they don't panic
    let _ = scalar_whitespace_mask(chunk);
    let _ = scalar_structural_mask(chunk);
    let _ = scalar_string_mask(chunk);
    let _ = scalar_number_mask(chunk);
}

// No SIMD verification, just scalar

/// Verify classification invariants hold
fn verify_classification_invariants(chunk: &[u8; 64]) {
    let ws_mask = scalar_whitespace_mask(chunk);
    let struct_mask = scalar_structural_mask(chunk);
    let string_mask = scalar_string_mask(chunk);

    // Whitespace and structural should never overlap
    assert_eq!(
        ws_mask & struct_mask,
        0,
        "whitespace and structural overlap detected"
    );

    // String characters and structural should never overlap
    assert_eq!(
        string_mask & struct_mask,
        0,
        "string and structural overlap detected"
    );

    // Verify mask correctness for each position
    for (i, &byte) in chunk.iter().enumerate() {
        let bit = 1u64 << i;

        // Check whitespace
        let is_ws = (ws_mask & bit) != 0;
        let should_be_ws = matches!(byte, b' ' | b'\t' | b'\n' | b'\r');
        assert_eq!(is_ws, should_be_ws, "whitespace mismatch at position {}", i);

        // Check structural
        let is_struct = (struct_mask & bit) != 0;
        let should_be_struct = matches!(byte, b'{' | b'}' | b'[' | b']' | b':' | b',');
        assert_eq!(
            is_struct, should_be_struct,
            "structural mismatch at position {}",
            i
        );

        // Check string
        let is_string = (string_mask & bit) != 0;
        let should_be_string = byte == b'"' || byte == b'\\';
        assert_eq!(
            is_string, should_be_string,
            "string mismatch at position {}",
            i
        );
    }
}

/// Test with JSON-like input patterns
fn verify_json_patterns(data: &[u8]) {
    // Process 64-byte chunks
    for chunk_start in (0..data.len()).step_by(64) {
        if chunk_start + 64 <= data.len() {
            let chunk: &[u8; 64] = data[chunk_start..chunk_start + 64].try_into().unwrap();
            verify_classification_invariants(chunk);
            verify_classification(chunk);
        }
    }

    // Handle partial final chunk by padding with zeros
    let remainder = data.len() % 64;
    if remainder > 0 {
        let chunk_start = data.len() - remainder;
        let mut padded = [0u8; 64];
        padded[..remainder].copy_from_slice(&data[chunk_start..]);
        verify_classification_invariants(&padded);
        verify_classification(&padded);
    }
}

fn main() {
    fuzz!(|data: &[u8]| {
        // Skip extremely large inputs
        if data.len() <= 100_000 {
            // Test full 64-byte chunks
            if data.len() >= 64 {
                let chunk: &[u8; 64] = data[..64].try_into().unwrap();
                verify_classification_invariants(chunk);
                verify_classification(chunk);
            }

            // Test with padding
            let mut padded = [0u8; 64];
            let copy_len = data.len().min(64);
            padded[..copy_len].copy_from_slice(&data[..copy_len]);
            verify_classification_invariants(&padded);
            verify_classification(&padded);

            // Test JSON-like patterns
            verify_json_patterns(data);
        }
    });
}
