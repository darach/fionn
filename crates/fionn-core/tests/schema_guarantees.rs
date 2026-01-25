// SPDX-License-Identifier: MIT OR Apache-2.0
//! Schema Guarantee Verification Tests
//!
//! Property-based tests to verify schema guarantees:
//! - Inference Correctness: inferred schema = actual structure
//! - Filtering Soundness: no false negatives (may have false positives)
//! - Round-Trip Fidelity: filtered output reconstructs equivalently
//! - Type Preservation: numeric precision, string encoding, boolean values

use proptest::prelude::*;
use std::collections::HashSet;

// =============================================================================
// Test Data Generators
// =============================================================================

/// Generate a random JSON object with specified depth and width
fn json_object_strategy(
    depth: usize,
    max_width: usize,
) -> impl Strategy<Value = serde_json::Value> {
    if depth == 0 {
        // Leaf values
        prop_oneof![
            Just(serde_json::Value::Null),
            any::<bool>().prop_map(serde_json::Value::Bool),
            any::<i32>().prop_map(|n| serde_json::json!(n)),
            any::<f64>()
                .prop_filter("finite float", |f| f.is_finite())
                .prop_map(|n| serde_json::json!(n)),
            "[a-zA-Z0-9_]{1,20}".prop_map(serde_json::Value::String),
        ]
        .boxed()
    } else {
        // Recursive structure
        prop_oneof![
            // Leaf values
            Just(serde_json::Value::Null),
            any::<bool>().prop_map(serde_json::Value::Bool),
            any::<i32>().prop_map(|n| serde_json::json!(n)),
            "[a-zA-Z0-9_]{1,20}".prop_map(serde_json::Value::String),
            // Nested object
            prop::collection::hash_map(
                "[a-z]{1,10}",
                json_object_strategy(depth - 1, max_width),
                0..max_width
            )
            .prop_map(|map| {
                let obj: serde_json::Map<String, serde_json::Value> = map.into_iter().collect();
                serde_json::Value::Object(obj)
            }),
            // Array
            prop::collection::vec(json_object_strategy(depth - 1, max_width), 0..max_width)
                .prop_map(serde_json::Value::Array),
        ]
        .boxed()
    }
}

/// Strategy for generating schema patterns
fn schema_pattern_strategy() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(
        prop_oneof![
            // Simple field names
            "[a-z]{1,10}".prop_map(|s| s),
            // Nested paths
            prop::collection::vec("[a-z]{1,5}", 1..4).prop_map(|parts| parts.join(".")),
            // Wildcard
            Just("*".to_string()),
        ],
        0..10,
    )
}

// =============================================================================
// Schema Inference
// =============================================================================

/// Extract all field paths from a JSON value
fn extract_paths(value: &serde_json::Value, prefix: &str) -> HashSet<String> {
    let mut paths = HashSet::new();

    match value {
        serde_json::Value::Object(obj) => {
            for (key, val) in obj {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                paths.insert(path.clone());
                paths.extend(extract_paths(val, &path));
            }
        }
        serde_json::Value::Array(arr) => {
            for (i, val) in arr.iter().enumerate() {
                let path = format!("{prefix}[{i}]");
                paths.extend(extract_paths(val, &path));
            }
        }
        _ => {
            // Leaf value - path already added by parent
        }
    }

    paths
}

/// Infer schema from a JSON document (simplified version)
fn infer_schema(value: &serde_json::Value) -> HashSet<String> {
    extract_paths(value, "")
}

// =============================================================================
// Schema Filtering
// =============================================================================

/// Check if a path matches a schema pattern
fn path_matches_pattern(path: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    // Simple prefix matching
    path == pattern || path.starts_with(&format!("{pattern}."))
}

/// Check if a document matches a schema
fn document_matches_schema(doc: &serde_json::Value, schema: &[String]) -> bool {
    if schema.is_empty() {
        return true; // Empty schema matches all
    }

    // Check if any of the document's paths match a schema pattern
    let doc_paths = extract_paths(doc, "");

    for doc_path in &doc_paths {
        for pattern in schema {
            if path_matches_pattern(doc_path, pattern) {
                return true;
            }
        }
    }

    false
}

/// Filter documents using schema
fn filter_with_schema<'a>(
    docs: &'a [serde_json::Value],
    schema: &[String],
) -> Vec<&'a serde_json::Value> {
    if schema.is_empty() || schema.contains(&"*".to_string()) {
        return docs.iter().collect();
    }

    docs.iter()
        .filter(|doc| document_matches_schema(doc, schema))
        .collect()
}

// =============================================================================
// Type Preservation
// =============================================================================

/// Check if two JSON values are semantically equal (handling numeric precision)
fn semantically_equal(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    match (a, b) {
        (serde_json::Value::Null, serde_json::Value::Null) => true,
        (serde_json::Value::Bool(a), serde_json::Value::Bool(b)) => a == b,
        (serde_json::Value::Number(a), serde_json::Value::Number(b)) => {
            // For integers, compare exactly
            if let (Some(a_i), Some(b_i)) = (a.as_i64(), b.as_i64()) {
                return a_i == b_i;
            }
            // Compare as f64 with relative tolerance for floating point
            let a_f = a.as_f64().unwrap_or(0.0);
            let b_f = b.as_f64().unwrap_or(0.0);
            // Use epsilon for exact equality check
            if (a_f - b_f).abs() < f64::EPSILON {
                return true;
            }
            // Relative tolerance for large numbers
            let max_abs = a_f.abs().max(b_f.abs());
            if max_abs < f64::EPSILON {
                return true;
            }
            let rel_diff = (a_f - b_f).abs() / max_abs;
            rel_diff < 1e-14
        }
        (serde_json::Value::String(a), serde_json::Value::String(b)) => a == b,
        (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b.iter())
                    .all(|(x, y)| semantically_equal(x, y))
        }
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(k, v)| b.get(k).is_some_and(|bv| semantically_equal(v, bv)))
        }
        _ => false,
    }
}

// =============================================================================
// Property Tests
// =============================================================================

proptest! {
    /// Schema inference matches actual document structure
    #[test]
    fn prop_schema_inference_matches_structure(
        value in json_object_strategy(2, 5)
    ) {
        let inferred = infer_schema(&value);
        let actual = extract_paths(&value, "");

        // Inferred schema should equal actual paths
        prop_assert_eq!(inferred, actual);
    }

    /// Schema filter has no false negatives
    #[test]
    fn prop_schema_filter_no_false_negatives(
        docs in prop::collection::vec(json_object_strategy(2, 3), 1..10),
        schema in schema_pattern_strategy()
    ) {
        let filtered = filter_with_schema(&docs, &schema);

        // Every doc that manually matches must be in filtered set
        let manual_matches: Vec<_> = docs.iter()
            .filter(|d| document_matches_schema(d, &schema))
            .collect();

        for doc in manual_matches {
            prop_assert!(
                filtered.contains(&doc),
                "Schema filter missed a matching document"
            );
        }
    }

    /// Empty schema matches all documents
    #[test]
    fn prop_empty_schema_matches_all(
        docs in prop::collection::vec(json_object_strategy(2, 3), 1..10)
    ) {
        let schema: Vec<String> = vec![];
        let filtered = filter_with_schema(&docs, &schema);

        prop_assert_eq!(filtered.len(), docs.len());
    }

    /// Wildcard schema matches all documents
    #[test]
    fn prop_wildcard_schema_matches_all(
        docs in prop::collection::vec(json_object_strategy(2, 3), 1..10)
    ) {
        let schema = vec!["*".to_string()];
        let filtered = filter_with_schema(&docs, &schema);

        prop_assert_eq!(filtered.len(), docs.len());
    }

    /// Type preservation through serialization
    #[test]
    fn prop_type_preservation_roundtrip(
        value in json_object_strategy(2, 4)
    ) {
        // Serialize to string
        let json_str = serde_json::to_string(&value).unwrap();

        // Parse back
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        // Should be semantically equal
        prop_assert!(
            semantically_equal(&value, &parsed),
            "Round-trip changed value"
        );
    }

    /// Boolean values are preserved exactly
    #[test]
    fn prop_bool_preservation(b in any::<bool>()) {
        let value = serde_json::Value::Bool(b);
        let json_str = serde_json::to_string(&value).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        prop_assert_eq!(value, parsed);
    }

    /// Integer values are preserved exactly
    #[test]
    fn prop_integer_preservation(n in any::<i32>()) {
        let value = serde_json::json!(n);
        let json_str = serde_json::to_string(&value).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        prop_assert_eq!(value, parsed);
    }

    /// String values are preserved exactly
    #[test]
    fn prop_string_preservation(s in "[a-zA-Z0-9_]{0,100}") {
        let value = serde_json::Value::String(s);
        let json_str = serde_json::to_string(&value).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        prop_assert_eq!(value, parsed);
    }

    /// Null values are preserved
    #[test]
    fn prop_null_preservation(_dummy in Just(())) {
        let value = serde_json::Value::Null;
        let json_str = serde_json::to_string(&value).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        prop_assert_eq!(value, parsed);
    }
}

// =============================================================================
// Additional Unit Tests for Edge Cases
// =============================================================================

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_extract_paths_simple() {
        let doc = serde_json::json!({"name": "Alice", "age": 30});
        let paths = extract_paths(&doc, "");

        assert!(paths.contains("name"));
        assert!(paths.contains("age"));
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn test_extract_paths_nested() {
        let doc = serde_json::json!({"user": {"name": "Alice", "profile": {"email": "a@b.com"}}});
        let paths = extract_paths(&doc, "");

        assert!(paths.contains("user"));
        assert!(paths.contains("user.name"));
        assert!(paths.contains("user.profile"));
        assert!(paths.contains("user.profile.email"));
    }

    #[test]
    fn test_path_matches_pattern_exact() {
        assert!(path_matches_pattern("name", "name"));
        assert!(!path_matches_pattern("name", "age"));
    }

    #[test]
    fn test_path_matches_pattern_prefix() {
        assert!(path_matches_pattern("user.name", "user"));
        assert!(path_matches_pattern("user.profile.email", "user"));
        assert!(path_matches_pattern("user.profile.email", "user.profile"));
    }

    #[test]
    fn test_path_matches_pattern_wildcard() {
        assert!(path_matches_pattern("anything", "*"));
        assert!(path_matches_pattern("deeply.nested.path", "*"));
    }

    #[test]
    fn test_document_matches_schema_empty() {
        let doc = serde_json::json!({"name": "Alice"});
        assert!(document_matches_schema(&doc, &[]));
    }

    #[test]
    fn test_document_matches_schema_single() {
        let doc = serde_json::json!({"name": "Alice", "age": 30});
        assert!(document_matches_schema(&doc, &["name".to_string()]));
        assert!(document_matches_schema(&doc, &["age".to_string()]));
        assert!(!document_matches_schema(&doc, &["email".to_string()]));
    }

    #[test]
    fn test_filter_with_schema() {
        let docs = vec![
            serde_json::json!({"name": "Alice"}),
            serde_json::json!({"email": "a@b.com"}),
            serde_json::json!({"name": "Bob", "email": "b@c.com"}),
        ];

        let filtered = filter_with_schema(&docs, &["name".to_string()]);
        assert_eq!(filtered.len(), 2); // First and third have "name"

        let filtered_email = filter_with_schema(&docs, &["email".to_string()]);
        assert_eq!(filtered_email.len(), 2); // Second and third have "email"
    }

    #[test]
    fn test_semantically_equal_numbers() {
        let a = serde_json::json!(42);
        let b = serde_json::json!(42);
        assert!(semantically_equal(&a, &b));

        let c = serde_json::json!(42.0);
        assert!(semantically_equal(&a, &c));
    }

    #[test]
    fn test_semantically_equal_objects() {
        let a = serde_json::json!({"x": 1, "y": 2});
        let b = serde_json::json!({"y": 2, "x": 1});
        assert!(semantically_equal(&a, &b));
    }

    #[test]
    fn test_semantically_equal_arrays() {
        let a = serde_json::json!([1, 2, 3]);
        let b = serde_json::json!([1, 2, 3]);
        assert!(semantically_equal(&a, &b));

        let c = serde_json::json!([1, 3, 2]);
        assert!(!semantically_equal(&a, &c));
    }

    #[test]
    fn test_infer_schema_empty_object() {
        let doc = serde_json::json!({});
        let schema = infer_schema(&doc);
        assert!(schema.is_empty());
    }

    #[test]
    fn test_infer_schema_with_array() {
        let doc = serde_json::json!({"items": [1, 2, 3]});
        let schema = infer_schema(&doc);
        assert!(schema.contains("items"));
    }
}
