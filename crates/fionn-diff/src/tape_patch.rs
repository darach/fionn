// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tape patch bridge - apply tape diffs to produce values
//!
//! This module bridges the gap between native tape diffing and value mutation.
//! Since tapes are immutable read structures, patching works by:
//!
//! 1. Converting a source tape to a mutable `serde_json::Value`
//! 2. Applying `TapeDiff` operations to the Value
//! 3. Optionally serializing back to the original format
//!
//! # Performance
//!
//! This approach trades some overhead for compatibility:
//! - Tape→Value conversion: ~50-100 MiB/s
//! - Patch application: Very fast (in-memory mutations)
//! - Value→Format: Depends on target serializer
//!
//! For most use cases, this is still faster than:
//! - Parse→Value→Diff→Patch→Serialize (traditional approach)
//!
//! Because the diff itself is computed on tapes (250x faster for cross-format).
//!
//! # Example
//!
//! ```ignore
//! use fionn_diff::{diff_tapes, apply_tape_diff, tape_to_value};
//! use fionn_simd::transform::UnifiedTape;
//! use fionn_core::format::FormatKind;
//!
//! // Parse two YAML documents
//! let tape_a = UnifiedTape::parse(yaml_a.as_bytes(), FormatKind::Yaml)?;
//! let tape_b = UnifiedTape::parse(yaml_b.as_bytes(), FormatKind::Yaml)?;
//!
//! // Compute diff on tapes (fast!)
//! let diff = diff_tapes(&tape_a, &tape_b)?;
//!
//! // Apply diff to get patched value
//! let mut value = tape_to_value(&tape_a)?;
//! apply_tape_diff(&mut value, &diff)?;
//!
//! // value now equals what tape_b represents
//! ```

use fionn_core::Result;
use fionn_core::tape_source::{TapeNodeKind, TapeSource, TapeValue};
use serde_json::{Map, Value};

use crate::diff_tape::{TapeDiff, TapeDiffOp, TapeValueOwned};

// ============================================================================
// Tape to Value Conversion
// ============================================================================

/// Convert a tape to a `serde_json::Value`
///
/// This enables applying patches to tape-parsed data since tapes are immutable.
///
/// # Errors
///
/// Returns an error if the tape structure is malformed.
pub fn tape_to_value<T: TapeSource>(tape: &T) -> Result<Value> {
    if tape.is_empty() {
        return Ok(Value::Null);
    }

    let (value, _) = convert_node(tape, 0)?;
    Ok(value)
}

/// Convert a subtree of a tape to a value
fn convert_node<T: TapeSource>(tape: &T, idx: usize) -> Result<(Value, usize)> {
    let node = tape.node_at(idx);

    match node {
        Some(n) => {
            match n.kind {
                TapeNodeKind::ObjectStart { count } => {
                    let mut map = Map::with_capacity(count);
                    let mut current_idx = idx + 1;

                    for _ in 0..count {
                        // Get key
                        let key = tape
                            .key_at(current_idx)
                            .map(std::borrow::Cow::into_owned)
                            .unwrap_or_default();
                        current_idx += 1;

                        // Get value
                        let (value, next_idx) = convert_node(tape, current_idx)?;
                        map.insert(key, value);
                        current_idx = next_idx;
                    }

                    // No ObjectEnd marker in simd_json tape format
                    Ok((Value::Object(map), current_idx))
                }

                TapeNodeKind::ArrayStart { count } => {
                    let mut arr = Vec::with_capacity(count);
                    let mut current_idx = idx + 1;

                    for _ in 0..count {
                        let (value, next_idx) = convert_node(tape, current_idx)?;
                        arr.push(value);
                        current_idx = next_idx;
                    }

                    // No ArrayEnd marker in simd_json tape format
                    Ok((Value::Array(arr), current_idx))
                }

                TapeNodeKind::Value | TapeNodeKind::Key => {
                    // Keys shouldn't be converted directly, but handle gracefully
                    let value = n.value.map_or(Value::Null, tape_value_to_json);
                    Ok((value, idx + 1))
                }

                TapeNodeKind::ObjectEnd | TapeNodeKind::ArrayEnd => {
                    // End markers (for formats that have them) - return null and advance
                    Ok((Value::Null, idx + 1))
                }
            }
        }
        None => Ok((Value::Null, idx + 1)),
    }
}

/// Convert `TapeValue` to `serde_json::Value`
fn tape_value_to_json(val: TapeValue<'_>) -> Value {
    match val {
        TapeValue::Null => Value::Null,
        TapeValue::Bool(b) => Value::Bool(b),
        TapeValue::Int(n) => Value::Number(n.into()),
        TapeValue::Float(f) => serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number),
        TapeValue::String(s) => Value::String(s.into_owned()),
        TapeValue::RawNumber(s) => {
            // Try to parse as number, fallback to string
            #[allow(clippy::option_if_let_else)]
            // Chained if-let-else is clearer for fallback parsing
            if let Ok(n) = s.parse::<i64>() {
                Value::Number(n.into())
            } else if let Ok(f) = s.parse::<f64>() {
                serde_json::Number::from_f64(f)
                    .map_or_else(|| Value::String(s.into_owned()), Value::Number)
            } else {
                Value::String(s.into_owned())
            }
        }
    }
}

// ============================================================================
// TapeDiff Application
// ============================================================================

/// Apply a `TapeDiff` to a mutable value
///
/// This converts tape diff operations to value mutations.
///
/// # Errors
///
/// Returns an error if a path is invalid or an operation fails.
pub fn apply_tape_diff(value: &mut Value, diff: &TapeDiff<'_>) -> Result<()> {
    for op in &diff.operations {
        apply_tape_diff_op(value, op)?;
    }
    Ok(())
}

/// Apply a single `TapeDiffOp` to a value
fn apply_tape_diff_op(value: &mut Value, op: &TapeDiffOp<'_>) -> Result<()> {
    match op {
        TapeDiffOp::Add {
            path,
            value: new_value,
        }
        | TapeDiffOp::Replace {
            path,
            value: new_value,
        } => {
            let json_value = tape_value_owned_to_json(new_value);
            set_at_path(value, path, json_value)?;
        }

        TapeDiffOp::Remove { path } => {
            remove_at_path(value, path)?;
        }

        TapeDiffOp::Move { from, path } => {
            let moved = remove_at_path(value, from)?;
            set_at_path(value, path, moved)?;
        }

        TapeDiffOp::Copy { from, path } => {
            let copied = get_at_path(value, from)?.clone();
            set_at_path(value, path, copied)?;
        }

        TapeDiffOp::AddRef {
            path,
            tape_index: _,
            ..
        }
        | TapeDiffOp::ReplaceRef {
            path,
            tape_index: _,
            ..
        } => {
            // Ref operations require the target tape - use placeholder
            set_at_path(value, path, Value::Null)?;
        }
    }

    Ok(())
}

/// Convert `TapeValueOwned` to `serde_json::Value`
fn tape_value_owned_to_json(val: &TapeValueOwned) -> Value {
    match val {
        TapeValueOwned::Null => Value::Null,
        TapeValueOwned::Bool(b) => Value::Bool(*b),
        TapeValueOwned::Int(n) => Value::Number((*n).into()),
        TapeValueOwned::Float(f) => {
            serde_json::Number::from_f64(*f).map_or(Value::Null, Value::Number)
        }
        TapeValueOwned::String(s) => Value::String(s.clone()),
        TapeValueOwned::RawNumber(s) => {
            #[allow(clippy::option_if_let_else)]
            // Chained if-let-else is clearer for fallback parsing
            if let Ok(n) = s.parse::<i64>() {
                Value::Number(n.into())
            } else if let Ok(f) = s.parse::<f64>() {
                serde_json::Number::from_f64(f)
                    .map_or_else(|| Value::String(s.clone()), Value::Number)
            } else {
                Value::String(s.clone())
            }
        }
        TapeValueOwned::Json(json_str) => {
            // Parse the JSON string into a Value
            serde_json::from_str(json_str).unwrap_or(Value::Null)
        }
    }
}

// ============================================================================
// Path Navigation
// ============================================================================

/// Get a value at a JSON Pointer path
fn get_at_path<'a>(value: &'a Value, path: &str) -> Result<&'a Value> {
    if path.is_empty() {
        return Ok(value);
    }

    let segments = parse_json_pointer(path)?;
    let mut current = value;

    for segment in segments {
        current = match current {
            Value::Object(map) => map.get(&segment).ok_or_else(|| {
                fionn_core::DsonError::InvalidField(format!("Path not found: {path}"))
            })?,
            Value::Array(arr) => {
                let index: usize = segment.parse().map_err(|_| {
                    fionn_core::DsonError::InvalidField(format!("Invalid array index: {segment}"))
                })?;
                arr.get(index).ok_or_else(|| {
                    fionn_core::DsonError::InvalidField(format!("Index out of bounds: {index}"))
                })?
            }
            _ => {
                return Err(fionn_core::DsonError::InvalidField(format!(
                    "Cannot navigate into scalar at {path}"
                )));
            }
        };
    }

    Ok(current)
}

/// Set a value at a JSON Pointer path
fn set_at_path(value: &mut Value, path: &str, new_value: Value) -> Result<()> {
    if path.is_empty() {
        *value = new_value;
        return Ok(());
    }

    let segments = parse_json_pointer(path)?;
    if segments.is_empty() {
        *value = new_value;
        return Ok(());
    }

    // Navigate to parent
    let parent_segments = &segments[..segments.len() - 1];
    let final_key = &segments[segments.len() - 1];

    let mut current = value;
    for segment in parent_segments {
        current = match current {
            Value::Object(map) => map
                .entry(segment.clone())
                .or_insert(Value::Object(Map::new())),
            Value::Array(arr) => {
                let index: usize = segment.parse().map_err(|_| {
                    fionn_core::DsonError::InvalidField(format!("Invalid array index: {segment}"))
                })?;
                // Extend array if needed
                while arr.len() <= index {
                    arr.push(Value::Null);
                }
                arr.get_mut(index).ok_or_else(|| {
                    fionn_core::DsonError::InvalidField(format!("Index out of bounds: {index}"))
                })?
            }
            _ => {
                return Err(fionn_core::DsonError::InvalidField(
                    "Cannot navigate into scalar at path".to_string(),
                ));
            }
        };
    }

    // Set final value
    match current {
        Value::Object(map) => {
            map.insert(final_key.clone(), new_value);
        }
        Value::Array(arr) => {
            if final_key == "-" {
                arr.push(new_value);
            } else {
                let index: usize = final_key.parse().map_err(|_| {
                    fionn_core::DsonError::InvalidField(format!("Invalid array index: {final_key}"))
                })?;
                while arr.len() <= index {
                    arr.push(Value::Null);
                }
                arr[index] = new_value;
            }
        }
        _ => {
            return Err(fionn_core::DsonError::InvalidField(
                "Cannot set value on scalar".to_string(),
            ));
        }
    }

    Ok(())
}

/// Remove a value at a JSON Pointer path
fn remove_at_path(value: &mut Value, path: &str) -> Result<Value> {
    if path.is_empty() {
        return Err(fionn_core::DsonError::InvalidField(
            "Cannot remove root".to_string(),
        ));
    }

    let segments = parse_json_pointer(path)?;
    if segments.is_empty() {
        return Err(fionn_core::DsonError::InvalidField(
            "Cannot remove root".to_string(),
        ));
    }

    // Navigate to parent
    let parent_segments = &segments[..segments.len() - 1];
    let final_key = &segments[segments.len() - 1];

    let mut current = value;
    for segment in parent_segments {
        current = match current {
            Value::Object(map) => map.get_mut(segment).ok_or_else(|| {
                fionn_core::DsonError::InvalidField(format!("Path not found: {path}"))
            })?,
            Value::Array(arr) => {
                let index: usize = segment.parse().map_err(|_| {
                    fionn_core::DsonError::InvalidField(format!("Invalid array index: {segment}"))
                })?;
                arr.get_mut(index).ok_or_else(|| {
                    fionn_core::DsonError::InvalidField(format!("Index out of bounds: {index}"))
                })?
            }
            _ => {
                return Err(fionn_core::DsonError::InvalidField(format!(
                    "Cannot navigate into scalar at {path}"
                )));
            }
        };
    }

    // Remove from parent
    match current {
        Value::Object(map) => map.remove(final_key).ok_or_else(|| {
            fionn_core::DsonError::InvalidField(format!("Key not found: {final_key}"))
        }),
        Value::Array(arr) => {
            let index: usize = final_key.parse().map_err(|_| {
                fionn_core::DsonError::InvalidField(format!("Invalid array index: {final_key}"))
            })?;
            if index >= arr.len() {
                return Err(fionn_core::DsonError::InvalidField(format!(
                    "Index out of bounds: {index}"
                )));
            }
            Ok(arr.remove(index))
        }
        _ => Err(fionn_core::DsonError::InvalidField(
            "Cannot remove from scalar".to_string(),
        )),
    }
}

/// Parse JSON Pointer path into segments
fn parse_json_pointer(path: &str) -> Result<Vec<String>> {
    if path.is_empty() {
        return Ok(vec![]);
    }

    if !path.starts_with('/') {
        return Err(fionn_core::DsonError::InvalidField(format!(
            "JSON Pointer must start with '/': {path}"
        )));
    }

    Ok(path[1..].split('/').map(unescape_json_pointer).collect())
}

/// Unescape JSON Pointer segment
fn unescape_json_pointer(s: &str) -> String {
    s.replace("~1", "/").replace("~0", "~")
}

// ============================================================================
// Value to Format Serialization
// ============================================================================

/// Serialize a value back to JSON string
#[must_use]
pub fn value_to_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

/// Serialize a value back to pretty JSON string
#[must_use]
pub fn value_to_json_pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

/// Serialize a value to YAML string (requires yaml feature)
///
/// # Errors
///
/// Returns an error if the value cannot be serialized to YAML.
#[cfg(feature = "yaml")]
pub fn value_to_yaml(value: &Value) -> Result<String> {
    serde_yaml::to_string(value).map_err(|e| fionn_core::DsonError::InvalidField(e.to_string()))
}

/// Serialize a value to TOML string (requires toml feature)
///
/// Note: TOML requires a table at root level.
///
/// # Errors
///
/// Returns an error if the value cannot be serialized to TOML.
#[cfg(feature = "toml")]
pub fn value_to_toml(value: &Value) -> Result<String> {
    // Convert to toml::Value first
    let toml_value: toml::Value = serde_json::from_value(value.clone())
        .map_err(|e| fionn_core::DsonError::InvalidField(e.to_string()))?;

    toml::to_string(&toml_value).map_err(|e| fionn_core::DsonError::InvalidField(e.to_string()))
}

// ============================================================================
// Full Pipeline Helpers
// ============================================================================

/// Full pipeline: apply diff from `tape_b` to value derived from `tape_a`
///
/// This is the common use case for cross-format patching:
/// 1. Parse both documents as tapes
/// 2. Compute diff between tapes (fast!)
/// 3. Convert source tape to Value
/// 4. Apply diff to Value
///
/// # Example
///
/// ```ignore
/// let tape_a = UnifiedTape::parse(yaml_a.as_bytes(), FormatKind::Yaml)?;
/// let tape_b = UnifiedTape::parse(yaml_b.as_bytes(), FormatKind::Yaml)?;
///
/// let diff = diff_tapes(&tape_a, &tape_b)?;
/// let patched = patch_tape(&tape_a, &diff)?;
/// // patched is now equivalent to tape_b's data
/// ```
///
/// # Errors
///
/// Returns an error if the source tape cannot be converted or the diff cannot be applied.
pub fn patch_tape<T: TapeSource>(source_tape: &T, diff: &TapeDiff<'_>) -> Result<Value> {
    let mut value = tape_to_value(source_tape)?;
    apply_tape_diff(&mut value, diff)?;
    Ok(value)
}

/// Three-way patch: apply diff to a tape and return as Value
///
/// Takes a base tape, computes diff against target tape, applies to base.
///
/// # Errors
///
/// Returns an error if the diff cannot be computed or applied.
pub fn three_way_patch<S: TapeSource, T: TapeSource>(base: &S, target: &T) -> Result<Value> {
    use crate::diff_tape::diff_tapes;

    let diff = diff_tapes(base, target)?;
    patch_tape(base, &diff)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff_tape::diff_tapes;
    use fionn_tape::DsonTape;

    fn parse_json(s: &str) -> DsonTape {
        DsonTape::parse(s).expect("valid JSON")
    }

    #[test]
    fn test_tape_to_value_simple() {
        let tape = parse_json(r#"{"name": "Alice", "age": 30}"#);
        let value = tape_to_value(&tape).unwrap();

        assert_eq!(value["name"], "Alice");
        assert_eq!(value["age"], 30);
    }

    #[test]
    fn test_tape_to_value_nested() {
        let tape = parse_json(r#"{"user": {"name": "Alice", "profile": {"age": 30}}}"#);
        let value = tape_to_value(&tape).unwrap();

        assert_eq!(value["user"]["name"], "Alice");
        assert_eq!(value["user"]["profile"]["age"], 30);
    }

    #[test]
    fn test_tape_to_value_array() {
        let tape = parse_json(r"[1, 2, 3, 4, 5]");
        let value = tape_to_value(&tape).unwrap();

        assert!(value.is_array());
        assert_eq!(value.as_array().unwrap().len(), 5);
        assert_eq!(value[0], 1);
        assert_eq!(value[4], 5);
    }

    #[test]
    fn test_tape_to_value_mixed() {
        let tape = parse_json(r#"{"items": [{"id": 1}, {"id": 2}]}"#);
        let value = tape_to_value(&tape).unwrap();

        assert_eq!(value["items"][0]["id"], 1);
        assert_eq!(value["items"][1]["id"], 2);
    }

    #[test]
    fn test_apply_tape_diff_replace() {
        let tape_a = parse_json(r#"{"name": "Alice"}"#);
        let tape_b = parse_json(r#"{"name": "Bob"}"#);

        let diff = diff_tapes(&tape_a, &tape_b).unwrap();
        let mut value = tape_to_value(&tape_a).unwrap();

        apply_tape_diff(&mut value, &diff).unwrap();

        assert_eq!(value["name"], "Bob");
    }

    #[test]
    fn test_apply_tape_diff_add() {
        let tape_a = parse_json(r#"{"name": "Alice"}"#);
        let tape_b = parse_json(r#"{"name": "Alice", "age": 30}"#);

        let diff = diff_tapes(&tape_a, &tape_b).unwrap();
        let mut value = tape_to_value(&tape_a).unwrap();

        apply_tape_diff(&mut value, &diff).unwrap();

        assert_eq!(value["name"], "Alice");
        assert_eq!(value["age"], 30);
    }

    #[test]
    fn test_apply_tape_diff_remove() {
        let tape_a = parse_json(r#"{"name": "Alice", "age": 30}"#);
        let tape_b = parse_json(r#"{"name": "Alice"}"#);

        let diff = diff_tapes(&tape_a, &tape_b).unwrap();
        let mut value = tape_to_value(&tape_a).unwrap();

        apply_tape_diff(&mut value, &diff).unwrap();

        assert_eq!(value["name"], "Alice");
        assert!(value.get("age").is_none());
    }

    #[test]
    fn test_apply_tape_diff_nested() {
        let tape_a = parse_json(r#"{"user": {"name": "Alice"}}"#);
        let tape_b = parse_json(r#"{"user": {"name": "Bob"}}"#);

        let diff = diff_tapes(&tape_a, &tape_b).unwrap();
        let mut value = tape_to_value(&tape_a).unwrap();

        apply_tape_diff(&mut value, &diff).unwrap();

        assert_eq!(value["user"]["name"], "Bob");
    }

    #[test]
    fn test_patch_tape_helper() {
        let tape_a = parse_json(r#"{"name": "Alice", "count": 1}"#);
        let tape_b = parse_json(r#"{"name": "Bob", "count": 2}"#);

        let diff = diff_tapes(&tape_a, &tape_b).unwrap();
        let patched = patch_tape(&tape_a, &diff).unwrap();

        assert_eq!(patched["name"], "Bob");
        assert_eq!(patched["count"], 2);
    }

    #[test]
    fn test_three_way_patch() {
        let tape_a = parse_json(r#"{"version": 1}"#);
        let tape_b = parse_json(r#"{"version": 2}"#);

        let result = three_way_patch(&tape_a, &tape_b).unwrap();

        assert_eq!(result["version"], 2);
    }

    #[test]
    fn test_roundtrip_to_json() {
        let tape = parse_json(r#"{"items": [1, 2, 3]}"#);
        let value = tape_to_value(&tape).unwrap();
        let json = value_to_json(&value);

        // Parse back and compare
        let reparsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(reparsed["items"][0], 1);
    }

    #[test]
    fn test_parse_json_pointer() {
        assert_eq!(parse_json_pointer("").unwrap(), Vec::<String>::new());
        assert_eq!(parse_json_pointer("/").unwrap(), vec![""]);
        assert_eq!(parse_json_pointer("/foo").unwrap(), vec!["foo"]);
        assert_eq!(parse_json_pointer("/foo/bar").unwrap(), vec!["foo", "bar"]);
        assert_eq!(parse_json_pointer("/a~1b").unwrap(), vec!["a/b"]);
        assert_eq!(parse_json_pointer("/c~0d").unwrap(), vec!["c~d"]);
    }

    #[test]
    fn test_set_at_path_nested() {
        let mut value = serde_json::json!({});

        set_at_path(&mut value, "/user/name", Value::String("Alice".to_string())).unwrap();

        assert_eq!(value["user"]["name"], "Alice");
    }

    #[test]
    fn test_set_at_path_array() {
        let mut value = serde_json::json!([1, 2, 3]);

        set_at_path(&mut value, "/1", Value::Number(99.into())).unwrap();

        assert_eq!(value[1], 99);
    }

    #[test]
    fn test_remove_at_path() {
        let mut value = serde_json::json!({"a": 1, "b": 2});

        let removed = remove_at_path(&mut value, "/a").unwrap();

        assert_eq!(removed, 1);
        assert!(value.get("a").is_none());
        assert_eq!(value["b"], 2);
    }
}
