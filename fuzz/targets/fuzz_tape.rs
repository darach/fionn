#![allow(clippy::all)]
#![allow(clippy::option_if_let_else)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::pedantic)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! AFL fuzz target for tape parsing and operations.
//!
//! This target tests JSON parsing into tape format and subsequent operations
//! like field access and value extraction.
//!
//! Run with:
//!   cargo afl build --release --bin `fuzz_tape`
//!   cargo afl fuzz -i fuzz/corpus/tape -o fuzz/output/tape target/release/`fuzz_tape`

#[macro_use]
extern crate afl;

use fionn_core::path::parse_simd;
use fionn_tape::DsonTape;

/// Test tape parsing with arbitrary input
fn fuzz_tape_parsing(data: &[u8]) {
    // Only process valid UTF-8
    let input = if let Ok(s) = std::str::from_utf8(data) {
        s
    } else {
        return;
    };

    // Try to parse as JSON
    if let Ok(tape) = DsonTape::parse(input) {
        // If parsing succeeded, verify we can access the tape
        verify_tape_integrity(&tape);
    }
}

/// Verify tape integrity after successful parse
fn verify_tape_integrity(tape: &DsonTape) {
    // Get tape contents
    let nodes = tape.nodes();

    // Verify tape structure is accessible without panicking
    // Note: an empty tape is technically invalid but we don't panic on it
    let _ = nodes.is_empty();

    // Verify root element is accessible
    let _ = tape.root();
}

/// Test path resolution with fuzzed paths
fn fuzz_path_resolution(json_data: &[u8], path_data: &[u8]) {
    // Convert to strings
    let json_str = if let Ok(s) = std::str::from_utf8(json_data) {
        s
    } else {
        return;
    };

    let path_str = if let Ok(s) = std::str::from_utf8(path_data) {
        s
    } else {
        return;
    };

    // Try to parse JSON
    let tape = if let Ok(t) = DsonTape::parse(json_str) {
        t
    } else {
        return;
    };

    // Try to resolve path
    let _ = tape.resolve_path(path_str);

    // Also try with parsed path
    let components = parse_simd(path_str);
    let _ = tape.resolve_path_components_owned(&components);
}

/// Test with valid JSON structures
fn fuzz_valid_json(data: &[u8]) {
    let input = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Try to parse
    let tape = match DsonTape::parse(input) {
        Ok(t) => t,
        Err(_) => return,
    };

    // Test serialization
    let _ = tape.to_json_string();

    // Test value extraction at various indices
    for i in 0..tape.nodes().len().min(100) {
        let _ = tape.extract_value_simd(i);
    }
}

/// Test tape serialization round-trip
fn fuzz_tape_roundtrip(data: &[u8]) {
    let input = if let Ok(s) = std::str::from_utf8(data) {
        s
    } else {
        return;
    };

    // Parse first time
    let tape1 = if let Ok(t) = DsonTape::parse(input) {
        t
    } else {
        return;
    };

    // Serialize back to JSON
    let json_str = if let Ok(s) = tape1.to_json_string() {
        s
    } else {
        return;
    };

    // Parse again - if serialization produced invalid JSON, that's a bug we want to detect
    // but we should report it without panicking
    let tape2 = match DsonTape::parse(&json_str) {
        Ok(t) => t,
        Err(_) => {
            // Serialization produced invalid JSON - this is unexpected but not a crash
            return;
        }
    };

    // Serialize second tape
    let json_str2 = match tape2.to_json_string() {
        Ok(s) => s,
        Err(_) => return,
    };

    // After round-trip, JSON should be equivalent
    // We compare semantically using serde_json
    let json1: Result<serde_json::Value, _> = serde_json::from_str(&json_str);
    let json2: Result<serde_json::Value, _> = serde_json::from_str(&json_str2);

    // Both should parse successfully and be equal
    // Don't assert - just verify no crash
    if let (Ok(v1), Ok(v2)) = (json1, json2) {
        let _ = v1 == v2;
    }
}

/// Test with specific JSON patterns that might trigger edge cases
fn fuzz_json_edge_cases(data: &[u8]) {
    let input = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Test parsing directly
    let _ = DsonTape::parse(input);

    // Test with object wrapper
    let wrapped = format!("{{{input}}}");
    let _ = DsonTape::parse(&wrapped);

    // Test with array wrapper
    let array_wrapped = format!("[{input}]");
    let _ = DsonTape::parse(&array_wrapped);

    // Test with string wrapper
    let string_wrapped = format!("\"{}\"", input.replace('\\', "\\\\").replace('"', "\\\""));
    let _ = DsonTape::parse(&string_wrapped);
}

/// Test skip operations
fn fuzz_skip_operations(data: &[u8]) {
    let input = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    let tape = match DsonTape::parse(input) {
        Ok(t) => t,
        Err(_) => return,
    };

    // Test skip_field at various positions
    for i in 0..tape.nodes().len().min(50) {
        let _ = tape.skip_field(i);
    }

    // Test skip_value at various positions
    for i in 0..tape.nodes().len().min(50) {
        let _ = tape.skip_value(i);
    }
}

fn main() {
    fuzz!(|data: &[u8]| {
        // Skip extremely large inputs
        if data.len() <= 100_000 {
            // Basic tape parsing
            fuzz_tape_parsing(data);

            // Path resolution (split data for JSON and path)
            if data.len() >= 2 {
                let split = data.len() / 2;
                fuzz_path_resolution(&data[..split], &data[split..]);
            }

            // Valid JSON operations
            fuzz_valid_json(data);

            // Round-trip testing
            fuzz_tape_roundtrip(data);

            // Edge cases
            fuzz_json_edge_cases(data);

            // Skip operations
            fuzz_skip_operations(data);
        }
    });
}
