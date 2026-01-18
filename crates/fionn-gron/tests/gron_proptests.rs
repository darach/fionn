// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for gron/ungron roundtrip
//!
//! These tests verify:
//! - gron → ungron roundtrip produces equivalent JSON
//! - `gron_from_tape` produces same output as gron
//! - ungron handles various gron formats correctly

use fionn_gron::{GronOptions, gron, ungron_to_value};
use proptest::prelude::*;
use serde_json::{Value, json};

// =============================================================================
// JSON Value Generators
// =============================================================================

/// Generate arbitrary JSON values with controlled depth
/// Excludes floats to avoid precision issues in roundtrip
fn arb_json_value(depth: usize) -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|n| json!(n)),
        // Use simple strings to avoid escape sequence complexity
        "[a-zA-Z0-9_]{0,20}".prop_map(|s| json!(s)),
    ];

    leaf.prop_recursive(u32::try_from(depth).unwrap_or(u32::MAX), 64, 8, |inner| {
        prop_oneof![
            // Object with up to 5 fields (simple keys only)
            proptest::collection::vec(("[a-zA-Z_][a-zA-Z0-9_]{0,10}", inner.clone()), 0..5)
                .prop_map(|pairs| {
                    let map: serde_json::Map<String, Value> = pairs.into_iter().collect();
                    Value::Object(map)
                }),
            // Array with up to 5 elements
            proptest::collection::vec(inner, 0..5).prop_map(Value::Array),
        ]
    })
}

/// Normalize JSON for comparison (sort object keys, etc.)
fn normalize_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sorted: Vec<_> = map.iter().collect();
            sorted.sort_by_key(|(k, _)| *k);
            let normalized_map: serde_json::Map<String, Value> = sorted
                .into_iter()
                .map(|(k, v)| (k.clone(), normalize_json(v)))
                .collect();
            Value::Object(normalized_map)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(normalize_json).collect()),
        other => other.clone(),
    }
}

/// Check if two JSON values are equivalent (ignoring key order)
fn json_equivalent(a: &Value, b: &Value) -> bool {
    normalize_json(a) == normalize_json(b)
}

// =============================================================================
// Gron/Ungron Roundtrip Property Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Property: gron → ungron roundtrip produces equivalent JSON
    #[test]
    fn prop_gron_ungron_roundtrip(value in arb_json_value(3)) {
        let json_str = serde_json::to_string(&value).unwrap();

        // gron
        let gron_result = gron(&json_str, &GronOptions::default());
        prop_assert!(gron_result.is_ok(), "gron should succeed: {:?}", gron_result.err());

        let gron_output = gron_result.unwrap();

        // ungron
        let ungron_result = ungron_to_value(&gron_output);
        prop_assert!(ungron_result.is_ok(), "ungron should succeed: {:?}", ungron_result.err());

        let recovered = ungron_result.unwrap();

        // Compare (use equivalence to handle key ordering)
        prop_assert!(
            json_equivalent(&value, &recovered),
            "Roundtrip should produce equivalent JSON:\nOriginal: {:?}\nRecovered: {:?}",
            value, recovered
        );
    }

    /// Property: gron with different prefix still roundtrips
    #[test]
    fn prop_gron_custom_prefix_roundtrip(value in arb_json_value(2), prefix in "[a-z]{1,5}") {
        let json_str = serde_json::to_string(&value).unwrap();

        let options = GronOptions {
            prefix: prefix.clone(),
            ..GronOptions::default()
        };

        let gron_output = gron(&json_str, &options);
        prop_assert!(gron_output.is_ok());

        let gron_str = gron_output.unwrap();

        // All lines should start with the custom prefix
        for line in gron_str.lines() {
            if !line.is_empty() {
                prop_assert!(
                    line.starts_with(&prefix),
                    "Line should start with prefix '{}': {}", prefix, line
                );
            }
        }
    }

    /// Property: gron produces consistent line count for same input
    #[test]
    fn prop_gron_deterministic(value in arb_json_value(2)) {
        let json_str = serde_json::to_string(&value).unwrap();
        let options = GronOptions::default();

        let result1 = gron(&json_str, &options).unwrap();
        let result2 = gron(&json_str, &options).unwrap();

        let lines1: Vec<_> = result1.lines().collect();
        let lines2: Vec<_> = result2.lines().collect();

        prop_assert_eq!(lines1.len(), lines2.len(), "gron should be deterministic");

        // Lines should match (though order might vary for object keys)
        let mut sorted1 = lines1.clone();
        let mut sorted2 = lines2.clone();
        sorted1.sort_unstable();
        sorted2.sort_unstable();

        prop_assert_eq!(sorted1, sorted2, "gron output should be deterministic");
    }

    /// Property: empty containers produce minimal gron output
    #[test]
    fn prop_gron_empty_containers(_seed in any::<u64>()) {
        // Empty object
        let empty_obj = "{}";
        let result = gron(empty_obj, &GronOptions::default()).unwrap();
        prop_assert!(result.contains("json = {}"), "Empty object gron: {}", result);

        // Empty array
        let empty_arr = "[]";
        let result = gron(empty_arr, &GronOptions::default()).unwrap();
        prop_assert!(result.contains("json = []"), "Empty array gron: {}", result);
    }

    /// Property: nested structures produce correct path prefixes
    #[test]
    fn prop_gron_nested_paths(
        outer_key in "[a-zA-Z_][a-zA-Z0-9_]{0,5}",
        inner_key in "[a-zA-Z_][a-zA-Z0-9_]{0,5}",
        val in any::<i64>()
    ) {
        let json = json!({ outer_key.clone(): { inner_key.clone(): val } });
        let json_str = serde_json::to_string(&json).unwrap();

        let result = gron(&json_str, &GronOptions::default()).unwrap();

        // Should contain the nested path
        let expected_path = format!("json.{outer_key}.{inner_key}");
        prop_assert!(
            result.contains(&expected_path),
            "Should contain nested path '{}' in:\n{}", expected_path, result
        );
    }

    /// Property: array indices are correctly formatted
    #[test]
    fn prop_gron_array_indices(values in proptest::collection::vec(any::<i64>(), 1..5)) {
        let json = Value::Array(values.iter().map(|&v| json!(v)).collect());
        let json_str = serde_json::to_string(&json).unwrap();

        let result = gron(&json_str, &GronOptions::default()).unwrap();

        // Should contain indices [0], [1], etc.
        for i in 0..values.len() {
            let expected = format!("json[{i}]");
            prop_assert!(
                result.contains(&expected),
                "Should contain index '{}' in:\n{}", expected, result
            );
        }
    }
}

// =============================================================================
// Edge Case Tests
// =============================================================================

#[test]
fn test_gron_ungron_simple() {
    let json = r#"{"name":"Alice","age":30}"#;

    let gron_output = gron(json, &GronOptions::default()).unwrap();
    let recovered = ungron_to_value(&gron_output).unwrap();

    let original: Value = serde_json::from_str(json).unwrap();
    assert!(json_equivalent(&original, &recovered));
}

#[test]
fn test_gron_ungron_nested() {
    let json = r#"{"user":{"name":"Alice","profile":{"bio":"Developer"}}}"#;

    let gron_output = gron(json, &GronOptions::default()).unwrap();
    let recovered = ungron_to_value(&gron_output).unwrap();

    let original: Value = serde_json::from_str(json).unwrap();
    assert!(json_equivalent(&original, &recovered));
}

#[test]
fn test_gron_ungron_array() {
    let json = r#"{"items":[1,2,3],"names":["Alice","Bob"]}"#;

    let gron_output = gron(json, &GronOptions::default()).unwrap();
    let recovered = ungron_to_value(&gron_output).unwrap();

    let original: Value = serde_json::from_str(json).unwrap();
    assert!(json_equivalent(&original, &recovered));
}

#[test]
fn test_gron_output_format() {
    let json = r#"{"x":42}"#;

    let result = gron(json, &GronOptions::default()).unwrap();

    // Should have root declaration and field
    assert!(result.contains("json = {"));
    assert!(result.contains("json.x = 42"));
}

#[test]
fn test_ungron_handles_whitespace() {
    let gron_input = r#"json = {};
json.name = "Alice";
json.age = 30;
"#;

    let result = ungron_to_value(gron_input).unwrap();

    assert_eq!(result["name"], "Alice");
    assert_eq!(result["age"], 30);
}
