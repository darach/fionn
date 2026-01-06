// SPDX-License-Identifier: MIT OR Apache-2.0
//! libFuzzer target for tape parsing and operations.
//!
//! This is the libFuzzer equivalent of the AFL fuzz_tape target.
//! Run with: cargo +nightly fuzz run fuzz_tape_libfuzzer

#![no_main]

use libfuzzer_sys::fuzz_target;
use fionn_core::path::parse_simd;
use fionn_tape::DsonTape;

fuzz_target!(|data: &[u8]| {
    // Skip extremely large inputs
    if data.len() > 100_000 {
        return;
    }

    // Only process valid UTF-8
    let input = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Basic tape parsing
    if let Ok(tape) = DsonTape::parse(input) {
        // Verify tape integrity
        let nodes = tape.nodes();
        if nodes.is_empty() {
            return;
        }
        let _root = tape.root();

        // Test serialization round-trip
        if let Ok(json_str) = tape.to_json_string() {
            // Parse again - should succeed
            let _ = DsonTape::parse(&json_str);
        }

        // Test value extraction
        for i in 0..nodes.len().min(100) {
            let _ = tape.extract_value_simd(i);
        }

        // Test skip operations
        for i in 0..nodes.len().min(50) {
            let _ = tape.skip_field(i);
            let _ = tape.skip_value(i);
        }
    }

    // Test path parsing (always safe)
    let _ = parse_simd(input);
});
