// SPDX-License-Identifier: MIT OR Apache-2.0
//! JSON Patch implementation (RFC 6902).
//!
//! Provides operations to modify JSON documents:
//! - `add`: Insert a value at a path
//! - `remove`: Delete a value at a path
//! - `replace`: Replace a value at a path
//! - `move`: Move a value from one path to another
//! - `copy`: Copy a value from one path to another
//! - `test`: Verify a value equals the expected value

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// Error type for patch operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchError {
    /// Path does not exist in the document.
    PathNotFound(String),
    /// Cannot add to non-container (object/array).
    InvalidTarget(String),
    /// Array index out of bounds.
    IndexOutOfBounds {
        /// The path where the error occurred.
        path: String,
        /// The requested index.
        index: usize,
        /// The actual array length.
        len: usize,
    },
    /// Test operation failed.
    TestFailed {
        /// The path where the test failed.
        path: String,
        /// The expected value.
        expected: String,
        /// The actual value found.
        actual: String,
    },
    /// Invalid path format.
    InvalidPath(String),
    /// Invalid array index.
    InvalidIndex(String),
    /// Cannot remove from root.
    CannotRemoveRoot,
    /// Move/copy source path not found.
    SourceNotFound(String),
}

impl fmt::Display for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PathNotFound(path) => write!(f, "path not found: {path}"),
            Self::InvalidTarget(path) => write!(f, "invalid target at: {path}"),
            Self::IndexOutOfBounds { path, index, len } => {
                write!(f, "index {index} out of bounds (len {len}) at: {path}")
            }
            Self::TestFailed {
                path,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "test failed at {path}: expected {expected}, got {actual}"
                )
            }
            Self::InvalidPath(msg) => write!(f, "invalid path: {msg}"),
            Self::InvalidIndex(idx) => write!(f, "invalid array index: {idx}"),
            Self::CannotRemoveRoot => write!(f, "cannot remove root document"),
            Self::SourceNotFound(path) => write!(f, "source path not found: {path}"),
        }
    }
}

impl std::error::Error for PatchError {}

/// A single patch operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum PatchOperation {
    /// Add a value at the specified path.
    Add {
        /// JSON Pointer path (RFC 6901).
        path: String,
        /// Value to add.
        value: Value,
    },
    /// Remove the value at the specified path.
    Remove {
        /// JSON Pointer path.
        path: String,
    },
    /// Replace the value at the specified path.
    Replace {
        /// JSON Pointer path.
        path: String,
        /// New value.
        value: Value,
    },
    /// Move a value from one path to another.
    Move {
        /// Source path.
        from: String,
        /// Destination path.
        path: String,
    },
    /// Copy a value from one path to another.
    Copy {
        /// Source path.
        from: String,
        /// Destination path.
        path: String,
    },
    /// Test that a value equals the expected value.
    Test {
        /// JSON Pointer path.
        path: String,
        /// Expected value.
        value: Value,
    },
}

/// A JSON Patch document (sequence of operations).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JsonPatch {
    /// The operations in this patch.
    pub operations: Vec<PatchOperation>,
}

impl JsonPatch {
    /// Create a new empty patch.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            operations: Vec::new(),
        }
    }

    /// Create a patch from a vector of operations.
    #[must_use]
    pub const fn from_operations(operations: Vec<PatchOperation>) -> Self {
        Self { operations }
    }

    /// Add an operation to the patch.
    pub fn push(&mut self, op: PatchOperation) {
        self.operations.push(op);
    }

    /// Check if the patch is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    /// Get the number of operations.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.operations.len()
    }

    /// Apply this patch to a value, returning a new value.
    ///
    /// # Errors
    ///
    /// Returns an error if any operation fails.
    pub fn apply(&self, target: &Value) -> Result<Value, PatchError> {
        let mut result = target.clone();
        apply_patch_mut(&mut result, self)?;
        Ok(result)
    }
}

impl Default for JsonPatch {
    fn default() -> Self {
        Self::new()
    }
}

impl IntoIterator for JsonPatch {
    type Item = PatchOperation;
    type IntoIter = std::vec::IntoIter<PatchOperation>;

    fn into_iter(self) -> Self::IntoIter {
        self.operations.into_iter()
    }
}

impl JsonPatch {
    /// Returns an iterator over the operations.
    pub fn iter(&self) -> std::slice::Iter<'_, PatchOperation> {
        self.operations.iter()
    }
}

impl<'a> IntoIterator for &'a JsonPatch {
    type Item = &'a PatchOperation;
    type IntoIter = std::slice::Iter<'a, PatchOperation>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Apply a JSON Patch to a value, returning a new value.
///
/// This is a convenience function that clones the input.
///
/// # Errors
///
/// Returns an error if any operation fails.
pub fn apply_patch(target: &Value, patch: &JsonPatch) -> Result<Value, PatchError> {
    patch.apply(target)
}

/// Apply a JSON Patch to a value in place.
///
/// # Errors
///
/// Returns an error if any operation fails. On error, the target
/// may be partially modified.
pub fn apply_patch_mut(target: &mut Value, patch: &JsonPatch) -> Result<(), PatchError> {
    for op in &patch.operations {
        apply_operation(target, op)?;
    }
    Ok(())
}

/// Apply a single patch operation.
fn apply_operation(target: &mut Value, op: &PatchOperation) -> Result<(), PatchError> {
    match op {
        PatchOperation::Add { path, value } => op_add(target, path, value.clone()),
        PatchOperation::Remove { path } => op_remove(target, path),
        PatchOperation::Replace { path, value } => op_replace(target, path, value.clone()),
        PatchOperation::Move { from, path } => op_move(target, from, path),
        PatchOperation::Copy { from, path } => op_copy(target, from, path),
        PatchOperation::Test { path, value } => op_test(target, path, value),
    }
}

/// Parse a JSON Pointer path into segments.
fn parse_pointer(path: &str) -> Result<Vec<String>, PatchError> {
    if path.is_empty() {
        return Ok(vec![]);
    }

    if !path.starts_with('/') {
        return Err(PatchError::InvalidPath(format!(
            "JSON Pointer must start with '/': {path}"
        )));
    }

    Ok(path[1..]
        .split('/')
        .map(|s| {
            // Unescape JSON Pointer encoding
            s.replace("~1", "/").replace("~0", "~")
        })
        .collect())
}

/// Get a mutable reference to the parent of the target path.
fn get_parent_mut<'a>(
    target: &'a mut Value,
    segments: &'a [String],
) -> Result<(&'a mut Value, &'a str), PatchError> {
    if segments.is_empty() {
        return Err(PatchError::CannotRemoveRoot);
    }

    let (parent_segments, last) = segments.split_at(segments.len() - 1);
    let last_key = &last[0];

    let parent = navigate_to_mut(target, parent_segments)?;
    Ok((parent, last_key))
}

/// Navigate to a path, returning a mutable reference.
fn navigate_to_mut<'a>(
    target: &'a mut Value,
    segments: &[String],
) -> Result<&'a mut Value, PatchError> {
    let mut current = target;

    for segment in segments {
        current = match current {
            Value::Object(map) => map
                .get_mut(segment)
                .ok_or_else(|| PatchError::PathNotFound(segment.clone()))?,
            Value::Array(arr) => {
                let index = parse_array_index(segment)?;
                let len = arr.len();
                arr.get_mut(index)
                    .ok_or_else(|| PatchError::IndexOutOfBounds {
                        path: segment.clone(),
                        index,
                        len,
                    })?
            }
            _ => return Err(PatchError::InvalidTarget(segment.clone())),
        };
    }

    Ok(current)
}

/// Navigate to a path, returning an immutable reference.
fn navigate_to<'a>(target: &'a Value, segments: &[String]) -> Result<&'a Value, PatchError> {
    let mut current = target;

    for segment in segments {
        current = match current {
            Value::Object(map) => map
                .get(segment)
                .ok_or_else(|| PatchError::PathNotFound(segment.clone()))?,
            Value::Array(arr) => {
                let index = parse_array_index(segment)?;
                arr.get(index).ok_or_else(|| PatchError::IndexOutOfBounds {
                    path: segment.clone(),
                    index,
                    len: arr.len(),
                })?
            }
            _ => return Err(PatchError::InvalidTarget(segment.clone())),
        };
    }

    Ok(current)
}

/// Parse an array index from a path segment.
fn parse_array_index(segment: &str) -> Result<usize, PatchError> {
    // Special case: "-" means end of array (for add)
    if segment == "-" {
        return Err(PatchError::InvalidIndex("-".to_string()));
    }

    segment
        .parse::<usize>()
        .map_err(|_| PatchError::InvalidIndex(segment.to_string()))
}

/// Add operation.
fn op_add(target: &mut Value, path: &str, value: Value) -> Result<(), PatchError> {
    let segments = parse_pointer(path)?;

    if segments.is_empty() {
        // Replace the entire document
        *target = value;
        return Ok(());
    }

    let (parent, key) = get_parent_mut(target, &segments)?;
    let key_owned = key.to_string();

    match parent {
        Value::Object(map) => {
            map.insert(key_owned, value);
            Ok(())
        }
        Value::Array(arr) => {
            if key == "-" {
                // Append to end
                arr.push(value);
            } else {
                let index = parse_array_index(key)?;
                if index > arr.len() {
                    return Err(PatchError::IndexOutOfBounds {
                        path: path.to_string(),
                        index,
                        len: arr.len(),
                    });
                }
                arr.insert(index, value);
            }
            Ok(())
        }
        _ => Err(PatchError::InvalidTarget(path.to_string())),
    }
}

/// Remove operation.
fn op_remove(target: &mut Value, path: &str) -> Result<(), PatchError> {
    let segments = parse_pointer(path)?;

    if segments.is_empty() {
        return Err(PatchError::CannotRemoveRoot);
    }

    let (parent, key) = get_parent_mut(target, &segments)?;
    let key_owned = key.to_string();

    match parent {
        Value::Object(map) => {
            if map.remove(&key_owned).is_none() {
                return Err(PatchError::PathNotFound(path.to_string()));
            }
            Ok(())
        }
        Value::Array(arr) => {
            let index = parse_array_index(&key_owned)?;
            if index >= arr.len() {
                return Err(PatchError::IndexOutOfBounds {
                    path: path.to_string(),
                    index,
                    len: arr.len(),
                });
            }
            arr.remove(index);
            Ok(())
        }
        _ => Err(PatchError::InvalidTarget(path.to_string())),
    }
}

/// Replace operation.
fn op_replace(target: &mut Value, path: &str, value: Value) -> Result<(), PatchError> {
    let segments = parse_pointer(path)?;

    if segments.is_empty() {
        *target = value;
        return Ok(());
    }

    let (parent, key) = get_parent_mut(target, &segments)?;
    let key_owned = key.to_string();

    match parent {
        Value::Object(map) => {
            if !map.contains_key(&key_owned) {
                return Err(PatchError::PathNotFound(path.to_string()));
            }
            map.insert(key_owned, value);
            Ok(())
        }
        Value::Array(arr) => {
            let index = parse_array_index(&key_owned)?;
            if index >= arr.len() {
                return Err(PatchError::IndexOutOfBounds {
                    path: path.to_string(),
                    index,
                    len: arr.len(),
                });
            }
            arr[index] = value;
            Ok(())
        }
        _ => Err(PatchError::InvalidTarget(path.to_string())),
    }
}

/// Move operation.
fn op_move(target: &mut Value, from: &str, to: &str) -> Result<(), PatchError> {
    // Get the value at 'from'
    let segments = parse_pointer(from)?;
    let value = navigate_to(target, &segments)?.clone();

    // Remove from source
    op_remove(target, from)?;

    // Add to destination
    op_add(target, to, value)
}

/// Copy operation.
fn op_copy(target: &mut Value, from: &str, to: &str) -> Result<(), PatchError> {
    // Get the value at 'from'
    let segments = parse_pointer(from)?;
    let value = navigate_to(target, &segments)
        .map_err(|_| PatchError::SourceNotFound(from.to_string()))?
        .clone();

    // Add to destination
    op_add(target, to, value)
}

/// Test operation.
fn op_test(target: &Value, path: &str, expected: &Value) -> Result<(), PatchError> {
    let segments = parse_pointer(path)?;
    let actual = navigate_to(target, &segments)?;

    if actual != expected {
        return Err(PatchError::TestFailed {
            path: path.to_string(),
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_add_to_object() {
        let mut doc = json!({"foo": "bar"});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Add {
            path: "/baz".to_string(),
            value: json!("qux"),
        }]);

        apply_patch_mut(&mut doc, &patch).unwrap();
        assert_eq!(doc, json!({"foo": "bar", "baz": "qux"}));
    }

    #[test]
    fn test_add_to_array() {
        let mut doc = json!({"foo": ["bar", "baz"]});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Add {
            path: "/foo/1".to_string(),
            value: json!("qux"),
        }]);

        apply_patch_mut(&mut doc, &patch).unwrap();
        assert_eq!(doc, json!({"foo": ["bar", "qux", "baz"]}));
    }

    #[test]
    fn test_add_to_array_end() {
        let mut doc = json!({"foo": ["bar"]});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Add {
            path: "/foo/-".to_string(),
            value: json!("qux"),
        }]);

        apply_patch_mut(&mut doc, &patch).unwrap();
        assert_eq!(doc, json!({"foo": ["bar", "qux"]}));
    }

    #[test]
    fn test_remove_from_object() {
        let mut doc = json!({"foo": "bar", "baz": "qux"});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Remove {
            path: "/baz".to_string(),
        }]);

        apply_patch_mut(&mut doc, &patch).unwrap();
        assert_eq!(doc, json!({"foo": "bar"}));
    }

    #[test]
    fn test_remove_from_array() {
        let mut doc = json!({"foo": ["bar", "qux", "baz"]});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Remove {
            path: "/foo/1".to_string(),
        }]);

        apply_patch_mut(&mut doc, &patch).unwrap();
        assert_eq!(doc, json!({"foo": ["bar", "baz"]}));
    }

    #[test]
    fn test_replace_value() {
        let mut doc = json!({"foo": "bar"});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Replace {
            path: "/foo".to_string(),
            value: json!("baz"),
        }]);

        apply_patch_mut(&mut doc, &patch).unwrap();
        assert_eq!(doc, json!({"foo": "baz"}));
    }

    #[test]
    fn test_move_value() {
        let mut doc = json!({"foo": {"bar": "baz"}, "qux": {"corge": "grault"}});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Move {
            from: "/foo/bar".to_string(),
            path: "/qux/thud".to_string(),
        }]);

        apply_patch_mut(&mut doc, &patch).unwrap();
        assert_eq!(
            doc,
            json!({"foo": {}, "qux": {"corge": "grault", "thud": "baz"}})
        );
    }

    #[test]
    fn test_copy_value() {
        let mut doc = json!({"foo": {"bar": "baz"}});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Copy {
            from: "/foo/bar".to_string(),
            path: "/foo/qux".to_string(),
        }]);

        apply_patch_mut(&mut doc, &patch).unwrap();
        assert_eq!(doc, json!({"foo": {"bar": "baz", "qux": "baz"}}));
    }

    #[test]
    fn test_test_success() {
        let doc = json!({"foo": "bar"});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Test {
            path: "/foo".to_string(),
            value: json!("bar"),
        }]);

        apply_patch(&doc, &patch).unwrap();
    }

    #[test]
    fn test_test_failure() {
        let doc = json!({"foo": "bar"});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Test {
            path: "/foo".to_string(),
            value: json!("baz"),
        }]);

        let result = apply_patch(&doc, &patch);
        assert!(matches!(result, Err(PatchError::TestFailed { .. })));
    }

    #[test]
    fn test_replace_root() {
        let mut doc = json!({"foo": "bar"});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Replace {
            path: String::new(),
            value: json!({"completely": "new"}),
        }]);

        apply_patch_mut(&mut doc, &patch).unwrap();
        assert_eq!(doc, json!({"completely": "new"}));
    }

    #[test]
    fn test_nested_path() {
        let mut doc = json!({"foo": {"bar": {"baz": "qux"}}});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Replace {
            path: "/foo/bar/baz".to_string(),
            value: json!("new value"),
        }]);

        apply_patch_mut(&mut doc, &patch).unwrap();
        assert_eq!(doc, json!({"foo": {"bar": {"baz": "new value"}}}));
    }

    #[test]
    fn test_path_with_special_chars() {
        let mut doc = json!({"foo/bar": "baz"});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Replace {
            path: "/foo~1bar".to_string(), // ~1 encodes /
            value: json!("qux"),
        }]);

        apply_patch_mut(&mut doc, &patch).unwrap();
        assert_eq!(doc, json!({"foo/bar": "qux"}));
    }

    #[test]
    fn test_path_with_tilde() {
        let mut doc = json!({"foo~bar": "baz"});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Replace {
            path: "/foo~0bar".to_string(), // ~0 encodes ~
            value: json!("qux"),
        }]);

        apply_patch_mut(&mut doc, &patch).unwrap();
        assert_eq!(doc, json!({"foo~bar": "qux"}));
    }

    #[test]
    fn test_patch_serialization() {
        let patch = JsonPatch::from_operations(vec![
            PatchOperation::Add {
                path: "/foo".to_string(),
                value: json!("bar"),
            },
            PatchOperation::Remove {
                path: "/baz".to_string(),
            },
        ]);

        let json = serde_json::to_string(&patch).unwrap();
        let parsed: JsonPatch = serde_json::from_str(&json).unwrap();
        assert_eq!(patch, parsed);
    }

    #[test]
    fn test_error_path_not_found() {
        let mut doc = json!({"foo": "bar"});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Remove {
            path: "/nonexistent".to_string(),
        }]);

        let result = apply_patch_mut(&mut doc, &patch);
        assert!(matches!(result, Err(PatchError::PathNotFound(_))));
    }

    #[test]
    fn test_error_index_out_of_bounds() {
        let mut doc = json!({"arr": [1, 2, 3]});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Replace {
            path: "/arr/10".to_string(),
            value: json!(42),
        }]);

        let result = apply_patch_mut(&mut doc, &patch);
        assert!(matches!(result, Err(PatchError::IndexOutOfBounds { .. })));
    }

    #[test]
    fn test_json_patch_builder() {
        let mut patch = JsonPatch::new();
        assert!(patch.is_empty());

        patch.push(PatchOperation::Add {
            path: "/foo".to_string(),
            value: json!("bar"),
        });
        assert_eq!(patch.len(), 1);
        assert!(!patch.is_empty());
    }

    // =========================================================================
    // PatchError Display Tests
    // =========================================================================

    #[test]
    fn test_error_display_path_not_found() {
        let err = PatchError::PathNotFound("/foo".to_string());
        assert_eq!(err.to_string(), "path not found: /foo");
    }

    #[test]
    fn test_error_display_invalid_target() {
        let err = PatchError::InvalidTarget("/bar".to_string());
        assert_eq!(err.to_string(), "invalid target at: /bar");
    }

    #[test]
    fn test_error_display_index_out_of_bounds() {
        let err = PatchError::IndexOutOfBounds {
            path: "/arr".to_string(),
            index: 10,
            len: 3,
        };
        assert_eq!(err.to_string(), "index 10 out of bounds (len 3) at: /arr");
    }

    #[test]
    fn test_error_display_test_failed() {
        let err = PatchError::TestFailed {
            path: "/foo".to_string(),
            expected: "\"bar\"".to_string(),
            actual: "\"baz\"".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "test failed at /foo: expected \"bar\", got \"baz\""
        );
    }

    #[test]
    fn test_error_display_invalid_path() {
        let err = PatchError::InvalidPath("no slash".to_string());
        assert_eq!(err.to_string(), "invalid path: no slash");
    }

    #[test]
    fn test_error_display_invalid_index() {
        let err = PatchError::InvalidIndex("abc".to_string());
        assert_eq!(err.to_string(), "invalid array index: abc");
    }

    #[test]
    fn test_error_display_cannot_remove_root() {
        let err = PatchError::CannotRemoveRoot;
        assert_eq!(err.to_string(), "cannot remove root document");
    }

    #[test]
    fn test_error_display_source_not_found() {
        let err = PatchError::SourceNotFound("/src".to_string());
        assert_eq!(err.to_string(), "source path not found: /src");
    }

    #[test]
    fn test_error_is_std_error() {
        let err: Box<dyn std::error::Error> = Box::new(PatchError::CannotRemoveRoot);
        assert!(err.to_string().contains("cannot remove root"));
    }

    // =========================================================================
    // JsonPatch Default and Iterator Tests
    // =========================================================================

    #[test]
    fn test_json_patch_default() {
        let patch = JsonPatch::default();
        assert!(patch.is_empty());
    }

    #[test]
    fn test_json_patch_into_iter() {
        let patch = JsonPatch::from_operations(vec![
            PatchOperation::Add {
                path: "/a".to_string(),
                value: json!(1),
            },
            PatchOperation::Add {
                path: "/b".to_string(),
                value: json!(2),
            },
        ]);

        let ops_count = patch.into_iter().count();
        assert_eq!(ops_count, 2);
    }

    #[test]
    fn test_json_patch_iter_ref() {
        let patch = JsonPatch::from_operations(vec![PatchOperation::Add {
            path: "/x".to_string(),
            value: json!(1),
        }]);

        for op in &patch {
            assert!(matches!(op, PatchOperation::Add { .. }));
        }
        // patch still valid
        assert_eq!(patch.len(), 1);
    }

    #[test]
    fn test_json_patch_apply() {
        let patch = JsonPatch::from_operations(vec![PatchOperation::Add {
            path: "/new".to_string(),
            value: json!("value"),
        }]);

        let doc = json!({"existing": 1});
        let result = patch.apply(&doc).unwrap();
        assert_eq!(result, json!({"existing": 1, "new": "value"}));
    }

    // =========================================================================
    // Error Cases Tests
    // =========================================================================

    #[test]
    fn test_error_remove_root() {
        let mut doc = json!({"foo": "bar"});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Remove {
            path: String::new(),
        }]);

        let result = apply_patch_mut(&mut doc, &patch);
        assert!(matches!(result, Err(PatchError::CannotRemoveRoot)));
    }

    #[test]
    fn test_error_invalid_path_no_slash() {
        let mut doc = json!({"foo": "bar"});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Add {
            path: "no_slash".to_string(),
            value: json!(1),
        }]);

        let result = apply_patch_mut(&mut doc, &patch);
        assert!(matches!(result, Err(PatchError::InvalidPath(_))));
    }

    #[test]
    fn test_error_invalid_array_index() {
        let mut doc = json!({"arr": [1, 2, 3]});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Replace {
            path: "/arr/abc".to_string(),
            value: json!(42),
        }]);

        let result = apply_patch_mut(&mut doc, &patch);
        assert!(matches!(result, Err(PatchError::InvalidIndex(_))));
    }

    #[test]
    fn test_error_invalid_target() {
        let mut doc = json!({"str": "not_an_object"});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Add {
            path: "/str/key".to_string(),
            value: json!(1),
        }]);

        let result = apply_patch_mut(&mut doc, &patch);
        assert!(matches!(result, Err(PatchError::InvalidTarget(_))));
    }

    #[test]
    fn test_error_copy_source_not_found() {
        let mut doc = json!({"foo": "bar"});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Copy {
            from: "/nonexistent".to_string(),
            path: "/target".to_string(),
        }]);

        let result = apply_patch_mut(&mut doc, &patch);
        assert!(matches!(result, Err(PatchError::SourceNotFound(_))));
    }

    #[test]
    fn test_error_add_to_array_out_of_bounds() {
        let mut doc = json!({"arr": [1, 2]});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Add {
            path: "/arr/10".to_string(),
            value: json!(42),
        }]);

        let result = apply_patch_mut(&mut doc, &patch);
        assert!(matches!(result, Err(PatchError::IndexOutOfBounds { .. })));
    }

    #[test]
    fn test_error_remove_from_array_out_of_bounds() {
        let mut doc = json!({"arr": [1, 2]});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Remove {
            path: "/arr/10".to_string(),
        }]);

        let result = apply_patch_mut(&mut doc, &patch);
        assert!(matches!(result, Err(PatchError::IndexOutOfBounds { .. })));
    }

    #[test]
    fn test_error_replace_nonexistent_key() {
        let mut doc = json!({"foo": "bar"});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Replace {
            path: "/nonexistent".to_string(),
            value: json!(1),
        }]);

        let result = apply_patch_mut(&mut doc, &patch);
        assert!(matches!(result, Err(PatchError::PathNotFound(_))));
    }

    #[test]
    fn test_error_navigate_invalid_target() {
        let mut doc = json!({"num": 42});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Add {
            path: "/num/field".to_string(),
            value: json!(1),
        }]);

        let result = apply_patch_mut(&mut doc, &patch);
        assert!(matches!(result, Err(PatchError::InvalidTarget(_))));
    }

    #[test]
    fn test_error_remove_from_invalid_target() {
        let mut doc = json!({"num": 42});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Remove {
            path: "/num/field".to_string(),
        }]);

        let result = apply_patch_mut(&mut doc, &patch);
        assert!(matches!(result, Err(PatchError::InvalidTarget(_))));
    }

    // =========================================================================
    // Additional Operation Tests
    // =========================================================================

    #[test]
    fn test_add_root() {
        let mut doc = json!({"foo": "bar"});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Add {
            path: String::new(),
            value: json!([1, 2, 3]),
        }]);

        apply_patch_mut(&mut doc, &patch).unwrap();
        assert_eq!(doc, json!([1, 2, 3]));
    }

    #[test]
    fn test_replace_array_element() {
        let mut doc = json!({"arr": [1, 2, 3]});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Replace {
            path: "/arr/1".to_string(),
            value: json!(42),
        }]);

        apply_patch_mut(&mut doc, &patch).unwrap();
        assert_eq!(doc, json!({"arr": [1, 42, 3]}));
    }

    #[test]
    fn test_remove_from_nested_array() {
        let mut doc = json!({"nested": {"arr": [1, 2, 3]}});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Remove {
            path: "/nested/arr/0".to_string(),
        }]);

        apply_patch_mut(&mut doc, &patch).unwrap();
        assert_eq!(doc, json!({"nested": {"arr": [2, 3]}}));
    }

    #[test]
    fn test_multiple_operations() {
        let mut doc = json!({"a": 1, "b": 2});
        let patch = JsonPatch::from_operations(vec![
            PatchOperation::Add {
                path: "/c".to_string(),
                value: json!(3),
            },
            PatchOperation::Remove {
                path: "/a".to_string(),
            },
            PatchOperation::Replace {
                path: "/b".to_string(),
                value: json!(20),
            },
        ]);

        apply_patch_mut(&mut doc, &patch).unwrap();
        assert_eq!(doc, json!({"b": 20, "c": 3}));
    }

    #[test]
    fn test_test_at_root() {
        let doc = json!({"foo": "bar"});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Test {
            path: String::new(),
            value: json!({"foo": "bar"}),
        }]);

        let result = apply_patch(&doc, &patch);
        assert!(result.is_ok());
    }

    #[test]
    fn test_copy_to_nested() {
        let mut doc = json!({"source": "value", "target": {}});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Copy {
            from: "/source".to_string(),
            path: "/target/copied".to_string(),
        }]);

        apply_patch_mut(&mut doc, &patch).unwrap();
        assert_eq!(
            doc,
            json!({"source": "value", "target": {"copied": "value"}})
        );
    }

    #[test]
    fn test_move_between_arrays() {
        let mut doc = json!({"arr1": [1, 2], "arr2": [3, 4]});
        let patch = JsonPatch::from_operations(vec![PatchOperation::Move {
            from: "/arr1/0".to_string(),
            path: "/arr2/-".to_string(),
        }]);

        apply_patch_mut(&mut doc, &patch).unwrap();
        assert_eq!(doc, json!({"arr1": [2], "arr2": [3, 4, 1]}));
    }

    // =========================================================================
    // Serialization Tests
    // =========================================================================

    #[test]
    fn test_patch_operation_serialize_all_types() {
        let ops = vec![
            PatchOperation::Add {
                path: "/a".to_string(),
                value: json!(1),
            },
            PatchOperation::Remove {
                path: "/b".to_string(),
            },
            PatchOperation::Replace {
                path: "/c".to_string(),
                value: json!(2),
            },
            PatchOperation::Move {
                from: "/d".to_string(),
                path: "/e".to_string(),
            },
            PatchOperation::Copy {
                from: "/f".to_string(),
                path: "/g".to_string(),
            },
            PatchOperation::Test {
                path: "/h".to_string(),
                value: json!(3),
            },
        ];

        let patch = JsonPatch::from_operations(ops);
        let json_str = serde_json::to_string(&patch).unwrap();

        // Verify can round-trip
        let parsed: JsonPatch = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.len(), 6);
    }

    // =========================================================================
    // PatchError Clone and PartialEq Tests
    // =========================================================================

    #[test]
    fn test_error_clone() {
        let err = PatchError::PathNotFound("/test".to_string());
        let cloned = err.clone();
        assert_eq!(err, cloned);
    }

    #[test]
    fn test_error_partial_eq() {
        let err1 = PatchError::CannotRemoveRoot;
        let err2 = PatchError::CannotRemoveRoot;
        let err3 = PatchError::PathNotFound("x".to_string());

        assert_eq!(err1, err2);
        assert_ne!(err1, err3);
    }

    #[test]
    fn test_error_debug() {
        let err = PatchError::IndexOutOfBounds {
            path: "/arr".to_string(),
            index: 5,
            len: 3,
        };
        let debug_str = format!("{err:?}");
        assert!(debug_str.contains("IndexOutOfBounds"));
    }
}
