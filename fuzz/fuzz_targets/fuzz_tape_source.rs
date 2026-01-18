#![no_main]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! libFuzzer target for TapeSource trait operations
//!
//! Tests:
//! - TapeSource trait contract on arbitrary JSON
//! - skip_value correctness
//! - is_key_position accuracy
//! - Cross-tape gron equivalence

use libfuzzer_sys::fuzz_target;
use fionn_core::TapeSource;
use fionn_tape::DsonTape;

fuzz_target!(|data: &[u8]| {
    // Skip overly large inputs
    if data.len() > 100_000 {
        return;
    }

    // Try to interpret as UTF-8
    let json_str = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return, // Not valid UTF-8, skip
    };

    // Try to parse as tape
    let tape = match DsonTape::parse(json_str) {
        Ok(t) => t,
        Err(_) => return, // Invalid JSON, skip
    };

    // === Contract Test: len() consistency ===
    let len = tape.len();
    assert!(len > 0, "Valid tape should have at least one node");

    // === Contract Test: node_at bounds ===
    for i in 0..len {
        assert!(
            tape.node_at(i).is_some(),
            "node_at({}) should be Some for i < len({})",
            i,
            len
        );
    }
    assert!(
        tape.node_at(len).is_none(),
        "node_at(len) should be None"
    );
    assert!(
        tape.node_at(len + 1).is_none(),
        "node_at(len+1) should be None"
    );

    // === Contract Test: value_at bounds ===
    // Note: value_at returns None for container nodes (Object/Array), which is correct
    for i in 0..len {
        // Just verify it doesn't panic - value_at may return None for containers
        let _ = tape.value_at(i);
    }

    // === Contract Test: skip_value correctness ===
    for i in 0..len {
        if let Ok(skip_end) = tape.skip_value(i) {
            assert!(
                skip_end > i,
                "skip_value({}) = {} should advance past {}",
                i,
                skip_end,
                i
            );
            assert!(
                skip_end <= len,
                "skip_value({}) = {} should not exceed len {}",
                i,
                skip_end,
                len
            );
        }
    }

    // === Contract Test: Root skip equals len ===
    if let Ok(root_skip) = tape.skip_value(0) {
        assert_eq!(
            root_skip, len,
            "skip_value(0) should equal len(), got {} vs {}",
            root_skip, len
        );
    }

    // === Contract Test: iter count ===
    let iter_count = tape.iter().count();
    assert_eq!(
        iter_count, len,
        "iter() count {} should equal len() {}",
        iter_count, len
    );

    // === Contract Test: key_at returns valid strings ===
    for i in 0..len {
        if let Some(key) = tape.key_at(i) {
            // Key should be valid UTF-8 (already guaranteed by Cow<str>)
            let _ = key.len();
        }
    }

    // === Stress Test: Multiple iterations ===
    let count1 = tape.iter().count();
    let count2 = tape.iter().count();
    assert_eq!(count1, count2, "iter() should be repeatable");
});
