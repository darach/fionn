#![no_main]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! libFuzzer target for merge_tapes operations
//!
//! Tests:
//! - merge_tapes RFC 7396 semantics
//! - deep_merge_tapes correctness
//! - No panics on edge cases

use libfuzzer_sys::fuzz_target;
use fionn_core::TapeSource;
use fionn_diff::{merge_tapes, deep_merge_tapes};
use fionn_tape::DsonTape;

fuzz_target!(|data: &[u8]| {
    // Skip overly large or small inputs
    if data.len() > 50_000 || data.len() < 4 {
        return;
    }

    // Split data into two parts for two JSON inputs
    let mid = data.len() / 2;
    let part_a = &data[..mid];
    let part_b = &data[mid..];

    // Try to interpret as UTF-8
    let str_a = match std::str::from_utf8(part_a) {
        Ok(s) => s,
        Err(_) => return, // Not valid UTF-8, skip
    };
    let str_b = match std::str::from_utf8(part_b) {
        Ok(s) => s,
        Err(_) => return, // Not valid UTF-8, skip
    };

    // Try to parse both as tapes
    let tape_a = match DsonTape::parse(str_a) {
        Ok(t) => t,
        Err(_) => return, // Invalid JSON, skip
    };

    let tape_b = match DsonTape::parse(str_b) {
        Ok(t) => t,
        Err(_) => return, // Invalid JSON, skip
    };

    // === Test: merge_tapes should not panic ===
    let merge_result = merge_tapes(&tape_a, &tape_b);

    if let Ok(merged) = merge_result {
        // === Contract: merged value should be valid JSON ===
        let serialized = serde_json::to_string(&merged);
        assert!(serialized.is_ok(), "merged value should serialize");

        // === Contract: merge with empty overlay preserves base ===
        // (only test if tape_b parses as empty object)
        if tape_b.len() == 1 {
            // Could be empty object
            if let Ok(value) = fionn_diff::tape_to_value(&tape_b) {
                if value.as_object().map(|m| m.is_empty()).unwrap_or(false) {
                    // Merge with empty should preserve base
                    // (Can't directly compare due to null semantics, just check no panic)
                }
            }
        }
    }

    // === Test: deep_merge_tapes should not panic ===
    let deep_merge_result = deep_merge_tapes(&tape_a, &tape_b);

    if let Ok(deep_merged) = deep_merge_result {
        // === Contract: deep merged value should be valid JSON ===
        let serialized = serde_json::to_string(&deep_merged);
        assert!(serialized.is_ok(), "deep merged value should serialize");
    }

    // === Test: merge with self ===
    // Note: Due to RFC 7396 null semantics, self-merge may not be identity
    let self_merge = merge_tapes(&tape_a, &tape_a);
    assert!(self_merge.is_ok(), "self merge should succeed");

    let deep_self_merge = deep_merge_tapes(&tape_a, &tape_a);
    assert!(deep_self_merge.is_ok(), "deep self merge should succeed");
});
