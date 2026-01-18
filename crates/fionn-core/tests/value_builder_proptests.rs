// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for `ValueBuilder` implementations
//!
//! These tests verify:
//! - `ValueBuilder` primitive creation consistency
//! - Object and array construction
//! - Serialization roundtrip where applicable

use fionn_core::value_builder::{JsonBuilder, ValueBuilder};
use proptest::prelude::*;
use serde_json::{Value, json};

// =============================================================================
// Value Generators
// =============================================================================

/// Generate arbitrary strings for testing
fn arb_string() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_]{0,20}".prop_map(String::from)
}

/// Generate arbitrary integers
fn arb_int() -> impl Strategy<Value = i64> {
    any::<i64>()
}

/// Generate arbitrary floats (finite only)
fn arb_float() -> impl Strategy<Value = f64> {
    any::<f64>().prop_filter("finite", |f| f.is_finite())
}

// =============================================================================
// JsonBuilder Property Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Property: null() always produces JSON null
    #[test]
    fn prop_json_null_consistent(_seed in any::<u64>()) {
        let mut builder = JsonBuilder;
        let value = builder.null();
        prop_assert!(value.is_null(), "null() should produce null");
    }

    /// Property: bool() produces correct JSON booleans
    #[test]
    fn prop_json_bool_roundtrip(b in any::<bool>()) {
        let mut builder = JsonBuilder;
        let value = builder.bool(b);
        prop_assert_eq!(value.as_bool(), Some(b), "bool() should roundtrip");
    }

    /// Property: int() produces correct JSON integers
    #[test]
    fn prop_json_int_roundtrip(n in arb_int()) {
        let mut builder = JsonBuilder;
        let value = builder.int(n);
        prop_assert_eq!(value.as_i64(), Some(n), "int() should roundtrip");
    }

    /// Property: float() produces correct JSON floats
    #[test]
    fn prop_json_float_roundtrip(f in arb_float()) {
        let mut builder = JsonBuilder;
        let value = builder.float(f);
        let result = value.as_f64();
        prop_assert!(result.is_some(), "float() should produce a number");
        let diff = (result.unwrap() - f).abs();
        prop_assert!(diff < 1e-10, "float() should roundtrip: {} vs {}", f, result.unwrap());
    }

    /// Property: string() produces correct JSON strings
    #[test]
    fn prop_json_string_roundtrip(s in arb_string()) {
        let mut builder = JsonBuilder;
        let value = builder.string(&s);
        prop_assert_eq!(value.as_str(), Some(s.as_str()), "string() should roundtrip");
    }

    /// Property: empty_object() produces empty JSON object
    #[test]
    fn prop_json_empty_object(_seed in any::<u64>()) {
        let mut builder = JsonBuilder;
        let value = builder.empty_object();
        prop_assert!(value.is_object(), "empty_object() should produce object");
        prop_assert_eq!(value.as_object().unwrap().len(), 0, "empty_object() should be empty");
    }

    /// Property: empty_array() produces empty JSON array
    #[test]
    fn prop_json_empty_array(_seed in any::<u64>()) {
        let mut builder = JsonBuilder;
        let value = builder.empty_array();
        prop_assert!(value.is_array(), "empty_array() should produce array");
        prop_assert_eq!(value.as_array().unwrap().len(), 0, "empty_array() should be empty");
    }

    /// Property: insert_field() adds field correctly
    #[test]
    fn prop_json_insert_field(key in arb_string(), val in arb_int()) {
        let mut builder = JsonBuilder;
        let mut obj = builder.empty_object();
        let int_val = builder.int(val);
        builder.insert_field(&mut obj, &key, int_val);

        prop_assert!(obj.is_object(), "Should still be object");
        prop_assert_eq!(obj.get(&key).and_then(Value::as_i64), Some(val), "Field should be inserted");
    }

    /// Property: push_element() adds element correctly
    #[test]
    fn prop_json_push_element(val in arb_int()) {
        let mut builder = JsonBuilder;
        let mut arr = builder.empty_array();
        let int_val = builder.int(val);
        builder.push_element(&mut arr, int_val);

        prop_assert!(arr.is_array(), "Should still be array");
        let arr_ref = arr.as_array().unwrap();
        prop_assert_eq!(arr_ref.len(), 1, "Array should have 1 element");
        prop_assert_eq!(arr_ref[0].as_i64(), Some(val), "Element should be correct");
    }

    /// Property: multiple inserts accumulate
    #[test]
    fn prop_json_multiple_inserts(keys in proptest::collection::vec(arb_string(), 1..5)) {
        let mut builder = JsonBuilder;
        let mut obj = builder.empty_object();

        for (i, key) in keys.iter().enumerate() {
            let val = builder.int(i as i64);
            builder.insert_field(&mut obj, key, val);
        }

        let obj_map = obj.as_object().unwrap();
        // Note: duplicate keys will overwrite, so we count unique keys
        let unique_keys: std::collections::HashSet<_> = keys.iter().collect();
        prop_assert_eq!(obj_map.len(), unique_keys.len(), "Should have correct number of fields");
    }

    /// Property: multiple pushes accumulate in order
    #[test]
    fn prop_json_multiple_pushes(values in proptest::collection::vec(arb_int(), 1..10)) {
        let mut builder = JsonBuilder;
        let mut arr = builder.empty_array();

        for val in &values {
            let elem = builder.int(*val);
            builder.push_element(&mut arr, elem);
        }

        let arr_ref = arr.as_array().unwrap();
        prop_assert_eq!(arr_ref.len(), values.len(), "Should have correct length");

        for (i, val) in values.iter().enumerate() {
            prop_assert_eq!(arr_ref[i].as_i64(), Some(*val), "Element {} should be correct", i);
        }
    }

    /// Property: nested structures work correctly
    #[test]
    fn prop_json_nested_object(
        outer_key in arb_string(),
        inner_key in arb_string(),
        val in arb_int()
    ) {
        let mut builder = JsonBuilder;

        // Build inner object
        let mut inner = builder.empty_object();
        let int_val = builder.int(val);
        builder.insert_field(&mut inner, &inner_key, int_val);

        // Build outer object
        let mut outer = builder.empty_object();
        builder.insert_field(&mut outer, &outer_key, inner);

        // Verify structure
        prop_assert!(outer.is_object());
        if let Some(inner_val) = outer.get(&outer_key) {
            prop_assert!(inner_val.is_object());
            prop_assert_eq!(
                inner_val.get(&inner_key).and_then(Value::as_i64),
                Some(val)
            );
        }
    }

    /// Property: serialize produces valid JSON
    #[test]
    fn prop_json_serialize_valid(key in arb_string(), val in arb_string()) {
        let mut builder = JsonBuilder;
        let mut obj = builder.empty_object();
        let str_val = builder.string(&val);
        builder.insert_field(&mut obj, &key, str_val);

        let serialized = builder.serialize(&obj);
        prop_assert!(serialized.is_ok(), "serialize should succeed");

        // Verify it's valid JSON by parsing it
        let parsed: Result<Value, _> = serde_json::from_str(&serialized.unwrap());
        prop_assert!(parsed.is_ok(), "serialized JSON should be valid");
    }

    /// Property: build_from_tape_value produces correct output
    #[test]
    fn prop_json_from_tape_value_int(n in arb_int()) {
        use fionn_core::tape_source::TapeValue;

        let mut builder = JsonBuilder;
        let tape_val = TapeValue::Int(n);
        let result = builder.build_from_tape_value(&tape_val);

        prop_assert_eq!(result.as_i64(), Some(n), "build_from_tape_value should convert Int correctly");
    }

    /// Property: build_from_tape_value preserves string content
    #[test]
    fn prop_json_from_tape_value_string(s in arb_string()) {
        use fionn_core::tape_source::TapeValue;
        use std::borrow::Cow;

        let mut builder = JsonBuilder;
        let tape_val = TapeValue::String(Cow::Owned(s.clone()));
        let result = builder.build_from_tape_value(&tape_val);

        prop_assert_eq!(result.as_str(), Some(s.as_str()), "build_from_tape_value should convert String correctly");
    }
}

// =============================================================================
// Integration Tests
// =============================================================================

#[test]
fn test_json_builder_complex_structure() {
    let mut builder = JsonBuilder;

    // Build a complex structure
    let mut root = builder.empty_object();

    // Add a string field
    let name = builder.string("test");
    builder.insert_field(&mut root, "name", name);

    // Add a numeric field
    let count = builder.int(42);
    builder.insert_field(&mut root, "count", count);

    // Add an array field
    let mut items = builder.empty_array();
    let elem1 = builder.int(1);
    builder.push_element(&mut items, elem1);
    let elem2 = builder.int(2);
    builder.push_element(&mut items, elem2);
    let elem3 = builder.int(3);
    builder.push_element(&mut items, elem3);
    builder.insert_field(&mut root, "items", items);

    // Add a nested object
    let mut nested = builder.empty_object();
    let active = builder.bool(true);
    builder.insert_field(&mut nested, "active", active);
    builder.insert_field(&mut root, "nested", nested);

    // Serialize and verify
    let json_str = builder.serialize(&root).unwrap();
    let parsed: Value = serde_json::from_str(&json_str).unwrap();

    assert_eq!(parsed["name"], "test");
    assert_eq!(parsed["count"], 42);
    assert_eq!(parsed["items"], json!([1, 2, 3]));
    assert_eq!(parsed["nested"]["active"], true);
}

#[test]
fn test_json_builder_target_format() {
    use fionn_core::format::FormatKind;

    let builder = JsonBuilder;
    assert_eq!(builder.target_format(), FormatKind::Json);
}
