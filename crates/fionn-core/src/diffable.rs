// SPDX-License-Identifier: MIT OR Apache-2.0
//! Format-agnostic diff and patch traits
//!
//! This module provides traits for computing diffs and applying patches
//! across different data formats. The [`DiffableValue`] trait enables
//! structural comparison, while [`GenericPatch`] represents format-agnostic
//! patch operations.
//!
//! # Key Types
//!
//! - [`DiffableValue`] - Trait for values that can be compared for diffing
//! - [`DiffValueKind`] - Classification of value types for diff operations
//! - [`GenericPatchOperation`] - A single patch operation
//! - [`GenericPatch`] - A collection of patch operations
//!
//! # Example
//!
//! ```ignore
//! use fionn_core::diffable::{DiffableValue, compute_diff};
//!
//! let source: serde_json::Value = serde_json::json!({"name": "Alice"});
//! let target: serde_json::Value = serde_json::json!({"name": "Bob"});
//!
//! let patch = compute_diff(&source, &target);
//! ```

use std::fmt;

// ============================================================================
// DiffValueKind - Value type classification
// ============================================================================

/// Classification of value types for diff operations
///
/// This enum provides a uniform way to identify value types across
/// different data formats, enabling type-aware diff algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiffValueKind {
    /// Null/nil/none value
    Null,
    /// Boolean value
    Bool,
    /// Numeric value (integer or float)
    Number,
    /// String value
    String,
    /// Array/sequence container
    Array,
    /// Object/map container
    Object,
}

impl DiffValueKind {
    /// Check if this is a scalar type
    #[must_use]
    pub const fn is_scalar(self) -> bool {
        matches!(self, Self::Null | Self::Bool | Self::Number | Self::String)
    }

    /// Check if this is a container type
    #[must_use]
    pub const fn is_container(self) -> bool {
        matches!(self, Self::Array | Self::Object)
    }
}

impl fmt::Display for DiffValueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => write!(f, "null"),
            Self::Bool => write!(f, "boolean"),
            Self::Number => write!(f, "number"),
            Self::String => write!(f, "string"),
            Self::Array => write!(f, "array"),
            Self::Object => write!(f, "object"),
        }
    }
}

// ============================================================================
// DiffableValue Trait
// ============================================================================

/// Trait for values that can be compared for diffing
///
/// This trait provides the interface needed by diff algorithms to:
/// - Compare values for equality
/// - Determine value types
/// - Navigate container contents
///
/// # Implementation Notes
///
/// - `equals()` should perform deep equality comparison
/// - `as_object()` returns an iterator over (key, value) pairs
/// - `as_array()` returns a slice of elements
pub trait DiffableValue: Clone + PartialEq {
    /// Check deep equality with another value
    fn equals(&self, other: &Self) -> bool;

    /// Get the value type classification
    fn value_kind(&self) -> DiffValueKind;

    /// Get object entries as an iterator if this is an object
    ///
    /// Returns `None` if this is not an object.
    fn as_object(&self) -> Option<Box<dyn Iterator<Item = (&str, &Self)> + '_>>;

    /// Get object keys as a vector if this is an object
    fn object_keys(&self) -> Option<Vec<&str>> {
        self.as_object().map(|iter| iter.map(|(k, _)| k).collect())
    }

    /// Get a field value from an object
    fn get_field(&self, key: &str) -> Option<&Self>;

    /// Get array elements as a slice if this is an array
    fn as_array(&self) -> Option<&[Self]>
    where
        Self: Sized;

    /// Get array length if this is an array
    fn array_len(&self) -> Option<usize> {
        self.as_array().map(<[Self]>::len)
    }

    /// Get an array element by index
    fn get_element(&self, index: usize) -> Option<&Self>
    where
        Self: Sized,
    {
        self.as_array().and_then(|arr| arr.get(index))
    }

    /// Get as string reference if this is a string
    fn as_str(&self) -> Option<&str>;

    /// Get as boolean if this is a boolean
    fn as_bool(&self) -> Option<bool>;

    /// Get as i64 if this is a number
    fn as_i64(&self) -> Option<i64>;

    /// Get as f64 if this is a number
    fn as_f64(&self) -> Option<f64>;

    /// Create a deep clone of this value
    #[must_use]
    fn deep_clone(&self) -> Self
    where
        Self: Sized,
    {
        self.clone()
    }

    /// Check if this is a null value
    fn is_null(&self) -> bool {
        matches!(self.value_kind(), DiffValueKind::Null)
    }

    /// Check if this is an object
    fn is_object(&self) -> bool {
        matches!(self.value_kind(), DiffValueKind::Object)
    }

    /// Check if this is an array
    fn is_array(&self) -> bool {
        matches!(self.value_kind(), DiffValueKind::Array)
    }
}

// ============================================================================
// GenericPatchOperation
// ============================================================================

/// A single patch operation (format-agnostic)
///
/// Modeled after RFC 6902 (JSON Patch) but generic over value types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenericPatchOperation<V: DiffableValue> {
    /// Add a value at a path
    Add {
        /// The target path
        path: String,
        /// The value to add
        value: V,
    },
    /// Remove the value at a path
    Remove {
        /// The path to remove
        path: String,
    },
    /// Replace the value at a path
    Replace {
        /// The target path
        path: String,
        /// The new value
        value: V,
    },
    /// Move a value from one path to another
    Move {
        /// The source path
        from: String,
        /// The destination path
        path: String,
    },
    /// Copy a value from one path to another
    Copy {
        /// The source path
        from: String,
        /// The destination path
        path: String,
    },
    /// Test that a value at a path equals the expected value
    Test {
        /// The path to test
        path: String,
        /// The expected value
        value: V,
    },
}

impl<V: DiffableValue> GenericPatchOperation<V> {
    /// Get the target path of this operation
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

    /// Get the source path for move/copy operations
    #[must_use]
    pub fn from_path(&self) -> Option<&str> {
        match self {
            Self::Move { from, .. } | Self::Copy { from, .. } => Some(from),
            _ => None,
        }
    }

    /// Check if this is an add operation
    #[must_use]
    pub const fn is_add(&self) -> bool {
        matches!(self, Self::Add { .. })
    }

    /// Check if this is a remove operation
    #[must_use]
    pub const fn is_remove(&self) -> bool {
        matches!(self, Self::Remove { .. })
    }

    /// Check if this is a replace operation
    #[must_use]
    pub const fn is_replace(&self) -> bool {
        matches!(self, Self::Replace { .. })
    }
}

impl<V: DiffableValue + fmt::Debug> fmt::Display for GenericPatchOperation<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Add { path, value } => write!(f, "add({path}, {value:?})"),
            Self::Remove { path } => write!(f, "remove({path})"),
            Self::Replace { path, value } => write!(f, "replace({path}, {value:?})"),
            Self::Move { from, path } => write!(f, "move({from} -> {path})"),
            Self::Copy { from, path } => write!(f, "copy({from} -> {path})"),
            Self::Test { path, value } => write!(f, "test({path}, {value:?})"),
        }
    }
}

// ============================================================================
// GenericPatch
// ============================================================================

/// A collection of patch operations (format-agnostic)
///
/// Represents a complete diff between two values as a sequence of operations.
#[derive(Debug, Clone, Default)]
pub struct GenericPatch<V: DiffableValue> {
    /// The operations in this patch
    pub operations: Vec<GenericPatchOperation<V>>,
}

impl<V: DiffableValue> GenericPatch<V> {
    /// Create a new empty patch
    #[must_use]
    pub const fn new() -> Self {
        Self {
            operations: Vec::new(),
        }
    }

    /// Create a patch with the given operations
    #[must_use]
    pub const fn with_operations(operations: Vec<GenericPatchOperation<V>>) -> Self {
        Self { operations }
    }

    /// Add an operation to this patch
    pub fn push(&mut self, op: GenericPatchOperation<V>) {
        self.operations.push(op);
    }

    /// Check if this patch is empty
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    /// Get the number of operations
    #[must_use]
    pub const fn len(&self) -> usize {
        self.operations.len()
    }

    /// Iterate over operations
    pub fn iter(&self) -> impl Iterator<Item = &GenericPatchOperation<V>> {
        self.operations.iter()
    }

    /// Get operations that affect a specific path prefix
    #[must_use]
    pub fn operations_at_prefix(&self, prefix: &str) -> Vec<&GenericPatchOperation<V>> {
        self.operations
            .iter()
            .filter(|op| op.path().starts_with(prefix))
            .collect()
    }
}

impl<V: DiffableValue> FromIterator<GenericPatchOperation<V>> for GenericPatch<V> {
    fn from_iter<I: IntoIterator<Item = GenericPatchOperation<V>>>(iter: I) -> Self {
        Self {
            operations: iter.into_iter().collect(),
        }
    }
}

impl<V: DiffableValue> IntoIterator for GenericPatch<V> {
    type Item = GenericPatchOperation<V>;
    type IntoIter = std::vec::IntoIter<GenericPatchOperation<V>>;

    fn into_iter(self) -> Self::IntoIter {
        self.operations.into_iter()
    }
}

// ============================================================================
// DiffableValue for serde_json::Value
// ============================================================================

impl DiffableValue for serde_json::Value {
    fn equals(&self, other: &Self) -> bool {
        self == other
    }

    fn value_kind(&self) -> DiffValueKind {
        match self {
            Self::Null => DiffValueKind::Null,
            Self::Bool(_) => DiffValueKind::Bool,
            Self::Number(_) => DiffValueKind::Number,
            Self::String(_) => DiffValueKind::String,
            Self::Array(_) => DiffValueKind::Array,
            Self::Object(_) => DiffValueKind::Object,
        }
    }

    fn as_object(&self) -> Option<Box<dyn Iterator<Item = (&str, &Self)> + '_>> {
        match self {
            Self::Object(map) => Some(Box::new(map.iter().map(|(k, v)| (k.as_str(), v)))),
            _ => None,
        }
    }

    fn get_field(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(map) => map.get(key),
            _ => None,
        }
    }

    fn as_array(&self) -> Option<&[Self]> {
        match self {
            Self::Array(arr) => Some(arr.as_slice()),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        self.as_str()
    }

    fn as_bool(&self) -> Option<bool> {
        self.as_bool()
    }

    fn as_i64(&self) -> Option<i64> {
        self.as_i64()
    }

    fn as_f64(&self) -> Option<f64> {
        self.as_f64()
    }
}

// ============================================================================
// Diff Algorithm Helpers
// ============================================================================

/// Options for diff computation
#[derive(Debug, Clone, Default)]
pub struct DiffOptions {
    /// Include paths to arrays that were reordered
    pub detect_reorder: bool,
    /// Use move operations for object field renames
    pub detect_moves: bool,
    /// Maximum depth to diff (0 = unlimited)
    pub max_depth: usize,
}

impl DiffOptions {
    /// Create default diff options
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable move detection
    #[must_use]
    pub const fn with_move_detection(mut self) -> Self {
        self.detect_moves = true;
        self
    }

    /// Enable reorder detection
    #[must_use]
    pub const fn with_reorder_detection(mut self) -> Self {
        self.detect_reorder = true;
        self
    }

    /// Set maximum diff depth
    #[must_use]
    pub const fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }
}

/// Compute a diff between two values
///
/// This is a generic diff algorithm that works with any [`DiffableValue`].
#[must_use]
pub fn compute_diff<V: DiffableValue>(source: &V, target: &V) -> GenericPatch<V> {
    compute_diff_with_options(source, target, &DiffOptions::default())
}

/// Compute a diff with custom options
#[must_use]
pub fn compute_diff_with_options<V: DiffableValue>(
    source: &V,
    target: &V,
    options: &DiffOptions,
) -> GenericPatch<V> {
    let mut patch = GenericPatch::new();
    diff_values(source, target, "", &mut patch, options, 0);
    patch
}

fn diff_values<V: DiffableValue>(
    source: &V,
    target: &V,
    current_path: &str,
    result: &mut GenericPatch<V>,
    options: &DiffOptions,
    depth: usize,
) {
    // Check depth limit
    if options.max_depth > 0 && depth >= options.max_depth {
        if !source.equals(target) {
            result.push(GenericPatchOperation::Replace {
                path: current_path.to_string(),
                value: target.deep_clone(),
            });
        }
        return;
    }

    // Fast path: equal values
    if source.equals(target) {
        return;
    }

    // Different types: replace
    if source.value_kind() != target.value_kind() {
        result.push(GenericPatchOperation::Replace {
            path: current_path.to_string(),
            value: target.deep_clone(),
        });
        return;
    }

    // Same type, different values
    match (source.value_kind(), target.value_kind()) {
        (DiffValueKind::Object, DiffValueKind::Object) => {
            diff_objects(source, target, current_path, result, options, depth);
        }
        (DiffValueKind::Array, DiffValueKind::Array) => {
            diff_arrays(source, target, current_path, result, options, depth);
        }
        _ => {
            // Scalar types: just replace
            result.push(GenericPatchOperation::Replace {
                path: current_path.to_string(),
                value: target.deep_clone(),
            });
        }
    }
}

fn diff_objects<V: DiffableValue>(
    source: &V,
    target: &V,
    current_path: &str,
    result: &mut GenericPatch<V>,
    options: &DiffOptions,
    depth: usize,
) {
    let src_keys: std::collections::HashSet<_> = source
        .object_keys()
        .map(|keys| keys.into_iter().collect())
        .unwrap_or_default();
    let tgt_keys: std::collections::HashSet<_> = target
        .object_keys()
        .map(|keys| keys.into_iter().collect())
        .unwrap_or_default();

    // Removed keys
    for key in &src_keys {
        if !tgt_keys.contains(key) {
            let field_path = format_path(current_path, key);
            result.push(GenericPatchOperation::Remove { path: field_path });
        }
    }

    // Added and modified keys
    for key in &tgt_keys {
        let field_path = format_path(current_path, key);

        if src_keys.contains(key) {
            // Modified (recursively diff)
            if let (Some(src_val), Some(tgt_val)) = (source.get_field(key), target.get_field(key)) {
                diff_values(src_val, tgt_val, &field_path, result, options, depth + 1);
            }
        } else {
            // Added
            if let Some(val) = target.get_field(key) {
                result.push(GenericPatchOperation::Add {
                    path: field_path,
                    value: val.deep_clone(),
                });
            }
        }
    }
}

fn diff_arrays<V: DiffableValue>(
    source: &V,
    target: &V,
    current_path: &str,
    result: &mut GenericPatch<V>,
    options: &DiffOptions,
    depth: usize,
) {
    let src_arr = source.as_array().unwrap_or(&[]);
    let tgt_arr = target.as_array().unwrap_or(&[]);

    let src_len = src_arr.len();
    let tgt_len = tgt_arr.len();

    // Simple element-by-element diff
    let min_len = src_len.min(tgt_len);

    // Diff common elements
    for (idx, (src_elem, tgt_elem)) in src_arr.iter().zip(tgt_arr.iter()).take(min_len).enumerate()
    {
        let elem_path = format!("{current_path}/{idx}");
        diff_values(src_elem, tgt_elem, &elem_path, result, options, depth + 1);
    }

    // Handle length differences
    if tgt_len > src_len {
        // Elements added
        for (idx, elem) in tgt_arr.iter().enumerate().skip(src_len) {
            let elem_path = format!("{current_path}/{idx}");
            result.push(GenericPatchOperation::Add {
                path: elem_path,
                value: elem.deep_clone(),
            });
        }
    } else if src_len > tgt_len {
        // Elements removed (remove from end first)
        for idx in (tgt_len..src_len).rev() {
            let elem_path = format!("{current_path}/{idx}");
            result.push(GenericPatchOperation::Remove { path: elem_path });
        }
    }
}

fn format_path(base: &str, key: &str) -> String {
    if base.is_empty() {
        format!("/{}", escape_json_pointer(key))
    } else {
        format!("{base}/{}", escape_json_pointer(key))
    }
}

fn escape_json_pointer(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_diff_value_kind() {
        assert!(DiffValueKind::Null.is_scalar());
        assert!(DiffValueKind::Bool.is_scalar());
        assert!(DiffValueKind::Number.is_scalar());
        assert!(DiffValueKind::String.is_scalar());
        assert!(!DiffValueKind::Array.is_scalar());
        assert!(!DiffValueKind::Object.is_scalar());

        assert!(DiffValueKind::Array.is_container());
        assert!(DiffValueKind::Object.is_container());
    }

    #[test]
    fn test_diffable_value_json() {
        let val = json!({"name": "Alice", "age": 30});

        assert_eq!(val.value_kind(), DiffValueKind::Object);
        assert!(val.is_object());

        let keys = val.object_keys().unwrap();
        assert!(keys.contains(&"name"));
        assert!(keys.contains(&"age"));

        assert_eq!(val.get_field("name"), Some(&json!("Alice")));
    }

    #[test]
    fn test_diffable_value_array() {
        let val = json!([1, 2, 3]);

        assert_eq!(val.value_kind(), DiffValueKind::Array);
        assert!(val.is_array());
        assert_eq!(val.array_len(), Some(3));
        assert_eq!(val.get_element(0), Some(&json!(1)));
    }

    #[test]
    fn test_compute_diff_equal() {
        let source = json!({"name": "Alice"});
        let target = json!({"name": "Alice"});

        let patch = compute_diff(&source, &target);
        assert!(patch.is_empty());
    }

    #[test]
    fn test_compute_diff_replace_scalar() {
        let source = json!({"name": "Alice"});
        let target = json!({"name": "Bob"});

        let patch = compute_diff(&source, &target);
        assert_eq!(patch.len(), 1);
        assert!(
            matches!(&patch.operations[0], GenericPatchOperation::Replace { path, .. } if path == "/name")
        );
    }

    #[test]
    fn test_compute_diff_add_field() {
        let source = json!({"name": "Alice"});
        let target = json!({"name": "Alice", "age": 30});

        let patch = compute_diff(&source, &target);
        assert_eq!(patch.len(), 1);
        assert!(
            matches!(&patch.operations[0], GenericPatchOperation::Add { path, .. } if path == "/age")
        );
    }

    #[test]
    fn test_compute_diff_remove_field() {
        let source = json!({"name": "Alice", "age": 30});
        let target = json!({"name": "Alice"});

        let patch = compute_diff(&source, &target);
        assert_eq!(patch.len(), 1);
        assert!(
            matches!(&patch.operations[0], GenericPatchOperation::Remove { path } if path == "/age")
        );
    }

    #[test]
    fn test_compute_diff_array_add() {
        let source = json!([1, 2]);
        let target = json!([1, 2, 3]);

        let patch = compute_diff(&source, &target);
        assert_eq!(patch.len(), 1);
        assert!(
            matches!(&patch.operations[0], GenericPatchOperation::Add { path, .. } if path == "/2")
        );
    }

    #[test]
    fn test_compute_diff_array_remove() {
        let source = json!([1, 2, 3]);
        let target = json!([1, 2]);

        let patch = compute_diff(&source, &target);
        assert_eq!(patch.len(), 1);
        assert!(
            matches!(&patch.operations[0], GenericPatchOperation::Remove { path } if path == "/2")
        );
    }

    #[test]
    fn test_compute_diff_type_change() {
        let source = json!({"value": "string"});
        let target = json!({"value": 42});

        let patch = compute_diff(&source, &target);
        assert_eq!(patch.len(), 1);
        assert!(patch.operations[0].is_replace());
    }

    #[test]
    fn test_compute_diff_nested() {
        let source = json!({"user": {"name": "Alice"}});
        let target = json!({"user": {"name": "Bob"}});

        let patch = compute_diff(&source, &target);
        assert_eq!(patch.len(), 1);
        assert!(
            matches!(&patch.operations[0], GenericPatchOperation::Replace { path, .. } if path == "/user/name")
        );
    }

    #[test]
    fn test_generic_patch_operations() {
        let op: GenericPatchOperation<serde_json::Value> = GenericPatchOperation::Add {
            path: "/test".to_string(),
            value: json!(42),
        };

        assert_eq!(op.path(), "/test");
        assert!(op.is_add());
        assert!(!op.is_remove());
    }

    #[test]
    fn test_generic_patch_iter() {
        let patch = GenericPatch::with_operations(vec![
            GenericPatchOperation::Add {
                path: "/a".to_string(),
                value: json!(1),
            },
            GenericPatchOperation::Add {
                path: "/b".to_string(),
                value: json!(2),
            },
        ]);

        assert_eq!(patch.len(), 2);
        assert!(!patch.is_empty());

        let paths: Vec<_> = patch.iter().map(GenericPatchOperation::path).collect();
        assert_eq!(paths, vec!["/a", "/b"]);
    }

    #[test]
    fn test_diff_options() {
        let options = DiffOptions::new().with_move_detection().with_max_depth(5);

        assert!(options.detect_moves);
        assert_eq!(options.max_depth, 5);
    }

    #[test]
    fn test_escape_json_pointer() {
        assert_eq!(escape_json_pointer("simple"), "simple");
        assert_eq!(escape_json_pointer("with/slash"), "with~1slash");
        assert_eq!(escape_json_pointer("with~tilde"), "with~0tilde");
    }
}
