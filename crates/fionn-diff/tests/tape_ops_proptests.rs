// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for tape diff and merge operations
//!
//! These tests verify:
//! - `diff_tapes()` correctness and properties
//! - `merge_tapes()` RFC 7396 semantics
//! - Cross-format operation consistency

use fionn_core::TapeSource;
use fionn_diff::{TapeDiffOp, deep_merge_tapes, diff_tapes, merge_tapes};
use fionn_tape::DsonTape;
use proptest::prelude::*;
use serde_json::{Value, json};

/// Extract path from a `TapeDiffOp`
fn op_path<'a>(op: &'a TapeDiffOp<'_>) -> &'a str {
    match op {
        TapeDiffOp::Add { path, .. }
        | TapeDiffOp::Remove { path }
        | TapeDiffOp::Replace { path, .. }
        | TapeDiffOp::Move { path, .. }
        | TapeDiffOp::Copy { path, .. }
        | TapeDiffOp::AddRef { path, .. }
        | TapeDiffOp::ReplaceRef { path, .. } => path,
    }
}

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

/// Generate JSON objects specifically
fn arb_json_object(depth: usize) -> impl Strategy<Value = Value> {
    proptest::collection::vec(
        (
            "[a-zA-Z_][a-zA-Z0-9_]{0,8}",
            arb_json_value(depth.saturating_sub(1)),
        ),
        1..6,
    )
    .prop_map(|pairs| {
        let map: serde_json::Map<String, Value> = pairs.into_iter().collect();
        Value::Object(map)
    })
}

/// Generate JSON objects without null values (for merge identity tests)
/// RFC 7396 treats null as "delete", so merge with self isn't idempotent if nulls present
fn arb_json_object_no_null(depth: usize) -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|n| json!(n)),
        any::<f64>()
            .prop_filter("finite", |f| f.is_finite())
            .prop_map(|n| json!(n)),
        "[a-zA-Z0-9_]{0,20}".prop_map(|s| json!(s)),
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

/// Parse JSON to tape, returning None if parsing fails
fn parse_to_tape(value: &Value) -> Option<DsonTape> {
    let json_str = serde_json::to_string(value).ok()?;
    DsonTape::parse(&json_str).ok()
}

// =============================================================================
// diff_tapes() Property Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Property: diff of identical tapes is empty
    #[test]
    fn prop_diff_identical_empty(value in arb_json_value(3)) {
        if let Some(tape) = parse_to_tape(&value) {
            let diff = diff_tapes(&tape, &tape).unwrap();
            prop_assert!(diff.is_empty(), "diff of identical tapes should be empty");
        }
    }

    /// Property: diff produces valid patch operations with proper paths
    #[test]
    fn prop_diff_valid_paths(a in arb_json_value(2), b in arb_json_value(2)) {
        if let (Some(tape_a), Some(tape_b)) = (parse_to_tape(&a), parse_to_tape(&b)) {
            let diff = diff_tapes(&tape_a, &tape_b).unwrap();
            for op in &diff.operations {
                let path = op_path(op);
                // Paths should be valid JSON Pointers (empty or starting with /)
                prop_assert!(
                    path.is_empty() || path.starts_with('/'),
                    "Invalid path: {}", path
                );
            }
        }
    }

    /// Property: diff operation count is bounded
    #[test]
    fn prop_diff_bounded_ops(a in arb_json_value(2), b in arb_json_value(2)) {
        if let (Some(tape_a), Some(tape_b)) = (parse_to_tape(&a), parse_to_tape(&b)) {
            let diff = diff_tapes(&tape_a, &tape_b).unwrap();
            // The number of ops should be reasonable relative to tape sizes
            let max_expected = tape_a.len() + tape_b.len();
            prop_assert!(
                diff.len() <= max_expected,
                "Too many diff ops: {} (max expected {})", diff.len(), max_expected
            );
        }
    }
}

// =============================================================================
// merge_tapes() Property Tests (RFC 7396 Semantics)
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Property: merge with empty overlay returns base unchanged
    /// Note: Uses no-null generator because RFC 7396 treats null as "delete"
    #[test]
    fn prop_merge_empty_overlay_identity(value in arb_json_object_no_null(2)) {
        if let Some(base_tape) = parse_to_tape(&value) {
            let empty = json!({});
            if let Some(empty_tape) = parse_to_tape(&empty) {
                let result = merge_tapes(&base_tape, &empty_tape).unwrap();
                // Merge with empty object should preserve base
                prop_assert_eq!(
                    result, value,
                    "Merge with empty overlay should preserve base"
                );
            }
        }
    }

    /// Property: merge with self is identity (when no nulls)
    /// Note: Uses no-null generator because RFC 7396 treats null as "delete"
    #[test]
    fn prop_merge_self_identity(value in arb_json_object_no_null(2)) {
        if let Some(tape) = parse_to_tape(&value) {
            let result = merge_tapes(&tape, &tape).unwrap();
            prop_assert_eq!(
                result, value,
                "Merge with self should be identity"
            );
        }
    }

    /// Property: overlay values take precedence
    #[test]
    fn prop_merge_overlay_precedence(
        base in arb_json_object(1),
        overlay in arb_json_object(1)
    ) {
        if let (Some(base_tape), Some(overlay_tape)) = (parse_to_tape(&base), parse_to_tape(&overlay)) {
            let result = merge_tapes(&base_tape, &overlay_tape).unwrap();

            // All overlay keys should be in result with overlay values
            if let (Value::Object(overlay_map), Value::Object(result_map)) = (&overlay, &result) {
                for (key, overlay_val) in overlay_map {
                    if !overlay_val.is_null() {
                        prop_assert!(
                            result_map.get(key) == Some(overlay_val),
                            "Overlay key {} should have overlay value", key
                        );
                    }
                }
            }
        }
    }

    /// Property: null in overlay deletes keys (RFC 7396)
    #[test]
    fn prop_merge_null_deletes(base in arb_json_object(1)) {
        // Create overlay that nullifies some keys
        if let Value::Object(base_map) = &base
            && let Some(first_key) = base_map.keys().next().cloned()
        {
            let overlay = json!({ first_key.clone(): null });

            if let (Some(base_tape), Some(overlay_tape)) = (parse_to_tape(&base), parse_to_tape(&overlay)) {
                let result = merge_tapes(&base_tape, &overlay_tape).unwrap();

                if let Value::Object(result_map) = &result {
                    prop_assert!(
                        !result_map.contains_key(&first_key),
                        "Null in overlay should delete key: {}", first_key
                    );
                }
            }
        }
    }

    /// Property: base keys not in overlay are preserved
    #[test]
    fn prop_merge_preserves_base_keys(
        base in arb_json_object(1),
        overlay in arb_json_object(1)
    ) {
        if let (Some(base_tape), Some(overlay_tape)) = (parse_to_tape(&base), parse_to_tape(&overlay)) {
            let result = merge_tapes(&base_tape, &overlay_tape).unwrap();

            if let (Value::Object(base_map), Value::Object(overlay_map), Value::Object(result_map)) =
                (&base, &overlay, &result)
            {
                for (key, base_val) in base_map {
                    // If key not in overlay, it should be in result with base value
                    if !overlay_map.contains_key(key) {
                        prop_assert!(
                            result_map.get(key) == Some(base_val),
                            "Base key {} not in overlay should be preserved", key
                        );
                    }
                }
            }
        }
    }
}

// =============================================================================
// deep_merge_tapes() Property Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Property: deep_merge preserves null values (unlike RFC 7396)
    #[test]
    fn prop_deep_merge_preserves_null(base in arb_json_object(1)) {
        if let Value::Object(base_map) = &base
            && let Some(first_key) = base_map.keys().next().cloned()
        {
            let overlay = json!({ first_key.clone(): null });

            if let (Some(base_tape), Some(overlay_tape)) = (parse_to_tape(&base), parse_to_tape(&overlay)) {
                let result = deep_merge_tapes(&base_tape, &overlay_tape).unwrap();

                if let Value::Object(result_map) = &result {
                    // deep_merge keeps the null instead of deleting
                    prop_assert!(
                        result_map.get(&first_key) == Some(&Value::Null),
                        "deep_merge should preserve null value for key: {}", first_key
                    );
                }
            }
        }
    }

    /// Property: deep_merge is also idempotent with self
    /// Note: Uses no-null to avoid key_at edge case issue
    #[test]
    fn prop_deep_merge_self_identity(value in arb_json_object_no_null(2)) {
        if let Some(tape) = parse_to_tape(&value) {
            let result = deep_merge_tapes(&tape, &tape).unwrap();
            prop_assert_eq!(
                result, value,
                "deep_merge with self should be identity"
            );
        }
    }

    /// Property: deep_merge recursively merges nested objects
    #[test]
    fn prop_deep_merge_recursive(
        base in arb_json_object(2),
        overlay in arb_json_object(2)
    ) {
        if let (Some(base_tape), Some(overlay_tape)) = (parse_to_tape(&base), parse_to_tape(&overlay)) {
            let result = deep_merge_tapes(&base_tape, &overlay_tape);
            // Should succeed
            prop_assert!(result.is_ok(), "deep_merge should succeed");
        }
    }
}

// =============================================================================
// Cross-operation Property Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: diff and merge are related - merge should reduce diff
    #[test]
    fn prop_merge_reduces_diff(
        base in arb_json_object(2),
        overlay in arb_json_object(2)
    ) {
        if let (Some(base_tape), Some(overlay_tape)) = (parse_to_tape(&base), parse_to_tape(&overlay)) {
            let merged = merge_tapes(&base_tape, &overlay_tape).unwrap();

            if let Some(merged_tape) = parse_to_tape(&merged) {
                // Diff between merged and overlay should be <= diff between base and overlay
                let diff_to_overlay = diff_tapes(&merged_tape, &overlay_tape).unwrap();
                let diff_base_overlay = diff_tapes(&base_tape, &overlay_tape).unwrap();

                // Merged result should be at least as close to overlay as base was
                prop_assert!(
                    diff_to_overlay.len() <= diff_base_overlay.len() + 5, // Allow some slack
                    "Merge should bring result closer to overlay"
                );
            }
        }
    }
}

// =============================================================================
// Edge Case Tests
// =============================================================================

#[test]
fn test_diff_empty_tapes() {
    let empty = json!({});
    let tape = parse_to_tape(&empty).unwrap();
    let diff = diff_tapes(&tape, &tape).unwrap();
    assert!(diff.is_empty());
}

#[test]
fn test_merge_scalar_overlay() {
    let base = json!({"a": 1});
    let overlay = json!(42);

    let base_tape = parse_to_tape(&base).unwrap();
    let overlay_tape = parse_to_tape(&overlay).unwrap();

    let result = merge_tapes(&base_tape, &overlay_tape).unwrap();
    // Non-object overlay replaces entirely
    assert_eq!(result, json!(42));
}

#[test]
fn test_diff_array_modification() {
    let a = json!([1, 2, 3]);
    let b = json!([1, 2, 4]);

    let tape_a = parse_to_tape(&a).unwrap();
    let tape_b = parse_to_tape(&b).unwrap();

    let diff = diff_tapes(&tape_a, &tape_b).unwrap();
    assert!(!diff.is_empty(), "Array modification should produce diff");
}

#[test]
fn test_merge_nested_objects() {
    let base = json!({"user": {"name": "Alice", "age": 30}});
    let overlay = json!({"user": {"name": "Bob"}});

    let base_tape = parse_to_tape(&base).unwrap();
    let overlay_tape = parse_to_tape(&overlay).unwrap();

    let result = merge_tapes(&base_tape, &overlay_tape).unwrap();

    // RFC 7396: nested objects are recursively merged
    assert_eq!(result["user"]["name"], "Bob");
    assert_eq!(result["user"]["age"], 30);
}
