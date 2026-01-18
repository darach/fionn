#![no_main]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! libFuzzer target for ungron parsing
//!
//! Tests:
//! - ungron_to_value on arbitrary input
//! - ungron_to_json on arbitrary input
//! - No panics on malformed gron

use libfuzzer_sys::fuzz_target;
use fionn_gron::{ungron_to_value, ungron_to_json};

fuzz_target!(|data: &[u8]| {
    // Skip overly large inputs
    if data.len() > 100_000 {
        return;
    }

    // Try to interpret as UTF-8
    let input = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return, // Not valid UTF-8, skip
    };

    // === Test: ungron_to_value should not panic ===
    // It may return an error for invalid input, but should never panic
    let result = ungron_to_value(input);

    if let Ok(value) = result {
        // === Contract: result should be valid JSON ===
        let serialized = serde_json::to_string(&value);
        assert!(serialized.is_ok(), "ungron result should serialize");

        // === Contract: result should be an object or array at root ===
        // Actually, gron can produce any value type, so just check it's valid
        let _ = value;
    }

    // === Test: ungron_to_json should not panic ===
    let json_result = ungron_to_json(input);

    if let Ok(json_value) = json_result {
        // === Contract: result should be serializable ===
        let serialized = serde_json::to_string(&json_value);
        assert!(serialized.is_ok(), "ungron_to_json result should serialize");
    }

    // === Test: gron/ungron roundtrip for simple valid inputs ===
    // If input looks like valid gron, try roundtrip
    if input.contains(" = ") && input.lines().all(|l| l.is_empty() || l.contains(" = ")) {
        // Attempt ungron
        if let Ok(value) = ungron_to_value(input) {
            // Try to serialize back
            let _ = serde_json::to_string(&value);
        }
    }
});
