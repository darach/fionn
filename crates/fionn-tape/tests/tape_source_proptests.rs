// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for `TapeSource` implementations
//!
//! These tests verify:
//! - `TapeSource` trait contract consistency
//! - Skip operation correctness
//! - Key position detection accuracy

use fionn_core::{TapeNodeKind, TapeSource, TapeValue};
use fionn_tape::DsonTape;
use proptest::prelude::*;
use serde_json::{Value, json};

// =============================================================================
// JSON Value Generators
// =============================================================================

/// Generate arbitrary JSON values with controlled depth
fn arb_json_value(depth: usize) -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|n| json!(n)),
        any::<f64>()
            .prop_filter("finite", |f| f.is_finite())
            .prop_map(|n| json!(n)),
        "[a-zA-Z0-9_]{0,20}".prop_map(|s| json!(s)),
    ];

    leaf.prop_recursive(u32::try_from(depth).unwrap_or(u32::MAX), 64, 8, |inner| {
        prop_oneof![
            // Object with up to 5 fields
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

/// Generate JSON objects specifically (for testing key detection)
fn arb_json_object(depth: usize) -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|n| json!(n)),
        "[a-zA-Z0-9]{0,10}".prop_map(|s| json!(s)),
    ];

    proptest::collection::vec(
        (
            "[a-zA-Z_][a-zA-Z0-9_]{0,8}",
            leaf.prop_recursive(u32::try_from(depth).unwrap_or(u32::MAX), 32, 4, |inner| {
                prop_oneof![
                    proptest::collection::vec(("[a-zA-Z_][a-zA-Z0-9_]{0,8}", inner.clone()), 0..4)
                        .prop_map(|pairs| {
                            let map: serde_json::Map<String, Value> = pairs.into_iter().collect();
                            Value::Object(map)
                        }),
                    proptest::collection::vec(inner, 0..4).prop_map(Value::Array),
                ]
            }),
        ),
        1..6,
    )
    .prop_map(|pairs| {
        let map: serde_json::Map<String, Value> = pairs.into_iter().collect();
        Value::Object(map)
    })
}

// =============================================================================
// TapeSource Contract Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Property: len() == number of nodes traversable via node_at()
    #[test]
    fn prop_len_matches_traversal(value in arb_json_value(3)) {
        let json_str = serde_json::to_string(&value).unwrap();

        if let Ok(tape) = DsonTape::parse(&json_str) {
            let len = tape.len();
            let mut count = 0;
            for i in 0..len + 10 {  // Try past end
                if tape.node_at(i).is_some() {
                    count += 1;
                }
            }
            prop_assert_eq!(len, count, "len() should match traversable nodes");
        }
    }

    /// Property: format() always returns Json for DsonTape
    #[test]
    fn prop_format_is_json(value in arb_json_value(2)) {
        let json_str = serde_json::to_string(&value).unwrap();

        if let Ok(tape) = DsonTape::parse(&json_str) {
            prop_assert_eq!(tape.format(), fionn_core::FormatKind::Json);
        }
    }

    /// Property: node_at(0) is always valid for non-empty tape
    #[test]
    fn prop_root_always_valid(value in arb_json_value(2)) {
        let json_str = serde_json::to_string(&value).unwrap();

        if let Ok(tape) = DsonTape::parse(&json_str) {
            prop_assert!(tape.node_at(0).is_some(), "Root node should exist");
            // Note: value_at(0) returns None for containers (Object/Array)
            // Only scalar values have a TapeValue
            let node = tape.node_at(0).unwrap();
            if !matches!(node.kind, TapeNodeKind::ObjectStart { .. } | TapeNodeKind::ArrayStart { .. }) {
                prop_assert!(tape.value_at(0).is_some(), "Scalar root value should exist");
            }
        }
    }

    /// Property: skip_value() always advances past the value
    #[test]
    fn prop_skip_advances(value in arb_json_value(3)) {
        let json_str = serde_json::to_string(&value).unwrap();

        if let Ok(tape) = DsonTape::parse(&json_str) {
            let len = tape.len();
            if len > 0 {
                let skip_result = tape.skip_value(0);
                prop_assert!(skip_result.is_ok(), "skip_value should succeed");
                let end_idx = skip_result.unwrap();
                prop_assert!(end_idx >= 1, "skip should advance at least one");
                prop_assert!(end_idx <= len, "skip should not exceed tape length");
            }
        }
    }

    /// Property: skip_value(0) on root == len() (skips entire tape)
    #[test]
    fn prop_skip_root_equals_len(value in arb_json_value(3)) {
        let json_str = serde_json::to_string(&value).unwrap();

        if let Ok(tape) = DsonTape::parse(&json_str) {
            let len = tape.len();
            if let Ok(skip_end) = tape.skip_value(0) {
                prop_assert_eq!(skip_end, len, "skip_value(0) should equal len()");
            }
        }
    }

    /// Property: iter() yields exactly len() items
    #[test]
    fn prop_iter_count_matches_len(value in arb_json_value(3)) {
        let json_str = serde_json::to_string(&value).unwrap();

        if let Ok(tape) = DsonTape::parse(&json_str) {
            let len = tape.len();
            let iter_count = tape.iter().count();
            prop_assert_eq!(len, iter_count, "iter() should yield len() items");
        }
    }
}

// =============================================================================
// Key Position Detection Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Property: key_at() returns Some only at valid key positions
    #[test]
    fn prop_key_at_validity(obj in arb_json_object(2)) {
        let json_str = serde_json::to_string(&obj).unwrap();

        if let Ok(tape) = DsonTape::parse(&json_str) {
            for i in 0..tape.len() {
                if let Some(key) = tape.key_at(i) {
                    // If key_at returns Some, key should be a valid string reference
                    let _ = key; // Just verify we can access the key

                    // The node at this position should be a string (the key)
                    if let Some(node) = tape.node_at(i) {
                        prop_assert!(
                            matches!(node.kind, TapeNodeKind::Key | TapeNodeKind::Value),
                            "Node at key position should be Key or Value, got {:?}",
                            node.kind
                        );
                    }
                }
            }
        }
    }

    /// Property: Objects have keys that can be found
    #[test]
    fn prop_object_structure(obj in arb_json_object(1)) {
        let json_str = serde_json::to_string(&obj).unwrap();

        if let Ok(tape) = DsonTape::parse(&json_str) {
            // Count keys found
            let key_count: usize = (0..tape.len())
                .filter(|&i| tape.key_at(i).is_some())
                .count();

            // Should find at least one key if object is non-empty
            if let Some(obj_map) = obj.as_object()
                && !obj_map.is_empty()
            {
                prop_assert!(
                    key_count >= 1,
                    "Should find at least 1 key in non-empty object, found {}",
                    key_count
                );
            }
        }
    }
}

// =============================================================================
// Value Extraction Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Property: value_at() returns consistent types
    #[test]
    fn prop_value_consistency(value in arb_json_value(3)) {
        let json_str = serde_json::to_string(&value).unwrap();

        if let Ok(tape) = DsonTape::parse(&json_str) {
            for i in 0..tape.len() {
                if let Some(tape_value) = tape.value_at(i) {
                    // Value should be one of the valid types
                    let is_valid = matches!(
                        tape_value,
                        TapeValue::Null
                            | TapeValue::Bool(_)
                            | TapeValue::Int(_)
                            | TapeValue::Float(_)
                            | TapeValue::String(_)
                            | TapeValue::RawNumber(_)
                    );
                    prop_assert!(is_valid, "Value should be a valid TapeValue type");
                }
            }
        }
    }

    /// Property: Null values are correctly identified
    #[test]
    fn prop_null_detection(value in arb_json_value(2)) {
        let json_str = serde_json::to_string(&value).unwrap();

        if let Ok(tape) = DsonTape::parse(&json_str) {
            for i in 0..tape.len() {
                if let Some(tape_value) = tape.value_at(i)
                    && matches!(tape_value, TapeValue::Null)
                    && let Some(node) = tape.node_at(i)
                {
                    prop_assert!(
                        matches!(node.kind, TapeNodeKind::Value),
                        "TapeValue::Null should have TapeNodeKind::Value"
                    );
                }
            }
        }
    }

    /// Property: String values can be extracted
    #[test]
    fn prop_string_extraction(s in "[a-zA-Z0-9_ ]{0,50}") {
        let value = json!(s);
        let json_str = serde_json::to_string(&value).unwrap();

        if let Ok(tape) = DsonTape::parse(&json_str)
            && let Some(TapeValue::String(extracted)) = tape.value_at(0)
        {
            prop_assert_eq!(
                extracted.as_ref(),
                s.as_str(),
                "Extracted string should match original"
            );
        }
    }

    /// Property: Integer values roundtrip correctly
    #[test]
    fn prop_integer_roundtrip(n in any::<i64>()) {
        let value = json!(n);
        let json_str = serde_json::to_string(&value).unwrap();

        if let Ok(tape) = DsonTape::parse(&json_str)
            && let Some(TapeValue::Int(extracted)) = tape.value_at(0)
        {
            prop_assert_eq!(extracted, n, "Integer should roundtrip correctly");
        }
    }
}

// =============================================================================
// Skip Value Correctness Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Property: skip_value on each node returns valid index
    #[test]
    fn prop_skip_all_nodes_valid(value in arb_json_value(3)) {
        let json_str = serde_json::to_string(&value).unwrap();

        if let Ok(tape) = DsonTape::parse(&json_str) {
            let len = tape.len();
            for i in 0..len {
                if let Ok(skip_end) = tape.skip_value(i) {
                    prop_assert!(
                        skip_end > i,
                        "skip_value({}) should advance past {}, got {}",
                        i, i, skip_end
                    );
                    prop_assert!(
                        skip_end <= len,
                        "skip_value({}) = {} should not exceed len {}",
                        i, skip_end, len
                    );
                }
            }
        }
    }

    /// Property: Consecutive skips cover the entire tape
    #[test]
    fn prop_consecutive_skips_cover_tape(obj in arb_json_object(2)) {
        let json_str = serde_json::to_string(&obj).unwrap();

        if let Ok(tape) = DsonTape::parse(&json_str) {
            // Skip from root should reach end
            if let Ok(end) = tape.skip_value(0) {
                prop_assert_eq!(end, tape.len(), "Root skip should reach end");
            }
        }
    }
}

// =============================================================================
// Nested Structure Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Property: Deeply nested objects are handled correctly
    #[test]
    fn prop_deep_nesting(depth in 1usize..6) {
        // Create deeply nested object: {"a": {"a": {"a": ...}}}
        let mut value = json!({"leaf": "value"});
        for _ in 0..depth {
            value = json!({"nested": value});
        }

        let json_str = serde_json::to_string(&value).unwrap();

        if let Ok(tape) = DsonTape::parse(&json_str) {
            // Should have at least depth+1 object nodes
            let object_count = (0..tape.len())
                .filter(|&i| {
                    tape.node_at(i)
                        .is_some_and(|n| matches!(n.kind, TapeNodeKind::ObjectStart { .. }))
                })
                .count();

            prop_assert!(
                object_count > depth,
                "Should have at least {} objects, found {}",
                depth + 1,
                object_count
            );
        }
    }

    /// Property: Sibling objects (the is_key_position bug case)
    #[test]
    fn prop_sibling_objects(
        key1 in "[a-z]{1,5}",
        key2 in "[a-z]{1,5}",
        val1 in "[a-z]{1,5}",
        val2 in "[a-z]{1,5}"
    ) {
        // This is the exact pattern that caused the is_key_position bug
        let obj = json!({
            &*key1: {"inner": val1},
            &*key2: {"inner": val2}
        });

        let json_str = serde_json::to_string(&obj).unwrap();

        if let Ok(tape) = DsonTape::parse(&json_str) {
            // Count unique keys found
            let keys: Vec<String> = (0..tape.len())
                .filter_map(|i| tape.key_at(i).map(|k| k.to_string()))
                .collect();

            // Should find all keys: key1, "inner", key2, "inner"
            // (key1 and key2 may be same due to generation)
            prop_assert!(
                keys.len() >= 2,
                "Should find at least 2 keys, found: {:?}",
                keys
            );
        }
    }
}

// =============================================================================
// Array Structure Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Property: Array elements are traversable
    #[test]
    fn prop_array_traversal(arr in proptest::collection::vec(any::<i64>(), 0..20)) {
        let value: Value = arr.iter().map(|&n| json!(n)).collect();
        let json_str = serde_json::to_string(&value).unwrap();

        if let Ok(tape) = DsonTape::parse(&json_str) {
            // First node should be array start
            if let Some(node) = tape.node_at(0)
                && let TapeNodeKind::ArrayStart { count } = node.kind
            {
                prop_assert_eq!(
                    count,
                    arr.len(),
                    "Array count should match: expected {}, got {}",
                    arr.len(),
                    count
                );
            }
        }
    }
}

// =============================================================================
// Edge Case Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: Empty object/array handled correctly
    #[test]
    fn prop_empty_containers(use_array in any::<bool>()) {
        let value = if use_array { json!([]) } else { json!({}) };
        let json_str = serde_json::to_string(&value).unwrap();

        if let Ok(tape) = DsonTape::parse(&json_str) {
            prop_assert_eq!(tape.len(), 1, "Empty container should have 1 node");

            if let Some(node) = tape.node_at(0) {
                match node.kind {
                    TapeNodeKind::ArrayStart { count } => prop_assert_eq!(count, 0),
                    TapeNodeKind::ObjectStart { count } => prop_assert_eq!(count, 0),
                    _ => prop_assert!(false, "Expected ArrayStart or ObjectStart"),
                }
            }
        }
    }

    /// Property: Unicode strings handled correctly
    #[test]
    fn prop_unicode_strings(s in "[\\u{0020}-\\u{007E}]{0,20}") {
        let value = json!(s);
        let json_str = serde_json::to_string(&value).unwrap();

        if let Ok(tape) = DsonTape::parse(&json_str)
            && let Some(TapeValue::String(extracted)) = tape.value_at(0)
        {
            prop_assert_eq!(
                extracted.as_ref(),
                s.as_str(),
                "Unicode string should roundtrip"
            );
        }
    }
}
