// SPDX-License-Identifier: MIT OR Apache-2.0
//! Value construction trait for ungron reconstruction
//!
//! This module provides the [`ValueBuilder`] trait for constructing output values
//! during ungron (reverse gron) operations. Different formats can implement
//! this trait to enable reconstruction to their native types.
//!
//! # Key Types
//!
//! - [`ValueBuilder`] - Trait for building output values
//! - [`JsonBuilder`] - Implementation for `serde_json::Value`
//!
//! # Example
//!
//! ```ignore
//! use fionn_core::value_builder::{ValueBuilder, JsonBuilder};
//!
//! let mut builder = JsonBuilder;
//! let mut obj = builder.empty_object();
//! builder.insert_field(&mut obj, "name", builder.string("Alice"));
//! builder.insert_field(&mut obj, "age", builder.int(30));
//! let json = builder.serialize(&obj)?;
//! ```

use crate::format::FormatKind;
use crate::tape_source::TapeValue;
use crate::{DsonError, Result};

// ============================================================================
// ValueBuilder Trait
// ============================================================================

/// Trait for building output values during ungron reconstruction
///
/// This trait provides a uniform interface for constructing values across
/// different output formats. Implementors can target any format (JSON, YAML,
/// TOML, etc.) while using the same ungron algorithm.
///
/// # Type Parameters
///
/// The associated type `Output` is the target value type (e.g., `serde_json::Value`).
///
/// # Implementation Notes
///
/// - `empty_object()` and `empty_array()` should create mutable containers
/// - `insert_field()` and `push_element()` modify their container in place
/// - `serialize()` produces the final string output
pub trait ValueBuilder {
    /// The output value type produced by this builder
    type Output;

    /// Get the target format for this builder
    fn target_format(&self) -> FormatKind;

    /// Create a null value
    fn null(&mut self) -> Self::Output;

    /// Create a boolean value
    fn bool(&mut self, value: bool) -> Self::Output;

    /// Create an integer value
    fn int(&mut self, value: i64) -> Self::Output;

    /// Create a floating-point value
    fn float(&mut self, value: f64) -> Self::Output;

    /// Create a string value
    fn string(&mut self, value: &str) -> Self::Output;

    /// Create an empty object/map
    fn empty_object(&mut self) -> Self::Output;

    /// Create an empty array/sequence
    fn empty_array(&mut self) -> Self::Output;

    /// Insert a key-value pair into an object
    ///
    /// Modifies `obj` in place.
    fn insert_field(&mut self, obj: &mut Self::Output, key: &str, value: Self::Output);

    /// Push a value to an array
    ///
    /// Modifies `arr` in place.
    fn push_element(&mut self, arr: &mut Self::Output, value: Self::Output);

    /// Set a value at a specific array index
    ///
    /// Extends the array with null values if necessary.
    fn set_element(&mut self, arr: &mut Self::Output, index: usize, value: Self::Output);

    /// Check if a value is null
    fn is_null(&self, value: &Self::Output) -> bool;

    /// Check if a value is an object
    fn is_object(&self, value: &Self::Output) -> bool;

    /// Check if a value is an array
    fn is_array(&self, value: &Self::Output) -> bool;

    /// Serialize the output value to a string
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    fn serialize(&self, value: &Self::Output) -> Result<String>;

    /// Serialize with pretty printing
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    fn serialize_pretty(&self, value: &Self::Output) -> Result<String> {
        // Default implementation just uses regular serialize
        self.serialize(value)
    }

    /// Build a value from a [`TapeValue`]
    fn build_from_tape_value(&mut self, value: &TapeValue<'_>) -> Self::Output {
        match value {
            TapeValue::Null => self.null(),
            TapeValue::Bool(b) => self.bool(*b),
            TapeValue::Int(n) => self.int(*n),
            TapeValue::Float(n) => self.float(*n),
            TapeValue::String(s) => self.string(s),
            TapeValue::RawNumber(s) => {
                // Try to parse as int first, then float
                if let Ok(n) = s.parse::<i64>() {
                    self.int(n)
                } else if let Ok(n) = s.parse::<f64>() {
                    self.float(n)
                } else {
                    self.string(s)
                }
            }
        }
    }
}

// ============================================================================
// JSON Builder Implementation
// ============================================================================

/// Value builder for `serde_json::Value`
///
/// This is the default builder for JSON output, used when reconstructing
/// ungron output back to JSON.
#[derive(Debug, Clone, Copy, Default)]
pub struct JsonBuilder;

impl JsonBuilder {
    /// Create a new JSON builder
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ValueBuilder for JsonBuilder {
    type Output = serde_json::Value;

    fn target_format(&self) -> FormatKind {
        FormatKind::Json
    }

    fn null(&mut self) -> Self::Output {
        serde_json::Value::Null
    }

    fn bool(&mut self, value: bool) -> Self::Output {
        serde_json::Value::Bool(value)
    }

    fn int(&mut self, value: i64) -> Self::Output {
        serde_json::Value::Number(serde_json::Number::from(value))
    }

    fn float(&mut self, value: f64) -> Self::Output {
        serde_json::Number::from_f64(value)
            .map_or(serde_json::Value::Null, serde_json::Value::Number)
    }

    fn string(&mut self, value: &str) -> Self::Output {
        serde_json::Value::String(value.to_string())
    }

    fn empty_object(&mut self) -> Self::Output {
        serde_json::Value::Object(serde_json::Map::new())
    }

    fn empty_array(&mut self) -> Self::Output {
        serde_json::Value::Array(Vec::new())
    }

    fn insert_field(&mut self, obj: &mut Self::Output, key: &str, value: Self::Output) {
        if let serde_json::Value::Object(map) = obj {
            map.insert(key.to_string(), value);
        }
    }

    fn push_element(&mut self, arr: &mut Self::Output, value: Self::Output) {
        if let serde_json::Value::Array(vec) = arr {
            vec.push(value);
        }
    }

    fn set_element(&mut self, arr: &mut Self::Output, index: usize, value: Self::Output) {
        if let serde_json::Value::Array(vec) = arr {
            // Extend with nulls if necessary
            while vec.len() <= index {
                vec.push(serde_json::Value::Null);
            }
            vec[index] = value;
        }
    }

    fn is_null(&self, value: &Self::Output) -> bool {
        value.is_null()
    }

    fn is_object(&self, value: &Self::Output) -> bool {
        value.is_object()
    }

    fn is_array(&self, value: &Self::Output) -> bool {
        value.is_array()
    }

    fn serialize(&self, value: &Self::Output) -> Result<String> {
        serde_json::to_string(value).map_err(|e| DsonError::SerializationError(e.to_string()))
    }

    fn serialize_pretty(&self, value: &Self::Output) -> Result<String> {
        serde_json::to_string_pretty(value)
            .map_err(|e| DsonError::SerializationError(e.to_string()))
    }
}

// ============================================================================
// Path Navigation Helper
// ============================================================================

/// Path segment for navigation during ungron
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSegment {
    /// Object field access
    Field(String),
    /// Array index access
    Index(usize),
}

impl PathSegment {
    /// Parse a path segment from string
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        if s.starts_with('[') && s.ends_with(']') {
            let inner = &s[1..s.len() - 1];
            if inner.starts_with('"') && inner.ends_with('"') {
                // Quoted field name
                Some(Self::Field(inner[1..inner.len() - 1].to_string()))
            } else {
                // Array index
                inner.parse().ok().map(Self::Index)
            }
        } else if let Some(stripped) = s.strip_prefix('.') {
            // Dotted field name
            Some(Self::Field(stripped.to_string()))
        } else {
            // Bare field name
            Some(Self::Field(s.to_string()))
        }
    }
}

// ============================================================================
// Direct JSON Value Path Navigation
// ============================================================================

/// Navigate and set a value at a path in a JSON value tree
///
/// This is the core algorithm for ungron reconstruction. Unlike the generic
/// approach, this works directly with `serde_json::Value` to avoid borrow
/// checker issues with trait-based mutable access.
///
/// # Panics
///
/// Does not panic. If the path traversal encounters unexpected types,
/// those segments are skipped (type mismatches are handled gracefully).
pub fn set_at_path_json(
    root: &mut serde_json::Value,
    path: &[PathSegment],
    value: serde_json::Value,
) {
    if path.is_empty() {
        *root = value;
        return;
    }

    // Ensure root is the right container type for the first segment
    match &path[0] {
        PathSegment::Field(_) if !root.is_object() => {
            *root = serde_json::Value::Object(serde_json::Map::new());
        }
        PathSegment::Index(_) if !root.is_array() => {
            *root = serde_json::Value::Array(Vec::new());
        }
        _ => {}
    }

    // Navigate to the parent of the target location
    let mut current = root;
    for (i, segment) in path.iter().enumerate() {
        let is_last = i == path.len() - 1;

        if is_last {
            // Set the value at the final location
            match segment {
                PathSegment::Field(key) => {
                    if let serde_json::Value::Object(map) = current {
                        map.insert(key.clone(), value);
                    }
                }
                PathSegment::Index(index) => {
                    if let serde_json::Value::Array(arr) = current {
                        while arr.len() <= *index {
                            arr.push(serde_json::Value::Null);
                        }
                        arr[*index] = value;
                    }
                }
            }
            return;
        }

        // Determine what type the next container should be
        let next_is_index = matches!(path.get(i + 1), Some(PathSegment::Index(_)));

        // Navigate to or create the next container
        match segment {
            PathSegment::Field(key) => {
                if let serde_json::Value::Object(map) = current {
                    if map.contains_key(key) {
                        // Ensure existing value is the right type
                        let existing = map.get(key).unwrap();
                        if (next_is_index && !existing.is_array())
                            || (!next_is_index && !existing.is_object())
                        {
                            let new_container = if next_is_index {
                                serde_json::Value::Array(Vec::new())
                            } else {
                                serde_json::Value::Object(serde_json::Map::new())
                            };
                            map.insert(key.clone(), new_container);
                        }
                    } else {
                        let new_container = if next_is_index {
                            serde_json::Value::Array(Vec::new())
                        } else {
                            serde_json::Value::Object(serde_json::Map::new())
                        };
                        map.insert(key.clone(), new_container);
                    }
                    current = map.get_mut(key).unwrap();
                }
            }
            PathSegment::Index(index) => {
                if let serde_json::Value::Array(arr) = current {
                    // Extend array if needed
                    while arr.len() <= *index {
                        arr.push(serde_json::Value::Null);
                    }

                    // Initialize element if null
                    if arr[*index].is_null() {
                        arr[*index] = if next_is_index {
                            serde_json::Value::Array(Vec::new())
                        } else {
                            serde_json::Value::Object(serde_json::Map::new())
                        };
                    } else {
                        // Ensure existing value is the right type
                        let existing = &arr[*index];
                        if (next_is_index && !existing.is_array())
                            || (!next_is_index && !existing.is_object())
                        {
                            arr[*index] = if next_is_index {
                                serde_json::Value::Array(Vec::new())
                            } else {
                                serde_json::Value::Object(serde_json::Map::new())
                            };
                        }
                    }
                    current = &mut arr[*index];
                }
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_builder_primitives() {
        let mut builder = JsonBuilder::new();

        assert_eq!(builder.null(), serde_json::Value::Null);
        assert_eq!(builder.bool(true), serde_json::Value::Bool(true));
        assert_eq!(builder.bool(false), serde_json::Value::Bool(false));
        assert_eq!(
            builder.int(42),
            serde_json::Value::Number(serde_json::Number::from(42))
        );
        assert_eq!(
            builder.string("hello"),
            serde_json::Value::String("hello".to_string())
        );
    }

    #[test]
    fn test_json_builder_float() {
        let mut builder = JsonBuilder::new();

        let float_val = builder.float(3.15);
        assert!(float_val.is_number());

        // NaN becomes null
        let nan_val = builder.float(f64::NAN);
        assert!(nan_val.is_null());
    }

    #[test]
    fn test_json_builder_containers() {
        let mut builder = JsonBuilder::new();

        let obj = builder.empty_object();
        assert!(builder.is_object(&obj));

        let arr = builder.empty_array();
        assert!(builder.is_array(&arr));
    }

    #[test]
    fn test_json_builder_insert_field() {
        let mut builder = JsonBuilder::new();

        let mut obj = builder.empty_object();
        let value = builder.string("world");
        builder.insert_field(&mut obj, "hello", value);

        if let serde_json::Value::Object(map) = &obj {
            assert!(map.contains_key("hello"));
        } else {
            panic!("Expected object");
        }
    }

    #[test]
    fn test_json_builder_push_element() {
        let mut builder = JsonBuilder::new();

        let mut arr = builder.empty_array();
        let val1 = builder.int(1);
        builder.push_element(&mut arr, val1);
        let val2 = builder.int(2);
        builder.push_element(&mut arr, val2);

        if let serde_json::Value::Array(vec) = &arr {
            assert_eq!(vec.len(), 2);
        } else {
            panic!("Expected array");
        }
    }

    #[test]
    fn test_json_builder_set_element() {
        let mut builder = JsonBuilder::new();

        let mut arr = builder.empty_array();
        let val = builder.string("test");
        builder.set_element(&mut arr, 3, val);

        // Should have expanded with nulls
        if let serde_json::Value::Array(vec) = &arr {
            assert_eq!(vec.len(), 4);
            assert!(vec[0].is_null());
            assert!(vec[1].is_null());
            assert!(vec[2].is_null());
            assert_eq!(vec[3], serde_json::Value::String("test".to_string()));
        } else {
            panic!("Expected array");
        }
    }

    #[test]
    fn test_json_builder_serialize() {
        let mut builder = JsonBuilder::new();

        let mut obj = builder.empty_object();
        let name_val = builder.string("Alice");
        builder.insert_field(&mut obj, "name", name_val);
        let age_val = builder.int(30);
        builder.insert_field(&mut obj, "age", age_val);

        let json = builder.serialize(&obj).unwrap();
        assert!(json.contains("Alice"));
        assert!(json.contains("30"));
    }

    #[test]
    fn test_json_builder_from_tape_value() {
        let mut builder = JsonBuilder::new();

        assert_eq!(
            builder.build_from_tape_value(&TapeValue::Null),
            serde_json::Value::Null
        );
        assert_eq!(
            builder.build_from_tape_value(&TapeValue::Bool(true)),
            serde_json::Value::Bool(true)
        );
        assert_eq!(
            builder.build_from_tape_value(&TapeValue::Int(42)),
            serde_json::json!(42)
        );
    }

    #[test]
    fn test_path_segment_parse_field() {
        assert_eq!(
            PathSegment::parse(".name"),
            Some(PathSegment::Field("name".to_string()))
        );
        assert_eq!(
            PathSegment::parse("name"),
            Some(PathSegment::Field("name".to_string()))
        );
    }

    #[test]
    fn test_path_segment_parse_index() {
        assert_eq!(PathSegment::parse("[0]"), Some(PathSegment::Index(0)));
        assert_eq!(PathSegment::parse("[42]"), Some(PathSegment::Index(42)));
    }

    #[test]
    fn test_path_segment_parse_quoted() {
        assert_eq!(
            PathSegment::parse("[\"special key\"]"),
            Some(PathSegment::Field("special key".to_string()))
        );
    }

    #[test]
    fn test_set_at_path_json_simple() {
        let mut root = serde_json::Value::Null;

        set_at_path_json(
            &mut root,
            &[PathSegment::Field("name".to_string())],
            serde_json::Value::String("Alice".to_string()),
        );

        assert!(root.is_object());
        assert_eq!(root["name"], "Alice");
    }

    #[test]
    fn test_set_at_path_json_nested() {
        let mut root = serde_json::Value::Null;

        set_at_path_json(
            &mut root,
            &[
                PathSegment::Field("user".to_string()),
                PathSegment::Field("name".to_string()),
            ],
            serde_json::Value::String("Bob".to_string()),
        );

        assert!(root.is_object());
        assert_eq!(root["user"]["name"], "Bob");
    }

    #[test]
    fn test_set_at_path_json_array() {
        let mut root = serde_json::Value::Null;

        set_at_path_json(
            &mut root,
            &[
                PathSegment::Field("items".to_string()),
                PathSegment::Index(0),
            ],
            serde_json::Value::String("first".to_string()),
        );

        assert!(root.is_object());
        assert!(root["items"].is_array());
        assert_eq!(root["items"][0], "first");
    }

    #[test]
    fn test_set_at_path_json_mixed() {
        let mut root = serde_json::Value::Null;

        // json.users[0].name = "Charlie"
        set_at_path_json(
            &mut root,
            &[
                PathSegment::Field("users".to_string()),
                PathSegment::Index(0),
                PathSegment::Field("name".to_string()),
            ],
            serde_json::Value::String("Charlie".to_string()),
        );

        assert_eq!(root["users"][0]["name"], "Charlie");
    }

    #[test]
    fn test_set_at_path_json_sparse_array() {
        let mut root = serde_json::Value::Null;

        set_at_path_json(
            &mut root,
            &[PathSegment::Index(5)],
            serde_json::Value::String("fifth".to_string()),
        );

        if let serde_json::Value::Array(arr) = &root {
            assert_eq!(arr.len(), 6);
            for item in arr.iter().take(5) {
                assert!(item.is_null());
            }
            assert_eq!(arr[5], "fifth");
        } else {
            panic!("Expected array");
        }
    }

    #[test]
    fn test_set_at_path_json_empty_path() {
        let mut root = serde_json::Value::Null;

        set_at_path_json(
            &mut root,
            &[],
            serde_json::Value::String("value".to_string()),
        );

        assert_eq!(root, "value");
    }

    #[test]
    fn test_set_at_path_json_overwrite() {
        let mut root = serde_json::json!({
            "name": "old"
        });

        set_at_path_json(
            &mut root,
            &[PathSegment::Field("name".to_string())],
            serde_json::Value::String("new".to_string()),
        );

        assert_eq!(root["name"], "new");
    }

    #[test]
    fn test_target_format() {
        let builder = JsonBuilder::new();
        assert_eq!(builder.target_format(), FormatKind::Json);
    }
}

// ============================================================================
// YAML Builder Implementation
// ============================================================================

/// Value builder for YAML output using `serde_yaml::Value`
///
/// This builder enables ungron reconstruction to YAML format.
#[cfg(feature = "yaml")]
#[derive(Debug, Clone, Copy, Default)]
pub struct YamlBuilder;

#[cfg(feature = "yaml")]
impl YamlBuilder {
    /// Create a new YAML builder
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[cfg(feature = "yaml")]
impl ValueBuilder for YamlBuilder {
    type Output = serde_yaml::Value;

    fn target_format(&self) -> FormatKind {
        FormatKind::Yaml
    }

    fn null(&mut self) -> Self::Output {
        serde_yaml::Value::Null
    }

    fn bool(&mut self, value: bool) -> Self::Output {
        serde_yaml::Value::Bool(value)
    }

    fn int(&mut self, value: i64) -> Self::Output {
        serde_yaml::Value::Number(serde_yaml::Number::from(value))
    }

    fn float(&mut self, value: f64) -> Self::Output {
        serde_yaml::Value::Number(serde_yaml::Number::from(value))
    }

    fn string(&mut self, value: &str) -> Self::Output {
        serde_yaml::Value::String(value.to_string())
    }

    fn empty_object(&mut self) -> Self::Output {
        serde_yaml::Value::Mapping(serde_yaml::Mapping::new())
    }

    fn empty_array(&mut self) -> Self::Output {
        serde_yaml::Value::Sequence(Vec::new())
    }

    fn insert_field(&mut self, obj: &mut Self::Output, key: &str, value: Self::Output) {
        if let serde_yaml::Value::Mapping(map) = obj {
            map.insert(serde_yaml::Value::String(key.to_string()), value);
        }
    }

    fn push_element(&mut self, arr: &mut Self::Output, value: Self::Output) {
        if let serde_yaml::Value::Sequence(vec) = arr {
            vec.push(value);
        }
    }

    fn set_element(&mut self, arr: &mut Self::Output, index: usize, value: Self::Output) {
        if let serde_yaml::Value::Sequence(vec) = arr {
            while vec.len() <= index {
                vec.push(serde_yaml::Value::Null);
            }
            vec[index] = value;
        }
    }

    fn is_null(&self, value: &Self::Output) -> bool {
        matches!(value, serde_yaml::Value::Null)
    }

    fn is_object(&self, value: &Self::Output) -> bool {
        matches!(value, serde_yaml::Value::Mapping(_))
    }

    fn is_array(&self, value: &Self::Output) -> bool {
        matches!(value, serde_yaml::Value::Sequence(_))
    }

    fn serialize(&self, value: &Self::Output) -> Result<String> {
        serde_yaml::to_string(value).map_err(|e| DsonError::SerializationError(e.to_string()))
    }
}

// ============================================================================
// TOML Builder Implementation
// ============================================================================

/// Value builder for TOML output using `toml::Value`
///
/// Note: TOML requires a table at the root level. Primitive roots
/// will be wrapped in a table with key "_".
#[cfg(feature = "toml")]
#[derive(Debug, Clone, Copy, Default)]
pub struct TomlBuilder;

#[cfg(feature = "toml")]
impl TomlBuilder {
    /// Create a new TOML builder
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[cfg(feature = "toml")]
impl ValueBuilder for TomlBuilder {
    type Output = toml_crate::Value;

    fn target_format(&self) -> FormatKind {
        FormatKind::Toml
    }

    fn null(&mut self) -> Self::Output {
        // TOML doesn't have null - use empty string as placeholder
        toml_crate::Value::String(String::new())
    }

    fn bool(&mut self, value: bool) -> Self::Output {
        toml_crate::Value::Boolean(value)
    }

    fn int(&mut self, value: i64) -> Self::Output {
        toml_crate::Value::Integer(value)
    }

    fn float(&mut self, value: f64) -> Self::Output {
        toml_crate::Value::Float(value)
    }

    fn string(&mut self, value: &str) -> Self::Output {
        toml_crate::Value::String(value.to_string())
    }

    fn empty_object(&mut self) -> Self::Output {
        toml_crate::Value::Table(toml_crate::Table::new())
    }

    fn empty_array(&mut self) -> Self::Output {
        toml_crate::Value::Array(Vec::new())
    }

    fn insert_field(&mut self, obj: &mut Self::Output, key: &str, value: Self::Output) {
        if let toml_crate::Value::Table(table) = obj {
            table.insert(key.to_string(), value);
        }
    }

    fn push_element(&mut self, arr: &mut Self::Output, value: Self::Output) {
        if let toml_crate::Value::Array(vec) = arr {
            vec.push(value);
        }
    }

    fn set_element(&mut self, arr: &mut Self::Output, index: usize, value: Self::Output) {
        if let toml_crate::Value::Array(vec) = arr {
            while vec.len() <= index {
                vec.push(toml_crate::Value::String(String::new())); // No null in TOML
            }
            vec[index] = value;
        }
    }

    fn is_null(&self, value: &Self::Output) -> bool {
        // TOML has no null - check for empty string placeholder
        matches!(value, toml_crate::Value::String(s) if s.is_empty())
    }

    fn is_object(&self, value: &Self::Output) -> bool {
        matches!(value, toml_crate::Value::Table(_))
    }

    fn is_array(&self, value: &Self::Output) -> bool {
        matches!(value, toml_crate::Value::Array(_))
    }

    fn serialize(&self, value: &Self::Output) -> Result<String> {
        toml_crate::to_string(value).map_err(|e| DsonError::SerializationError(e.to_string()))
    }

    fn serialize_pretty(&self, value: &Self::Output) -> Result<String> {
        toml_crate::to_string_pretty(value)
            .map_err(|e| DsonError::SerializationError(e.to_string()))
    }
}

// ============================================================================
// ISON Builder Implementation
// ============================================================================

/// Value builder for ISON (Indented Structured Object Notation) output
///
/// ISON is an LLM-optimized format using indentation for structure.
/// Output format:
/// ```text
/// key: value
/// nested:
///   child: value
/// list:
///   - item1
///   - item2
/// ```
#[cfg(feature = "ison")]
#[derive(Debug, Clone, Copy, Default)]
pub struct IsonBuilder;

#[cfg(feature = "ison")]
impl IsonBuilder {
    /// Create a new ISON builder
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

/// Internal ISON value representation
#[cfg(feature = "ison")]
#[derive(Debug, Clone)]
pub enum IsonValue {
    /// Null value
    Null,
    /// Boolean value
    Bool(bool),
    /// Integer value
    Int(i64),
    /// Float value
    Float(f64),
    /// String value
    String(String),
    /// Object/map
    Object(Vec<(String, Self)>),
    /// Array/sequence
    Array(Vec<Self>),
}

#[cfg(feature = "ison")]
impl ValueBuilder for IsonBuilder {
    type Output = IsonValue;

    fn target_format(&self) -> FormatKind {
        FormatKind::Ison
    }

    fn null(&mut self) -> Self::Output {
        IsonValue::Null
    }

    fn bool(&mut self, value: bool) -> Self::Output {
        IsonValue::Bool(value)
    }

    fn int(&mut self, value: i64) -> Self::Output {
        IsonValue::Int(value)
    }

    fn float(&mut self, value: f64) -> Self::Output {
        IsonValue::Float(value)
    }

    fn string(&mut self, value: &str) -> Self::Output {
        IsonValue::String(value.to_string())
    }

    fn empty_object(&mut self) -> Self::Output {
        IsonValue::Object(Vec::new())
    }

    fn empty_array(&mut self) -> Self::Output {
        IsonValue::Array(Vec::new())
    }

    fn insert_field(&mut self, obj: &mut Self::Output, key: &str, value: Self::Output) {
        if let IsonValue::Object(fields) = obj {
            // Update existing or append
            if let Some(field) = fields.iter_mut().find(|(k, _)| k == key) {
                field.1 = value;
            } else {
                fields.push((key.to_string(), value));
            }
        }
    }

    fn push_element(&mut self, arr: &mut Self::Output, value: Self::Output) {
        if let IsonValue::Array(elements) = arr {
            elements.push(value);
        }
    }

    fn set_element(&mut self, arr: &mut Self::Output, index: usize, value: Self::Output) {
        if let IsonValue::Array(elements) = arr {
            while elements.len() <= index {
                elements.push(IsonValue::Null);
            }
            elements[index] = value;
        }
    }

    fn is_null(&self, value: &Self::Output) -> bool {
        matches!(value, IsonValue::Null)
    }

    fn is_object(&self, value: &Self::Output) -> bool {
        matches!(value, IsonValue::Object(_))
    }

    fn is_array(&self, value: &Self::Output) -> bool {
        matches!(value, IsonValue::Array(_))
    }

    fn serialize(&self, value: &Self::Output) -> Result<String> {
        Ok(serialize_ison(value, 0))
    }
}

#[cfg(feature = "ison")]
fn serialize_ison(value: &IsonValue, indent: usize) -> String {
    let prefix = "  ".repeat(indent);

    match value {
        IsonValue::Null => "null".to_string(),
        IsonValue::Bool(b) => b.to_string(),
        IsonValue::Int(n) => n.to_string(),
        IsonValue::Float(f) => f.to_string(),
        IsonValue::String(s) => {
            // Quote if contains special characters
            if s.contains(':') || s.contains('\n') || s.starts_with(' ') || s.ends_with(' ') {
                format!("\"{}\"", s.replace('\"', "\\\""))
            } else {
                s.clone()
            }
        }
        IsonValue::Object(fields) if fields.is_empty() => "{}".to_string(),
        IsonValue::Object(fields) => {
            use std::fmt::Write as _;
            let mut output = String::new();
            for (key, val) in fields {
                match val {
                    IsonValue::Object(_) | IsonValue::Array(_) => {
                        let _ = writeln!(output, "{prefix}{key}:");
                        output.push_str(&serialize_ison(val, indent + 1));
                    }
                    _ => {
                        let _ = writeln!(output, "{prefix}{key}: {}", serialize_ison(val, 0));
                    }
                }
            }
            output
        }
        IsonValue::Array(elements) if elements.is_empty() => "[]".to_string(),
        IsonValue::Array(elements) => {
            use std::fmt::Write as _;
            let mut output = String::new();
            for elem in elements {
                match elem {
                    IsonValue::Object(_) | IsonValue::Array(_) => {
                        let _ = writeln!(output, "{prefix}-");
                        output.push_str(&serialize_ison(elem, indent + 1));
                    }
                    _ => {
                        let _ = writeln!(output, "{prefix}- {}", serialize_ison(elem, 0));
                    }
                }
            }
            output
        }
    }
}

// ============================================================================
// TOON Builder Implementation
// ============================================================================

/// Value builder for TOON (TOML-like Object Notation) output
///
/// TOON is an LLM-optimized minimal format similar to TOML.
/// Output format:
/// ```text
/// key = value
/// [section]
/// nested = value
/// [[array]]
/// item = value
/// ```
#[cfg(feature = "toon")]
#[derive(Debug, Clone, Default)]
pub struct ToonBuilder;

#[cfg(feature = "toon")]
impl ToonBuilder {
    /// Create a new TOON builder
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

/// Internal TOON value representation (reuses `IsonValue` structure)
#[cfg(feature = "toon")]
pub use self::toon_impl::ToonValue;

// ============================================================================
// CSV Builder Implementation
// ============================================================================

/// Value builder for CSV output
///
/// CSV ungron works by detecting the pattern `csv.rows[N].column = value`
/// and reconstructing the tabular structure.
///
/// # Strictness Modes
///
/// - **Strict**: Errors on column order ambiguity, missing cells, type coercion
/// - **Non-strict**: Best-effort reconstruction with sensible defaults
///
/// # Example Output
///
/// ```text
/// name,age,city
/// Alice,30,NYC
/// Bob,25,LA
/// ```
#[cfg(feature = "csv")]
#[derive(Debug, Clone)]
pub struct CsvBuilder {
    /// Whether to use strict mode
    pub strict: bool,
    /// Column delimiter
    pub delimiter: char,
    /// Whether to quote all fields
    pub quote_all: bool,
}

#[cfg(feature = "csv")]
impl Default for CsvBuilder {
    fn default() -> Self {
        Self {
            strict: false,
            delimiter: ',',
            quote_all: false,
        }
    }
}

#[cfg(feature = "csv")]
impl CsvBuilder {
    /// Create a new CSV builder with default settings
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a strict CSV builder that errors on ambiguity
    #[must_use]
    pub fn strict() -> Self {
        Self {
            strict: true,
            ..Self::default()
        }
    }

    /// Set the delimiter character
    #[must_use]
    pub const fn with_delimiter(mut self, delimiter: char) -> Self {
        self.delimiter = delimiter;
        self
    }
}

/// CSV value representation optimized for tabular reconstruction
#[cfg(feature = "csv")]
#[derive(Debug, Clone)]
pub enum CsvValue {
    /// Null/empty value
    Null,
    /// Boolean
    Bool(bool),
    /// Integer
    Int(i64),
    /// Float
    Float(f64),
    /// String
    String(String),
    /// Row object (column -> value)
    Row(Vec<(String, Self)>),
    /// Array of rows
    Rows(Vec<Self>),
    /// Generic object (for non-tabular data)
    Object(Vec<(String, Self)>),
    /// Generic array
    Array(Vec<Self>),
}

#[cfg(feature = "csv")]
impl ValueBuilder for CsvBuilder {
    type Output = CsvValue;

    fn target_format(&self) -> FormatKind {
        FormatKind::Csv
    }

    fn null(&mut self) -> Self::Output {
        CsvValue::Null
    }

    fn bool(&mut self, value: bool) -> Self::Output {
        CsvValue::Bool(value)
    }

    fn int(&mut self, value: i64) -> Self::Output {
        CsvValue::Int(value)
    }

    fn float(&mut self, value: f64) -> Self::Output {
        CsvValue::Float(value)
    }

    fn string(&mut self, value: &str) -> Self::Output {
        CsvValue::String(value.to_string())
    }

    fn empty_object(&mut self) -> Self::Output {
        CsvValue::Object(Vec::new())
    }

    fn empty_array(&mut self) -> Self::Output {
        CsvValue::Array(Vec::new())
    }

    fn insert_field(&mut self, obj: &mut Self::Output, key: &str, value: Self::Output) {
        match obj {
            CsvValue::Object(fields) | CsvValue::Row(fields) => {
                if let Some(field) = fields.iter_mut().find(|(k, _)| k == key) {
                    field.1 = value;
                } else {
                    fields.push((key.to_string(), value));
                }
            }
            _ => {}
        }
    }

    fn push_element(&mut self, arr: &mut Self::Output, value: Self::Output) {
        match arr {
            CsvValue::Array(elements) | CsvValue::Rows(elements) => {
                elements.push(value);
            }
            _ => {}
        }
    }

    fn set_element(&mut self, arr: &mut Self::Output, index: usize, value: Self::Output) {
        match arr {
            CsvValue::Array(elements) | CsvValue::Rows(elements) => {
                while elements.len() <= index {
                    elements.push(CsvValue::Null);
                }
                elements[index] = value;
            }
            _ => {}
        }
    }

    fn is_null(&self, value: &Self::Output) -> bool {
        matches!(value, CsvValue::Null)
    }

    fn is_object(&self, value: &Self::Output) -> bool {
        matches!(value, CsvValue::Object(_) | CsvValue::Row(_))
    }

    fn is_array(&self, value: &Self::Output) -> bool {
        matches!(value, CsvValue::Array(_) | CsvValue::Rows(_))
    }

    fn serialize(&self, value: &Self::Output) -> Result<String> {
        serialize_csv(value, self.delimiter, self.quote_all, self.strict)
    }
}

#[cfg(feature = "csv")]
fn serialize_csv(
    value: &CsvValue,
    delimiter: char,
    quote_all: bool,
    strict: bool,
) -> Result<String> {
    // Try to detect tabular structure: look for "rows" key containing array of objects
    if let CsvValue::Object(fields) = value
        && let Some((_, CsvValue::Array(rows) | CsvValue::Rows(rows))) =
            fields.iter().find(|(k, _)| k == "rows")
    {
        return serialize_rows_to_csv(rows, delimiter, quote_all, strict);
    }

    // Direct array of objects
    if let CsvValue::Array(rows) | CsvValue::Rows(rows) = value
        && rows
            .iter()
            .all(|r| matches!(r, CsvValue::Object(_) | CsvValue::Row(_)))
    {
        return serialize_rows_to_csv(rows, delimiter, quote_all, strict);
    }

    // Not tabular - error in strict mode, fallback to JSON-like in non-strict
    if strict {
        return Err(DsonError::SerializationError(
            "CSV ungron requires tabular structure (rows[N].column pattern)".to_string(),
        ));
    }

    // Non-strict fallback: single value or non-tabular
    Ok(csv_value_to_string(value, quote_all))
}

#[cfg(feature = "csv")]
fn serialize_rows_to_csv(
    rows: &[CsvValue],
    delimiter: char,
    quote_all: bool,
    strict: bool,
) -> Result<String> {
    if rows.is_empty() {
        return Ok(String::new());
    }

    // Collect all column names in order of first occurrence
    let mut columns: Vec<String> = Vec::new();
    for row in rows {
        if let CsvValue::Object(fields) | CsvValue::Row(fields) = row {
            for (key, _) in fields {
                if !columns.contains(key) {
                    columns.push(key.clone());
                }
            }
        }
    }

    if columns.is_empty() {
        return Ok(String::new());
    }

    let mut output = String::new();

    // Header row
    let header_line: Vec<String> = columns
        .iter()
        .map(|c| quote_csv_field(c, delimiter, quote_all))
        .collect();
    output.push_str(&header_line.join(&delimiter.to_string()));
    output.push('\n');

    // Data rows
    for (row_idx, row) in rows.iter().enumerate() {
        if let CsvValue::Object(fields) | CsvValue::Row(fields) = row {
            let mut row_values: Vec<String> = Vec::with_capacity(columns.len());

            for col in &columns {
                let value = fields.iter().find(|(k, _)| k == col).map(|(_, v)| v);

                if let Some(v) = value {
                    row_values.push(quote_csv_field(
                        &csv_value_to_string(v, quote_all),
                        delimiter,
                        quote_all,
                    ));
                } else {
                    if strict {
                        return Err(DsonError::SerializationError(format!(
                            "Missing cell at row {row_idx}, column '{col}'"
                        )));
                    }
                    row_values.push(String::new());
                }
            }

            output.push_str(&row_values.join(&delimiter.to_string()));
            output.push('\n');
        } else if strict {
            return Err(DsonError::SerializationError(format!(
                "Row {row_idx} is not an object"
            )));
        }
    }

    Ok(output)
}

#[cfg(feature = "csv")]
fn csv_value_to_string(value: &CsvValue, _quote_all: bool) -> String {
    match value {
        CsvValue::Null => String::new(),
        CsvValue::Bool(b) => b.to_string(),
        CsvValue::Int(n) => n.to_string(),
        CsvValue::Float(f) => f.to_string(),
        CsvValue::String(s) => s.clone(),
        CsvValue::Row(fields) | CsvValue::Object(fields) => {
            // Nested object - serialize as JSON
            let pairs: Vec<String> = fields
                .iter()
                .map(|(k, v)| format!("\"{}\":{}", k, csv_value_to_json(v)))
                .collect();
            format!("{{{}}}", pairs.join(","))
        }
        CsvValue::Array(elements) | CsvValue::Rows(elements) => {
            // Nested array - serialize as JSON
            let items: Vec<String> = elements.iter().map(csv_value_to_json).collect();
            format!("[{}]", items.join(","))
        }
    }
}

#[cfg(feature = "csv")]
fn csv_value_to_json(value: &CsvValue) -> String {
    match value {
        CsvValue::Null => "null".to_string(),
        CsvValue::Bool(b) => b.to_string(),
        CsvValue::Int(n) => n.to_string(),
        CsvValue::Float(f) => f.to_string(),
        CsvValue::String(s) => format!("\"{}\"", s.replace('\"', "\\\"")),
        CsvValue::Row(fields) | CsvValue::Object(fields) => {
            let pairs: Vec<String> = fields
                .iter()
                .map(|(k, v)| format!("\"{}\":{}", k, csv_value_to_json(v)))
                .collect();
            format!("{{{}}}", pairs.join(","))
        }
        CsvValue::Array(elements) | CsvValue::Rows(elements) => {
            let items: Vec<String> = elements.iter().map(csv_value_to_json).collect();
            format!("[{}]", items.join(","))
        }
    }
}

#[cfg(feature = "csv")]
fn quote_csv_field(field: &str, delimiter: char, quote_all: bool) -> String {
    let needs_quoting = quote_all
        || field.contains(delimiter)
        || field.contains('"')
        || field.contains('\n')
        || field.contains('\r');

    if needs_quoting {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

#[cfg(feature = "toon")]
mod toon_impl {
    use super::{FormatKind, Result, ToonBuilder, ValueBuilder};

    /// TOON value type
    #[derive(Debug, Clone)]
    pub enum ToonValue {
        /// Null value
        Null,
        /// Boolean
        Bool(bool),
        /// Integer
        Int(i64),
        /// Float
        Float(f64),
        /// String
        String(String),
        /// Table/object
        Table(Vec<(String, Self)>),
        /// Array
        Array(Vec<Self>),
    }

    impl ValueBuilder for ToonBuilder {
        type Output = ToonValue;

        fn target_format(&self) -> FormatKind {
            FormatKind::Toon
        }

        fn null(&mut self) -> Self::Output {
            ToonValue::Null
        }

        fn bool(&mut self, value: bool) -> Self::Output {
            ToonValue::Bool(value)
        }

        fn int(&mut self, value: i64) -> Self::Output {
            ToonValue::Int(value)
        }

        fn float(&mut self, value: f64) -> Self::Output {
            ToonValue::Float(value)
        }

        fn string(&mut self, value: &str) -> Self::Output {
            ToonValue::String(value.to_string())
        }

        fn empty_object(&mut self) -> Self::Output {
            ToonValue::Table(Vec::new())
        }

        fn empty_array(&mut self) -> Self::Output {
            ToonValue::Array(Vec::new())
        }

        fn insert_field(&mut self, obj: &mut Self::Output, key: &str, value: Self::Output) {
            if let ToonValue::Table(fields) = obj {
                if let Some(field) = fields.iter_mut().find(|(k, _)| k == key) {
                    field.1 = value;
                } else {
                    fields.push((key.to_string(), value));
                }
            }
        }

        fn push_element(&mut self, arr: &mut Self::Output, value: Self::Output) {
            if let ToonValue::Array(elements) = arr {
                elements.push(value);
            }
        }

        fn set_element(&mut self, arr: &mut Self::Output, index: usize, value: Self::Output) {
            if let ToonValue::Array(elements) = arr {
                while elements.len() <= index {
                    elements.push(ToonValue::Null);
                }
                elements[index] = value;
            }
        }

        fn is_null(&self, value: &Self::Output) -> bool {
            matches!(value, ToonValue::Null)
        }

        fn is_object(&self, value: &Self::Output) -> bool {
            matches!(value, ToonValue::Table(_))
        }

        fn is_array(&self, value: &Self::Output) -> bool {
            matches!(value, ToonValue::Array(_))
        }

        fn serialize(&self, value: &Self::Output) -> Result<String> {
            Ok(serialize_toon(value, &[]))
        }
    }

    fn serialize_toon(value: &ToonValue, path: &[String]) -> String {
        use std::fmt::Write as _;

        match value {
            ToonValue::Null => String::new(),
            ToonValue::Bool(b) => b.to_string(),
            ToonValue::Int(n) => n.to_string(),
            ToonValue::Float(f) => f.to_string(),
            ToonValue::String(s) => {
                // Quote strings in TOON format
                format!("\"{}\"", s.replace('\"', "\\\""))
            }
            ToonValue::Table(fields) => {
                let mut output = String::new();
                let mut simple_fields = Vec::new();
                let mut complex_fields = Vec::new();

                // Separate simple vs complex fields
                for (key, val) in fields {
                    match val {
                        ToonValue::Table(_) | ToonValue::Array(_) => {
                            complex_fields.push((key, val));
                        }
                        _ => {
                            simple_fields.push((key, val));
                        }
                    }
                }

                // Emit section header if we have a path
                if !path.is_empty() {
                    let _ = writeln!(output, "[{}]", path.join("."));
                }

                // Emit simple fields first
                for (key, val) in &simple_fields {
                    let _ = writeln!(output, "{key} = {}", serialize_toon(val, &[]));
                }

                // Then nested tables/arrays
                for (key, val) in complex_fields {
                    let mut new_path = path.to_vec();
                    new_path.push(key.clone());

                    match val {
                        ToonValue::Array(elements) => {
                            for elem in elements {
                                output.push('\n');
                                let _ = writeln!(output, "[[{}]]", new_path.join("."));
                                if let ToonValue::Table(inner_fields) = elem {
                                    for (k, v) in inner_fields {
                                        match v {
                                            ToonValue::Table(_) | ToonValue::Array(_) => {
                                                // Nested complex - recurse
                                                let mut nested_path = new_path.clone();
                                                nested_path.push(k.clone());
                                                output.push_str(&serialize_toon(v, &nested_path));
                                            }
                                            _ => {
                                                let _ = writeln!(
                                                    output,
                                                    "{k} = {}",
                                                    serialize_toon(v, &[])
                                                );
                                            }
                                        }
                                    }
                                } else {
                                    // Non-table array element
                                    output.push_str(&serialize_toon(elem, &[]));
                                    output.push('\n');
                                }
                            }
                        }
                        ToonValue::Table(_) => {
                            output.push('\n');
                            output.push_str(&serialize_toon(val, &new_path));
                        }
                        _ => {}
                    }
                }

                output
            }
            ToonValue::Array(elements) => {
                // Inline array for simple values
                let items: Vec<String> = elements.iter().map(|e| serialize_toon(e, &[])).collect();
                format!("[{}]", items.join(", "))
            }
        }
    }
}
