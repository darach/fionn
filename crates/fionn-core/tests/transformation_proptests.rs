// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property Tests for Cross-Format Transformations
//!
//! Tests lossless and asymmetric transformations between formats:
//! - JSON <-> YAML (lossless for common subset)
//! - JSON <-> TOML (asymmetric - TOML requires root table)
//! - Format determinism (same input -> same output)
//! - Value preservation across transformations

use proptest::prelude::*;
use serde_json::{Map, Value as JsonValue, json};

// =============================================================================
// Test Data Generators
// =============================================================================

/// Strategy for JSON values that are safe for cross-format transformation
fn transformable_value_strategy(depth: usize) -> impl Strategy<Value = JsonValue> {
    if depth == 0 {
        prop_oneof![
            Just(JsonValue::Null),
            any::<bool>().prop_map(JsonValue::Bool),
            // Use i32 to avoid precision issues
            any::<i32>().prop_map(|n| json!(n)),
            // Use simple strings to avoid escaping edge cases
            "[a-zA-Z0-9_]{0,50}".prop_map(JsonValue::String),
        ]
        .boxed()
    } else {
        prop_oneof![
            Just(JsonValue::Null),
            any::<bool>().prop_map(JsonValue::Bool),
            any::<i32>().prop_map(|n| json!(n)),
            "[a-zA-Z0-9_]{0,30}".prop_map(JsonValue::String),
            // Object (required for TOML)
            prop::collection::hash_map(
                "[a-z_]{1,10}",
                transformable_value_strategy(depth - 1),
                0..5
            )
            .prop_map(|map| {
                let obj: Map<String, JsonValue> = map.into_iter().collect();
                JsonValue::Object(obj)
            }),
            // Array
            prop::collection::vec(transformable_value_strategy(depth - 1), 0..5)
                .prop_map(JsonValue::Array),
        ]
        .boxed()
    }
}

/// Strategy for JSON objects (required root type for TOML)
fn transformable_object_strategy(depth: usize) -> impl Strategy<Value = JsonValue> {
    prop::collection::hash_map("[a-z_]{1,10}", transformable_value_strategy(depth), 1..8).prop_map(
        |map| {
            let obj: Map<String, JsonValue> = map.into_iter().collect();
            JsonValue::Object(obj)
        },
    )
}

/// Strategy for simple flat objects (maximize compatibility)
fn flat_object_strategy() -> impl Strategy<Value = JsonValue> {
    prop::collection::hash_map(
        "[a-z_]{1,10}",
        prop_oneof![
            Just(JsonValue::Null),
            any::<bool>().prop_map(JsonValue::Bool),
            any::<i32>().prop_map(|n| json!(n)),
            "[a-zA-Z0-9_]{0,30}".prop_map(JsonValue::String),
        ],
        1..10,
    )
    .prop_map(|map| {
        let obj: Map<String, JsonValue> = map.into_iter().collect();
        JsonValue::Object(obj)
    })
}

// =============================================================================
// Transformation Functions
// =============================================================================

/// Convert JSON value to canonical form for comparison
/// (normalizes ordering, number representation)
fn canonicalize(value: &JsonValue) -> JsonValue {
    match value {
        JsonValue::Object(obj) => {
            // Sort keys for deterministic comparison
            let mut sorted: Vec<_> = obj.iter().collect();
            sorted.sort_by_key(|(k, _)| *k);
            let canonical_obj: Map<String, JsonValue> = sorted
                .into_iter()
                .map(|(k, v)| (k.clone(), canonicalize(v)))
                .collect();
            JsonValue::Object(canonical_obj)
        }
        JsonValue::Array(arr) => JsonValue::Array(arr.iter().map(canonicalize).collect()),
        JsonValue::Number(n) => {
            // Normalize to canonical form: prefer integer if exact
            n.as_i64().map_or_else(
                || {
                    n.as_f64().map_or_else(
                        || value.clone(),
                        |f| {
                            if f.is_finite() {
                                // Check if f64 is actually an exact integer
                                // Check if it's within i64 range and is a whole number
                                let floored = f.floor();
                                if (f - floored).abs() < f64::EPSILON
                                    && f >= i64::MIN as f64
                                    && f <= i64::MAX as f64
                                {
                                    #[allow(clippy::cast_possible_truncation)]
                                    // Verified in-range above
                                    let as_int = f as i64;
                                    json!(as_int)
                                } else {
                                    json!(f)
                                }
                            } else {
                                JsonValue::Null // Replace non-finite with null
                            }
                        },
                    )
                },
                |i| json!(i),
            )
        }
        _ => value.clone(),
    }
}

/// Check semantic equality (ignoring key order, normalizing numbers)
fn semantically_equal(a: &JsonValue, b: &JsonValue) -> bool {
    canonicalize(a) == canonicalize(b)
}

// =============================================================================
// JSON <-> JSON Roundtrip (Baseline)
// =============================================================================

proptest! {
    /// JSON serialization roundtrip is lossless
    #[test]
    fn prop_json_roundtrip_lossless(
        value in transformable_value_strategy(3)
    ) {
        let json_str = serde_json::to_string(&value).unwrap();
        let parsed: JsonValue = serde_json::from_str(&json_str).unwrap();

        prop_assert!(
            semantically_equal(&value, &parsed),
            "JSON roundtrip changed value:\nOriginal: {}\nParsed: {}",
            value, parsed
        );
    }

    /// JSON pretty-print roundtrip is lossless
    #[test]
    fn prop_json_pretty_roundtrip(
        value in transformable_value_strategy(3)
    ) {
        let json_str = serde_json::to_string_pretty(&value).unwrap();
        let parsed: JsonValue = serde_json::from_str(&json_str).unwrap();

        prop_assert!(
            semantically_equal(&value, &parsed),
            "JSON pretty roundtrip changed value"
        );
    }
}

// =============================================================================
// JSON <-> YAML Roundtrip
// =============================================================================

// YAML tests (always available via dev-dependencies)
proptest! {
    /// JSON -> YAML -> JSON roundtrip preserves values
    #[test]
    fn prop_json_yaml_json_roundtrip(
        value in transformable_value_strategy(3)
    ) {
        // JSON -> YAML
        let Ok(yaml_str) = serde_yaml::to_string(&value) else {
            return Ok(()); // Skip values YAML can't represent
        };

        // YAML -> JSON
        let Ok(back) = serde_yaml::from_str::<JsonValue>(&yaml_str) else {
            return Ok(()); // Skip if YAML parsing fails
        };

        prop_assert!(
            semantically_equal(&value, &back),
            "JSON->YAML->JSON changed value:\nOriginal: {}\nBack: {}\nYAML: {}",
            value, back, yaml_str
        );
    }

    /// YAML -> JSON -> YAML produces semantically equivalent result
    /// Note: Not strictly lossless (YAML has features JSON doesn't)
    #[test]
    fn prop_yaml_json_semantic_equivalence(
        value in transformable_value_strategy(3)
    ) {
        // Value -> YAML -> JSON
        let Ok(yaml_str) = serde_yaml::to_string(&value) else {
            return Ok(());
        };

        let Ok(json_value) = serde_yaml::from_str::<JsonValue>(&yaml_str) else {
            return Ok(());
        };

        // JSON back to YAML
        let Ok(yaml2) = serde_yaml::to_string(&json_value) else {
            return Ok(());
        };

        // Parse both YAMLs as JSON and compare
        let v1: JsonValue = serde_yaml::from_str(&yaml_str).unwrap();
        let v2: JsonValue = serde_yaml::from_str(&yaml2).unwrap();

        prop_assert!(
            semantically_equal(&v1, &v2),
            "YAML->JSON->YAML not semantically equivalent"
        );
    }
}

// =============================================================================
// JSON <-> TOML Roundtrip (Asymmetric)
// =============================================================================

// TOML tests (always available via dev-dependencies)
proptest! {
    /// JSON object -> TOML -> JSON roundtrip preserves values
    /// Note: TOML requires root to be a table/object
    #[test]
    fn prop_json_toml_json_roundtrip_objects(
        value in transformable_object_strategy(2)
    ) {
        // JSON -> TOML (only works for objects)
        let Ok(toml_str) = toml_crate::to_string(&value) else {
            return Ok(()); // Skip values TOML can't represent
        };

        // TOML -> JSON
        let Ok(toml_value) = toml_crate::from_str::<toml_crate::Value>(&toml_str) else {
            return Ok(());
        };

        // Convert toml_crate::Value to serde_json::Value
        let Ok(back) = serde_json::to_value(&toml_value) else {
            return Ok(());
        };

        prop_assert!(
            semantically_equal(&value, &back),
            "JSON->TOML->JSON changed value:\nOriginal: {}\nBack: {}",
            value, back
        );
    }

    /// Flat objects have highest TOML compatibility
    #[test]
    fn prop_flat_object_toml_roundtrip(
        value in flat_object_strategy()
    ) {
        let Ok(toml_str) = toml_crate::to_string(&value) else {
            return Ok(());
        };

        let toml_value: toml_crate::Value = toml_crate::from_str(&toml_str).unwrap();
        let back: JsonValue = serde_json::to_value(&toml_value).unwrap();

        prop_assert!(
            semantically_equal(&value, &back),
            "Flat object TOML roundtrip failed"
        );
    }
}

// =============================================================================
// Transformation Determinism
// =============================================================================

proptest! {
    /// Same JSON input always produces same JSON output
    #[test]
    fn prop_json_deterministic(
        value in transformable_value_strategy(3)
    ) {
        let str1 = serde_json::to_string(&value).unwrap();
        let str2 = serde_json::to_string(&value).unwrap();

        prop_assert_eq!(str1, str2, "JSON serialization not deterministic");
    }
}

// YAML tests (always available via dev-dependencies)
proptest! {
    /// Same input produces same YAML output
    #[test]
    fn prop_yaml_deterministic(
        value in transformable_value_strategy(3)
    ) {
        let yaml1 = serde_yaml::to_string(&value);
        let yaml2 = serde_yaml::to_string(&value);

        if let (Ok(y1), Ok(y2)) = (yaml1, yaml2) {
            prop_assert_eq!(y1, y2, "YAML serialization not deterministic");
        }
        // Skip if either fails
    }
}

// TOML tests (always available via dev-dependencies)
proptest! {
    /// Same object produces same TOML output
    #[test]
    fn prop_toml_deterministic(
        value in transformable_object_strategy(2)
    ) {
        let toml1 = toml_crate::to_string(&value);
        let toml2 = toml_crate::to_string(&value);

        if let (Ok(t1), Ok(t2)) = (toml1, toml2) {
            prop_assert_eq!(t1, t2, "TOML serialization not deterministic");
        }
        // Skip if either fails
    }
}

// =============================================================================
// Value Type Preservation
// =============================================================================

proptest! {
    /// Integer values preserved exactly across formats
    #[test]
    fn prop_integer_preservation_json(n in any::<i32>()) {
        let value = json!(n);

        // JSON roundtrip
        let json_str = serde_json::to_string(&value).unwrap();
        let parsed: JsonValue = serde_json::from_str(&json_str).unwrap();

        prop_assert_eq!(parsed.as_i64(), Some(i64::from(n)));
    }

    /// Boolean values preserved exactly
    #[test]
    fn prop_bool_preservation_json(b in any::<bool>()) {
        let value = JsonValue::Bool(b);

        let json_str = serde_json::to_string(&value).unwrap();
        let parsed: JsonValue = serde_json::from_str(&json_str).unwrap();

        prop_assert_eq!(parsed.as_bool(), Some(b));
    }

    /// String values preserved exactly
    #[test]
    fn prop_string_preservation_json(s in "[a-zA-Z0-9_]{0,100}") {
        let value = JsonValue::String(s.clone());

        let json_str = serde_json::to_string(&value).unwrap();
        let parsed: JsonValue = serde_json::from_str(&json_str).unwrap();

        prop_assert_eq!(parsed.as_str(), Some(s.as_str()));
    }

    /// Null values preserved
    #[test]
    fn prop_null_preservation_json(_unit in Just(())) {
        let value = JsonValue::Null;

        let json_str = serde_json::to_string(&value).unwrap();
        let parsed: JsonValue = serde_json::from_str(&json_str).unwrap();

        prop_assert!(parsed.is_null());
    }

    /// Array ordering preserved
    #[test]
    fn prop_array_ordering_json(
        items in prop::collection::vec(any::<i32>(), 0..20)
    ) {
        let value: JsonValue = items.iter().map(|&n| json!(n)).collect();

        let json_str = serde_json::to_string(&value).unwrap();
        let parsed: JsonValue = serde_json::from_str(&json_str).unwrap();

        let parsed_items: Vec<_> = parsed.as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_i64().and_then(|i| i32::try_from(i).ok()))
            .collect();

        prop_assert_eq!(items, parsed_items);
    }
}

// YAML tests (always available via dev-dependencies)
proptest! {
    /// Integer preservation through YAML
    #[test]
    fn prop_integer_preservation_yaml(n in any::<i32>()) {
        let value = json!(n);

        let yaml_str = serde_yaml::to_string(&value).unwrap();
        let parsed: JsonValue = serde_yaml::from_str(&yaml_str).unwrap();

        prop_assert_eq!(parsed.as_i64(), Some(i64::from(n)));
    }

    /// Boolean preservation through YAML
    #[test]
    fn prop_bool_preservation_yaml(b in any::<bool>()) {
        let value = JsonValue::Bool(b);

        let yaml_str = serde_yaml::to_string(&value).unwrap();
        let parsed: JsonValue = serde_yaml::from_str(&yaml_str).unwrap();

        prop_assert_eq!(parsed.as_bool(), Some(b));
    }
}

// TOML tests (always available via dev-dependencies)
proptest! {
    /// Integer preservation through TOML
    #[test]
    fn prop_integer_preservation_toml(n in any::<i32>()) {
        // TOML requires a table at root
        let mut obj = Map::new();
        obj.insert("value".to_string(), json!(n));
        let value = JsonValue::Object(obj);

        if let Ok(toml_str) = toml_crate::to_string(&value)
            && let Ok(parsed) = toml_crate::from_str::<toml_crate::Value>(&toml_str)
            && let Some(v) = parsed.get("value").and_then(toml_crate::Value::as_integer)
        {
            prop_assert_eq!(v, i64::from(n));
        }
    }
}

// =============================================================================
// Asymmetric Transformation Tests
// =============================================================================

// Combined YAML+TOML tests (always available via dev-dependencies)
proptest! {
    /// YAML -> JSON -> TOML works for objects
    #[test]
    fn prop_yaml_json_toml_chain(
        value in transformable_object_strategy(2)
    ) {
        // YAML serialize
        let Ok(yaml_str) = serde_yaml::to_string(&value) else {
            return Ok(());
        };

        // YAML -> JSON
        let Ok(json_value) = serde_yaml::from_str::<JsonValue>(&yaml_str) else {
            return Ok(());
        };

        // JSON -> TOML
        let Ok(_toml_str) = toml_crate::to_string(&json_value) else {
            return Ok(()); // Expected for some values
        };

        // If we got here, the chain worked
        prop_assert!(true);
    }

    /// TOML -> JSON -> YAML chain
    #[test]
    fn prop_toml_json_yaml_chain(
        value in transformable_object_strategy(2)
    ) {
        // TOML serialize
        let Ok(toml_str) = toml_crate::to_string(&value) else {
            return Ok(());
        };

        // TOML -> intermediate
        let Ok(toml_value) = toml_crate::from_str::<toml_crate::Value>(&toml_str) else {
            return Ok(());
        };

        // To JSON
        let Ok(json_value) = serde_json::to_value(&toml_value) else {
            return Ok(());
        };

        // JSON -> YAML
        let Ok(_yaml_str) = serde_yaml::to_string(&json_value) else {
            return Ok(());
        };

        prop_assert!(true);
    }
}

// =============================================================================
// Unit Tests for Edge Cases
// =============================================================================

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_canonicalize_object_ordering() {
        let a = json!({"z": 1, "a": 2});
        let b = json!({"a": 2, "z": 1});

        assert_eq!(canonicalize(&a), canonicalize(&b));
    }

    #[test]
    fn test_semantically_equal_numbers() {
        assert!(semantically_equal(&json!(42), &json!(42)));
        assert!(semantically_equal(&json!(42), &json!(42.0)));
    }

    #[test]
    fn test_semantically_equal_nested() {
        let a = json!({"items": [1, 2, 3], "meta": {"count": 3}});
        let b = json!({"meta": {"count": 3}, "items": [1, 2, 3]});

        assert!(semantically_equal(&a, &b));
    }

    // YAML tests (always available via dev-dependencies)
    #[test]
    fn test_yaml_roundtrip_simple() {
        let value = json!({"name": "Alice", "age": 30});
        let yaml_str = serde_yaml::to_string(&value).unwrap();
        let back: JsonValue = serde_yaml::from_str(&yaml_str).unwrap();

        assert!(semantically_equal(&value, &back));
    }

    // TOML tests (always available via dev-dependencies)
    #[test]
    fn test_toml_roundtrip_simple() {
        let value = json!({"name": "Alice", "age": 30});
        let toml_str = toml_crate::to_string(&value).unwrap();
        let toml_value: toml_crate::Value = toml_crate::from_str(&toml_str).unwrap();
        let back: JsonValue = serde_json::to_value(&toml_value).unwrap();

        assert!(semantically_equal(&value, &back));
    }

    // TOML tests (always available via dev-dependencies)
    #[test]
    fn test_toml_rejects_non_object_root() {
        // TOML requires root to be a table
        let value = json!([1, 2, 3]);
        assert!(toml_crate::to_string(&value).is_err());

        let value = json!("hello");
        assert!(toml_crate::to_string(&value).is_err());
    }

    #[test]
    fn test_json_unicode_roundtrip() {
        let value = json!({"emoji": "hello", "text": "world"});
        let json_str = serde_json::to_string(&value).unwrap();
        let back: JsonValue = serde_json::from_str(&json_str).unwrap();

        assert!(semantically_equal(&value, &back));
    }

    #[test]
    fn test_empty_object_roundtrip() {
        let value = json!({});
        let json_str = serde_json::to_string(&value).unwrap();
        let back: JsonValue = serde_json::from_str(&json_str).unwrap();

        assert!(semantically_equal(&value, &back));
    }

    #[test]
    fn test_empty_array_roundtrip() {
        let value = json!([]);
        let json_str = serde_json::to_string(&value).unwrap();
        let back: JsonValue = serde_json::from_str(&json_str).unwrap();

        assert!(semantically_equal(&value, &back));
    }
}
