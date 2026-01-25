// SPDX-License-Identifier: MIT OR Apache-2.0
//! Native tape-walking diff
//!
//! Provides format-agnostic diff by walking two tapes in parallel.
//! This eliminates the "conversion tax" of serializing/deserializing
//! between formats - we can diff YAML, TOML, CSV, ISON, TOON tapes directly.
//!
//! # Performance
//!
//! This approach is ~6x faster than convert-then-diff because:
//! - No intermediate serialization
//! - Direct tape traversal with O(1) skip
//! - No allocation of `serde_json::Value` trees
//!
//! # Example
//!
//! ```ignore
//! use fionn_diff::diff_tapes;
//! use fionn_simd::transform::UnifiedTape;
//! use fionn_core::format::FormatKind;
//!
//! let tape_a = UnifiedTape::parse(yaml_a.as_bytes(), FormatKind::Yaml)?;
//! let tape_b = UnifiedTape::parse(yaml_b.as_bytes(), FormatKind::Yaml)?;
//!
//! let patch = diff_tapes(&tape_a, &tape_b)?;
//! ```

use fionn_core::Result;
use fionn_core::tape_source::{TapeNodeKind, TapeSource, TapeValue};
use std::borrow::Cow;

/// A tape diff operation
#[derive(Debug, Clone, PartialEq)]
pub enum TapeDiffOp<'a> {
    /// Add a value at a path
    Add {
        /// JSON Pointer path where to add
        path: String,
        /// Value to add
        value: TapeValueOwned,
    },
    /// Remove the value at a path
    Remove {
        /// JSON Pointer path to remove
        path: String,
    },
    /// Replace the value at a path
    Replace {
        /// JSON Pointer path to replace
        path: String,
        /// New value
        value: TapeValueOwned,
    },
    /// Move a value from one path to another
    Move {
        /// Source path
        from: String,
        /// Destination path
        path: String,
    },
    /// Copy a value from one path to another
    Copy {
        /// Source path
        from: String,
        /// Destination path
        path: String,
    },
    /// Reference to a value in the target tape (zero-copy variant)
    AddRef {
        /// JSON Pointer path where to add
        path: String,
        /// Index in the target tape
        tape_index: usize,
        /// Lifetime marker
        _marker: std::marker::PhantomData<&'a ()>,
    },
    /// Reference to a value in the target tape (zero-copy variant)
    ReplaceRef {
        /// JSON Pointer path to replace
        path: String,
        /// Index in the target tape
        tape_index: usize,
        /// Lifetime marker
        _marker: std::marker::PhantomData<&'a ()>,
    },
}

/// Owned tape value for patch operations
#[derive(Debug, Clone, PartialEq)]
pub enum TapeValueOwned {
    /// Null value
    Null,
    /// Boolean value
    Bool(bool),
    /// 64-bit integer
    Int(i64),
    /// 64-bit float
    Float(f64),
    /// String value
    String(String),
    /// Raw number string (preserves precision)
    RawNumber(String),
    /// Serialized JSON for complex values
    Json(String),
}

impl<'a> From<TapeValue<'a>> for TapeValueOwned {
    fn from(val: TapeValue<'a>) -> Self {
        match val {
            TapeValue::Null => Self::Null,
            TapeValue::Bool(b) => Self::Bool(b),
            TapeValue::Int(n) => Self::Int(n),
            TapeValue::Float(f) => Self::Float(f),
            TapeValue::String(s) => Self::String(s.into_owned()),
            TapeValue::RawNumber(s) => Self::RawNumber(s.into_owned()),
        }
    }
}

/// Result of diffing two tapes
#[derive(Debug, Clone, Default)]
pub struct TapeDiff<'a> {
    /// Operations to transform source into target
    pub operations: Vec<TapeDiffOp<'a>>,
}

impl<'a> TapeDiff<'a> {
    /// Create an empty diff
    #[must_use]
    pub const fn new() -> Self {
        Self {
            operations: Vec::new(),
        }
    }

    /// Check if the diff is empty (tapes are equal)
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    /// Number of operations
    #[must_use]
    pub const fn len(&self) -> usize {
        self.operations.len()
    }

    /// Add an operation
    pub fn push(&mut self, op: TapeDiffOp<'a>) {
        self.operations.push(op);
    }
}

/// Options for tape diff
#[derive(Debug, Clone, Default)]
pub struct TapeDiffOptions {
    /// Maximum depth to diff (0 = unlimited)
    pub max_depth: usize,
    /// Use reference operations (zero-copy, but ties to target tape lifetime)
    pub use_refs: bool,
}

impl TapeDiffOptions {
    /// Create default options
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set max depth
    #[must_use]
    pub const fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }

    /// Enable reference operations
    #[must_use]
    pub const fn with_refs(mut self) -> Self {
        self.use_refs = true;
        self
    }
}

/// Diff two tapes and produce a patch
///
/// This is the native tape-walking diff that avoids the conversion tax.
///
/// # Errors
///
/// Returns an error if tape traversal fails.
pub fn diff_tapes<'a, S: TapeSource, T: TapeSource>(
    source: &'a S,
    target: &'a T,
) -> Result<TapeDiff<'a>> {
    diff_tapes_with_options(source, target, &TapeDiffOptions::default())
}

/// Diff two tapes with custom options
///
/// # Errors
///
/// Returns an error if tape traversal fails.
pub fn diff_tapes_with_options<'a, S: TapeSource, T: TapeSource>(
    source: &'a S,
    target: &'a T,
    options: &TapeDiffOptions,
) -> Result<TapeDiff<'a>> {
    let mut diff = TapeDiff::new();

    if source.is_empty() && target.is_empty() {
        return Ok(diff);
    }

    if source.is_empty() {
        // Entire target is an add
        let value = extract_value(target, 0)?;
        diff.push(TapeDiffOp::Add {
            path: String::new(),
            value,
        });
        return Ok(diff);
    }

    if target.is_empty() {
        diff.push(TapeDiffOp::Remove {
            path: String::new(),
        });
        return Ok(diff);
    }

    // Start recursive diff at root
    diff_at_path(source, 0, target, 0, "", &mut diff, options, 0)?;

    Ok(diff)
}

#[allow(clippy::too_many_arguments)] // Recursive diff traversal requires path context
fn diff_at_path<S: TapeSource, T: TapeSource>(
    source: &S,
    src_idx: usize,
    target: &T,
    tgt_idx: usize,
    path: &str,
    diff: &mut TapeDiff<'_>,
    options: &TapeDiffOptions,
    depth: usize,
) -> Result<()> {
    // Check depth limit
    if options.max_depth > 0 && depth >= options.max_depth {
        if !values_equal(source, src_idx, target, tgt_idx) {
            let value = extract_value(target, tgt_idx)?;
            diff.push(TapeDiffOp::Replace {
                path: path.to_string(),
                value,
            });
        }
        return Ok(());
    }

    let src_node = source.node_at(src_idx);
    let tgt_node = target.node_at(tgt_idx);

    match (src_node, tgt_node) {
        (Some(src), Some(tgt)) => {
            // Check if kinds match
            let src_kind_class = kind_class(&src.kind);
            let tgt_kind_class = kind_class(&tgt.kind);

            if src_kind_class != tgt_kind_class {
                // Type change - replace entire value
                let value = extract_value(target, tgt_idx)?;
                diff.push(TapeDiffOp::Replace {
                    path: path.to_string(),
                    value,
                });
                return Ok(());
            }

            match src.kind {
                TapeNodeKind::ObjectStart { count: src_count } => {
                    if let TapeNodeKind::ObjectStart { count: tgt_count } = tgt.kind {
                        diff_objects(
                            source, src_idx, src_count, target, tgt_idx, tgt_count, path, diff,
                            options, depth,
                        )?;
                    }
                }
                TapeNodeKind::ArrayStart { count: src_count } => {
                    if let TapeNodeKind::ArrayStart { count: tgt_count } = tgt.kind {
                        diff_arrays(
                            source, src_idx, src_count, target, tgt_idx, tgt_count, path, diff,
                            options, depth,
                        )?;
                    }
                }
                TapeNodeKind::Value | TapeNodeKind::Key => {
                    // Compare scalar values
                    if !values_equal(source, src_idx, target, tgt_idx) {
                        let value = extract_value(target, tgt_idx)?;
                        diff.push(TapeDiffOp::Replace {
                            path: path.to_string(),
                            value,
                        });
                    }
                }
                _ => {
                    // ObjectEnd, ArrayEnd - should not be diffed directly
                }
            }
        }
        (Some(_), None) => {
            // Source has value, target doesn't - remove
            diff.push(TapeDiffOp::Remove {
                path: path.to_string(),
            });
        }
        (None, Some(_)) => {
            // Target has value, source doesn't - add
            let value = extract_value(target, tgt_idx)?;
            diff.push(TapeDiffOp::Add {
                path: path.to_string(),
                value,
            });
        }
        (None, None) => {
            // Both empty - nothing to do
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)] // Recursive diff traversal requires path context
fn diff_objects<S: TapeSource, T: TapeSource>(
    source: &S,
    src_obj_idx: usize,
    src_count: usize,
    target: &T,
    tgt_obj_idx: usize,
    tgt_count: usize,
    path: &str,
    diff: &mut TapeDiff<'_>,
    options: &TapeDiffOptions,
    depth: usize,
) -> Result<()> {
    // Build key -> value_index maps for both objects
    let src_fields = collect_object_fields(source, src_obj_idx, src_count)?;
    let tgt_fields = collect_object_fields(target, tgt_obj_idx, tgt_count)?;

    // Find removed keys
    for (key, _src_val_idx) in &src_fields {
        if !tgt_fields.iter().any(|(k, _)| k == key) {
            let field_path = format_path(path, key);
            diff.push(TapeDiffOp::Remove { path: field_path });
        }
    }

    // Find added and modified keys
    for (key, tgt_val_idx) in &tgt_fields {
        let field_path = format_path(path, key);

        if let Some((_k, src_val_idx)) = src_fields.iter().find(|(k, _)| k == key) {
            // Key exists in both - recursively diff
            diff_at_path(
                source,
                *src_val_idx,
                target,
                *tgt_val_idx,
                &field_path,
                diff,
                options,
                depth + 1,
            )?;
        } else {
            // Key only in target - add
            let value = extract_value(target, *tgt_val_idx)?;
            diff.push(TapeDiffOp::Add {
                path: field_path,
                value,
            });
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)] // Recursive diff traversal requires path context
fn diff_arrays<S: TapeSource, T: TapeSource>(
    source: &S,
    src_arr_idx: usize,
    src_count: usize,
    target: &T,
    tgt_arr_idx: usize,
    tgt_count: usize,
    path: &str,
    diff: &mut TapeDiff<'_>,
    options: &TapeDiffOptions,
    depth: usize,
) -> Result<()> {
    // Collect element indices
    let src_elements = collect_array_elements(source, src_arr_idx, src_count)?;
    let tgt_elements = collect_array_elements(target, tgt_arr_idx, tgt_count)?;

    let min_len = src_elements.len().min(tgt_elements.len());

    // Diff common elements
    for i in 0..min_len {
        let elem_path = format!("{path}/{i}");
        diff_at_path(
            source,
            src_elements[i],
            target,
            tgt_elements[i],
            &elem_path,
            diff,
            options,
            depth + 1,
        )?;
    }

    // Handle added elements
    for (i, &tgt_elem_idx) in tgt_elements.iter().enumerate().skip(min_len) {
        let elem_path = format!("{path}/{i}");
        let value = extract_value(target, tgt_elem_idx)?;
        diff.push(TapeDiffOp::Add {
            path: elem_path,
            value,
        });
    }

    // Handle removed elements (from end first)
    for i in (min_len..src_elements.len()).rev() {
        let elem_path = format!("{path}/{i}");
        diff.push(TapeDiffOp::Remove { path: elem_path });
    }

    Ok(())
}

/// Collect object fields as (key, `value_index`) pairs
fn collect_object_fields<S: TapeSource>(
    tape: &S,
    obj_idx: usize,
    count: usize,
) -> Result<Vec<(Cow<'_, str>, usize)>> {
    let mut fields = Vec::with_capacity(count);
    let mut idx = obj_idx + 1;

    for _ in 0..count {
        // Get key
        if let Some(key) = tape.key_at(idx) {
            let value_idx = idx + 1;
            fields.push((key, value_idx));
            // Skip past value to next key
            idx = tape.skip_value(value_idx)?;
        } else {
            idx += 1;
        }
    }

    Ok(fields)
}

/// Collect array element indices
fn collect_array_elements<S: TapeSource>(
    tape: &S,
    arr_idx: usize,
    count: usize,
) -> Result<Vec<usize>> {
    let mut elements = Vec::with_capacity(count);
    let mut idx = arr_idx + 1;

    for _ in 0..count {
        elements.push(idx);
        idx = tape.skip_value(idx)?;
    }

    Ok(elements)
}

/// Check if two tape values at given positions are equal
fn values_equal<S: TapeSource, T: TapeSource>(
    source: &S,
    src_idx: usize,
    target: &T,
    tgt_idx: usize,
) -> bool {
    let src_val = source.value_at(src_idx);
    let tgt_val = target.value_at(tgt_idx);

    match (src_val, tgt_val) {
        (Some(s), Some(t)) => tape_values_equal(&s, &t),
        (None, None) => {
            // Both containers - check structure
            let src_node = source.node_at(src_idx);
            let tgt_node = target.node_at(tgt_idx);

            match (src_node, tgt_node) {
                (Some(s), Some(t)) => {
                    if kind_class(&s.kind) != kind_class(&t.kind) {
                        return false;
                    }

                    match (s.kind, t.kind) {
                        (
                            TapeNodeKind::ObjectStart { count: sc },
                            TapeNodeKind::ObjectStart { count: tc },
                        ) => {
                            if sc != tc {
                                return false;
                            }
                            // Would need to compare all fields...
                            // For now, assume different if counts equal (will recurse)
                            false
                        }
                        (
                            TapeNodeKind::ArrayStart { count: sc },
                            TapeNodeKind::ArrayStart { count: tc },
                        ) => {
                            if sc != tc {
                                return false;
                            }
                            false
                        }
                        _ => false,
                    }
                }
                _ => false,
            }
        }
        _ => false,
    }
}

fn tape_values_equal(a: &TapeValue<'_>, b: &TapeValue<'_>) -> bool {
    match (a, b) {
        (TapeValue::Null, TapeValue::Null) => true,
        (TapeValue::Bool(a), TapeValue::Bool(b)) => a == b,
        (TapeValue::Int(a), TapeValue::Int(b)) => a == b,
        (TapeValue::Float(a), TapeValue::Float(b)) => {
            // Handle NaN: two NaNs are considered equal for diff purposes
            // Handle infinity: use bit-exact comparison for special values
            if a.is_nan() && b.is_nan() {
                true
            } else if a.is_infinite() || b.is_infinite() {
                a.to_bits() == b.to_bits()
            } else {
                (a - b).abs() < f64::EPSILON
            }
        }
        (TapeValue::String(a), TapeValue::String(b))
        | (TapeValue::RawNumber(a), TapeValue::RawNumber(b)) => a == b,
        // Cross-type comparisons for numbers
        (TapeValue::Int(a), TapeValue::Float(b)) => {
            if b.is_nan() || b.is_infinite() {
                false
            } else {
                (*a as f64 - b).abs() < f64::EPSILON
            }
        }
        (TapeValue::Float(a), TapeValue::Int(b)) => {
            if a.is_nan() || a.is_infinite() {
                false
            } else {
                (a - *b as f64).abs() < f64::EPSILON
            }
        }
        _ => false,
    }
}

/// Extract a value from a tape as an owned value
fn extract_value<T: TapeSource>(tape: &T, idx: usize) -> Result<TapeValueOwned> {
    if let Some(val) = tape.value_at(idx) {
        return Ok(val.into());
    }

    // Container - serialize to JSON
    let node = tape.node_at(idx);
    match node {
        Some(n) => {
            match n.kind {
                TapeNodeKind::ObjectStart { .. } | TapeNodeKind::ArrayStart { .. } => {
                    // Serialize container to JSON
                    let json = serialize_subtree(tape, idx)?;
                    Ok(TapeValueOwned::Json(json))
                }
                _ => Ok(TapeValueOwned::Null),
            }
        }
        None => Ok(TapeValueOwned::Null),
    }
}

/// Serialize a subtree to JSON string
fn serialize_subtree<T: TapeSource>(tape: &T, start_idx: usize) -> Result<String> {
    let mut output = String::new();
    serialize_value(tape, start_idx, &mut output)?;
    Ok(output)
}

fn serialize_value<T: TapeSource>(tape: &T, idx: usize, output: &mut String) -> Result<usize> {
    let node = tape.node_at(idx);

    match node {
        Some(n) => {
            match n.kind {
                TapeNodeKind::ObjectStart { count } => {
                    output.push('{');
                    let mut current_idx = idx + 1;
                    for i in 0..count {
                        if i > 0 {
                            output.push(',');
                        }
                        // Key
                        if let Some(key) = tape.key_at(current_idx) {
                            output.push('"');
                            output.push_str(&escape_json_string(&key));
                            output.push_str("\":");
                            current_idx += 1;
                        }
                        // Value
                        current_idx = serialize_value(tape, current_idx, output)?;
                    }
                    output.push('}');
                    // Skip past ObjectEnd
                    Ok(current_idx + 1)
                }
                TapeNodeKind::ArrayStart { count } => {
                    output.push('[');
                    let mut current_idx = idx + 1;
                    for i in 0..count {
                        if i > 0 {
                            output.push(',');
                        }
                        current_idx = serialize_value(tape, current_idx, output)?;
                    }
                    output.push(']');
                    // Skip past ArrayEnd
                    Ok(current_idx + 1)
                }
                TapeNodeKind::Value | TapeNodeKind::Key => {
                    if let Some(val) = tape.value_at(idx) {
                        serialize_tape_value(&val, output);
                    }
                    Ok(idx + 1)
                }
                TapeNodeKind::ObjectEnd | TapeNodeKind::ArrayEnd => Ok(idx + 1),
            }
        }
        None => Ok(idx + 1),
    }
}

fn serialize_tape_value(val: &TapeValue<'_>, output: &mut String) {
    match val {
        TapeValue::Null => output.push_str("null"),
        TapeValue::Bool(true) => output.push_str("true"),
        TapeValue::Bool(false) => output.push_str("false"),
        TapeValue::Int(n) => output.push_str(&n.to_string()),
        TapeValue::Float(f) => output.push_str(&f.to_string()),
        TapeValue::String(s) => {
            output.push('"');
            output.push_str(&escape_json_string(s));
            output.push('"');
        }
        TapeValue::RawNumber(s) => output.push_str(s),
    }
}

fn escape_json_string(s: &str) -> Cow<'_, str> {
    if s.bytes()
        .any(|b| matches!(b, b'"' | b'\\' | b'\n' | b'\r' | b'\t'))
    {
        let escaped = s
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t");
        Cow::Owned(escaped)
    } else {
        Cow::Borrowed(s)
    }
}

/// Classify node kind for type comparison
const fn kind_class(kind: &TapeNodeKind) -> u8 {
    match kind {
        TapeNodeKind::ObjectStart { .. } | TapeNodeKind::ObjectEnd => 1,
        TapeNodeKind::ArrayStart { .. } | TapeNodeKind::ArrayEnd => 2,
        TapeNodeKind::Key | TapeNodeKind::Value => 3,
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
    use fionn_tape::DsonTape;

    fn parse_json(s: &str) -> DsonTape {
        DsonTape::parse(s).expect("valid JSON")
    }

    #[test]
    fn test_diff_equal_objects() {
        let a = parse_json(r#"{"name": "Alice", "age": 30}"#);
        let b = parse_json(r#"{"name": "Alice", "age": 30}"#);

        let diff = diff_tapes(&a, &b).unwrap();
        assert!(diff.is_empty());
    }

    #[test]
    fn test_diff_scalar_change() {
        let a = parse_json(r#"{"name": "Alice"}"#);
        let b = parse_json(r#"{"name": "Bob"}"#);

        let diff = diff_tapes(&a, &b).unwrap();
        assert_eq!(diff.len(), 1);
        assert!(matches!(&diff.operations[0], TapeDiffOp::Replace { path, .. } if path == "/name"));
    }

    #[test]
    fn test_diff_add_field() {
        let a = parse_json(r#"{"name": "Alice"}"#);
        let b = parse_json(r#"{"name": "Alice", "age": 30}"#);

        let diff = diff_tapes(&a, &b).unwrap();
        assert_eq!(diff.len(), 1);
        assert!(matches!(&diff.operations[0], TapeDiffOp::Add { path, .. } if path == "/age"));
    }

    #[test]
    fn test_diff_remove_field() {
        let a = parse_json(r#"{"name": "Alice", "age": 30}"#);
        let b = parse_json(r#"{"name": "Alice"}"#);

        let diff = diff_tapes(&a, &b).unwrap();
        assert_eq!(diff.len(), 1);
        assert!(matches!(&diff.operations[0], TapeDiffOp::Remove { path } if path == "/age"));
    }

    #[test]
    fn test_diff_nested_change() {
        let a = parse_json(r#"{"user": {"name": "Alice"}}"#);
        let b = parse_json(r#"{"user": {"name": "Bob"}}"#);

        let diff = diff_tapes(&a, &b).unwrap();
        assert_eq!(diff.len(), 1);
        assert!(
            matches!(&diff.operations[0], TapeDiffOp::Replace { path, .. } if path == "/user/name")
        );
    }

    #[test]
    fn test_diff_array_add() {
        let a = parse_json(r"[1, 2]");
        let b = parse_json(r"[1, 2, 3]");

        let diff = diff_tapes(&a, &b).unwrap();
        assert_eq!(diff.len(), 1);
        assert!(matches!(&diff.operations[0], TapeDiffOp::Add { path, .. } if path == "/2"));
    }

    #[test]
    fn test_diff_array_remove() {
        let a = parse_json(r"[1, 2, 3]");
        let b = parse_json(r"[1, 2]");

        let diff = diff_tapes(&a, &b).unwrap();
        assert_eq!(diff.len(), 1);
        assert!(matches!(&diff.operations[0], TapeDiffOp::Remove { path } if path == "/2"));
    }

    #[test]
    fn test_diff_type_change() {
        let a = parse_json(r#"{"value": "string"}"#);
        let b = parse_json(r#"{"value": 42}"#);

        let diff = diff_tapes(&a, &b).unwrap();
        assert_eq!(diff.len(), 1);
        assert!(
            matches!(&diff.operations[0], TapeDiffOp::Replace { path, .. } if path == "/value")
        );
    }

    #[test]
    fn test_diff_empty_to_object() {
        let a = parse_json(r"{}");
        let b = parse_json(r#"{"name": "Alice"}"#);

        let diff = diff_tapes(&a, &b).unwrap();
        assert_eq!(diff.len(), 1);
        assert!(matches!(&diff.operations[0], TapeDiffOp::Add { .. }));
    }

    #[test]
    fn test_diff_array_scalar_change() {
        // First test: array with scalar change
        let a = parse_json(r"[1, 2, 3]");
        let b = parse_json(r"[1, 99, 3]");

        let diff = diff_tapes(&a, &b).unwrap();
        assert_eq!(diff.len(), 1, "Expected exactly one change");
        assert!(matches!(&diff.operations[0], TapeDiffOp::Replace { path, .. } if path == "/1"));
    }

    #[test]
    fn test_diff_complex_nested() {
        // Test array of objects with nested change
        let a = parse_json(r#"[{"name": "Alice"}, {"name": "Bob"}]"#);
        let b = parse_json(r#"[{"name": "Alice"}, {"name": "Carol"}]"#);

        let diff = diff_tapes(&a, &b).unwrap();

        // Should find difference at /1/name
        assert_eq!(diff.len(), 1, "Expected exactly one change");
        assert!(
            matches!(&diff.operations[0], TapeDiffOp::Replace { path, .. } if path == "/1/name")
        );
    }

    #[test]
    fn test_diff_deeply_nested() {
        let a = parse_json(r#"{"users": [{"profile": {"name": "Alice"}}]}"#);
        let b = parse_json(r#"{"users": [{"profile": {"name": "Bob"}}]}"#);

        let diff = diff_tapes(&a, &b).unwrap();
        assert_eq!(diff.len(), 1);
        assert!(
            matches!(&diff.operations[0], TapeDiffOp::Replace { path, .. } if path == "/users/0/profile/name")
        );
    }

    #[test]
    fn test_serialize_subtree() {
        let tape = parse_json(r#"{"nested": {"a": 1, "b": "hello"}}"#);
        // Subtree at index 2 (the nested object)
        // Index 0: ObjectStart, 1: Key("nested"), 2: ObjectStart...
        let json = serialize_subtree(&tape, 2).unwrap();
        assert!(json.contains("\"a\":1") || json.contains("\"a\": 1"));
    }
}
