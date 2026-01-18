// SPDX-License-Identifier: MIT OR Apache-2.0
//! Format-agnostic patch application
//!
//! This module provides the [`Patchable`] trait for applying patches to values.
//! It extends [`DiffableValue`] with mutation capabilities needed for patch
//! application.
//!
//! # Key Types
//!
//! - [`Patchable`] - Trait for values that can have patches applied
//! - [`PatchError`] - Errors that can occur during patch application
//!
//! # Example
//!
//! ```ignore
//! use fionn_core::patchable::{Patchable, apply_patch};
//! use fionn_core::diffable::{GenericPatch, GenericPatchOperation};
//!
//! let mut target = serde_json::json!({"name": "Alice"});
//! let patch = GenericPatch::with_operations(vec![
//!     GenericPatchOperation::Replace {
//!         path: "/name".to_string(),
//!         value: serde_json::json!("Bob"),
//!     },
//! ]);
//!
//! apply_patch(&mut target, &patch)?;
//! assert_eq!(target["name"], "Bob");
//! ```

use crate::diffable::{DiffableValue, GenericPatch, GenericPatchOperation};
use thiserror::Error;

// ============================================================================
// PatchError
// ============================================================================

/// Errors that can occur during patch application
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum PatchError {
    /// The target path does not exist
    #[error("Path not found: {0}")]
    PathNotFound(String),

    /// The target at the path is not the expected type
    #[error("Invalid target at {path}: expected {expected}, found {found}")]
    InvalidTarget {
        /// The path that failed
        path: String,
        /// What was expected
        expected: String,
        /// What was found
        found: String,
    },

    /// Array index out of bounds
    #[error("Index {index} out of bounds at {path} (length {len})")]
    IndexOutOfBounds {
        /// The path containing the array
        path: String,
        /// The requested index
        index: usize,
        /// The array length
        len: usize,
    },

    /// Test operation failed
    #[error("Test failed at {path}: values do not match")]
    TestFailed {
        /// The path that was tested
        path: String,
    },

    /// Cannot remove the root element
    #[error("Cannot remove the root element")]
    CannotRemoveRoot,

    /// Invalid path syntax
    #[error("Invalid path syntax: {0}")]
    InvalidPath(String),

    /// Move/copy source not found
    #[error("Source path not found: {0}")]
    SourceNotFound(String),
}

// ============================================================================
// Patchable Trait
// ============================================================================

/// Trait for values that can have patches applied
///
/// This trait extends [`DiffableValue`] with mutation capabilities needed
/// for applying patch operations like add, remove, replace, move, and copy.
///
/// # Implementation Notes
///
/// - Navigation methods should handle JSON Pointer syntax (RFC 6901)
/// - Mutations should be atomic when possible
/// - Test operations should not modify the value
pub trait Patchable: DiffableValue + Sized {
    /// Apply a single patch operation
    ///
    /// # Errors
    ///
    /// Returns an error if the operation cannot be applied.
    fn apply_operation(&mut self, op: &GenericPatchOperation<Self>) -> Result<(), PatchError>;

    /// Navigate to a path and return a mutable reference
    ///
    /// # Errors
    ///
    /// Returns an error if the path doesn't exist or is invalid.
    fn get_mut_at_path(&mut self, path: &str) -> Result<&mut Self, PatchError>;

    /// Navigate to a path and return an immutable reference
    ///
    /// # Errors
    ///
    /// Returns an error if the path doesn't exist or is invalid.
    fn get_at_path(&self, path: &str) -> Result<&Self, PatchError>;

    /// Set a value at a path (creating intermediate containers as needed)
    ///
    /// # Errors
    ///
    /// Returns an error if the path is invalid.
    fn set_at_path(&mut self, path: &str, value: Self) -> Result<(), PatchError>;

    /// Remove a value at a path
    ///
    /// # Errors
    ///
    /// Returns an error if the path doesn't exist.
    fn remove_at_path(&mut self, path: &str) -> Result<Self, PatchError>;

    /// Apply an entire patch
    ///
    /// Operations are applied in order. If any operation fails, the
    /// value may be in a partially modified state.
    ///
    /// # Errors
    ///
    /// Returns an error if any operation fails.
    fn apply_patch(&mut self, patch: &GenericPatch<Self>) -> Result<(), PatchError> {
        for op in &patch.operations {
            self.apply_operation(op)?;
        }
        Ok(())
    }
}

// ============================================================================
// JSON Pointer Parsing
// ============================================================================

/// Parse a JSON Pointer path into segments
///
/// # Errors
///
/// Returns an error if the path is invalid.
pub fn parse_pointer(path: &str) -> Result<Vec<String>, PatchError> {
    if path.is_empty() {
        return Ok(vec![]);
    }

    if !path.starts_with('/') {
        return Err(PatchError::InvalidPath(format!(
            "JSON Pointer must start with '/': {path}"
        )));
    }

    Ok(path[1..].split('/').map(unescape_json_pointer).collect())
}

/// Unescape JSON Pointer segment
fn unescape_json_pointer(s: &str) -> String {
    s.replace("~1", "/").replace("~0", "~")
}

/// Get the parent path and final segment
fn split_parent_key(path: &str) -> Result<(String, String), PatchError> {
    let segments = parse_pointer(path)?;
    if segments.is_empty() {
        return Err(PatchError::CannotRemoveRoot);
    }

    let key = segments.last().unwrap().clone();
    let parent_path = if segments.len() == 1 {
        String::new()
    } else {
        format!(
            "/{}",
            segments[..segments.len() - 1]
                .iter()
                .map(|s| escape_json_pointer(s))
                .collect::<Vec<_>>()
                .join("/")
        )
    };

    Ok((parent_path, key))
}

fn escape_json_pointer(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

// ============================================================================
// Patchable for serde_json::Value
// ============================================================================

/// Helper: insert value into array at key position
fn insert_into_array(
    arr: &mut Vec<serde_json::Value>,
    key: &str,
    value: &serde_json::Value,
    path: &str,
) -> Result<(), PatchError> {
    if key == "-" {
        arr.push(value.clone());
    } else {
        let index: usize = key
            .parse()
            .map_err(|_| PatchError::InvalidPath(format!("Invalid array index: {key}")))?;
        if index > arr.len() {
            return Err(PatchError::IndexOutOfBounds {
                path: path.to_string(),
                index,
                len: arr.len(),
            });
        }
        arr.insert(index, value.clone());
    }
    Ok(())
}

/// Helper: remove from array at key position
fn remove_from_array(
    arr: &mut Vec<serde_json::Value>,
    key: &str,
    path: &str,
) -> Result<(), PatchError> {
    let index: usize = key
        .parse()
        .map_err(|_| PatchError::InvalidPath(format!("Invalid array index: {key}")))?;
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

/// Helper: apply Add operation
fn apply_add(
    doc: &mut serde_json::Value,
    path: &str,
    value: &serde_json::Value,
) -> Result<(), PatchError> {
    use crate::diffable::DiffableValue;

    if path.is_empty() {
        *doc = value.clone();
        return Ok(());
    }

    let (parent_path, key) = split_parent_key(path)?;
    let parent = if parent_path.is_empty() {
        doc
    } else {
        <serde_json::Value as Patchable>::get_mut_at_path(doc, &parent_path)?
    };

    match parent {
        serde_json::Value::Object(map) => {
            map.insert(key, value.clone());
            Ok(())
        }
        serde_json::Value::Array(arr) => insert_into_array(arr, &key, value, path),
        _ => Err(PatchError::InvalidTarget {
            path: parent_path,
            expected: "object or array".to_string(),
            found: format!("{:?}", parent.value_kind()),
        }),
    }
}

/// Helper: apply Remove operation
fn apply_remove(doc: &mut serde_json::Value, path: &str) -> Result<(), PatchError> {
    use crate::diffable::DiffableValue;

    if path.is_empty() {
        return Err(PatchError::CannotRemoveRoot);
    }

    let (parent_path, key) = split_parent_key(path)?;
    let parent = if parent_path.is_empty() {
        doc
    } else {
        <serde_json::Value as Patchable>::get_mut_at_path(doc, &parent_path)?
    };

    match parent {
        serde_json::Value::Object(map) => {
            map.remove(&key)
                .ok_or_else(|| PatchError::PathNotFound(path.to_string()))?;
            Ok(())
        }
        serde_json::Value::Array(arr) => remove_from_array(arr, &key, path),
        _ => Err(PatchError::InvalidTarget {
            path: parent_path,
            expected: "object or array".to_string(),
            found: format!("{:?}", parent.value_kind()),
        }),
    }
}

/// Helper: apply Replace operation
fn apply_replace(
    doc: &mut serde_json::Value,
    path: &str,
    value: &serde_json::Value,
) -> Result<(), PatchError> {
    if path.is_empty() {
        *doc = value.clone();
        return Ok(());
    }

    let target = <serde_json::Value as Patchable>::get_mut_at_path(doc, path)?;
    *target = value.clone();
    Ok(())
}

impl Patchable for serde_json::Value {
    fn apply_operation(&mut self, op: &GenericPatchOperation<Self>) -> Result<(), PatchError> {
        match op {
            GenericPatchOperation::Add { path, value } => apply_add(self, path, value),
            GenericPatchOperation::Remove { path } => apply_remove(self, path),
            GenericPatchOperation::Replace { path, value } => apply_replace(self, path, value),
            GenericPatchOperation::Move { from, path } => {
                let val = self.remove_at_path(from)?;
                self.set_at_path(path, val)
            }
            GenericPatchOperation::Copy { from, path } => {
                let val = self.get_at_path(from)?.clone();
                self.set_at_path(path, val)
            }
            GenericPatchOperation::Test { path, value } => {
                let actual = self.get_at_path(path)?;
                if actual == value {
                    Ok(())
                } else {
                    Err(PatchError::TestFailed { path: path.clone() })
                }
            }
        }
    }

    fn get_mut_at_path(&mut self, path: &str) -> Result<&mut Self, PatchError> {
        let segments = parse_pointer(path)?;
        let mut current = self;

        for (i, segment) in segments.iter().enumerate() {
            let path_so_far = format!(
                "/{}",
                segments[..=i]
                    .iter()
                    .map(|s| escape_json_pointer(s))
                    .collect::<Vec<_>>()
                    .join("/")
            );

            current = match current {
                Self::Object(map) => map
                    .get_mut(segment)
                    .ok_or(PatchError::PathNotFound(path_so_far))?,
                Self::Array(arr) => {
                    let index: usize = segment.parse().map_err(|_| {
                        PatchError::InvalidPath(format!("Invalid array index: {segment}"))
                    })?;
                    let arr_len = arr.len();
                    arr.get_mut(index).ok_or(PatchError::IndexOutOfBounds {
                        path: path_so_far,
                        index,
                        len: arr_len,
                    })?
                }
                _ => {
                    return Err(PatchError::InvalidTarget {
                        path: path_so_far,
                        expected: "object or array".to_string(),
                        found: format!("{:?}", current.value_kind()),
                    });
                }
            };
        }

        Ok(current)
    }

    fn get_at_path(&self, path: &str) -> Result<&Self, PatchError> {
        let segments = parse_pointer(path)?;
        let mut current = self;

        for (i, segment) in segments.iter().enumerate() {
            let path_so_far = format!(
                "/{}",
                segments[..=i]
                    .iter()
                    .map(|s| escape_json_pointer(s))
                    .collect::<Vec<_>>()
                    .join("/")
            );

            current = match current {
                Self::Object(map) => map
                    .get(segment)
                    .ok_or(PatchError::PathNotFound(path_so_far))?,
                Self::Array(arr) => {
                    let index: usize = segment.parse().map_err(|_| {
                        PatchError::InvalidPath(format!("Invalid array index: {segment}"))
                    })?;
                    arr.get(index).ok_or(PatchError::IndexOutOfBounds {
                        path: path_so_far,
                        index,
                        len: arr.len(),
                    })?
                }
                _ => {
                    return Err(PatchError::InvalidTarget {
                        path: path_so_far,
                        expected: "object or array".to_string(),
                        found: format!("{:?}", current.value_kind()),
                    });
                }
            };
        }

        Ok(current)
    }

    fn set_at_path(&mut self, path: &str, value: Self) -> Result<(), PatchError> {
        if path.is_empty() {
            *self = value;
            return Ok(());
        }

        let (parent_path, key) = split_parent_key(path)?;
        let parent = if parent_path.is_empty() {
            self
        } else {
            self.get_mut_at_path(&parent_path)?
        };

        match parent {
            Self::Object(map) => {
                map.insert(key, value);
                Ok(())
            }
            Self::Array(arr) => {
                let index: usize = key
                    .parse()
                    .map_err(|_| PatchError::InvalidPath(format!("Invalid array index: {key}")))?;
                while arr.len() <= index {
                    arr.push(Self::Null);
                }
                arr[index] = value;
                Ok(())
            }
            _ => Err(PatchError::InvalidTarget {
                path: parent_path,
                expected: "object or array".to_string(),
                found: format!("{:?}", parent.value_kind()),
            }),
        }
    }

    fn remove_at_path(&mut self, path: &str) -> Result<Self, PatchError> {
        if path.is_empty() {
            return Err(PatchError::CannotRemoveRoot);
        }

        let (parent_path, key) = split_parent_key(path)?;
        let parent = if parent_path.is_empty() {
            self
        } else {
            self.get_mut_at_path(&parent_path)?
        };

        match parent {
            Self::Object(map) => map
                .remove(&key)
                .ok_or_else(|| PatchError::PathNotFound(path.to_string())),
            Self::Array(arr) => {
                let index: usize = key
                    .parse()
                    .map_err(|_| PatchError::InvalidPath(format!("Invalid array index: {key}")))?;
                if index >= arr.len() {
                    return Err(PatchError::IndexOutOfBounds {
                        path: path.to_string(),
                        index,
                        len: arr.len(),
                    });
                }
                Ok(arr.remove(index))
            }
            _ => Err(PatchError::InvalidTarget {
                path: parent_path,
                expected: "object or array".to_string(),
                found: format!("{:?}", parent.value_kind()),
            }),
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Apply a patch to a value
///
/// Convenience function that calls `value.apply_patch(patch)`.
///
/// # Errors
///
/// Returns an error if any operation fails.
pub fn apply_patch<V: Patchable>(value: &mut V, patch: &GenericPatch<V>) -> Result<(), PatchError> {
    value.apply_patch(patch)
}

/// Apply a single operation to a value
///
/// Convenience function that calls `value.apply_operation(op)`.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn apply_operation<V: Patchable>(
    value: &mut V,
    op: &GenericPatchOperation<V>,
) -> Result<(), PatchError> {
    value.apply_operation(op)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_pointer_empty() {
        let segments = parse_pointer("").unwrap();
        assert!(segments.is_empty());
    }

    #[test]
    fn test_parse_pointer_root() {
        let segments = parse_pointer("/").unwrap();
        assert_eq!(segments, vec![""]);
    }

    #[test]
    fn test_parse_pointer_simple() {
        let segments = parse_pointer("/foo/bar").unwrap();
        assert_eq!(segments, vec!["foo", "bar"]);
    }

    #[test]
    fn test_parse_pointer_escaped() {
        let segments = parse_pointer("/a~1b/c~0d").unwrap();
        assert_eq!(segments, vec!["a/b", "c~d"]);
    }

    #[test]
    fn test_parse_pointer_invalid() {
        assert!(parse_pointer("no-leading-slash").is_err());
    }

    #[test]
    fn test_get_at_path_simple() {
        let value = json!({"name": "Alice"});
        let name = value.get_at_path("/name").unwrap();
        assert_eq!(name, &json!("Alice"));
    }

    #[test]
    fn test_get_at_path_nested() {
        let value = json!({"user": {"name": "Alice"}});
        let name = value.get_at_path("/user/name").unwrap();
        assert_eq!(name, &json!("Alice"));
    }

    #[test]
    fn test_get_at_path_array() {
        let value = json!([1, 2, 3]);
        assert_eq!(value.get_at_path("/0").unwrap(), &json!(1));
        assert_eq!(value.get_at_path("/2").unwrap(), &json!(3));
    }

    #[test]
    fn test_get_at_path_not_found() {
        let value = json!({"name": "Alice"});
        assert!(matches!(
            value.get_at_path("/age"),
            Err(PatchError::PathNotFound(_))
        ));
    }

    #[test]
    fn test_apply_add_to_object() {
        let mut value = json!({"name": "Alice"});
        let op = GenericPatchOperation::Add {
            path: "/age".to_string(),
            value: json!(30),
        };

        value.apply_operation(&op).unwrap();
        assert_eq!(value["age"], json!(30));
    }

    #[test]
    fn test_apply_add_to_array() {
        let mut value = json!([1, 2]);
        let op = GenericPatchOperation::Add {
            path: "/-".to_string(),
            value: json!(3),
        };

        value.apply_operation(&op).unwrap();
        assert_eq!(value, json!([1, 2, 3]));
    }

    #[test]
    fn test_apply_add_to_array_at_index() {
        let mut value = json!([1, 3]);
        let op = GenericPatchOperation::Add {
            path: "/1".to_string(),
            value: json!(2),
        };

        value.apply_operation(&op).unwrap();
        assert_eq!(value, json!([1, 2, 3]));
    }

    #[test]
    fn test_apply_remove_from_object() {
        let mut value = json!({"name": "Alice", "age": 30});
        let op = GenericPatchOperation::Remove {
            path: "/age".to_string(),
        };

        value.apply_operation(&op).unwrap();
        assert_eq!(value, json!({"name": "Alice"}));
    }

    #[test]
    fn test_apply_remove_from_array() {
        let mut value = json!([1, 2, 3]);
        let op = GenericPatchOperation::Remove {
            path: "/1".to_string(),
        };

        value.apply_operation(&op).unwrap();
        assert_eq!(value, json!([1, 3]));
    }

    #[test]
    fn test_apply_replace() {
        let mut value = json!({"name": "Alice"});
        let op = GenericPatchOperation::Replace {
            path: "/name".to_string(),
            value: json!("Bob"),
        };

        value.apply_operation(&op).unwrap();
        assert_eq!(value["name"], json!("Bob"));
    }

    #[test]
    fn test_apply_replace_root() {
        let mut value = json!({"old": "data"});
        let op = GenericPatchOperation::Replace {
            path: String::new(),
            value: json!({"new": "data"}),
        };

        value.apply_operation(&op).unwrap();
        assert_eq!(value, json!({"new": "data"}));
    }

    #[test]
    fn test_apply_move() {
        let mut value = json!({"first": "Alice", "last": "Smith"});
        let op = GenericPatchOperation::Move {
            from: "/first".to_string(),
            path: "/name".to_string(),
        };

        value.apply_operation(&op).unwrap();
        assert!(value.get("first").is_none());
        assert_eq!(value["name"], json!("Alice"));
    }

    #[test]
    fn test_apply_copy() {
        let mut value = json!({"name": "Alice"});
        let op = GenericPatchOperation::Copy {
            from: "/name".to_string(),
            path: "/alias".to_string(),
        };

        value.apply_operation(&op).unwrap();
        assert_eq!(value["name"], json!("Alice"));
        assert_eq!(value["alias"], json!("Alice"));
    }

    #[test]
    fn test_apply_test_success() {
        let mut value = json!({"name": "Alice"});
        let op = GenericPatchOperation::Test {
            path: "/name".to_string(),
            value: json!("Alice"),
        };

        assert!(value.apply_operation(&op).is_ok());
    }

    #[test]
    fn test_apply_test_failure() {
        let mut value = json!({"name": "Alice"});
        let op = GenericPatchOperation::Test {
            path: "/name".to_string(),
            value: json!("Bob"),
        };

        assert!(matches!(
            value.apply_operation(&op),
            Err(PatchError::TestFailed { .. })
        ));
    }

    #[test]
    fn test_apply_patch() {
        let mut value = json!({"name": "Alice"});
        let patch = GenericPatch::with_operations(vec![
            GenericPatchOperation::Add {
                path: "/age".to_string(),
                value: json!(30),
            },
            GenericPatchOperation::Replace {
                path: "/name".to_string(),
                value: json!("Alice Smith"),
            },
        ]);

        apply_patch(&mut value, &patch).unwrap();
        assert_eq!(value["name"], json!("Alice Smith"));
        assert_eq!(value["age"], json!(30));
    }

    #[test]
    fn test_remove_root_error() {
        let mut value = json!({"name": "Alice"});
        let op: GenericPatchOperation<serde_json::Value> = GenericPatchOperation::Remove {
            path: String::new(),
        };

        assert!(matches!(
            value.apply_operation(&op),
            Err(PatchError::CannotRemoveRoot)
        ));
    }

    #[test]
    fn test_set_at_path() {
        let mut value = json!({"user": {}});
        value.set_at_path("/user/name", json!("Alice")).unwrap();
        assert_eq!(value["user"]["name"], json!("Alice"));
    }

    #[test]
    fn test_remove_at_path() {
        let mut value = json!({"name": "Alice", "age": 30});
        let removed = value.remove_at_path("/age").unwrap();
        assert_eq!(removed, json!(30));
        assert!(value.get("age").is_none());
    }
}
