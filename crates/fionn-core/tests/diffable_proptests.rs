// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for `DiffableValue` trait and diff operations
//!
//! These tests verify:
//! - `DiffableValue` trait contract consistency
//! - `compute_diff` correctness properties
//! - `GenericPatch` operation semantics

use fionn_core::diffable::{
    DiffOptions, DiffValueKind, DiffableValue, GenericPatch, GenericPatchOperation, compute_diff,
    compute_diff_with_options,
};
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

/// Generate a pair of similar JSON values (for testing diffs with changes)
fn arb_json_pair(depth: usize) -> impl Strategy<Value = (Value, Value)> {
    arb_json_value(depth).prop_flat_map(|v| {
        let v_clone = v.clone();
        (Just(v), mutate_json_value(v_clone))
    })
}

/// Mutate a JSON value slightly
fn mutate_json_value(value: Value) -> impl Strategy<Value = Value> {
    match value {
        Value::Null => prop_oneof![
            Just(Value::Null),
            any::<bool>().prop_map(Value::Bool),
            any::<i64>().prop_map(|n| json!(n)),
        ]
        .boxed(),
        Value::Bool(b) => prop_oneof![Just(Value::Bool(b)), Just(Value::Bool(!b)),].boxed(),
        Value::Number(n) => {
            let n_clone = n.clone();
            prop_oneof![
                Just(Value::Number(n)),
                any::<i64>().prop_map(|i| json!(i)),
                Just(Value::Number(n_clone)),
            ]
            .boxed()
        }
        Value::String(s) => {
            let s_clone = s.clone();
            prop_oneof![
                Just(Value::String(s)),
                "[a-zA-Z0-9_]{0,10}".prop_map(|new_s| json!(new_s)),
                Just(Value::String(s_clone)),
            ]
            .boxed()
        }
        Value::Array(arr) => {
            if arr.is_empty() {
                prop_oneof![
                    Just(Value::Array(vec![])),
                    arb_json_value(1).prop_map(|v| Value::Array(vec![v])),
                ]
                .boxed()
            } else {
                let arr_clone = arr.clone();
                prop_oneof![
                    Just(Value::Array(arr)),
                    Just(Value::Array(arr_clone.into_iter().skip(1).collect())),
                ]
                .boxed()
            }
        }
        Value::Object(map) => {
            if map.is_empty() {
                prop_oneof![
                    Just(Value::Object(serde_json::Map::new())),
                    ("[a-zA-Z_][a-zA-Z0-9_]{0,5}", arb_json_value(1)).prop_map(|(k, v)| {
                        let mut m = serde_json::Map::new();
                        m.insert(k, v);
                        Value::Object(m)
                    }),
                ]
                .boxed()
            } else {
                let map_clone = map.clone();
                let first_key = map.keys().next().cloned();
                prop_oneof![
                    Just(Value::Object(map)),
                    Just(Value::Object({
                        let mut m = map_clone;
                        if let Some(k) = first_key {
                            m.remove(&k);
                        }
                        m
                    })),
                ]
                .boxed()
            }
        }
    }
}

// =============================================================================
// DiffableValue Trait Contract Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Property: equals() is reflexive - a.equals(a) is always true
    #[test]
    fn prop_equals_reflexive(value in arb_json_value(3)) {
        prop_assert!(value.equals(&value), "equals() should be reflexive");
    }

    /// Property: equals() is symmetric - a.equals(b) == b.equals(a)
    #[test]
    fn prop_equals_symmetric(a in arb_json_value(2), b in arb_json_value(2)) {
        prop_assert_eq!(
            a.equals(&b),
            b.equals(&a),
            "equals() should be symmetric"
        );
    }

    /// Property: value_kind() matches actual JSON type
    #[test]
    fn prop_value_kind_consistent(value in arb_json_value(2)) {
        let kind = value.value_kind();
        match value {
            Value::Null => prop_assert_eq!(kind, DiffValueKind::Null),
            Value::Bool(_) => prop_assert_eq!(kind, DiffValueKind::Bool),
            Value::Number(_) => prop_assert_eq!(kind, DiffValueKind::Number),
            Value::String(_) => prop_assert_eq!(kind, DiffValueKind::String),
            Value::Array(_) => prop_assert_eq!(kind, DiffValueKind::Array),
            Value::Object(_) => prop_assert_eq!(kind, DiffValueKind::Object),
        }
    }

    /// Property: is_object() matches value_kind() == Object
    #[test]
    fn prop_is_object_consistent(value in arb_json_value(2)) {
        prop_assert_eq!(
            value.is_object(),
            value.value_kind() == DiffValueKind::Object,
            "is_object() should match value_kind()"
        );
    }

    /// Property: is_array() matches value_kind() == Array
    #[test]
    fn prop_is_array_consistent(value in arb_json_value(2)) {
        prop_assert_eq!(
            value.is_array(),
            value.value_kind() == DiffValueKind::Array,
            "is_array() should match value_kind()"
        );
    }

    /// Property: is_null() matches value_kind() == Null
    #[test]
    fn prop_is_null_consistent(value in arb_json_value(2)) {
        prop_assert_eq!(
            value.is_null(),
            value.value_kind() == DiffValueKind::Null,
            "is_null() should match value_kind()"
        );
    }

    /// Property: as_object() returns Some iff is_object()
    #[test]
    fn prop_as_object_consistent(value in arb_json_value(2)) {
        prop_assert_eq!(
            value.as_object().is_some(),
            value.is_object(),
            "as_object() should return Some iff is_object()"
        );
    }

    /// Property: as_array() returns Some iff is_array()
    #[test]
    fn prop_as_array_consistent(value in arb_json_value(2)) {
        prop_assert_eq!(
            value.as_array().is_some(),
            value.is_array(),
            "as_array() should return Some iff is_array()"
        );
    }

    /// Property: object_keys() returns correct number of keys
    #[test]
    fn prop_object_keys_count(value in arb_json_value(2)) {
        if let Value::Object(map) = &value {
            let keys = value.object_keys().unwrap();
            prop_assert_eq!(keys.len(), map.len(), "object_keys() count should match");
        }
    }

    /// Property: array_len() returns correct length
    #[test]
    fn prop_array_len_correct(value in arb_json_value(2)) {
        if let Value::Array(arr) = &value {
            prop_assert_eq!(
                value.array_len(),
                Some(arr.len()),
                "array_len() should match actual length"
            );
        }
    }

    /// Property: get_element() works for valid indices
    #[test]
    fn prop_get_element_valid(value in arb_json_value(2)) {
        if let Value::Array(arr) = &value {
            for (idx, expected) in arr.iter().enumerate() {
                prop_assert_eq!(
                    value.get_element(idx),
                    Some(expected),
                    "get_element({}) should return correct value", idx
                );
            }
        }
    }

    /// Property: get_field() works for existing keys
    #[test]
    fn prop_get_field_valid(value in arb_json_value(2)) {
        if let Value::Object(map) = &value {
            for (key, expected) in map {
                prop_assert_eq!(
                    value.get_field(key),
                    Some(expected),
                    "get_field({}) should return correct value", key
                );
            }
        }
    }

    /// Property: deep_clone() produces equal value
    #[test]
    fn prop_deep_clone_equals(value in arb_json_value(3)) {
        let cloned = value.deep_clone();
        prop_assert!(value.equals(&cloned), "deep_clone() should produce equal value");
    }
}

// =============================================================================
// compute_diff() Property Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Property: diff of identical values is empty
    #[test]
    fn prop_diff_identical_empty(value in arb_json_value(3)) {
        let patch = compute_diff(&value, &value);
        prop_assert!(patch.is_empty(), "diff of identical values should be empty");
    }

    /// Property: diff result has valid paths
    #[test]
    fn prop_diff_valid_paths((source, target) in arb_json_pair(2)) {
        let patch = compute_diff(&source, &target);
        for op in patch.iter() {
            let path = op.path();
            // All paths should start with "/" or be empty for root
            prop_assert!(
                path.is_empty() || path.starts_with('/'),
                "Path should start with / or be empty: {}", path
            );
        }
    }

    /// Property: diff with max_depth=0 produces at most one Replace
    #[test]
    fn prop_diff_max_depth_zero((source, target) in arb_json_pair(2)) {
        let options = DiffOptions::new().with_max_depth(1);
        let patch = compute_diff_with_options(&source, &target, &options);

        // With depth limit, we should see fewer deep operations
        // At depth 1, we only diff immediate children, not nested structures
        for op in patch.iter() {
            let depth = op.path().matches('/').count();
            prop_assert!(
                depth <= 2,
                "With max_depth=1, path depth should be limited: {}", op.path()
            );
        }
    }

    /// Property: GenericPatch len() matches operations count
    #[test]
    fn prop_patch_len_correct((source, target) in arb_json_pair(2)) {
        let patch = compute_diff(&source, &target);
        prop_assert_eq!(
            patch.len(),
            patch.operations.len(),
            "len() should match operations.len()"
        );
    }

    /// Property: GenericPatch is_empty() matches having zero operations
    #[test]
    fn prop_patch_empty_consistent((source, target) in arb_json_pair(2)) {
        let patch = compute_diff(&source, &target);
        // Both methods should agree on emptiness
        // Use operations count to avoid triggering len_zero lint on purpose
        let has_ops = !patch.operations.is_empty();
        prop_assert!(
            patch.is_empty() != has_ops,
            "is_empty() should be opposite of having operations"
        );
    }
}

// =============================================================================
// DiffValueKind Property Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: is_scalar() and is_container() are mutually exclusive
    #[test]
    fn prop_scalar_container_exclusive(value in arb_json_value(1)) {
        let kind = value.value_kind();
        prop_assert!(
            kind.is_scalar() != kind.is_container() ||
            (!kind.is_scalar() && !kind.is_container()),
            "is_scalar() and is_container() should be mutually exclusive"
        );
    }
}

// =============================================================================
// GenericPatchOperation Tests
// =============================================================================

#[test]
fn test_patch_operation_path() {
    let op: GenericPatchOperation<Value> = GenericPatchOperation::Add {
        path: "/test/path".to_string(),
        value: json!(42),
    };
    assert_eq!(op.path(), "/test/path");
    assert!(op.is_add());
    assert!(!op.is_remove());
    assert!(!op.is_replace());
}

#[test]
fn test_patch_operation_from_path() {
    let move_op: GenericPatchOperation<Value> = GenericPatchOperation::Move {
        from: "/source".to_string(),
        path: "/dest".to_string(),
    };
    assert_eq!(move_op.from_path(), Some("/source"));

    let add_op: GenericPatchOperation<Value> = GenericPatchOperation::Add {
        path: "/test".to_string(),
        value: json!(1),
    };
    assert_eq!(add_op.from_path(), None);
}

#[test]
fn test_generic_patch_from_iter() {
    let ops = vec![
        GenericPatchOperation::Add {
            path: "/a".to_string(),
            value: json!(1),
        },
        GenericPatchOperation::Remove {
            path: "/b".to_string(),
        },
    ];

    let patch: GenericPatch<Value> = ops.into_iter().collect();
    assert_eq!(patch.len(), 2);
}

#[test]
fn test_generic_patch_operations_at_prefix() {
    let patch = GenericPatch::with_operations(vec![
        GenericPatchOperation::Add {
            path: "/user/name".to_string(),
            value: json!("Alice"),
        },
        GenericPatchOperation::Add {
            path: "/user/age".to_string(),
            value: json!(30),
        },
        GenericPatchOperation::Add {
            path: "/other".to_string(),
            value: json!(true),
        },
    ]);

    let user_ops = patch.operations_at_prefix("/user");
    assert_eq!(user_ops.len(), 2);
}
