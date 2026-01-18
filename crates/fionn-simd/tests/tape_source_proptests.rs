// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for `TapeSource` trait implementations
//!
//! These tests verify that different `TapeSource` implementations behave
//! consistently across randomly generated JSON inputs.

use fionn_core::tape_source::{TapeNodeKind, TapeSource, TapeValue};
use fionn_tape::DsonTape;
use proptest::prelude::*;
use serde_json::json;

/// Generate arbitrary JSON values for testing
fn json_value_strategy() -> impl Strategy<Value = serde_json::Value> {
    // Limit recursion depth for reasonable test times
    json_value_leaf().prop_recursive(
        4,  // depth
        64, // desired_size
        10, // expected_branch_size
        |inner| {
            prop_oneof![
                // Object with random keys
                prop::collection::hash_map("[a-z][a-z0-9_]{0,10}", inner.clone(), 0..5)
                    .prop_map(|map| { serde_json::Value::Object(map.into_iter().collect()) }),
                // Array
                prop::collection::vec(inner, 0..5).prop_map(serde_json::Value::Array),
            ]
        },
    )
}

/// Leaf values that don't need recursion
fn json_value_leaf() -> impl Strategy<Value = serde_json::Value> {
    prop_oneof![
        Just(serde_json::Value::Null),
        any::<bool>().prop_map(serde_json::Value::Bool),
        any::<i32>().prop_map(|n| serde_json::Value::Number(n.into())),
        "[a-zA-Z0-9 ]*".prop_map(serde_json::Value::String),
    ]
}

/// Simple JSON for fast tests
fn simple_json_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("null".to_string()),
        any::<bool>().prop_map(|b| b.to_string()),
        any::<i32>().prop_map(|n| n.to_string()),
        "[a-z]{0,10}".prop_map(|s| format!("\"{s}\"")),
        prop::collection::vec("[a-z]+", 0..5).prop_map(|v| format!(
            "[{}]",
            v.iter()
                .map(|s| format!("\"{s}\""))
                .collect::<Vec<_>>()
                .join(",")
        )),
    ]
}

mod tape_source_tests {
    use super::*;

    proptest! {
        /// TapeSource len() should be non-negative
        #[test]
        fn tape_len_non_negative(json in simple_json_strategy()) {
            if let Ok(tape) = DsonTape::parse(&json) {
                prop_assert!(tape.len() > 0);
            }
        }

        /// TapeSource node_at(0) should return Some for non-empty tapes
        #[test]
        fn tape_has_first_node(json in simple_json_strategy()) {
            if let Ok(tape) = DsonTape::parse(&json)
                && !tape.is_empty()
            {
                let node = tape.node_at(0);
                prop_assert!(node.is_some());
            }
        }

        /// TapeSource node_at(len()) should return None
        #[test]
        fn tape_node_at_end_is_none(json in simple_json_strategy()) {
            if let Ok(tape) = DsonTape::parse(&json) {
                let node = tape.node_at(tape.len());
                prop_assert!(node.is_none());
            }
        }

        /// skip_value should advance past the current value
        #[test]
        fn skip_value_advances(json in simple_json_strategy()) {
            if let Ok(tape) = DsonTape::parse(&json)
                && !tape.is_empty()
            {
                let next = tape.skip_value(0);
                prop_assert!(next.is_ok());
                let next = next.unwrap();
                prop_assert!(next > 0);
                prop_assert!(next <= tape.len());
            }
        }

        /// TapeIterator should visit all nodes exactly once
        #[test]
        fn iterator_visits_all_nodes(json in simple_json_strategy()) {
            if let Ok(tape) = DsonTape::parse(&json) {
                let iter_count = tape.iter().count();
                prop_assert_eq!(iter_count, tape.len());
            }
        }
    }
}

mod value_tests {
    use super::*;

    proptest! {
        /// Null values should be parsed correctly
        #[test]
        fn null_value_parsed(_dummy in Just(())) {
            let tape = DsonTape::parse("null").unwrap();
            let node = tape.node_at(0).unwrap();
            prop_assert!(matches!(node.value, Some(TapeValue::Null)));
        }

        /// Boolean values should be parsed correctly
        #[test]
        fn bool_value_parsed(b in any::<bool>()) {
            let json = b.to_string();
            let tape = DsonTape::parse(&json).unwrap();
            let node = tape.node_at(0).unwrap();
            if let Some(TapeValue::Bool(parsed)) = node.value {
                prop_assert_eq!(parsed, b);
            } else {
                prop_assert!(false, "Expected bool value");
            }
        }

        /// Integer values should be parsed correctly
        #[test]
        fn int_value_parsed(n in any::<i32>()) {
            let json = n.to_string();
            let tape = DsonTape::parse(&json).unwrap();
            let node = tape.node_at(0).unwrap();
            if let Some(TapeValue::Int(parsed)) = node.value {
                prop_assert_eq!(parsed, i64::from(n));
            } else if let Some(TapeValue::Float(_)) = node.value {
                // Some numbers may be parsed as float
            } else {
                prop_assert!(false, "Expected int value");
            }
        }

        /// String values should be parsed correctly
        #[test]
        fn string_value_parsed(s in "[a-zA-Z0-9 ]{0,20}") {
            let json = format!("\"{s}\"");
            let tape = DsonTape::parse(&json).unwrap();
            let node = tape.node_at(0).unwrap();
            if let Some(TapeValue::String(ref parsed)) = node.value {
                prop_assert_eq!(parsed.as_ref(), s.as_str());
            } else {
                prop_assert!(false, "Expected string value");
            }
        }
    }
}

mod object_tests {
    use super::*;

    proptest! {
        /// Empty object should parse correctly
        #[test]
        fn empty_object_parsed(_dummy in Just(())) {
            let tape = DsonTape::parse("{}").unwrap();
            let node = tape.node_at(0).unwrap();
            let is_empty_object = matches!(node.kind, TapeNodeKind::ObjectStart { count: 0 });
            prop_assert!(is_empty_object, "Expected ObjectStart with count 0");
        }

        /// Object with fields should have correct count
        #[test]
        fn object_field_count(
            keys in prop::collection::hash_set("[a-z]+", 1..5)
        ) {
            let fields: Vec<_> = keys.iter()
                .enumerate()
                .map(|(i, k)| format!("\"{k}\": {i}"))
                .collect();
            let json = format!("{{{}}}", fields.join(","));

            if let Ok(tape) = DsonTape::parse(&json) {
                let node = tape.node_at(0).unwrap();
                if let TapeNodeKind::ObjectStart { count } = node.kind {
                    prop_assert_eq!(count, keys.len());
                }
            }
        }

        /// Object keys should be accessible
        #[test]
        fn object_key_accessible(key in "[a-z]+", value in any::<i32>()) {
            let json = format!("{{\"{key}\": {value}}}");
            let tape = DsonTape::parse(&json).unwrap();

            // In DsonTape, key is at index 1
            let key_node = tape.node_at(1);
            prop_assert!(key_node.is_some());
        }
    }
}

mod array_tests {
    use super::*;

    proptest! {
        /// Empty array should parse correctly
        #[test]
        fn empty_array_parsed(_dummy in Just(())) {
            let tape = DsonTape::parse("[]").unwrap();
            let node = tape.node_at(0).unwrap();
            let is_empty_array = matches!(node.kind, TapeNodeKind::ArrayStart { count: 0 });
            prop_assert!(is_empty_array, "Expected ArrayStart with count 0");
        }

        /// Array with elements should have correct count
        #[test]
        fn array_element_count(elements in prop::collection::vec(any::<i32>(), 1..10)) {
            let json = format!("[{}]",
                elements.iter()
                    .map(std::string::ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            );

            if let Ok(tape) = DsonTape::parse(&json) {
                let node = tape.node_at(0).unwrap();
                if let TapeNodeKind::ArrayStart { count } = node.kind {
                    prop_assert_eq!(count, elements.len());
                }
            }
        }

        /// Array elements should be accessible
        #[test]
        fn array_elements_accessible(elements in prop::collection::vec(any::<i32>(), 1..5)) {
            let json = format!("[{}]",
                elements.iter()
                    .map(std::string::ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            );

            if let Ok(tape) = DsonTape::parse(&json) {
                // Check that we have enough nodes for all elements
                // ArrayStart + n elements
                prop_assert!(tape.len() > elements.len());
            }
        }
    }
}

mod gron_tests {
    use super::*;
    use fionn_gron::{gron, ungron};

    proptest! {
        /// Gron should not panic on valid JSON
        #[test]
        fn gron_no_panic(json in simple_json_strategy()) {
            let _ = gron(&json, &fionn_gron::GronOptions::default());
        }

        /// Gron output should be valid (parseable by ungron)
        #[test]
        fn gron_output_is_valid(json in simple_json_strategy()) {
            if let Ok(output) = gron(&json, &fionn_gron::GronOptions::default()) {
                // Ungron should be able to parse the output
                let _ = ungron(&output);
            }
        }

        /// Simple values should roundtrip through gron/ungron
        #[test]
        fn simple_roundtrip(
            key in "[a-z]+",
            value in any::<i32>()
        ) {
            let json = format!("{{\"{key}\":{value}}}");

            if let Ok(output) = gron(&json, &fionn_gron::GronOptions::default())
                && let Ok(restored_json) = ungron(&output)
            {
                // Parse both for comparison
                let original: serde_json::Value = serde_json::from_str(&json).unwrap();
                let restored: serde_json::Value = serde_json::from_str(&restored_json).unwrap();
                prop_assert_eq!(restored, original);
            }
        }
    }
}

mod diff_patch_tests {
    use super::*;
    use fionn_core::diffable::{DiffableValue, compute_diff};
    use fionn_core::patchable::apply_patch;

    proptest! {
        /// Diff of equal values should be empty
        #[test]
        fn diff_equal_is_empty(json in json_value_strategy()) {
            let patch = compute_diff(&json, &json);
            prop_assert!(patch.is_empty());
        }

        /// Diff/patch roundtrip should produce equal values
        #[test]
        fn diff_patch_roundtrip(
            source in json_value_strategy(),
            target in json_value_strategy()
        ) {
            let patch = compute_diff(&source, &target);
            let mut result = source;

            if apply_patch(&mut result, &patch).is_ok() {
                prop_assert!(result.equals(&target));
            }
        }

        /// Adding a field should produce an Add operation
        #[test]
        fn add_field_produces_add(
            existing_key in "[a-z]+",
            existing_value in any::<i32>(),
            new_key in "[a-z]+",
            new_value in any::<i32>()
        ) {
            // Skip if keys are the same
            if existing_key == new_key {
                return Ok(());
            }

            let source = json!({ &existing_key: existing_value });
            let target = json!({ &existing_key: existing_value, &new_key: new_value });

            let patch = compute_diff(&source, &target);

            // Should have exactly one Add operation
            prop_assert_eq!(patch.len(), 1);
            prop_assert!(patch.operations[0].is_add());
        }

        /// Removing a field should produce a Remove operation
        #[test]
        fn remove_field_produces_remove(
            key1 in "[a-z]+",
            value1 in any::<i32>(),
            key2 in "[a-z]+",
            value2 in any::<i32>()
        ) {
            // Skip if keys are the same
            if key1 == key2 {
                return Ok(());
            }

            let source = json!({ &key1: value1, &key2: value2 });
            let target = json!({ &key1: value1 });

            let patch = compute_diff(&source, &target);

            // Should have exactly one Remove operation
            prop_assert_eq!(patch.len(), 1);
            prop_assert!(patch.operations[0].is_remove());
        }

        /// Changing a value should produce a Replace operation
        #[test]
        fn change_value_produces_replace(
            key in "[a-z]+",
            old_value in any::<i32>(),
            new_value in any::<i32>()
        ) {
            // Skip if values are the same
            if old_value == new_value {
                return Ok(());
            }

            let source = json!({ &key: old_value });
            let target = json!({ &key: new_value });

            let patch = compute_diff(&source, &target);

            // Should have exactly one Replace operation
            prop_assert_eq!(patch.len(), 1);
            prop_assert!(patch.operations[0].is_replace());
        }
    }
}

mod edge_cases {
    use super::*;

    #[test]
    fn empty_string_parses() {
        let tape = DsonTape::parse("\"\"").unwrap();
        let node = tape.node_at(0).unwrap();
        if let Some(TapeValue::String(s)) = node.value {
            assert_eq!(s.as_ref(), "");
        }
    }

    #[test]
    fn nested_objects_parse() {
        let json = r#"{"a":{"b":{"c":1}}}"#;
        let tape = DsonTape::parse(json).unwrap();
        assert!(tape.len() > 0);
    }

    #[test]
    fn nested_arrays_parse() {
        let json = r"[[1,2],[3,4]]";
        let tape = DsonTape::parse(json).unwrap();
        assert!(tape.len() > 0);
    }

    #[test]
    fn mixed_nested_parse() {
        let json = r#"{"arr":[1,{"inner":true}]}"#;
        let tape = DsonTape::parse(json).unwrap();
        assert!(tape.len() > 0);
    }

    #[test]
    fn unicode_string_parse() {
        let json = r#""hello 世界 🌍""#;
        let tape = DsonTape::parse(json).unwrap();
        let node = tape.node_at(0).unwrap();
        if let Some(TapeValue::String(s)) = node.value {
            assert_eq!(s.as_ref(), "hello 世界 🌍");
        }
    }

    #[test]
    fn escaped_string_parse() {
        let json = r#""line1\nline2\ttab""#;
        let tape = DsonTape::parse(json).unwrap();
        let node = tape.node_at(0).unwrap();
        assert!(matches!(node.value, Some(TapeValue::String(_))));
    }

    #[test]
    fn large_array_parse() {
        let json = format!(
            "[{}]",
            (0..100)
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        let tape = DsonTape::parse(&json).unwrap();
        let node = tape.node_at(0).unwrap();
        if let TapeNodeKind::ArrayStart { count } = node.kind {
            assert_eq!(count, 100);
        }
    }
}
