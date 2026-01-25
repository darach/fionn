// SPDX-License-Identifier: MIT OR Apache-2.0
//! Zero-copy diff output using Cow.
//!
//! This module provides diff output that minimizes allocations by borrowing
//! paths when possible and only allocating for computed paths or values.

use serde_json::Value;
use std::borrow::Cow;

/// A single patch operation with zero-copy paths where possible.
#[derive(Debug, Clone)]
pub enum PatchOperationRef<'a> {
    /// Add a value at the specified path.
    Add {
        /// JSON Pointer path (RFC 6901).
        path: Cow<'a, str>,
        /// Value to add.
        value: Cow<'a, Value>,
    },
    /// Remove the value at the specified path.
    Remove {
        /// JSON Pointer path.
        path: Cow<'a, str>,
    },
    /// Replace the value at the specified path.
    Replace {
        /// JSON Pointer path.
        path: Cow<'a, str>,
        /// New value.
        value: Cow<'a, Value>,
    },
    /// Move a value from one path to another.
    Move {
        /// Source path.
        from: Cow<'a, str>,
        /// Destination path.
        path: Cow<'a, str>,
    },
    /// Copy a value from one path to another.
    Copy {
        /// Source path.
        from: Cow<'a, str>,
        /// Destination path.
        path: Cow<'a, str>,
    },
    /// Test that a value equals the expected value.
    Test {
        /// JSON Pointer path.
        path: Cow<'a, str>,
        /// Expected value.
        value: Cow<'a, Value>,
    },
}

impl<'a> PatchOperationRef<'a> {
    /// Create an Add operation with owned path and borrowed value.
    #[must_use]
    pub const fn add(path: String, value: &'a Value) -> Self {
        Self::Add {
            path: Cow::Owned(path),
            value: Cow::Borrowed(value),
        }
    }

    /// Create a Remove operation with owned path.
    #[must_use]
    pub const fn remove(path: String) -> Self {
        Self::Remove {
            path: Cow::Owned(path),
        }
    }

    /// Create a Replace operation with owned path and borrowed value.
    #[must_use]
    pub const fn replace(path: String, value: &'a Value) -> Self {
        Self::Replace {
            path: Cow::Owned(path),
            value: Cow::Borrowed(value),
        }
    }

    /// Create a Move operation with owned paths.
    #[must_use]
    pub const fn move_op(from: String, path: String) -> Self {
        Self::Move {
            from: Cow::Owned(from),
            path: Cow::Owned(path),
        }
    }

    /// Create a Copy operation with owned paths.
    #[must_use]
    pub const fn copy(from: String, path: String) -> Self {
        Self::Copy {
            from: Cow::Owned(from),
            path: Cow::Owned(path),
        }
    }

    /// Create a Test operation with owned path and borrowed value.
    #[must_use]
    pub const fn test(path: String, value: &'a Value) -> Self {
        Self::Test {
            path: Cow::Owned(path),
            value: Cow::Borrowed(value),
        }
    }

    /// Get the operation type as a string.
    #[must_use]
    pub const fn op_type(&self) -> &'static str {
        match self {
            Self::Add { .. } => "add",
            Self::Remove { .. } => "remove",
            Self::Replace { .. } => "replace",
            Self::Move { .. } => "move",
            Self::Copy { .. } => "copy",
            Self::Test { .. } => "test",
        }
    }

    /// Get the primary path of this operation.
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::Add { path, .. }
            | Self::Remove { path }
            | Self::Replace { path, .. }
            | Self::Move { path, .. }
            | Self::Copy { path, .. }
            | Self::Test { path, .. } => path,
        }
    }

    /// Convert to owned version (for storing beyond borrow lifetime).
    #[must_use]
    pub fn into_owned(self) -> PatchOperationRef<'static> {
        match self {
            Self::Add { path, value } => PatchOperationRef::Add {
                path: Cow::Owned(path.into_owned()),
                value: Cow::Owned(value.into_owned()),
            },
            Self::Remove { path } => PatchOperationRef::Remove {
                path: Cow::Owned(path.into_owned()),
            },
            Self::Replace { path, value } => PatchOperationRef::Replace {
                path: Cow::Owned(path.into_owned()),
                value: Cow::Owned(value.into_owned()),
            },
            Self::Move { from, path } => PatchOperationRef::Move {
                from: Cow::Owned(from.into_owned()),
                path: Cow::Owned(path.into_owned()),
            },
            Self::Copy { from, path } => PatchOperationRef::Copy {
                from: Cow::Owned(from.into_owned()),
                path: Cow::Owned(path.into_owned()),
            },
            Self::Test { path, value } => PatchOperationRef::Test {
                path: Cow::Owned(path.into_owned()),
                value: Cow::Owned(value.into_owned()),
            },
        }
    }
}

/// A JSON Patch document with zero-copy operations.
#[derive(Debug, Default, Clone)]
pub struct JsonPatchRef<'a> {
    /// The operations in this patch.
    pub operations: Vec<PatchOperationRef<'a>>,
}

impl<'a> JsonPatchRef<'a> {
    /// Create a new empty patch.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            operations: Vec::new(),
        }
    }

    /// Create with pre-allocated capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            operations: Vec::with_capacity(capacity),
        }
    }

    /// Add an operation to the patch.
    pub fn push(&mut self, op: PatchOperationRef<'a>) {
        self.operations.push(op);
    }

    /// Get the number of operations.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.operations.len()
    }

    /// Check if patch is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    /// Iterate over operations.
    pub fn iter(&self) -> impl Iterator<Item = &PatchOperationRef<'a>> {
        self.operations.iter()
    }

    /// Convert to owned version.
    #[must_use]
    pub fn into_owned(self) -> JsonPatchRef<'static> {
        JsonPatchRef {
            operations: self
                .operations
                .into_iter()
                .map(PatchOperationRef::into_owned)
                .collect(),
        }
    }

    /// Convert to the standard (allocating) `JsonPatch` format.
    #[must_use]
    pub fn to_json_patch(&self) -> super::patch::JsonPatch {
        use super::patch::PatchOperation;

        let ops: Vec<PatchOperation> = self
            .operations
            .iter()
            .map(|op| match op {
                PatchOperationRef::Add { path, value } => PatchOperation::Add {
                    path: path.to_string(),
                    value: value.as_ref().clone(),
                },
                PatchOperationRef::Remove { path } => PatchOperation::Remove {
                    path: path.to_string(),
                },
                PatchOperationRef::Replace { path, value } => PatchOperation::Replace {
                    path: path.to_string(),
                    value: value.as_ref().clone(),
                },
                PatchOperationRef::Move { from, path } => PatchOperation::Move {
                    from: from.to_string(),
                    path: path.to_string(),
                },
                PatchOperationRef::Copy { from, path } => PatchOperation::Copy {
                    from: from.to_string(),
                    path: path.to_string(),
                },
                PatchOperationRef::Test { path, value } => PatchOperation::Test {
                    path: path.to_string(),
                    value: value.as_ref().clone(),
                },
            })
            .collect();

        super::patch::JsonPatch { operations: ops }
    }
}

impl<'a> IntoIterator for JsonPatchRef<'a> {
    type Item = PatchOperationRef<'a>;
    type IntoIter = std::vec::IntoIter<PatchOperationRef<'a>>;

    fn into_iter(self) -> Self::IntoIter {
        self.operations.into_iter()
    }
}

impl<'a, 'b> IntoIterator for &'b JsonPatchRef<'a> {
    type Item = &'b PatchOperationRef<'a>;
    type IntoIter = std::slice::Iter<'b, PatchOperationRef<'a>>;

    fn into_iter(self) -> Self::IntoIter {
        self.operations.iter()
    }
}

/// Generate a zero-copy JSON Patch that transforms `source` into `target`.
///
/// # Arguments
///
/// * `source` - The original JSON document
/// * `target` - The desired JSON document
///
/// # Returns
///
/// A `JsonPatchRef` containing operations to transform source into target.
/// The operations borrow values from `target` where possible.
#[must_use]
pub fn json_diff_zerocopy<'a>(source: &Value, target: &'a Value) -> JsonPatchRef<'a> {
    let mut patch = JsonPatchRef::with_capacity(8);
    diff_values_zerocopy(source, target, String::new(), &mut patch);
    patch
}

/// Recursively diff two values with zero-copy output.
#[allow(clippy::similar_names)] // src_obj/tgt_obj naming follows source/target convention
fn diff_values_zerocopy<'a>(
    source: &Value,
    target: &'a Value,
    path: String,
    patch: &mut JsonPatchRef<'a>,
) {
    // Fast path: identical values
    if source == target {
        return;
    }

    match (source, target) {
        // Both objects - compare fields
        (Value::Object(src_map), Value::Object(tgt_map)) => {
            // Find removed keys
            for key in src_map.keys() {
                if !tgt_map.contains_key(key) {
                    let key_path = if path.is_empty() {
                        format!("/{}", escape_json_pointer(key))
                    } else {
                        format!("{}/{}", path, escape_json_pointer(key))
                    };
                    patch.push(PatchOperationRef::remove(key_path));
                }
            }

            // Find added/changed keys
            for (key, tgt_val) in tgt_map {
                let key_path = if path.is_empty() {
                    format!("/{}", escape_json_pointer(key))
                } else {
                    format!("{}/{}", path, escape_json_pointer(key))
                };

                match src_map.get(key) {
                    Some(src_val) => {
                        // Key exists - recurse
                        diff_values_zerocopy(src_val, tgt_val, key_path, patch);
                    }
                    None => {
                        // Key added
                        patch.push(PatchOperationRef::add(key_path, tgt_val));
                    }
                }
            }
        }

        // Both arrays - compare elements
        (Value::Array(src_arr), Value::Array(tgt_arr)) => {
            let src_len = src_arr.len();
            let tgt_len = tgt_arr.len();

            // Compare common prefix
            let min_len = src_len.min(tgt_len);
            for i in 0..min_len {
                let idx_path = if path.is_empty() {
                    format!("/{i}")
                } else {
                    format!("{path}/{i}")
                };
                diff_values_zerocopy(&src_arr[i], &tgt_arr[i], idx_path, patch);
            }

            // Handle length differences
            if tgt_len > src_len {
                // Add new elements
                for (i, item) in tgt_arr.iter().enumerate().take(tgt_len).skip(src_len) {
                    let idx_path = if path.is_empty() {
                        format!("/{i}")
                    } else {
                        format!("{path}/{i}")
                    };
                    patch.push(PatchOperationRef::add(idx_path, item));
                }
            } else if src_len > tgt_len {
                // Remove extra elements (in reverse order for valid indexing)
                for i in (tgt_len..src_len).rev() {
                    let idx_path = if path.is_empty() {
                        format!("/{i}")
                    } else {
                        format!("{path}/{i}")
                    };
                    patch.push(PatchOperationRef::remove(idx_path));
                }
            }
        }

        // Different types or different values - replace
        _ => {
            if path.is_empty() {
                // Root replacement - use empty path per RFC 6902
                patch.push(PatchOperationRef::replace(String::new(), target));
            } else {
                patch.push(PatchOperationRef::replace(path, target));
            }
        }
    }
}

/// Escape a string for use in a JSON Pointer (RFC 6901).
fn escape_json_pointer(s: &str) -> Cow<'_, str> {
    if s.contains('~') || s.contains('/') {
        let escaped = s.replace('~', "~0").replace('/', "~1");
        Cow::Owned(escaped)
    } else {
        Cow::Borrowed(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // =========================================================================
    // PatchOperationRef Constructor Tests
    // =========================================================================

    #[test]
    fn test_patch_operation_add() {
        let value = json!(42);
        let op = PatchOperationRef::add("/path".to_string(), &value);
        assert_eq!(op.op_type(), "add");
        assert_eq!(op.path(), "/path");
    }

    #[test]
    fn test_patch_operation_remove() {
        let op = PatchOperationRef::remove("/path".to_string());
        assert_eq!(op.op_type(), "remove");
        assert_eq!(op.path(), "/path");
    }

    #[test]
    fn test_patch_operation_replace() {
        let value = json!("new");
        let op = PatchOperationRef::replace("/path".to_string(), &value);
        assert_eq!(op.op_type(), "replace");
        assert_eq!(op.path(), "/path");
    }

    #[test]
    fn test_patch_operation_move() {
        let op = PatchOperationRef::move_op("/from".to_string(), "/to".to_string());
        assert_eq!(op.op_type(), "move");
        assert_eq!(op.path(), "/to");
    }

    #[test]
    fn test_patch_operation_copy() {
        let op = PatchOperationRef::copy("/from".to_string(), "/to".to_string());
        assert_eq!(op.op_type(), "copy");
        assert_eq!(op.path(), "/to");
    }

    #[test]
    fn test_patch_operation_test() {
        let value = json!(true);
        let op = PatchOperationRef::test("/path".to_string(), &value);
        assert_eq!(op.op_type(), "test");
        assert_eq!(op.path(), "/path");
    }

    // =========================================================================
    // PatchOperationRef into_owned Tests
    // =========================================================================

    #[test]
    fn test_patch_operation_into_owned_add() {
        let value = json!(42);
        let op = PatchOperationRef::add("/path".to_string(), &value);
        let owned = op.into_owned();
        assert_eq!(owned.op_type(), "add");
        assert_eq!(owned.path(), "/path");
    }

    #[test]
    fn test_patch_operation_into_owned_remove() {
        let op = PatchOperationRef::remove("/path".to_string());
        let owned = op.into_owned();
        assert_eq!(owned.op_type(), "remove");
        assert_eq!(owned.path(), "/path");
    }

    #[test]
    fn test_patch_operation_into_owned_replace() {
        let value = json!("new");
        let op = PatchOperationRef::replace("/path".to_string(), &value);
        let owned = op.into_owned();
        assert_eq!(owned.op_type(), "replace");
    }

    #[test]
    fn test_patch_operation_into_owned_move() {
        let op = PatchOperationRef::move_op("/from".to_string(), "/to".to_string());
        let owned = op.into_owned();
        assert_eq!(owned.op_type(), "move");
    }

    #[test]
    fn test_patch_operation_into_owned_copy() {
        let op = PatchOperationRef::copy("/from".to_string(), "/to".to_string());
        let owned = op.into_owned();
        assert_eq!(owned.op_type(), "copy");
    }

    #[test]
    fn test_patch_operation_into_owned_test() {
        let value = json!(true);
        let op = PatchOperationRef::test("/path".to_string(), &value);
        let owned = op.into_owned();
        assert_eq!(owned.op_type(), "test");
    }

    #[test]
    fn test_patch_operation_debug() {
        let value = json!(42);
        let op = PatchOperationRef::add("/path".to_string(), &value);
        let debug_str = format!("{op:?}");
        assert!(debug_str.contains("Add"));
    }

    #[test]
    fn test_patch_operation_clone() {
        let value = json!(42);
        let op = PatchOperationRef::add("/path".to_string(), &value);
        let cloned = op.clone();
        assert_eq!(cloned.path(), "/path");
    }

    // =========================================================================
    // JsonPatchRef Tests
    // =========================================================================

    #[test]
    fn test_json_patch_ref_new() {
        let patch = JsonPatchRef::new();
        assert!(patch.is_empty());
        assert_eq!(patch.len(), 0);
    }

    #[test]
    fn test_json_patch_ref_with_capacity() {
        let patch = JsonPatchRef::with_capacity(10);
        assert!(patch.is_empty());
    }

    #[test]
    fn test_json_patch_ref_push() {
        let value = json!(42);
        let mut patch = JsonPatchRef::new();
        patch.push(PatchOperationRef::add("/a".to_string(), &value));
        assert_eq!(patch.len(), 1);
    }

    #[test]
    fn test_json_patch_ref_iter() {
        let value = json!(42);
        let mut patch = JsonPatchRef::new();
        patch.push(PatchOperationRef::add("/a".to_string(), &value));
        patch.push(PatchOperationRef::remove("/b".to_string()));

        let paths: Vec<_> = patch.iter().map(super::PatchOperationRef::path).collect();
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], "/a");
        assert_eq!(paths[1], "/b");
    }

    #[test]
    fn test_json_patch_ref_into_iter_owned() {
        let value = json!(42);
        let mut patch = JsonPatchRef::new();
        patch.push(PatchOperationRef::add("/a".to_string(), &value));

        let ops_count = patch.into_iter().count();
        assert_eq!(ops_count, 1);
    }

    #[test]
    fn test_json_patch_ref_into_iter_ref() {
        let value = json!(42);
        let mut patch = JsonPatchRef::new();
        patch.push(PatchOperationRef::add("/a".to_string(), &value));

        let ops_count = (&patch).into_iter().count();
        assert_eq!(ops_count, 1);
    }

    #[test]
    fn test_json_patch_ref_default() {
        let patch = JsonPatchRef::default();
        assert!(patch.is_empty());
    }

    #[test]
    fn test_json_patch_ref_debug() {
        let patch = JsonPatchRef::new();
        let debug_str = format!("{patch:?}");
        assert!(debug_str.contains("JsonPatchRef"));
    }

    #[test]
    fn test_json_patch_ref_clone() {
        let value = json!(42);
        let mut patch = JsonPatchRef::new();
        patch.push(PatchOperationRef::add("/a".to_string(), &value));
        let cloned = patch.clone();
        assert_eq!(cloned.len(), 1);
    }

    // =========================================================================
    // to_json_patch Conversion Tests
    // =========================================================================

    #[test]
    fn test_to_json_patch_add() {
        let value = json!(42);
        let mut patch = JsonPatchRef::new();
        patch.push(PatchOperationRef::add("/a".to_string(), &value));
        let json_patch = patch.to_json_patch();
        assert_eq!(json_patch.operations.len(), 1);
    }

    #[test]
    fn test_to_json_patch_remove() {
        let mut patch = JsonPatchRef::new();
        patch.push(PatchOperationRef::remove("/a".to_string()));
        let json_patch = patch.to_json_patch();
        assert_eq!(json_patch.operations.len(), 1);
    }

    #[test]
    fn test_to_json_patch_replace() {
        let value = json!("new");
        let mut patch = JsonPatchRef::new();
        patch.push(PatchOperationRef::replace("/a".to_string(), &value));
        let json_patch = patch.to_json_patch();
        assert_eq!(json_patch.operations.len(), 1);
    }

    #[test]
    fn test_to_json_patch_move() {
        let mut patch = JsonPatchRef::new();
        patch.push(PatchOperationRef::move_op(
            "/from".to_string(),
            "/to".to_string(),
        ));
        let json_patch = patch.to_json_patch();
        assert_eq!(json_patch.operations.len(), 1);
    }

    #[test]
    fn test_to_json_patch_copy() {
        let mut patch = JsonPatchRef::new();
        patch.push(PatchOperationRef::copy(
            "/from".to_string(),
            "/to".to_string(),
        ));
        let json_patch = patch.to_json_patch();
        assert_eq!(json_patch.operations.len(), 1);
    }

    #[test]
    fn test_to_json_patch_test() {
        let value = json!(true);
        let mut patch = JsonPatchRef::new();
        patch.push(PatchOperationRef::test("/a".to_string(), &value));
        let json_patch = patch.to_json_patch();
        assert_eq!(json_patch.operations.len(), 1);
    }

    // =========================================================================
    // json_diff_zerocopy Tests
    // =========================================================================

    #[test]
    fn test_empty_diff() {
        let doc = json!({"a": 1});
        let patch = json_diff_zerocopy(&doc, &doc);
        assert!(patch.is_empty());
    }

    #[test]
    fn test_add_field() {
        let source = json!({});
        let target = json!({"name": "Alice"});
        let patch = json_diff_zerocopy(&source, &target);

        assert_eq!(patch.len(), 1);
        assert_eq!(patch.operations[0].op_type(), "add");
        assert_eq!(patch.operations[0].path(), "/name");
    }

    #[test]
    fn test_remove_field() {
        let source = json!({"name": "Alice"});
        let target = json!({});
        let patch = json_diff_zerocopy(&source, &target);

        assert_eq!(patch.len(), 1);
        assert_eq!(patch.operations[0].op_type(), "remove");
    }

    #[test]
    fn test_replace_value() {
        let source = json!({"name": "Alice"});
        let target = json!({"name": "Bob"});
        let patch = json_diff_zerocopy(&source, &target);

        assert_eq!(patch.len(), 1);
        assert_eq!(patch.operations[0].op_type(), "replace");
    }

    #[test]
    fn test_nested_diff() {
        let source = json!({"user": {"name": "Alice"}});
        let target = json!({"user": {"name": "Bob"}});
        let patch = json_diff_zerocopy(&source, &target);

        assert_eq!(patch.len(), 1);
        assert_eq!(patch.operations[0].path(), "/user/name");
    }

    #[test]
    fn test_array_diff_add() {
        let source = json!([1, 2, 3]);
        let target = json!([1, 2, 3, 4]);
        let patch = json_diff_zerocopy(&source, &target);

        assert_eq!(patch.len(), 1);
        assert_eq!(patch.operations[0].op_type(), "add");
    }

    #[test]
    fn test_array_diff_remove() {
        let source = json!([1, 2, 3, 4]);
        let target = json!([1, 2]);
        let patch = json_diff_zerocopy(&source, &target);

        // Should remove indices 3 and 2 in reverse order
        assert_eq!(patch.len(), 2);
        assert_eq!(patch.operations[0].op_type(), "remove");
        assert_eq!(patch.operations[1].op_type(), "remove");
    }

    #[test]
    fn test_array_diff_change() {
        let source = json!([1, 2, 3]);
        let target = json!([1, 99, 3]);
        let patch = json_diff_zerocopy(&source, &target);

        assert_eq!(patch.len(), 1);
        assert_eq!(patch.operations[0].path(), "/1");
        assert_eq!(patch.operations[0].op_type(), "replace");
    }

    #[test]
    fn test_type_change() {
        let source = json!({"x": 1});
        let target = json!({"x": "one"});
        let patch = json_diff_zerocopy(&source, &target);

        assert_eq!(patch.len(), 1);
        assert_eq!(patch.operations[0].op_type(), "replace");
    }

    #[test]
    fn test_root_replacement() {
        let source = json!(42);
        let target = json!("hello");
        let patch = json_diff_zerocopy(&source, &target);

        assert_eq!(patch.len(), 1);
        assert_eq!(patch.operations[0].op_type(), "replace");
        assert_eq!(patch.operations[0].path(), ""); // Empty path for root
    }

    #[test]
    fn test_nested_array_in_object() {
        let source = json!({"items": [1, 2]});
        let target = json!({"items": [1, 2, 3]});
        let patch = json_diff_zerocopy(&source, &target);

        assert_eq!(patch.len(), 1);
        assert_eq!(patch.operations[0].path(), "/items/2");
    }

    #[test]
    fn test_into_owned() {
        let source = json!({});
        let target = json!({"x": 1});
        let patch = json_diff_zerocopy(&source, &target);
        let owned: JsonPatchRef<'static> = patch.into_owned();

        assert!(!owned.is_empty());
    }

    #[test]
    fn test_to_json_patch() {
        let source = json!({});
        let target = json!({"x": 1});
        let patch_ref = json_diff_zerocopy(&source, &target);
        let patch = patch_ref.to_json_patch();

        assert_eq!(patch.operations.len(), 1);
    }

    #[test]
    fn test_escape_json_pointer() {
        assert_eq!(escape_json_pointer("simple"), "simple");
        assert_eq!(escape_json_pointer("has/slash"), "has~1slash");
        assert_eq!(escape_json_pointer("has~tilde"), "has~0tilde");
        assert_eq!(escape_json_pointer("both~and/"), "both~0and~1");
    }

    #[test]
    fn test_diff_with_escaped_keys() {
        let source = json!({});
        let target = json!({"a/b": 1, "c~d": 2});
        let patch = json_diff_zerocopy(&source, &target);

        assert_eq!(patch.len(), 2);
        // Paths should have escaped keys
        let paths: Vec<_> = patch.iter().map(super::PatchOperationRef::path).collect();
        assert!(paths.contains(&"/a~1b") || paths.contains(&"/c~0d"));
    }

    #[test]
    fn test_deeply_nested_diff() {
        let source = json!({"a": {"b": {"c": {"d": 1}}}});
        let target = json!({"a": {"b": {"c": {"d": 2}}}});
        let patch = json_diff_zerocopy(&source, &target);

        assert_eq!(patch.len(), 1);
        assert_eq!(patch.operations[0].path(), "/a/b/c/d");
    }

    #[test]
    fn test_empty_array_to_non_empty() {
        let source = json!([]);
        let target = json!([1, 2, 3]);
        let patch = json_diff_zerocopy(&source, &target);

        assert_eq!(patch.len(), 3);
    }

    #[test]
    fn test_non_empty_array_to_empty() {
        let source = json!([1, 2, 3]);
        let target = json!([]);
        let patch = json_diff_zerocopy(&source, &target);

        assert_eq!(patch.len(), 3);
        // All should be remove operations
        for op in patch.iter() {
            assert_eq!(op.op_type(), "remove");
        }
    }

    #[test]
    fn test_object_to_array_type_change() {
        let source = json!({"a": 1});
        let target = json!([1]);
        let patch = json_diff_zerocopy(&source, &target);

        assert_eq!(patch.len(), 1);
        assert_eq!(patch.operations[0].op_type(), "replace");
        assert_eq!(patch.operations[0].path(), "");
    }

    #[test]
    fn test_nested_object_add() {
        let source = json!({"user": {}});
        let target = json!({"user": {"name": "Alice"}});
        let patch = json_diff_zerocopy(&source, &target);

        assert_eq!(patch.len(), 1);
        assert_eq!(patch.operations[0].op_type(), "add");
        assert_eq!(patch.operations[0].path(), "/user/name");
    }

    #[test]
    fn test_nested_object_remove() {
        let source = json!({"user": {"name": "Alice", "age": 30}});
        let target = json!({"user": {"name": "Alice"}});
        let patch = json_diff_zerocopy(&source, &target);

        assert_eq!(patch.len(), 1);
        assert_eq!(patch.operations[0].op_type(), "remove");
        assert_eq!(patch.operations[0].path(), "/user/age");
    }
}
