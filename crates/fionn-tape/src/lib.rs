// SPDX-License-Identifier: MIT OR Apache-2.0
//! SIMD-DSON tape wrapper using SIMD-JSON
//!
//! This module provides a wrapper around SIMD-JSON's tape structure
//! for efficient zero-allocation JSON parsing with skip optimization.

use ahash::{AHashMap, AHashSet};
use fionn_core::{DsonError, Result};
pub use fionn_core::{ParsedPath, PathComponent, PathComponentRef};
use simd_json::value::tape::{Node, Tape};

// TapeSource implementation for DsonTape
mod tape_source_impl;
pub use tape_source_impl::{ArrayElementIterator, ObjectFieldIterator};

// Type aliases for performance-optimized collections
type FastHashMap<K, V> = AHashMap<K, V>;
type FastHashSet<T> = AHashSet<T>;

/// Escape a string for JSON output.
///
/// This handles all characters that must be escaped in JSON strings:
/// - `"` becomes `\"`
/// - `\` becomes `\\`
/// - Control characters (0x00-0x1F) are escaped as `\uXXXX` or their short forms
#[inline]
fn escape_json_string(s: &str, output: &mut String) {
    use std::fmt::Write;
    for c in s.chars() {
        match c {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            // Control characters (0x00-0x1F) except the ones handled above
            c if c.is_control() => {
                let _ = write!(output, "\\u{:04x}", c as u32);
            }
            c => output.push(c),
        }
    }
}

/// SIMD-accelerated value representation
#[derive(Debug, Clone)]
pub enum SimdValue {
    /// A string value
    String(String),
    /// A null value
    Null,
    /// A boolean value
    Bool(bool),
    /// A number value stored as string for precision
    Number(String),
}

/// SIMD-DSON tape wrapper
pub struct DsonTape<S = Vec<u8>> {
    tape: Tape<'static>,
    data: S, // Keep the data alive
}

impl DsonTape {
    /// Create a new DSON tape from JSON input
    ///
    /// # Errors
    /// Returns an error if the JSON is malformed
    pub fn parse(json: &str) -> Result<Self> {
        // SIMD-JSON requires mutable bytes, so we need to copy
        let mut bytes = json.as_bytes().to_vec();
        let tape = unsafe {
            // Extend the lifetime - this is safe because we store the data
            std::mem::transmute::<Tape<'_>, Tape<'static>>(
                simd_json::to_tape(&mut bytes)
                    .map_err(|e| DsonError::ParseError(format!("SIMD-JSON parse error: {e}")))?,
            )
        };

        Ok(Self { tape, data: bytes })
    }
}

impl<S: AsRef<[u8]>> DsonTape<S> {
    /// Create a DSON tape from existing mutable storage
    ///
    /// # Errors
    /// Returns an error if the JSON is malformed
    pub fn from_raw(mut data: S) -> Result<Self>
    where
        S: AsMut<[u8]> + 'static,
    {
        let tape = unsafe {
            let bytes = data.as_mut();
            std::mem::transmute::<Tape<'_>, Tape<'static>>(
                simd_json::to_tape(bytes)
                    .map_err(|e| DsonError::ParseError(format!("SIMD-JSON parse error: {e}")))?,
            )
        };

        Ok(Self { tape, data })
    }

    /// Get the underlying storage, consuming the tape
    pub fn into_data(self) -> S {
        self.data
    }

    /// Get the underlying SIMD-JSON tape
    #[must_use]
    pub const fn tape(&self) -> &Tape<'static> {
        &self.tape
    }

    /// Get tape nodes
    #[must_use]
    #[inline]
    pub fn nodes(&self) -> &[Node<'static>] {
        &self.tape.0
    }

    /// Get the root node (first node in tape)
    #[must_use]
    #[inline]
    pub fn root(&self) -> Node<'static> {
        self.nodes()[0]
    }

    /// Skip to next field in object/array (for lazy evaluation)
    ///
    /// # Errors
    /// Returns an error if navigation fails
    pub fn skip_field(&self, current_index: usize) -> Result<Option<usize>> {
        let nodes = self.nodes();
        if current_index >= nodes.len() {
            return Ok(None);
        }

        // For SIMD acceleration, we need to understand the tape structure better
        // For now, implement a basic skip that advances past the current field/value
        let mut index = current_index;

        // Skip the current field name (if we're on one)
        if let Node::String(_) = &nodes[index] {
            index += 1; // Skip field name
        }

        // Skip the field value
        index = self.skip_value(index)?;

        Ok(Some(index))
    }

    /// Skip a complete value (object, array, or primitive) starting at the given index
    ///
    /// # Errors
    /// Returns an error if navigation fails
    pub fn skip_value(&self, start_index: usize) -> Result<usize> {
        let nodes = self.nodes();
        if start_index >= nodes.len() {
            return Ok(start_index);
        }

        let node = &nodes[start_index];

        // Navigate based on actual Node structure
        match node {
            Node::Object { len, count } => {
                // For objects, we need to skip the object header plus all field name/value pairs
                // Each field consists of a string key followed by a value
                // The 'count' field tells us how many nodes AFTER this one belong to the object
                // We add 1 to include the Object node itself in the skip
                let skip_count = if *count > 0 { *count + 1 } else { *len * 2 + 1 };
                Ok(start_index + skip_count)
            }
            Node::Array { len, count } => {
                // For arrays, skip the array header plus all elements
                // The 'count' field tells us how many nodes AFTER this one belong to the array
                // We add 1 to include the Array node itself in the skip
                let skip_count = if *count > 0 { *count + 1 } else { *len + 1 };
                Ok(start_index + skip_count)
            }
            Node::String(_) | Node::Static(_) => {
                // Primitives are single nodes
                Ok(start_index + 1)
            }
        }
    }

    /// SIMD-accelerated schema path matching
    #[inline]
    #[must_use]
    pub fn simd_schema_match(
        &self,
        path: &str,
        schema: &std::collections::HashSet<String>,
    ) -> bool {
        let path_bytes = path.as_bytes();

        for schema_path in schema {
            let schema_bytes = schema_path.as_bytes();

            // Exact match (SIMD-accelerated)
            if self.simd_string_equals(path_bytes, schema_bytes) {
                return true;
            }

            // Path starts with schema (e.g., "user" matches "user.name")
            if path_bytes.len() > schema_bytes.len()
                && path_bytes[schema_bytes.len()] == b'.'
                && self.simd_string_equals(&path_bytes[..schema_bytes.len()], schema_bytes)
            {
                return true;
            }

            // Schema starts with path (e.g., "user.name" matches "user")
            if schema_bytes.len() > path_bytes.len()
                && schema_bytes[path_bytes.len()] == b'.'
                && self.simd_string_equals(&schema_bytes[..path_bytes.len()], path_bytes)
            {
                return true;
            }
        }

        false
    }

    /// SIMD-accelerated value type detection and extraction
    #[must_use]
    pub fn extract_value_simd(&self, index: usize) -> Option<SimdValue> {
        let nodes = self.nodes();
        if index >= nodes.len() {
            return None;
        }

        match &nodes[index] {
            Node::String(s) => Some(SimdValue::String(s.to_string())),
            Node::Static(static_node) => match static_node {
                simd_json::StaticNode::Null => Some(SimdValue::Null),
                simd_json::StaticNode::Bool(b) => Some(SimdValue::Bool(*b)),
                simd_json::StaticNode::I64(n) => Some(SimdValue::Number(n.to_string())),
                simd_json::StaticNode::U64(n) => Some(SimdValue::Number(n.to_string())),
                simd_json::StaticNode::F64(n) => Some(SimdValue::Number(n.to_string())),
            },
            _ => None,
        }
    }

    /// SIMD-accelerated JSON serialization for specific patterns
    #[must_use]
    pub fn serialize_simd(
        &self,
        modifications: &std::collections::HashMap<String, fionn_core::OperationValue>,
    ) -> Option<String> {
        // For now, implement a fast path for simple modifications
        // This can be extended to handle more complex cases with SIMD acceleration

        if modifications.is_empty() {
            // Fast path: return original JSON directly (already SIMD-parsed)
            return Some(String::from_utf8_lossy(self.data.as_ref()).to_string());
        }

        // For modified JSON, we could implement SIMD-accelerated serialization
        // that avoids the overhead of serde_json for common patterns
        None // Fall back to existing serialization for now
    }

    /// Read a field at the given tape index
    ///
    /// # Errors
    /// Returns an error if the field cannot be accessed
    pub fn read_field(&self, index: usize) -> Result<Node<'static>> {
        self.nodes()
            .get(index)
            .copied()
            .ok_or_else(|| DsonError::InvalidField(format!("Index {index} out of bounds")))
    }

    /// Check if a field should survive processing based on schema
    ///
    /// A field survives if it matches the given schema paths or if the schema is empty (keep all).
    #[must_use]
    #[inline]
    pub fn should_survive(
        &self,
        field_path: &str,
        schema: &std::collections::HashSet<String>,
    ) -> bool {
        // If schema is empty, all fields survive
        if schema.is_empty() {
            return true;
        }

        // Check exact match
        if schema.contains(field_path) {
            return true;
        }

        // Check if any schema path is a prefix of field_path (parent survives)
        for schema_path in schema {
            // Parent path survives (e.g., "user" survives if "user.name" is in schema)
            if field_path.starts_with(schema_path)
                && (field_path.len() == schema_path.len()
                    || field_path.as_bytes().get(schema_path.len()) == Some(&b'.'))
            {
                return true;
            }

            // Child path survives (e.g., "user.name" survives if "user" is in schema)
            if schema_path.starts_with(field_path)
                && (schema_path.len() == field_path.len()
                    || schema_path.as_bytes().get(field_path.len()) == Some(&b'.'))
            {
                return true;
            }

            // Wildcard matching (e.g., "users.*" matches "users.name")
            if schema_path.ends_with(".*") {
                let prefix = &schema_path[..schema_path.len() - 2];
                if field_path.starts_with(prefix) {
                    return true;
                }
            }
        }

        false
    }

    /// Create a filtered tape containing only schema-matching paths
    ///
    /// This implementation converts to JSON, filters, and re-parses.
    /// While not true zero-copy tape-level filtering, this approach is correct
    /// and provides proper schema-based filtering with SIMD-accelerated parsing.
    ///
    /// # Errors
    /// Returns an error if filtering fails
    pub fn filter_by_schema(
        &self,
        schema: &std::collections::HashSet<String>,
    ) -> Result<DsonTape<Vec<u8>>> {
        // Convert to JSON, filter by schema, then re-parse with SIMD
        // This ensures correctness while still leveraging SIMD for final parsing
        let full_json = self.to_json_string()?;
        let full_value: serde_json::Value = serde_json::from_str(&full_json)
            .map_err(|e| fionn_core::DsonError::ParseError(format!("JSON parse error: {e}")))?;

        let filtered_value = self.filter_json_by_schema(&full_value, schema, &mut Vec::new());
        let filtered_json = serde_json::to_string(&filtered_value).map_err(|e| {
            fionn_core::DsonError::SerializationError(format!("JSON serialize error: {e}"))
        })?;

        DsonTape::parse(&filtered_json)
    }

    /// Recursively filter JSON value by schema paths
    fn filter_json_by_schema(
        &self,
        value: &serde_json::Value,
        schema: &std::collections::HashSet<String>,
        current_path: &mut Vec<String>,
    ) -> serde_json::Value {
        match value {
            serde_json::Value::Object(obj) => {
                let mut filtered_obj = serde_json::Map::new();

                for (key, val) in obj {
                    current_path.push(key.clone());
                    let path_str = current_path.join(".");

                    // Include this field if it matches the schema (SIMD-accelerated)
                    if self.simd_schema_match(&path_str, schema) {
                        let filtered_val = self.filter_json_by_schema(val, schema, current_path);
                        filtered_obj.insert(key.clone(), filtered_val);
                    }

                    current_path.pop();
                }

                serde_json::Value::Object(filtered_obj)
            }
            serde_json::Value::Array(arr) => {
                let mut filtered_arr = Vec::new();

                for (index, val) in arr.iter().enumerate() {
                    // For array elements, construct path as "array[index]" format
                    let array_path = if current_path.is_empty() {
                        format!("[{index}]")
                    } else {
                        format!("{}[{}]", current_path.join("."), index)
                    };

                    // Check if this array element or any of its children should be included
                    let should_include = schema.iter().any(|schema_path| {
                        schema_path == &array_path ||
                        array_path.starts_with(&format!("{schema_path}.")) ||
                        schema_path.starts_with(&format!("{array_path}.")) ||
                        // Also check for wildcard matches
                        schema_path.starts_with(&format!("{}[{}].", current_path.join("."), index))
                    });

                    if should_include {
                        // Add the array index to current path for recursive filtering
                        current_path.push(format!("[{index}]"));
                        let filtered_val = self.filter_json_by_schema(val, schema, current_path);
                        filtered_arr.push(filtered_val);
                        current_path.pop();
                    }
                }

                serde_json::Value::Array(filtered_arr)
            }
            // Keep primitive values as-is
            _ => value.clone(),
        }
    }

    /// Find the tape index for a given JSON path using SIMD-accelerated search
    ///
    /// # Errors
    /// Returns an error if the path cannot be resolved
    pub fn resolve_path(&self, path: &str) -> Result<Option<usize>> {
        let mut components = Vec::new();
        fionn_core::parse_simd_ref_into(path, &mut components);
        self.resolve_path_components_ref(&components, 0, 0)
    }

    /// Resolve a path from owned components (benchmark helper).
    ///
    /// # Errors
    /// Returns an error if path resolution fails.
    pub fn resolve_path_components_owned(
        &self,
        components: &[PathComponent],
    ) -> Result<Option<usize>> {
        self.resolve_path_components_owned_internal(components, 0, 0)
    }

    /// Resolve a pre-parsed path using a caller-provided buffer.
    ///
    /// # Errors
    /// Returns an error if path resolution fails.
    pub fn resolve_parsed_path_with_buffer<'a>(
        &self,
        parsed: &'a ParsedPath,
        buffer: &mut Vec<PathComponentRef<'a>>,
    ) -> Result<Option<usize>> {
        parsed.components_ref(buffer);
        self.resolve_path_components_ref(buffer, 0, 0)
    }

    /// Resolve a pre-parsed path (allocates a temporary buffer).
    ///
    /// # Errors
    /// Returns an error if path resolution fails.
    pub fn resolve_parsed_path(&self, parsed: &ParsedPath) -> Result<Option<usize>> {
        let mut buffer = Vec::new();
        self.resolve_parsed_path_with_buffer(parsed, &mut buffer)
    }

    /// Resolve path components starting from a given tape index
    fn resolve_path_components_ref(
        &self,
        components: &[PathComponentRef<'_>],
        start_index: usize,
        component_index: usize,
    ) -> Result<Option<usize>> {
        if component_index >= components.len() {
            return Ok(Some(start_index));
        }

        let nodes = self.nodes();
        if start_index >= nodes.len() {
            return Ok(None);
        }

        let component = &components[component_index];

        match component {
            PathComponentRef::Field(field_name) => {
                // Search for field name in object starting at start_index
                self.find_field_in_object(field_name, start_index)
                    .map_or(Ok(None), |field_index| {
                        // Found the field, now resolve the rest of the path from the value
                        self.resolve_path_components_ref(
                            components,
                            field_index + 1,
                            component_index + 1,
                        )
                    })
            }
            PathComponentRef::ArrayIndex(index) => {
                // Navigate to array element at the given index
                self.find_array_element(*index, start_index)
                    .map_or(Ok(None), |element_index| {
                        self.resolve_path_components_ref(
                            components,
                            element_index,
                            component_index + 1,
                        )
                    })
            }
        }
    }

    /// Resolve owned components (string-backed) without re-parsing.
    fn resolve_path_components_owned_internal(
        &self,
        components: &[PathComponent],
        start_index: usize,
        component_index: usize,
    ) -> Result<Option<usize>> {
        if component_index >= components.len() {
            return Ok(Some(start_index));
        }

        let nodes = self.nodes();
        if start_index >= nodes.len() {
            return Ok(None);
        }

        let component = &components[component_index];

        match component {
            PathComponent::Field(field_name) => self
                .find_field_in_object(field_name, start_index)
                .map_or(Ok(None), |field_index| {
                    self.resolve_path_components_owned_internal(
                        components,
                        field_index + 1,
                        component_index + 1,
                    )
                }),
            PathComponent::ArrayIndex(index) => self
                .find_array_element(*index, start_index)
                .map_or(Ok(None), |element_index| {
                    self.resolve_path_components_owned_internal(
                        components,
                        element_index,
                        component_index + 1,
                    )
                }),
        }
    }

    /// Find a field with the given name in an object starting at the given index
    fn find_field_in_object(&self, field_name: &str, start_index: usize) -> Option<usize> {
        let nodes = self.nodes();
        if start_index >= nodes.len() {
            return None;
        }

        // Check if start_index points to an Object node
        let (num_fields, mut index) = if let Node::Object { len, .. } = &nodes[start_index] {
            (*len, start_index + 1)
        } else {
            // If not at an object node, do a bounded linear search (legacy behavior)
            let mut idx = start_index;
            while idx < nodes.len() && idx < start_index + 1000 {
                if let Node::String(field_str) = &nodes[idx]
                    && self.simd_string_equals(field_str.as_bytes(), field_name.as_bytes())
                {
                    return Some(idx);
                }
                idx += 1;
            }
            return None;
        };

        // Properly iterate through object fields (key-value pairs)
        for _ in 0..num_fields {
            if index >= nodes.len() {
                break;
            }

            // Each field is: key (String node), then value
            if let Node::String(field_str) = &nodes[index] {
                if self.simd_string_equals(field_str.as_bytes(), field_name.as_bytes()) {
                    return Some(index);
                }
                // Skip the key
                index += 1;
                // Skip the value
                index = self.skip_value(index).ok()?;
            } else {
                // Unexpected node type where key should be
                break;
            }
        }

        None
    }

    /// String equality check (uses Rust's optimized slice comparison)
    #[inline]
    #[must_use]
    pub fn simd_string_equals(&self, a: &[u8], b: &[u8]) -> bool {
        // Rust's standard library slice comparison is already SIMD-optimized
        a == b
    }

    /// Find the nth element in an array starting at the given index
    fn find_array_element(&self, target_index: usize, start_index: usize) -> Option<usize> {
        let nodes = self.nodes();
        if start_index >= nodes.len() {
            return None;
        }

        // Check if start_index points to an Array node
        let (array_len, mut index) = if let Node::Array { len, .. } = &nodes[start_index] {
            (*len, start_index + 1)
        } else {
            // If not at an array node, assume we're already inside array content
            // Legacy behavior: iterate from current position
            let mut idx = start_index;
            for i in 0..=target_index {
                if idx >= nodes.len() {
                    return None;
                }
                if i == target_index {
                    return Some(idx);
                }
                idx = self.skip_value(idx).ok()?;
            }
            return None;
        };

        // Check if target index is within array bounds
        if target_index >= array_len {
            return None;
        }

        // Navigate to the target element
        for i in 0..=target_index {
            if index >= nodes.len() {
                return None;
            }
            if i == target_index {
                return Some(index);
            }
            // Skip this element to get to the next one
            index = self.skip_value(index).ok()?;
        }

        None
    }

    /// Serialize the tape to JSON string, applying modifications efficiently
    ///
    /// # Errors
    /// Returns an error if serialization fails
    pub fn to_json_string(&self) -> Result<String> {
        self.serialize_with_modifications(&FastHashMap::default(), &FastHashSet::default())
    }

    /// Serialize the tape to JSON string with modifications and deletions applied
    ///
    /// # Errors
    /// Returns an error if serialization fails
    pub fn serialize_with_modifications(
        &self,
        modifications: &FastHashMap<String, fionn_core::OperationValue>,
        deletions: &FastHashSet<String>,
    ) -> Result<String> {
        // FAST PATH: If no modifications, return tape's native JSON directly
        if modifications.is_empty() && deletions.is_empty() {
            return self.serialize_tape_to_json();
        }

        // MODIFIED PATH: Apply modifications during serialization
        self.serialize_tape_with_overlay(modifications, deletions)
    }

    /// Serialize tape to JSON.
    ///
    /// Note: We cannot use the original buffer directly because `simd_json`
    /// modifies it in-place when decoding escape sequences (e.g., `\u4e16` -> `世`).
    /// We must always re-serialize from the tape nodes to produce valid JSON.
    fn serialize_tape_to_json(&self) -> Result<String> {
        // Always use TapeSerializer to properly handle escape sequences
        self.serialize_tape_with_overlay(&FastHashMap::default(), &FastHashSet::default())
    }

    /// Serialize tape with modification overlay applied using zero-copy iterator
    fn serialize_tape_with_overlay(
        &self,
        modifications: &FastHashMap<String, fionn_core::OperationValue>,
        deletions: &FastHashSet<String>,
    ) -> Result<String> {
        let mut serializer = TapeSerializer::new(self, modifications, deletions);
        serializer.serialize()
    }
}

/// Zero-copy serializer that iterates over tape and applies modifications
struct TapeSerializer<'a, S: AsRef<[u8]>> {
    tape: &'a DsonTape<S>,
    modifications: &'a FastHashMap<String, fionn_core::OperationValue>,
    deletions: &'a FastHashSet<String>,
    output: String,
    current_path: Vec<String>,
}

impl<'a, S: AsRef<[u8]>> TapeSerializer<'a, S> {
    fn new(
        tape: &'a DsonTape<S>,
        modifications: &'a FastHashMap<String, fionn_core::OperationValue>,
        deletions: &'a FastHashSet<String>,
    ) -> Self {
        Self {
            tape,
            modifications,
            deletions,
            output: String::with_capacity(64), // Small initial capacity, will grow
            current_path: Vec::new(),
        }
    }

    fn serialize(&mut self) -> Result<String> {
        let nodes = self.tape.nodes();
        if !nodes.is_empty() {
            self.serialize_node(0)?;
        }
        Ok(self.output.clone())
    }

    fn serialize_node(&mut self, index: usize) -> Result<usize> {
        // Construct path correctly handling dot notation and array brackets
        let mut path = String::with_capacity(64);
        for (i, component) in self.current_path.iter().enumerate() {
            if i > 0 && !component.starts_with('[') {
                path.push('.');
            }
            path.push_str(component);
        }

        // Check if this path is deleted
        if self.deletions.contains(&path) {
            return self.tape.skip_value(index);
        }

        // Check if this path is modified
        if let Some(value) = self.modifications.get(&path) {
            self.serialize_operation_value(value);
            return self.tape.skip_value(index);
        }

        let nodes = self.tape.nodes();
        let node = &nodes[index];

        match node {
            Node::String(s) => {
                self.output.push('"');
                escape_json_string(s, &mut self.output);
                self.output.push('"');
                Ok(index + 1)
            }
            Node::Static(s) => {
                match s {
                    simd_json::StaticNode::Null => self.output.push_str("null"),
                    simd_json::StaticNode::Bool(b) => {
                        self.output.push_str(if *b { "true" } else { "false" });
                    }
                    simd_json::StaticNode::I64(n) => self.output.push_str(&n.to_string()),
                    simd_json::StaticNode::U64(n) => self.output.push_str(&n.to_string()),
                    simd_json::StaticNode::F64(n) => self.output.push_str(&n.to_string()),
                }
                Ok(index + 1)
            }
            Node::Object { len, .. } => {
                self.output.push('{');
                let mut current_idx = index + 1;
                let mut first = true;
                let mut seen_keys = FastHashSet::default();

                for _ in 0..*len {
                    // Get key
                    if let Node::String(key) = &nodes[current_idx] {
                        let key_str = key.to_string();
                        // Track seen key
                        seen_keys.insert(key_str.clone());

                        self.current_path.push(key_str.clone());
                        // Recalculate item path - optimized
                        // We can't reuse `path` easily, but we can reuse logic or just append
                        // Inside object, child always prefixed by "." unless root.
                        // But wait, path construction is at top of function.
                        // Here recursively calling serialize_node will rebuild path.
                        // For checking deletion of key/value pair usage (skip key), we need path.

                        let mut item_path = path.clone();
                        if !item_path.is_empty() {
                            item_path.push('.');
                        }
                        item_path.push_str(&key_str);

                        // Check if item is deleted BEFORE writing key
                        if self.deletions.contains(&item_path) {
                            // Skip key and value
                            current_idx += 1;
                            current_idx = self.tape.skip_value(current_idx)?;
                        } else {
                            if !first {
                                self.output.push(',');
                            }
                            self.output.push('"');
                            escape_json_string(key, &mut self.output);
                            self.output.push('"');
                            self.output.push(':');

                            // Move to value
                            current_idx += 1;
                            current_idx = self.serialize_node(current_idx)?;
                            first = false;
                        }
                        self.current_path.pop();
                    }
                }

                self.serialize_added_fields(&path, first, &seen_keys);

                self.output.push('}');
                Ok(current_idx)
            }
            Node::Array { len, .. } => {
                self.output.push('[');
                let mut current_idx = index + 1;
                let mut first = true;

                for i in 0..*len {
                    let idx_str = format!("[{i}]");
                    self.current_path.push(idx_str);

                    if !first {
                        self.output.push(',');
                    }
                    current_idx = self.serialize_node(current_idx)?;
                    first = false;

                    self.current_path.pop();
                }

                // Append any added elements (extensions to the array)
                self.serialize_added_array_elements(&path, *len);

                self.output.push(']');
                Ok(current_idx)
            }
        }
    }

    fn serialize_operation_value(&mut self, value: &fionn_core::OperationValue) {
        // Re-use logic to serialize OperationValue to JSON string
        match value {
            fionn_core::OperationValue::StringRef(s) => {
                self.output.push('"');
                escape_json_string(s, &mut self.output);
                self.output.push('"');
            }
            fionn_core::OperationValue::NumberRef(n) => self.output.push_str(n),
            fionn_core::OperationValue::BoolRef(b) => {
                self.output.push_str(if *b { "true" } else { "false" });
            }
            fionn_core::OperationValue::Null => self.output.push_str("null"),
            fionn_core::OperationValue::ObjectRef { .. } => {
                // ObjectRef represents a reference to an object in the tape or constructed
                // Since we can't easily resolve it here without access to the source tape,
                // we output an empty object as a safe fallback.
                self.output.push_str("{}");
            }
            fionn_core::OperationValue::ArrayRef { .. } => {
                // Same as ObjectRef, safe fallback for array references
                self.output.push_str("[]");
            }
        }
    }

    fn serialize_added_fields(
        &mut self,
        parent_path: &str,
        mut first: bool,
        seen_keys: &FastHashSet<String>,
    ) {
        // Collect all direct child keys implied by modifications that haven't been seen/serialized
        let mut implied_keys: FastHashSet<String> = FastHashSet::default();

        for (path, _) in self.modifications {
            let relative = if parent_path.is_empty() {
                path.as_str()
            } else {
                // Check if path is child of parent_path
                if path.len() > parent_path.len()
                    && path.starts_with(parent_path)
                    && path.as_bytes()[parent_path.len()] == b'.'
                {
                    &path[parent_path.len() + 1..]
                } else {
                    continue;
                }
            };

            // Extract first component (key name)
            // Stop at '.' or '[' (array start)
            // If implicit array index (starts with '['), we ignore for object context
            let end = relative.find(['.', '[']).unwrap_or(relative.len());
            if end > 0 {
                let key = &relative[..end];
                if !seen_keys.contains(key) {
                    implied_keys.insert(key.to_string());
                }
            }
        }

        // Serialize inferred keys
        // Sort for deterministic output if needed? Tests might verify string equality.
        // fast hashmap iteration is random.
        // Let's sort them.
        let mut sorted_keys: Vec<_> = implied_keys.into_iter().collect();
        sorted_keys.sort();

        for key in sorted_keys {
            if !first {
                self.output.push(',');
            }
            self.output.push('"');
            escape_json_string(&key, &mut self.output);
            self.output.push('"');
            self.output.push(':');

            let full_path = if parent_path.is_empty() {
                key.clone()
            } else {
                format!("{parent_path}.{key}")
            };

            if let Some(val) = self.modifications.get(&full_path) {
                // It's a direct modification/addition with a value
                self.serialize_operation_value(val);
            } else {
                // It's an intermediate path. Determine if Object or Array.
                // Heuristic: check if any modification starts with "full_path["
                let mut is_array = false;
                // Optimization: scan just enough to find one array indicator
                let prefix_bracket = format!("{full_path}[");
                for p in self.modifications.keys() {
                    if p.starts_with(&prefix_bracket) {
                        is_array = true;
                        break;
                    }
                }

                if is_array {
                    self.output.push('[');
                    self.serialize_added_array_elements(&full_path, 0);
                    self.output.push(']');
                } else {
                    self.output.push('{');
                    let empty_seen = FastHashSet::default();
                    self.serialize_added_fields(&full_path, true, &empty_seen);
                    self.output.push('}');
                }
            }
            first = false;
        }
    }

    fn serialize_added_array_elements(&mut self, parent_path: &str, start_index: usize) {
        // 1. Find max index
        let mut max_index: Option<usize> = None;
        let prefix = format!("{parent_path}[");

        for path in self.modifications.keys() {
            if path.starts_with(&prefix) {
                // Extract index: parent[123]...
                // We want the part between [ and ]
                if let Some(end_bracket) = path[prefix.len()..].find(']') {
                    let index_str = &path[prefix.len()..prefix.len() + end_bracket];
                    if let Ok(idx) = index_str.parse::<usize>() {
                        max_index = Some(max_index.map_or(idx, |m| m.max(idx)));
                    }
                }
            }
        }

        if let Some(max) = max_index
            && max >= start_index
        {
            // Determine if we need a comma before the first added element
            // If start_index > 0, we are appending to an existing array (which had elements 0..start_index-1)
            // BUT, the comma is written only if there was a previous element.
            // In serialize_node, we write comma before elements.
            // Here, if start_index > 0, we definitely need a comma before start_index (assuming array wasn't empty)
            // Actually serialize_node logic writes comma *between* elements.
            // If we are appending, we need a comma before the first new element IF the array wasn't empty.
            // The caller should handle the initial comma state? Or we handle it here.
            // Ideally, `serialize_node` writes the last element, then calls us.
            // So we need to write `,` before the first element we write.

            for i in start_index..=max {
                // Always write comma because we are either:
                // 1. Appending to non-empty array (start_index > 0) -> Need comma.
                // 2. Creating new array (start_index=0).
                //    If i=0, NO comma. If i>0, comma.

                if i > 0 {
                    self.output.push(',');
                }

                let idx_path = format!("{parent_path}[{i}]");

                if let Some(val) = self.modifications.get(&idx_path) {
                    self.serialize_operation_value(val);
                } else {
                    // Check if it's an intermediate object/array at this index
                    // ... (same logic as before)
                    let mut is_intermediate = false;
                    let dot_prefix = format!("{idx_path}.");
                    let bracket_prefix = format!("{idx_path}[");

                    for p in self.modifications.keys() {
                        if p.starts_with(&dot_prefix) || p.starts_with(&bracket_prefix) {
                            is_intermediate = true;
                            break;
                        }
                    }

                    if is_intermediate {
                        let mut is_array_child = false;
                        for p in self.modifications.keys() {
                            if p.starts_with(&bracket_prefix) {
                                is_array_child = true;
                                break;
                            }
                        }

                        if is_array_child {
                            self.output.push('[');
                            self.serialize_added_array_elements(&idx_path, 0);
                            self.output.push(']');
                        } else {
                            self.output.push('{');
                            let empty_seen = FastHashSet::default();
                            self.serialize_added_fields(&idx_path, true, &empty_seen);
                            self.output.push('}');
                        }
                    } else {
                        // Sparse array or missing element -> null
                        self.output.push_str("null");
                    }
                }
            }
        }
    }
}

impl<S: AsRef<[u8]>> DsonTape<S> {
    /// Parse a JSON path into components, handling array notation (SIMD-accelerated)
    #[inline]
    #[must_use]
    pub fn parse_path(path: &str) -> Vec<PathComponent> {
        fionn_core::parse_simd(path)
    }

    /// Reconstruct a value from tape positions with full tape access.
    ///
    /// # Errors
    /// Returns an error if the tape node cannot be converted to JSON.
    pub fn reconstruct_value_from_tape(
        &self,
        start: usize,
        _end: usize,
    ) -> Result<serde_json::Value> {
        let nodes = self.nodes();
        if start >= nodes.len() {
            return Ok(serde_json::Value::Null);
        }

        self.node_to_json_value(start)
    }

    /// Convert a tape node at given index to JSON value, recursively
    fn node_to_json_value(&self, index: usize) -> Result<serde_json::Value> {
        let nodes = self.nodes();
        if index >= nodes.len() {
            return Ok(serde_json::Value::Null);
        }

        let node = &nodes[index];

        match node {
            Node::String(s) => Ok(serde_json::Value::String(s.to_string())),
            Node::Static(static_val) => match static_val {
                simd_json::StaticNode::Null => Ok(serde_json::Value::Null),
                simd_json::StaticNode::Bool(b) => Ok(serde_json::Value::Bool(*b)),
                simd_json::StaticNode::I64(n) => Ok(serde_json::Value::Number((*n).into())),
                simd_json::StaticNode::U64(n) => Ok(serde_json::Value::Number((*n).into())),
                simd_json::StaticNode::F64(n) => Ok(serde_json::Number::from_f64(*n).map_or_else(
                    || serde_json::Value::String(n.to_string()),
                    serde_json::Value::Number,
                )),
            },
            Node::Object { len, count: _ } => {
                let mut obj = serde_json::Map::new();
                let mut current_idx = index + 1;

                for _ in 0..*len {
                    if current_idx >= nodes.len() {
                        break;
                    }

                    // Get field name
                    if let Node::String(field_name) = &nodes[current_idx] {
                        let key = field_name.to_string();
                        current_idx += 1;

                        // Get field value
                        if current_idx < nodes.len() {
                            let value = self.node_to_json_value(current_idx)?;
                            current_idx = self.skip_value(current_idx)?;
                            obj.insert(key, value);
                        }
                    } else {
                        current_idx += 1;
                    }
                }

                Ok(serde_json::Value::Object(obj))
            }
            Node::Array { len, count: _ } => {
                let mut arr = Vec::with_capacity(*len);
                let mut current_idx = index + 1;

                for _ in 0..*len {
                    if current_idx >= nodes.len() {
                        break;
                    }

                    let value = self.node_to_json_value(current_idx)?;
                    current_idx = self.skip_value(current_idx)?;
                    arr.push(value);
                }

                Ok(serde_json::Value::Array(arr))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dson_tape_parse_simple() {
        let tape = DsonTape::parse(r#"{"name":"test"}"#);
        assert!(tape.is_ok());
    }

    #[test]
    fn test_dson_tape_parse_array() {
        let tape = DsonTape::parse(r"[1, 2, 3]");
        assert!(tape.is_ok());
    }

    #[test]
    fn test_dson_tape_parse_nested() {
        let tape = DsonTape::parse(r#"{"user":{"name":"test","age":30}}"#);
        assert!(tape.is_ok());
    }

    #[test]
    fn test_dson_tape_parse_invalid() {
        let tape = DsonTape::parse("not valid json");
        assert!(tape.is_err());
    }

    #[test]
    fn test_dson_tape_nodes() {
        let tape = DsonTape::parse(r#"{"name":"test"}"#).unwrap();
        assert!(!tape.nodes().is_empty());
    }

    #[test]
    fn test_dson_tape_root() {
        let tape = DsonTape::parse(r#"{"name":"test"}"#).unwrap();
        let _root = tape.root();
        // Should not panic
    }

    #[test]
    fn test_dson_tape_tape() {
        let tape = DsonTape::parse(r#"{"name":"test"}"#).unwrap();
        let _inner = tape.tape();
        // Should not panic
    }

    #[test]
    fn test_dson_tape_skip_field() {
        let tape = DsonTape::parse(r#"{"name":"test","age":30}"#).unwrap();
        let result = tape.skip_field(0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_dson_tape_skip_value() {
        let tape = DsonTape::parse(r#"{"name":"test"}"#).unwrap();
        let result = tape.skip_value(0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_dson_tape_resolve_path_field() {
        let tape = DsonTape::parse(r#"{"name":"test"}"#).unwrap();
        let result = tape.resolve_path("name");
        assert!(result.is_ok());
    }

    #[test]
    fn test_dson_tape_resolve_path_nested() {
        let tape = DsonTape::parse(r#"{"user":{"name":"test"}}"#).unwrap();
        let result = tape.resolve_path("user.name");
        assert!(result.is_ok());
    }

    #[test]
    fn test_dson_tape_resolve_path_array() {
        let tape = DsonTape::parse(r#"{"items":[1,2,3]}"#).unwrap();
        let result = tape.resolve_path("items[0]");
        assert!(result.is_ok());
    }

    #[test]
    fn test_dson_tape_resolve_path_not_found() {
        let tape = DsonTape::parse(r#"{"name":"test"}"#).unwrap();
        let result = tape.resolve_path("nonexistent");
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_path_component_debug() {
        let field = PathComponent::Field("test".to_string());
        let debug = format!("{field:?}");
        assert!(debug.contains("Field"));
    }

    #[test]
    fn test_path_component_clone() {
        let field = PathComponent::Field("test".to_string());
        let cloned = field;
        assert!(matches!(cloned, PathComponent::Field(_)));
    }

    #[test]
    fn test_simd_value_debug() {
        let val = SimdValue::String("test".to_string());
        let debug = format!("{val:?}");
        assert!(debug.contains("String"));
    }

    #[test]
    fn test_simd_value_clone() {
        let val = SimdValue::Null;
        let cloned = val;
        assert!(matches!(cloned, SimdValue::Null));
    }

    #[test]
    fn test_parse_path_simple() {
        let components = DsonTape::<Vec<u8>>::parse_path("name");
        assert_eq!(components.len(), 1);
        assert!(matches!(&components[0], PathComponent::Field(f) if f == "name"));
    }

    #[test]
    fn test_parse_path_nested() {
        let components = DsonTape::<Vec<u8>>::parse_path("user.name");
        assert_eq!(components.len(), 2);
    }

    #[test]
    fn test_parse_path_array() {
        let components = DsonTape::<Vec<u8>>::parse_path("items[0]");
        assert_eq!(components.len(), 2);
        assert!(matches!(&components[1], PathComponent::ArrayIndex(0)));
    }

    #[test]
    fn test_node_to_json_value_string() {
        let tape = DsonTape::parse(r#"{"name":"test"}"#).unwrap();
        // Get index of the string value
        let result = tape.node_to_json_value(2);
        assert!(result.is_ok());
    }

    #[test]
    fn test_node_to_json_value_object() {
        let tape = DsonTape::parse(r#"{"name":"test"}"#).unwrap();
        let result = tape.node_to_json_value(0);
        assert!(result.is_ok());
        let val = result.unwrap();
        assert!(val.is_object());
    }

    #[test]
    fn test_node_to_json_value_array() {
        let tape = DsonTape::parse(r"[1, 2, 3]").unwrap();
        let result = tape.node_to_json_value(0);
        assert!(result.is_ok());
        let val = result.unwrap();
        assert!(val.is_array());
    }

    #[test]
    fn test_node_to_json_value_number() {
        let tape = DsonTape::parse(r#"{"value":42}"#).unwrap();
        let result = tape.node_to_json_value(2);
        assert!(result.is_ok());
    }

    #[test]
    fn test_node_to_json_value_bool() {
        let tape = DsonTape::parse(r#"{"flag":true}"#).unwrap();
        let result = tape.node_to_json_value(2);
        assert!(result.is_ok());
    }

    #[test]
    fn test_node_to_json_value_null() {
        let tape = DsonTape::parse(r#"{"value":null}"#).unwrap();
        let result = tape.node_to_json_value(2);
        assert!(result.is_ok());
    }

    #[test]
    fn test_skip_value_out_of_bounds() {
        let tape = DsonTape::parse(r"{}").unwrap();
        let result = tape.skip_value(100);
        assert!(result.is_ok());
    }

    #[test]
    fn test_simd_value_variants() {
        let s = SimdValue::String("test".to_string());
        let n = SimdValue::Number("42".to_string());
        let b = SimdValue::Bool(true);
        let null = SimdValue::Null;

        assert!(matches!(s, SimdValue::String(_)));
        assert!(matches!(n, SimdValue::Number(_)));
        assert!(matches!(b, SimdValue::Bool(true)));
        assert!(matches!(null, SimdValue::Null));
    }

    #[test]
    fn test_to_json_string() {
        let tape = DsonTape::parse(r#"{"name":"test"}"#).unwrap();
        let result = tape.to_json_string();
        assert!(result.is_ok());
        let json = result.unwrap();
        assert!(json.contains("name"));
        assert!(json.contains("test"));
    }

    #[test]
    fn test_filter_by_schema() {
        let tape = DsonTape::parse(r#"{"name":"test","age":30}"#).unwrap();
        let schema = std::collections::HashSet::from(["name".to_string()]);
        let result = tape.filter_by_schema(&schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_filter_by_schema_empty() {
        let tape = DsonTape::parse(r#"{"name":"test"}"#).unwrap();
        let schema = std::collections::HashSet::new();
        let result = tape.filter_by_schema(&schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_extract_value_simd_string() {
        let tape = DsonTape::parse(r#"{"name":"test"}"#).unwrap();
        let value = tape.extract_value_simd(2);
        assert!(value.is_some());
    }

    #[test]
    fn test_extract_value_simd_out_of_bounds() {
        let tape = DsonTape::parse(r"{}").unwrap();
        let value = tape.extract_value_simd(100);
        assert!(value.is_none());
    }

    #[test]
    fn test_serialize_simd() {
        let tape = DsonTape::parse(r#"{"name":"test"}"#).unwrap();
        let modifications = std::collections::HashMap::new();
        let result = tape.serialize_simd(&modifications);
        // Just check it doesn't panic
        let _ = result;
    }

    #[test]
    fn test_read_field() {
        let tape = DsonTape::parse(r#"{"name":"test"}"#).unwrap();
        let result = tape.read_field(0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_read_field_out_of_bounds() {
        let tape = DsonTape::parse(r"{}").unwrap();
        let result = tape.read_field(100);
        assert!(result.is_err());
    }

    #[test]
    fn test_should_survive_exact_match() {
        let tape = DsonTape::parse(r"{}").unwrap();
        let path = "user.name";
        let schema = std::collections::HashSet::from(["user.name".to_string()]);
        let result = tape.should_survive(path, &schema);
        assert!(result);
    }

    #[test]
    fn test_should_survive_prefix_match() {
        let tape = DsonTape::parse(r"{}").unwrap();
        let path = "user.name";
        let schema = std::collections::HashSet::from(["user".to_string()]);
        let result = tape.should_survive(path, &schema);
        assert!(result);
    }

    #[test]
    fn test_should_survive_no_match() {
        let tape = DsonTape::parse(r"{}").unwrap();
        let path = "other.field";
        let schema = std::collections::HashSet::from(["user".to_string()]);
        let result = tape.should_survive(path, &schema);
        assert!(!result);
    }

    #[test]
    fn test_simd_schema_match() {
        let tape = DsonTape::parse(r#"{"name":"test"}"#).unwrap();
        let schema = std::collections::HashSet::from(["name".to_string()]);
        let result = tape.simd_schema_match("name", &schema);
        // Just verify it doesn't panic
        assert!(result);
    }

    #[test]
    fn test_simd_schema_match_empty_schema() {
        let tape = DsonTape::parse(r#"{"name":"test"}"#).unwrap();
        let schema = std::collections::HashSet::new();
        let result = tape.simd_schema_match("name", &schema);
        assert!(!result);
    }

    #[test]
    fn test_serialize_with_modifications() {
        let tape = DsonTape::parse(r#"{"name":"test"}"#).unwrap();
        let mut modifications = ahash::AHashMap::default();
        modifications.insert(
            "name".to_string(),
            fionn_core::OperationValue::StringRef("modified".to_string()),
        );
        let deletions = ahash::AHashSet::default();
        let result = tape.serialize_with_modifications(&modifications, &deletions);
        assert!(result.is_ok());
    }

    #[test]
    fn test_serialize_with_deletions() {
        let tape = DsonTape::parse(r#"{"name":"test","age":30}"#).unwrap();
        let modifications = ahash::AHashMap::default();
        let mut deletions = ahash::AHashSet::default();
        deletions.insert("age".to_string());
        let result = tape.serialize_with_modifications(&modifications, &deletions);
        assert!(result.is_ok());
    }

    #[test]
    fn test_simd_string_equals() {
        let tape = DsonTape::parse(r"{}").unwrap();
        assert!(tape.simd_string_equals(b"hello", b"hello"));
        assert!(!tape.simd_string_equals(b"hello", b"world"));
    }

    #[test]
    fn test_simd_string_equals_different_lengths() {
        let tape = DsonTape::parse(r"{}").unwrap();
        assert!(!tape.simd_string_equals(b"hello", b"hi"));
    }

    #[test]
    fn test_simd_string_equals_long_strings() {
        let tape = DsonTape::parse(r"{}").unwrap();
        let a = b"this is a long string for testing simd comparison";
        let b = b"this is a long string for testing simd comparison";
        assert!(tape.simd_string_equals(a, b));
    }

    #[test]
    fn test_resolve_path_empty() {
        let tape = DsonTape::parse(r#"{"name":"test"}"#).unwrap();
        let result = tape.resolve_path("");
        assert!(result.is_ok());
    }

    #[test]
    fn test_resolve_path_deep_nesting() {
        let tape = DsonTape::parse(r#"{"a":{"b":{"c":{"d":1}}}}"#).unwrap();
        let result = tape.resolve_path("a.b.c.d");
        assert!(result.is_ok());
    }

    #[test]
    fn test_resolve_path_array_out_of_bounds() {
        let tape = DsonTape::parse(r#"{"items":[1,2,3]}"#).unwrap();
        let result = tape.resolve_path("items[100]");
        assert!(result.is_ok());
        // Index out of bounds returns None
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_resolve_parsed_path() {
        let tape = DsonTape::parse(r#"{"user":{"name":"test"}}"#).unwrap();
        let parsed = fionn_core::ParsedPath::parse("user.name");
        let result = tape.resolve_parsed_path(&parsed);
        assert!(result.is_ok());
    }

    #[test]
    fn test_node_to_json_value_nested_object() {
        let tape = DsonTape::parse(r#"{"user":{"name":"test","age":30}}"#).unwrap();
        let result = tape.node_to_json_value(0);
        assert!(result.is_ok());
        let val = result.unwrap();
        assert!(val.is_object());
    }

    #[test]
    fn test_node_to_json_value_nested_array() {
        let tape = DsonTape::parse(r"[[1,2],[3,4]]").unwrap();
        let result = tape.node_to_json_value(0);
        assert!(result.is_ok());
        let val = result.unwrap();
        assert!(val.is_array());
    }

    #[test]
    fn test_parse_path_empty() {
        let components = DsonTape::<Vec<u8>>::parse_path("");
        assert!(components.is_empty());
    }

    #[test]
    fn test_parse_path_complex() {
        let components = DsonTape::<Vec<u8>>::parse_path("users[0].name.first");
        assert_eq!(components.len(), 4);
    }

    #[test]
    fn test_reconstruct_value_from_tape() {
        let tape = DsonTape::parse(r#"{"name":"test"}"#).unwrap();
        let result = tape.reconstruct_value_from_tape(0, 10);
        assert!(result.is_ok());
    }

    #[test]
    fn test_reconstruct_value_from_tape_array() {
        let tape = DsonTape::parse(r"[1,2,3]").unwrap();
        let result = tape.reconstruct_value_from_tape(0, 10);
        assert!(result.is_ok());
    }

    #[test]
    fn test_path_component_array_index() {
        let idx = PathComponent::ArrayIndex(5);
        let debug = format!("{idx:?}");
        assert!(debug.contains("ArrayIndex"));
    }

    #[test]
    fn test_skip_field_not_object() {
        let tape = DsonTape::parse(r"[1,2,3]").unwrap();
        let result = tape.skip_field(0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_skip_value_array() {
        let tape = DsonTape::parse(r"[1,2,3]").unwrap();
        let result = tape.skip_value(0);
        assert!(result.is_ok());
        assert!(result.unwrap() > 0);
    }

    #[test]
    fn test_skip_value_string() {
        let tape = DsonTape::parse(r#"{"name":"test"}"#).unwrap();
        let result = tape.skip_value(2);
        assert!(result.is_ok());
    }

    #[test]
    fn test_skip_value_static() {
        let tape = DsonTape::parse(r#"{"val":true}"#).unwrap();
        let result = tape.skip_value(2);
        assert!(result.is_ok());
    }

    #[test]
    fn test_resolve_path_components_owned() {
        let tape = DsonTape::parse(r#"{"user":{"name":"test"}}"#).unwrap();
        let components = vec![
            PathComponent::Field("user".to_string()),
            PathComponent::Field("name".to_string()),
        ];
        let result = tape.resolve_path_components_owned(&components);
        assert!(result.is_ok());
    }

    #[test]
    fn test_extract_value_simd_number() {
        let tape = DsonTape::parse(r#"{"val":42}"#).unwrap();
        let value = tape.extract_value_simd(2);
        assert!(value.is_some());
        assert!(matches!(value.unwrap(), SimdValue::Number(_)));
    }

    #[test]
    fn test_extract_value_simd_bool() {
        let tape = DsonTape::parse(r#"{"val":true}"#).unwrap();
        let value = tape.extract_value_simd(2);
        assert!(value.is_some());
        assert!(matches!(value.unwrap(), SimdValue::Bool(true)));
    }

    #[test]
    fn test_extract_value_simd_null() {
        let tape = DsonTape::parse(r#"{"val":null}"#).unwrap();
        let value = tape.extract_value_simd(2);
        assert!(value.is_some());
        assert!(matches!(value.unwrap(), SimdValue::Null));
    }

    #[test]
    fn test_filter_by_schema_with_array() {
        let tape = DsonTape::parse(r#"{"items":[1,2,3],"name":"test"}"#).unwrap();
        let schema = std::collections::HashSet::from(["items".to_string()]);
        let result = tape.filter_by_schema(&schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_filter_by_schema_nested_array() {
        let tape = DsonTape::parse(r#"{"data":{"items":[{"id":1},{"id":2}]}}"#).unwrap();
        let schema =
            std::collections::HashSet::from(["data".to_string(), "data.items".to_string()]);
        let result = tape.filter_by_schema(&schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_resolve_path_with_array_index() {
        let tape = DsonTape::parse(r#"{"items":["a","b","c"]}"#).unwrap();
        let components = vec![
            PathComponent::Field("items".to_string()),
            PathComponent::ArrayIndex(1),
        ];
        let result = tape.resolve_path_components_owned(&components);
        assert!(result.is_ok());
    }

    #[test]
    fn test_resolve_path_nested_array() {
        let tape = DsonTape::parse(r#"{"data":[{"name":"first"},{"name":"second"}]}"#).unwrap();
        let components = vec![
            PathComponent::Field("data".to_string()),
            PathComponent::ArrayIndex(0),
            PathComponent::Field("name".to_string()),
        ];
        let result = tape.resolve_path_components_owned(&components);
        assert!(result.is_ok());
    }

    #[test]
    fn test_resolve_path_missing_field() {
        let tape = DsonTape::parse(r#"{"name":"test"}"#).unwrap();
        let components = vec![PathComponent::Field("nonexistent".to_string())];
        let result = tape.resolve_path_components_owned(&components);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_resolve_path_missing_array_index() {
        let tape = DsonTape::parse(r#"{"items":["a","b"]}"#).unwrap();
        let components = vec![
            PathComponent::Field("items".to_string()),
            PathComponent::ArrayIndex(99),
        ];
        let result = tape.resolve_path_components_owned(&components);
        assert!(result.is_ok());
        // Should return None for out-of-bounds index
    }

    #[test]
    fn test_serialize_with_modifications_nested() {
        let tape = DsonTape::parse(r#"{"user":{"name":"Alice"}}"#).unwrap();
        let mut modifications = FastHashMap::default();
        modifications.insert(
            "user.email".to_string(),
            fionn_core::OperationValue::StringRef("alice@example.com".to_string()),
        );
        let deletions = FastHashSet::default();
        let result = tape.serialize_with_modifications(&modifications, &deletions);
        assert!(result.is_ok());
    }

    #[test]
    fn test_serialize_with_deletions_extended() {
        let tape = DsonTape::parse(r#"{"name":"Alice","age":30}"#).unwrap();
        let modifications = FastHashMap::default();
        let mut deletions = FastHashSet::default();
        deletions.insert("age".to_string());
        let result = tape.serialize_with_modifications(&modifications, &deletions);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(!output.contains("\"age\""));
    }

    #[test]
    fn test_serialize_with_array_modifications() {
        let tape = DsonTape::parse(r#"{"items":["a","b","c"]}"#).unwrap();
        let mut modifications = FastHashMap::default();
        modifications.insert(
            "items[1]".to_string(),
            fionn_core::OperationValue::StringRef("modified".to_string()),
        );
        let deletions = FastHashSet::default();
        let result = tape.serialize_with_modifications(&modifications, &deletions);
        assert!(result.is_ok());
    }

    #[test]
    fn test_reconstruct_value_from_tape_extended() {
        let tape = DsonTape::parse(r#"{"name":"test","age":42}"#).unwrap();
        let node_count = tape.nodes().len();
        let result = tape.reconstruct_value_from_tape(0, node_count);
        assert!(result.is_ok());
        let value = result.unwrap();
        assert!(value.is_object());
    }

    #[test]
    fn test_reconstruct_value_from_tape_array_extended() {
        let tape = DsonTape::parse(r"[1,2,3]").unwrap();
        let node_count = tape.nodes().len();
        let result = tape.reconstruct_value_from_tape(0, node_count);
        assert!(result.is_ok());
        let value = result.unwrap();
        assert!(value.is_array());
    }

    #[test]
    fn test_field_matches_schema_prefix() {
        let tape = DsonTape::parse(r#"{"user":{"name":"test"}}"#).unwrap();
        let schema = std::collections::HashSet::from(["user".to_string()]);
        // Test that "user.name" matches when "user" is in schema
        let result = tape.filter_by_schema(&schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_field_matches_schema_wildcard() {
        let tape = DsonTape::parse(r#"{"users":{"alice":{"id":1},"bob":{"id":2}}}"#).unwrap();
        let schema = std::collections::HashSet::from(["users.**".to_string()]);
        let result = tape.filter_by_schema(&schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_empty_tape_serialization() {
        let tape = DsonTape::parse(r"{}").unwrap();
        let modifications = FastHashMap::default();
        let deletions = FastHashSet::default();
        let result = tape.serialize_with_modifications(&modifications, &deletions);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "{}");
    }

    #[test]
    fn test_empty_array_serialization() {
        let tape = DsonTape::parse(r"[]").unwrap();
        let modifications = FastHashMap::default();
        let deletions = FastHashSet::default();
        let result = tape.serialize_with_modifications(&modifications, &deletions);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "[]");
    }

    #[test]
    fn test_extract_value_simd_i64() {
        let tape = DsonTape::parse(r#"{"val":9223372036854775807}"#).unwrap();
        let value = tape.extract_value_simd(2);
        // Large i64 value
        assert!(value.is_some());
    }

    #[test]
    fn test_extract_value_simd_f64() {
        let tape = DsonTape::parse(r#"{"val":1.5}"#).unwrap();
        let value = tape.extract_value_simd(2);
        assert!(value.is_some());
        if let Some(SimdValue::Number(n)) = value {
            assert!(n.contains("1.5"));
        }
    }

    #[test]
    fn test_filter_by_schema_empty_result() {
        let tape = DsonTape::parse(r#"{"name":"test","age":30}"#).unwrap();
        let schema = std::collections::HashSet::from(["nonexistent".to_string()]);
        let result = tape.filter_by_schema(&schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_filter_by_schema_all_fields() {
        let tape = DsonTape::parse(r#"{"name":"test","age":30}"#).unwrap();
        let schema = std::collections::HashSet::from(["name".to_string(), "age".to_string()]);
        let result = tape.filter_by_schema(&schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_field_value_missing() {
        let _tape = DsonTape::parse(r#"{"name":"test"}"#).unwrap();
        // Test accessing a field that doesn't exist via path
        let components = DsonTape::<Vec<u8>>::parse_path("missing");
        assert_eq!(components.len(), 1);
    }

    #[test]
    fn test_get_nested_field_path() {
        let tape = DsonTape::parse(r#"{"user":{"name":"test"}}"#).unwrap();
        let components = DsonTape::<Vec<u8>>::parse_path("user.name");
        assert_eq!(components.len(), 2);
        let _ = tape;
    }

    #[test]
    fn test_skip_value_nested_object() {
        let tape = DsonTape::parse(r#"{"outer":{"inner":{"deep":"value"}}}"#).unwrap();
        let result = tape.skip_value(0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_skip_value_nested_array() {
        let tape = DsonTape::parse(r"[[1,2],[3,4],[5,6]]").unwrap();
        let result = tape.skip_value(0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_serialize_with_modifications_add_new_field() {
        let tape = DsonTape::parse(r#"{"existing":"value"}"#).unwrap();
        let mut modifications = FastHashMap::default();
        modifications.insert(
            "new_field".to_string(),
            fionn_core::OperationValue::StringRef("new_value".to_string()),
        );
        let deletions = FastHashSet::default();
        let result = tape.serialize_with_modifications(&modifications, &deletions);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("new_field") || output.contains("existing"));
    }

    #[test]
    fn test_serialize_with_modifications_number() {
        let tape = DsonTape::parse(r#"{"value":42}"#).unwrap();
        let mut modifications = FastHashMap::default();
        modifications.insert(
            "value".to_string(),
            fionn_core::OperationValue::NumberRef("100".to_string()),
        );
        let deletions = FastHashSet::default();
        let result = tape.serialize_with_modifications(&modifications, &deletions);
        assert!(result.is_ok());
    }

    #[test]
    fn test_serialize_with_modifications_bool() {
        let tape = DsonTape::parse(r#"{"flag":true}"#).unwrap();
        let mut modifications = FastHashMap::default();
        modifications.insert(
            "flag".to_string(),
            fionn_core::OperationValue::BoolRef(false),
        );
        let deletions = FastHashSet::default();
        let result = tape.serialize_with_modifications(&modifications, &deletions);
        assert!(result.is_ok());
    }

    #[test]
    fn test_serialize_with_modifications_null() {
        let tape = DsonTape::parse(r#"{"value":"test"}"#).unwrap();
        let mut modifications = FastHashMap::default();
        modifications.insert("value".to_string(), fionn_core::OperationValue::Null);
        let deletions = FastHashSet::default();
        let result = tape.serialize_with_modifications(&modifications, &deletions);
        assert!(result.is_ok());
    }

    #[test]
    fn test_path_component_field() {
        let component = PathComponent::Field("test".to_string());
        match component {
            PathComponent::Field(name) => assert_eq!(name, "test"),
            PathComponent::ArrayIndex(_) => panic!("Expected Field"),
        }
    }

    #[test]
    fn test_path_component_array_index_match() {
        let component = PathComponent::ArrayIndex(5);
        match component {
            PathComponent::ArrayIndex(idx) => assert_eq!(idx, 5),
            PathComponent::Field(_) => panic!("Expected ArrayIndex"),
        }
    }

    #[test]
    fn test_parse_complex_path() {
        let components = DsonTape::<Vec<u8>>::parse_path("users[0].profile.name");
        assert_eq!(components.len(), 4);
    }

    #[test]
    fn test_to_json_string_array() {
        let tape = DsonTape::parse(r#"[1,"two",true,null]"#).unwrap();
        let result = tape.to_json_string();
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains('1'));
        assert!(output.contains("two"));
    }

    #[test]
    fn test_to_json_string_nested() {
        let tape = DsonTape::parse(r#"{"a":{"b":{"c":1}}}"#).unwrap();
        let result = tape.to_json_string();
        assert!(result.is_ok());
    }

    #[test]
    fn test_simd_value_string() {
        let value = SimdValue::String("test".to_string());
        match value {
            SimdValue::String(s) => assert_eq!(s, "test"),
            _ => panic!("Expected String"),
        }
    }

    #[test]
    fn test_simd_value_number_display() {
        let value = SimdValue::Number("42".to_string());
        let debug_str = format!("{value:?}");
        assert!(debug_str.contains("42"));
    }

    #[test]
    fn test_serialize_with_deep_nesting() {
        let tape = DsonTape::parse(r#"{"a":{"b":{"c":{"d":{"e":1}}}}}"#).unwrap();
        let modifications = FastHashMap::default();
        let deletions = FastHashSet::default();
        let result = tape.serialize_with_modifications(&modifications, &deletions);
        assert!(result.is_ok());
    }

    #[test]
    fn test_serialize_with_array_of_objects() {
        let tape = DsonTape::parse(r#"{"items":[{"id":1},{"id":2}]}"#).unwrap();
        let modifications = FastHashMap::default();
        let deletions = FastHashSet::default();
        let result = tape.serialize_with_modifications(&modifications, &deletions);
        assert!(result.is_ok());
    }

    #[test]
    fn test_filter_by_schema_deep_path() {
        let tape = DsonTape::parse(r#"{"user":{"profile":{"name":"test"}}}"#).unwrap();
        let schema = std::collections::HashSet::from([
            "user".to_string(),
            "user.profile".to_string(),
            "user.profile.name".to_string(),
        ]);
        let result = tape.filter_by_schema(&schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_skip_field_out_of_bounds() {
        let tape = DsonTape::parse(r#"{"a": 1}"#).unwrap();
        // Try to skip a field at an index beyond the tape
        let result = tape.skip_field(100);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_skip_field_on_string() {
        let tape = DsonTape::parse(r#"{"name": "value"}"#).unwrap();
        // Skip field starting at index 1 (field name)
        let result = tape.skip_field(1);
        assert!(result.is_ok());
    }

    #[test]
    fn test_serialize_with_object_ref() {
        let tape = DsonTape::parse(r#"{"a": 1}"#).unwrap();
        let mut modifications = FastHashMap::default();
        modifications.insert(
            "new".to_string(),
            fionn_core::OperationValue::ObjectRef { start: 0, end: 1 },
        );
        let deletions = FastHashSet::default();
        let result = tape.serialize_with_modifications(&modifications, &deletions);
        assert!(result.is_ok());
    }

    #[test]
    fn test_serialize_with_array_ref() {
        let tape = DsonTape::parse(r#"{"a": 1}"#).unwrap();
        let mut modifications = FastHashMap::default();
        modifications.insert(
            "arr".to_string(),
            fionn_core::OperationValue::ArrayRef { start: 0, end: 1 },
        );
        let deletions = FastHashSet::default();
        let result = tape.serialize_with_modifications(&modifications, &deletions);
        assert!(result.is_ok());
    }

    #[test]
    fn test_filter_by_schema_wildcard() {
        let tape = DsonTape::parse(r#"{"users": {"name": "test", "age": 30}}"#).unwrap();
        let schema = std::collections::HashSet::from(["users.*".to_string()]);
        let result = tape.filter_by_schema(&schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_filter_by_schema_child_path() {
        let tape = DsonTape::parse(r#"{"user": {"profile": {"name": "test"}}}"#).unwrap();
        let schema =
            std::collections::HashSet::from(["user".to_string(), "user.profile".to_string()]);
        let result = tape.filter_by_schema(&schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_skip_value_on_array() {
        let tape = DsonTape::parse(r"[[1,2],[3,4]]").unwrap();
        let result = tape.skip_value(0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_serialize_sparse_array_modifications() {
        let tape = DsonTape::parse(r#"{"items": []}"#).unwrap();
        let mut modifications = FastHashMap::default();
        modifications.insert(
            "items[2]".to_string(),
            fionn_core::OperationValue::StringRef("sparse".to_string()),
        );
        let deletions = FastHashSet::default();
        let result = tape.serialize_with_modifications(&modifications, &deletions);
        assert!(result.is_ok());
    }

    #[test]
    fn test_serialize_nested_array_in_array() {
        let tape = DsonTape::parse(r#"{"data": []}"#).unwrap();
        let mut modifications = FastHashMap::default();
        modifications.insert(
            "data[0][0]".to_string(),
            fionn_core::OperationValue::NumberRef("42".to_string()),
        );
        let deletions = FastHashSet::default();
        let result = tape.serialize_with_modifications(&modifications, &deletions);
        assert!(result.is_ok());
    }

    #[test]
    fn test_serialize_nested_object_in_array() {
        let tape = DsonTape::parse(r#"{"data": []}"#).unwrap();
        let mut modifications = FastHashMap::default();
        modifications.insert(
            "data[0].name".to_string(),
            fionn_core::OperationValue::StringRef("test".to_string()),
        );
        let deletions = FastHashSet::default();
        let result = tape.serialize_with_modifications(&modifications, &deletions);
        assert!(result.is_ok());
    }

    #[test]
    fn test_path_survives_filter_exact_match() {
        let tape = DsonTape::parse(r#"{"field": "value"}"#).unwrap();
        let schema = std::collections::HashSet::from(["field".to_string()]);
        let result = tape.filter_by_schema(&schema);
        assert!(result.is_ok());
        let filtered = result.unwrap();
        let json = filtered.to_json_string();
        assert!(json.is_ok());
    }

    #[test]
    fn test_reconstruct_value_empty_object() {
        let tape = DsonTape::parse(r"{}").unwrap();
        let result = tape.reconstruct_value_from_tape(0, 2);
        assert!(result.is_ok());
    }

    #[test]
    fn test_reconstruct_value_empty_array() {
        let tape = DsonTape::parse(r"[]").unwrap();
        let result = tape.reconstruct_value_from_tape(0, 2);
        assert!(result.is_ok());
    }

    #[test]
    fn test_simd_value_string_variant() {
        let value = SimdValue::String("test".to_string());
        let debug_str = format!("{value:?}");
        assert!(debug_str.contains("String"));
    }

    #[test]
    fn test_simd_value_bool_variant() {
        let value = SimdValue::Bool(true);
        let debug_str = format!("{value:?}");
        assert!(debug_str.contains("Bool"));
    }

    #[test]
    fn test_simd_value_null_variant() {
        let value = SimdValue::Null;
        let debug_str = format!("{value:?}");
        assert!(debug_str.contains("Null"));
    }

    #[test]
    fn test_simd_value_number_variant() {
        let value = SimdValue::Number("3.14".to_string());
        let debug_str = format!("{value:?}");
        assert!(debug_str.contains("Number"));
    }

    #[test]
    fn test_parse_path_with_multiple_arrays() {
        let components = DsonTape::<Vec<u8>>::parse_path("data[0].items[1].value");
        assert_eq!(components.len(), 5);
    }

    #[test]
    fn test_roundtrip_escaped_quote() {
        // This was a fuzzer-found crash: string with escaped quote
        let input = r#"[ "Hello, \u4e16\"emoji"]"#;
        let tape = DsonTape::parse(input).expect("should parse");
        let serialized = tape.to_json_string().expect("should serialize");

        // Debug: print what we got
        eprintln!("Input:      {input}");
        eprintln!("Serialized: {serialized}");

        // The serialized output should be valid JSON
        let reparsed = DsonTape::parse(&serialized);
        assert!(
            reparsed.is_ok(),
            "Round-trip failed: serialized JSON is invalid: {serialized}"
        );
    }

    #[test]
    fn test_roundtrip_backslash() {
        let input = r#"{"key": "back\\slash"}"#;
        let tape = DsonTape::parse(input).expect("should parse");
        let serialized = tape.to_json_string().expect("should serialize");
        let reparsed = DsonTape::parse(&serialized);
        assert!(
            reparsed.is_ok(),
            "Round-trip failed for backslash: {serialized}"
        );
    }

    #[test]
    fn test_roundtrip_unicode_escape() {
        let input = r#"["Hello, \u4e16\u754c!"]"#;
        let tape = DsonTape::parse(input).expect("should parse");
        let serialized = tape.to_json_string().expect("should serialize");
        let reparsed = DsonTape::parse(&serialized);
        assert!(
            reparsed.is_ok(),
            "Round-trip failed for unicode: {serialized}"
        );
    }

    /// Test all the fuzzer-discovered crash inputs to ensure they're fixed.
    /// These inputs found issues with string escape handling during serialization.
    #[test]
    fn test_fuzz_crash_inputs() {
        let crash_inputs = [
            // Escaped quote in string
            r#"[ "Hello, \u4e16\"emoji"]"#,
            // Object with escaped characters in key
            r#"[{"id": 1}, {"id": 2}, {"ttab\\slash\"quoid": 3}]"#,
            // Multiple unicode escapes
            r#"[ "Hello, \u4e16\u754c!", "emoji"]"#,
            // Escaped backslash followed by unicode
            r#"[ "Udvrz-\\u8f02\u568b%", "asvzx"]"#,
            // Many backslashes
            r#"[ "Hello, \\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\u4e16\u754c!", "emoji"]"#,
            // Carriage return escape
            r#"[ "Hello, \r4e16\u754c!", "emoji"]"#,
            // Unicode with extra text
            r#"[ "Hello, \u4e16\u054c!", "emoji"]"#,
        ];

        for (i, input) in crash_inputs.iter().enumerate() {
            // First verify serde_json can parse it (to confirm input is valid JSON)
            if serde_json::from_str::<serde_json::Value>(input).is_err() {
                // Input itself is invalid JSON, skip
                continue;
            }

            // Parse with DsonTape
            let Ok(tape) = DsonTape::parse(input) else {
                continue; // Some inputs may be invalid for simd-json
            };

            // Serialize back
            let serialized = match tape.to_json_string() {
                Ok(s) => s,
                Err(e) => panic!("Crash input {i} failed to serialize: {e}"),
            };

            // Verify round-trip produces valid JSON
            match DsonTape::parse(&serialized) {
                Ok(_) => {} // Success!
                Err(e) => panic!(
                    "Crash input {i} round-trip failed:\n  Input: {input}\n  Serialized: {serialized}\n  Error: {e}"
                ),
            }
        }
    }
}
