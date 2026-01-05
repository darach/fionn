// SPDX-License-Identifier: MIT OR Apache-2.0
//! JSON Merge Patch implementation (RFC 7396).
//!
//! A simpler alternative to JSON Patch where:
//! - Objects are recursively merged
//! - `null` values indicate deletion
//! - Other values replace existing ones
//!
//! # Example
//!
//! ```ignore
//! use fionn::diff::{json_merge_patch, merge_patch_to_value};
//! use serde_json::json;
//!
//! let original = json!({
//!     "title": "Hello",
//!     "author": {"name": "John"},
//!     "content": "..."
//! });
//!
//! let patch = json!({
//!     "title": "New Title",
//!     "author": {"name": null},  // Delete author.name
//!     "published": true          // Add new field
//! });
//!
//! let result = json_merge_patch(&original, &patch);
//! // result = {"title": "New Title", "author": {}, "content": "...", "published": true}
//! ```

use serde_json::Value;

/// Apply a JSON Merge Patch (RFC 7396) to a document.
///
/// # Arguments
///
/// * `target` - The original JSON document
/// * `patch` - The merge patch to apply
///
/// # Returns
///
/// A new JSON document with the patch applied.
///
/// # Rules
///
/// - If `patch` is not an object, it replaces `target` entirely
/// - If `patch` is an object:
///   - For each key in `patch`:
///     - If value is `null`, remove the key from `target`
///     - If value is an object and target has an object at that key, merge recursively
///     - Otherwise, set the key to the patch value
#[must_use]
pub fn json_merge_patch(target: &Value, patch: &Value) -> Value {
    let mut result = target.clone();
    merge_patch_mut(&mut result, patch);
    result
}

/// Apply a JSON Merge Patch in place.
pub fn merge_patch_mut(target: &mut Value, patch: &Value) {
    // If patch is not an object, it replaces the target entirely
    if !patch.is_object() {
        *target = patch.clone();
        return;
    }

    // Ensure target is an object (or convert it to one)
    if !target.is_object() {
        *target = Value::Object(serde_json::Map::new());
    }

    let target_obj = target.as_object_mut().expect("target should be object");
    let patch_obj = patch.as_object().expect("patch should be object");

    for (key, patch_value) in patch_obj {
        if patch_value.is_null() {
            // Null means delete
            target_obj.remove(key);
        } else if patch_value.is_object() {
            // Recursive merge for objects
            let target_value = target_obj
                .entry(key.clone())
                .or_insert_with(|| Value::Object(serde_json::Map::new()));
            merge_patch_mut(target_value, patch_value);
        } else {
            // Replace/add for non-objects
            target_obj.insert(key.clone(), patch_value.clone());
        }
    }
}

/// Generate a merge patch from two JSON documents.
///
/// Creates a patch that, when applied to `source`, produces `target`.
///
/// # Returns
///
/// A JSON value representing the merge patch.
#[must_use]
pub fn merge_patch_to_value(source: &Value, target: &Value) -> Value {
    generate_merge_patch(source, target)
}

/// Generate a merge patch between two values.
fn generate_merge_patch(source: &Value, target: &Value) -> Value {
    match (source, target) {
        // Both objects - generate object diff
        (Value::Object(src), Value::Object(tgt)) => {
            let mut patch = serde_json::Map::new();

            // Find removed keys (set to null)
            for key in src.keys() {
                if !tgt.contains_key(key) {
                    patch.insert(key.clone(), Value::Null);
                }
            }

            // Find added and modified keys
            for (key, tgt_value) in tgt {
                match src.get(key) {
                    Some(src_value) => {
                        // Key exists in both - check if different
                        if src_value != tgt_value {
                            let nested_patch = generate_merge_patch(src_value, tgt_value);
                            // Only include if there's actual change
                            if nested_patch != Value::Object(serde_json::Map::new()) {
                                patch.insert(key.clone(), nested_patch);
                            }
                        }
                    }
                    None => {
                        // Key is new
                        patch.insert(key.clone(), tgt_value.clone());
                    }
                }
            }

            Value::Object(patch)
        }

        // Different types or source is not object - full replacement
        _ => {
            if source == target {
                Value::Object(serde_json::Map::new()) // Empty patch = no change
            } else {
                target.clone()
            }
        }
    }
}

/// Merge multiple JSON documents using merge patch semantics.
///
/// Documents are merged left to right, with later documents taking precedence.
#[must_use]
pub fn merge_many(documents: &[Value]) -> Value {
    documents
        .iter()
        .fold(Value::Object(serde_json::Map::new()), |acc, doc| {
            json_merge_patch(&acc, doc)
        })
}

/// Deep merge two objects, preferring values from `overlay`.
///
/// Unlike merge patch, this doesn't treat `null` as deletion.
#[must_use]
pub fn deep_merge(base: &Value, overlay: &Value) -> Value {
    match (base, overlay) {
        (Value::Object(base_obj), Value::Object(overlay_obj)) => {
            let mut result = base_obj.clone();

            for (key, overlay_value) in overlay_obj {
                let merged = result.get(key).map_or_else(
                    || overlay_value.clone(),
                    |base_value| deep_merge(base_value, overlay_value),
                );
                result.insert(key.clone(), merged);
            }

            Value::Object(result)
        }
        // Non-objects: overlay wins
        (_, overlay) => overlay.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_merge_patch_add_field() {
        let target = json!({"a": "b"});
        let patch = json!({"c": "d"});
        let result = json_merge_patch(&target, &patch);
        assert_eq!(result, json!({"a": "b", "c": "d"}));
    }

    #[test]
    fn test_merge_patch_replace_field() {
        let target = json!({"a": "b"});
        let patch = json!({"a": "c"});
        let result = json_merge_patch(&target, &patch);
        assert_eq!(result, json!({"a": "c"}));
    }

    #[test]
    fn test_merge_patch_delete_field() {
        let target = json!({"a": "b", "c": "d"});
        let patch = json!({"a": null});
        let result = json_merge_patch(&target, &patch);
        assert_eq!(result, json!({"c": "d"}));
    }

    #[test]
    fn test_merge_patch_nested() {
        let target = json!({
            "title": "Hello",
            "author": {
                "name": "John",
                "email": "john@example.com"
            }
        });
        let patch = json!({
            "title": "New Title",
            "author": {
                "email": null,
                "twitter": "@john"
            }
        });
        let result = json_merge_patch(&target, &patch);
        assert_eq!(
            result,
            json!({
                "title": "New Title",
                "author": {
                    "name": "John",
                    "twitter": "@john"
                }
            })
        );
    }

    #[test]
    fn test_merge_patch_replace_object_with_value() {
        let target = json!({"a": {"b": "c"}});
        let patch = json!({"a": "value"});
        let result = json_merge_patch(&target, &patch);
        assert_eq!(result, json!({"a": "value"}));
    }

    #[test]
    fn test_merge_patch_replace_value_with_object() {
        let target = json!({"a": "value"});
        let patch = json!({"a": {"b": "c"}});
        let result = json_merge_patch(&target, &patch);
        assert_eq!(result, json!({"a": {"b": "c"}}));
    }

    #[test]
    fn test_merge_patch_array_replacement() {
        // Arrays are replaced entirely, not merged
        let target = json!({"arr": [1, 2, 3]});
        let patch = json!({"arr": [4, 5]});
        let result = json_merge_patch(&target, &patch);
        assert_eq!(result, json!({"arr": [4, 5]}));
    }

    #[test]
    fn test_merge_patch_non_object_patch() {
        let target = json!({"a": "b"});
        let patch = json!("string");
        let result = json_merge_patch(&target, &patch);
        assert_eq!(result, json!("string"));
    }

    #[test]
    fn test_merge_patch_to_non_object_target() {
        let target = json!("string");
        let patch = json!({"a": "b"});
        let result = json_merge_patch(&target, &patch);
        assert_eq!(result, json!({"a": "b"}));
    }

    #[test]
    fn test_generate_merge_patch_add() {
        let source = json!({"a": "b"});
        let target = json!({"a": "b", "c": "d"});
        let patch = merge_patch_to_value(&source, &target);
        assert_eq!(patch, json!({"c": "d"}));
    }

    #[test]
    fn test_generate_merge_patch_remove() {
        let source = json!({"a": "b", "c": "d"});
        let target = json!({"a": "b"});
        let patch = merge_patch_to_value(&source, &target);
        assert_eq!(patch, json!({"c": null}));
    }

    #[test]
    fn test_generate_merge_patch_modify() {
        let source = json!({"a": "b"});
        let target = json!({"a": "c"});
        let patch = merge_patch_to_value(&source, &target);
        assert_eq!(patch, json!({"a": "c"}));
    }

    #[test]
    fn test_generate_merge_patch_nested() {
        let source = json!({"outer": {"a": 1, "b": 2}});
        let target = json!({"outer": {"a": 1, "c": 3}});
        let patch = merge_patch_to_value(&source, &target);
        assert_eq!(patch, json!({"outer": {"b": null, "c": 3}}));
    }

    #[test]
    fn test_generate_merge_patch_identical() {
        let source = json!({"a": "b", "c": [1, 2, 3]});
        let patch = merge_patch_to_value(&source, &source);
        assert_eq!(patch, json!({}));
    }

    #[test]
    fn test_roundtrip() {
        let source = json!({
            "name": "Test",
            "value": 42,
            "nested": {"a": 1, "b": 2},
            "arr": [1, 2, 3]
        });

        let target = json!({
            "name": "Modified",
            "nested": {"a": 1, "c": 3},
            "arr": [4, 5],
            "new_field": true
        });

        let patch = merge_patch_to_value(&source, &target);
        let result = json_merge_patch(&source, &patch);
        assert_eq!(result, target);
    }

    #[test]
    fn test_merge_many() {
        let docs = vec![json!({"a": 1}), json!({"b": 2}), json!({"a": 10, "c": 3})];
        let result = merge_many(&docs);
        assert_eq!(result, json!({"a": 10, "b": 2, "c": 3}));
    }

    #[test]
    fn test_merge_many_empty() {
        let docs: Vec<Value> = vec![];
        let result = merge_many(&docs);
        assert_eq!(result, json!({}));
    }

    #[test]
    fn test_deep_merge() {
        let base = json!({
            "a": 1,
            "nested": {"x": 10, "y": 20}
        });
        let overlay = json!({
            "b": 2,
            "nested": {"y": 200, "z": 30}
        });
        let result = deep_merge(&base, &overlay);
        assert_eq!(
            result,
            json!({
                "a": 1,
                "b": 2,
                "nested": {"x": 10, "y": 200, "z": 30}
            })
        );
    }

    #[test]
    fn test_deep_merge_null_preserved() {
        // Unlike merge patch, deep_merge preserves nulls
        let base = json!({"a": 1});
        let overlay = json!({"a": null});
        let result = deep_merge(&base, &overlay);
        assert_eq!(result, json!({"a": null}));
    }

    #[test]
    fn test_rfc7396_examples() {
        // Examples from RFC 7396

        // Example 1
        let target = json!({"a": "b"});
        let patch = json!({"a": "c"});
        assert_eq!(json_merge_patch(&target, &patch), json!({"a": "c"}));

        // Example 2
        let target = json!({"a": "b"});
        let patch = json!({"b": "c"});
        assert_eq!(
            json_merge_patch(&target, &patch),
            json!({"a": "b", "b": "c"})
        );

        // Example 3
        let target = json!({"a": "b"});
        let patch = json!({"a": null});
        assert_eq!(json_merge_patch(&target, &patch), json!({}));

        // Example 4
        let target = json!({"a": "b", "b": "c"});
        let patch = json!({"a": null});
        assert_eq!(json_merge_patch(&target, &patch), json!({"b": "c"}));

        // Example 5
        let target = json!({"a": ["b"]});
        let patch = json!({"a": "c"});
        assert_eq!(json_merge_patch(&target, &patch), json!({"a": "c"}));

        // Example 6
        let target = json!({"a": "c"});
        let patch = json!({"a": ["b"]});
        assert_eq!(json_merge_patch(&target, &patch), json!({"a": ["b"]}));

        // Example 7
        let target = json!({"a": {"b": "c"}});
        let patch = json!({"a": {"b": "d", "c": null}});
        assert_eq!(json_merge_patch(&target, &patch), json!({"a": {"b": "d"}}));

        // Example 8
        let target = json!({"a": [{"b": "c"}]});
        let patch = json!({"a": [1]});
        assert_eq!(json_merge_patch(&target, &patch), json!({"a": [1]}));
    }
}
