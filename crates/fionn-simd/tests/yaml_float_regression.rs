// SPDX-License-Identifier: MIT OR Apache-2.0
//! Regression test for YAML float roundtrip precision
//!
//! This documents the expected behavior when large numbers are roundtripped
//! through YAML->JSON->Value. Due to IEEE 754 float precision limits,
//! exact equality cannot be guaranteed for numbers that exceed f64 precision.
//!
//! Crash input: Base64 `AjQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NA==`
//! Which decodes to: 0x02 + 57 bytes of '4' (ASCII 0x34)
//! The payload (after 2 format bytes): 56 '4's = "44444444444444444444444444444444444444444444444444444444"

use serde_json::Value;

/// Regression test for crash-ace58ffcab2db2fd025438b1b7f1bcc78c49f193
///
/// The issue: A 56-digit number like "4444...4444" (56 fours) cannot be
/// exactly represented in f64. When YAML parses it, it becomes an f64
/// approximation. When that f64 is serialized to JSON and parsed back,
/// slight precision differences can occur.
///
/// This is NOT a bug - it's expected IEEE 754 behavior.
#[test]
fn test_large_number_roundtrip_precision() {
    // The exact crash payload: 56 '4's
    let payload = "44444444444444444444444444444444444444444444444444444444";

    // Parse as YAML (interprets as number)
    let value: Value = serde_yaml::from_str(payload).expect("YAML parse should succeed");

    // Verify it's a number
    assert!(value.is_number(), "Should parse as number");

    // Get the f64 representation
    let original_f64 = value.as_f64().expect("Should be representable as f64");

    // Roundtrip through JSON string
    let json_str = serde_json::to_string(&value).expect("JSON serialize should succeed");
    let back: Value = serde_json::from_str(&json_str).expect("JSON parse should succeed");
    let roundtrip_f64 = back.as_f64().expect("Should be representable as f64");

    // The values may not be bit-identical due to float precision
    // But they should be very close (within relative epsilon)
    let diff = (original_f64 - roundtrip_f64).abs();
    let relative_diff = diff / original_f64.abs();

    // For numbers of this magnitude, we expect very small relative difference
    // f64 has ~15-17 significant decimal digits of precision
    assert!(
        relative_diff < 1e-14,
        "Float roundtrip should preserve value within precision limits.\n\
         Original: {original_f64}\n\
         Roundtrip: {roundtrip_f64}\n\
         Relative diff: {relative_diff}\n\
         JSON string: {json_str}",
    );
}

/// Test that various large numbers roundtrip within precision tolerance
#[test]
fn test_various_large_number_roundtrips() {
    let test_cases = [
        "999999999999999999999999999999", // 30 nines
        "123456789012345678901234567890", // Mixed digits
        "44444444444444444444444444444444444444444444444444444444", // The crash case
        "1e+100",                         // Scientific notation
        "9.999999999999999e+308",         // Near f64 max
    ];

    for payload in &test_cases {
        let Ok(value) = serde_yaml::from_str::<Value>(payload) else {
            continue;
        };
        let Some(original_f64) = value.as_f64() else {
            continue;
        };
        if original_f64.is_nan() || original_f64.is_infinite() {
            continue;
        }
        let Ok(json_str) = serde_json::to_string(&value) else {
            continue;
        };
        let Ok(back) = serde_json::from_str::<Value>(&json_str) else {
            continue;
        };
        let Some(roundtrip_f64) = back.as_f64() else {
            continue;
        };

        let diff = (original_f64 - roundtrip_f64).abs();
        let relative_diff = if original_f64.abs() > f64::EPSILON {
            diff / original_f64.abs()
        } else {
            diff
        };

        assert!(
            relative_diff < 1e-10,
            "Payload {payload:?} failed roundtrip: {original_f64} vs {roundtrip_f64} (rel diff: {relative_diff})",
        );
    }
}
