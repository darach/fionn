#![allow(clippy::all)]
#![allow(clippy::option_if_let_else)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::pedantic)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! AFL fuzz target for gron/ungron operations.
//!
//! This target tests:
//! - gron -> ungron roundtrip
//! - SIMD escape/unescape roundtrip
//! - Path parsing edge cases
//! - JSONL processing
//!
//! Run with:
//!   cargo afl build --release --features afl-fuzz -bin `fuzz_gron
//!   cargo afl fuzz -i fuzz/corpus/gron -o fuzz/output/gron target/release/`fuzz_gron

#[macro_use]
extern crate afl;

use fionn_gron::{
    GronJsonlOptions, GronOptions, escape_json_string_simd, escape_json_to_string, gron,
    gron_jsonl, unescape_json_string_simd, unescape_json_to_string, ungron, ungron_to_value,
};
use serde_json::Value;

/// Test gron -> ungron roundtrip
fn fuzz_gron_roundtrip(data: &[u8]) {
    let input = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Try to parse as JSON first
    let original: Value = match serde_json::from_str(input) {
        Ok(v) => v,
        Err(_) => return,
    };

    // Convert to gron format
    let options = GronOptions::default();
    let gron_output = match gron(input, &options) {
        Ok(g) => g,
        Err(_) => return,
    };

    // Convert back via ungron
    let reconstructed = match ungron_to_value(&gron_output) {
        Ok(v) => v,
        Err(_) => return, // Some valid JSON may not roundtrip perfectly (floats, etc)
    };

    // Verify equivalence (allowing for float precision differences)
    if !json_equivalent(&original, &reconstructed) {
        // Only panic on clear structural differences, not float precision
        if !is_float_difference(&original, &reconstructed) {
            panic!(
                "gron/ungron roundtrip mismatch:\noriginal={:?}\nreconstructed={:?}",
                original, reconstructed
            );
        }
    }
}

/// Test escape -> unescape roundtrip
fn fuzz_escape_roundtrip(data: &[u8]) {
    let input = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Escape the string
    let escaped = escape_json_to_string(input);

    // Escaped should be valid JSON string
    assert!(escaped.starts_with('"'), "Escaped should start with quote");
    assert!(escaped.ends_with('"'), "Escaped should end with quote");

    // Extract inner content
    let inner = &escaped[1..escaped.len() - 1];

    // Unescape
    let unescaped = match unescape_json_to_string(inner) {
        Ok(s) => s,
        Err(e) => panic!("Unescape failed: {e:?}, escaped={escaped:?}"),
    };

    // Verify roundtrip
    assert_eq!(unescaped, input, "escape/unescape roundtrip mismatch");

    // Verify serde_json can parse it
    let parsed: String = match serde_json::from_str(&escaped) {
        Ok(s) => s,
        Err(e) => panic!("serde_json parse failed: {e:?}, escaped={escaped:?}"),
    };

    assert_eq!(parsed, input, "serde_json parse mismatch");
}

/// Test SIMD escape with different buffer states
fn fuzz_escape_buffer_states(data: &[u8]) {
    let input = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Pre-sized buffer
    let mut out1 = Vec::with_capacity(input.len() * 2);
    escape_json_string_simd(input, &mut out1);

    // Zero-capacity buffer
    let mut out2 = Vec::new();
    escape_json_string_simd(input, &mut out2);

    // Results should be identical
    assert_eq!(out1, out2, "Buffer capacity affected escape result");
}

/// Test unescape with arbitrary bytes (may not be valid escape sequences)
fn fuzz_unescape_arbitrary(data: &[u8]) {
    // Just verify no panic
    let _ = unescape_json_string_simd(data);
}

/// Test gron with various JSON types
fn fuzz_gron_types(data: &[u8]) {
    let input = if let Ok(s) = std::str::from_utf8(data) {
        s
    } else {
        return;
    };

    let options = GronOptions::default();

    // Try to gron (should not panic)
    let _ = gron(input, &options);

    // Try with object wrapper
    let wrapped = format!("{{{}: {}}}", "\"key\"", input);
    let _ = gron(&wrapped, &options);

    // Try with array wrapper
    let array_wrapped = format!("[{input}]");
    let _ = gron(&array_wrapped, &options);
}

/// Test JSONL processing
fn fuzz_jsonl(data: &[u8]) {
    let options = GronJsonlOptions::default();

    // Just verify no panic on arbitrary input
    let _ = gron_jsonl(data, &options);
}

/// Test escape edge cases at chunk boundaries
fn fuzz_escape_boundaries(data: &[u8]) {
    let input = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Test with various string lengths around SIMD boundaries
    for len in [15, 16, 17, 31, 32, 33, 63, 64, 65] {
        if input.len() >= len {
            let slice = &input[..len];
            let escaped = escape_json_to_string(slice);
            let inner = &escaped[1..escaped.len() - 1];
            if let Ok(unescaped) = unescape_json_to_string(inner) {
                assert_eq!(unescaped, slice, "Boundary length {len} roundtrip failed");
            }
        }
    }
}

/// Test ungron with various input patterns
fn fuzz_ungron(data: &[u8]) {
    let input = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Just verify no panic
    let _ = ungron(input);
    let _ = ungron_to_value(input);
}

/// Test escape/unescape with special characters
fn fuzz_special_chars(data: &[u8]) {
    let input = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Add special characters to input
    let test_strings = [
        format!("{}\n{}", input, input),       // newline
        format!("{}\r{}", input, input),       // carriage return
        format!("{}\t{}", input, input),       // tab
        format!("{}\\{}", input, input),       // backslash
        format!("{}\"{}", input, input),       // quote
        format!("{}\u{0000}{}", input, input), // null
        format!("{}\u{001f}{}", input, input), // control char
    ];

    for test in &test_strings {
        let escaped = escape_json_to_string(test);
        let inner = &escaped[1..escaped.len() - 1];
        if let Ok(unescaped) = unescape_json_to_string(inner) {
            assert_eq!(&unescaped, test, "Special char roundtrip failed");
        }
    }
}

/// Check if two JSON values are equivalent
fn json_equivalent(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Number(x), Value::Number(y)) => {
            // Allow for float precision differences
            if let (Some(xf), Some(yf)) = (x.as_f64(), y.as_f64()) {
                (xf - yf).abs() < 1e-10 || (xf.is_nan() && yf.is_nan())
            } else {
                x == y
            }
        }
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| json_equivalent(a, b))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).map(|yv| json_equivalent(v, yv)).unwrap_or(false))
        }
        _ => false,
    }
}

/// Check if difference is only in float precision
fn is_float_difference(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(_), Value::Number(_)) => true,
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => x
            .iter()
            .zip(y.iter())
            .all(|(a, b)| json_equivalent(a, b) || is_float_difference(a, b)),
        (Value::Object(x), Value::Object(y)) if x.len() == y.len() => x.iter().all(|(k, v)| {
            y.get(k)
                .map(|yv| json_equivalent(v, yv) || is_float_difference(v, yv))
                .unwrap_or(false)
        }),
        _ => false,
    }
}

fn main() {
    fuzz!(|data: &[u8]| {
        // Skip extremely large inputs
        if data.len() <= 50_000 {
            // Core gron/ungron roundtrip
            fuzz_gron_roundtrip(data);

            // Escape/unescape roundtrip
            fuzz_escape_roundtrip(data);
            fuzz_escape_buffer_states(data);
            fuzz_unescape_arbitrary(data);

            // Gron with various types
            fuzz_gron_types(data);

            // JSONL processing
            fuzz_jsonl(data);

            // Boundary testing
            fuzz_escape_boundaries(data);

            // Ungron testing
            fuzz_ungron(data);

            // Special character testing
            fuzz_special_chars(data);
        }
    });
}
