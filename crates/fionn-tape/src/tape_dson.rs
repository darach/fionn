// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tape-DSON Operational Processor
//!
//! This module provides direct conversion between SIMD-JSON tape format
//! and DSON operations, enabling efficient tape-level processing without
//! intermediate serde conversions.

use crate::operations::{DsonOperation, OperationValue};
use crate::tape::DsonTape;
use simd_json::value::tape::Node;
use std::collections::HashMap;

/// Tape-DSON processor for direct tape-to-operation conversion
pub struct TapeDsonProcessor {
    /// Operations extracted from tape
    operations: Vec<DsonOperation>,
    /// Field values extracted during processing (path -> value)
    field_values: HashMap<String, OperationValue>,
}

impl TapeDsonProcessor {
    /// Create a new tape-DSON processor
    #[must_use]
    pub fn new() -> Self {
        Self {
            operations: Vec::new(),
            field_values: HashMap::new(),
        }
    }

    /// Extract operations from a tape
    ///
    /// # Errors
    /// Returns an error if tape processing fails
    pub fn extract_operations(&mut self, tape: &DsonTape) -> Result<&[DsonOperation], String> {
        self.operations.clear();
        self.field_values.clear();

        let nodes = tape.nodes();
        if nodes.is_empty() {
            return Ok(&self.operations);
        }

        // Process the tape recursively starting from root
        self.process_node(nodes, 0, &mut Vec::new())?;

        Ok(&self.operations)
    }

    /// Process a node at the given index, building operations
    fn process_node(
        &mut self,
        nodes: &[Node<'static>],
        index: usize,
        path_stack: &mut Vec<String>,
    ) -> Result<usize, String> {
        if index >= nodes.len() {
            return Ok(index);
        }

        let node = &nodes[index];
        let current_path = path_stack.join(".");

        match node {
            Node::String(s) => {
                if !current_path.is_empty() {
                    let value = OperationValue::StringRef(s.to_string());
                    self.field_values
                        .insert(current_path.clone(), value.clone());
                    self.operations.push(DsonOperation::FieldAdd {
                        path: current_path,
                        value,
                    });
                }
                Ok(index + 1)
            }
            Node::Static(static_val) => {
                if !current_path.is_empty() {
                    let value = match static_val {
                        simd_json::StaticNode::Null => OperationValue::Null,
                        simd_json::StaticNode::Bool(b) => OperationValue::BoolRef(*b),
                        simd_json::StaticNode::I64(n) => OperationValue::NumberRef(n.to_string()),
                        simd_json::StaticNode::U64(n) => OperationValue::NumberRef(n.to_string()),
                        simd_json::StaticNode::F64(n) => OperationValue::NumberRef(n.to_string()),
                    };
                    self.field_values
                        .insert(current_path.clone(), value.clone());
                    self.operations.push(DsonOperation::FieldAdd {
                        path: current_path,
                        value,
                    });
                }
                Ok(index + 1)
            }
            Node::Object { len, count: _ } => {
                self.operations.push(DsonOperation::ObjectStart {
                    path: current_path.clone(),
                });

                let mut current_idx = index + 1;

                // Process each field in the object
                for _ in 0..*len {
                    if current_idx >= nodes.len() {
                        break;
                    }

                    // Get field name (should be a String node)
                    if let Node::String(field_name) = &nodes[current_idx] {
                        let field_name_str = field_name.to_string();
                        path_stack.push(field_name_str);
                        current_idx += 1;

                        // Process the field value
                        if current_idx < nodes.len() {
                            current_idx = self.process_node(nodes, current_idx, path_stack)?;
                        }

                        path_stack.pop();
                    } else {
                        current_idx += 1;
                    }
                }

                self.operations
                    .push(DsonOperation::ObjectEnd { path: current_path });

                Ok(current_idx)
            }
            Node::Array { len, count: _ } => {
                self.operations.push(DsonOperation::ArrayStart {
                    path: current_path.clone(),
                });

                let mut current_idx = index + 1;

                // Process each element in the array
                for elem_idx in 0..*len {
                    if current_idx >= nodes.len() {
                        break;
                    }

                    path_stack.push(format!("[{elem_idx}]"));
                    current_idx = self.process_node(nodes, current_idx, path_stack)?;
                    path_stack.pop();
                }

                self.operations
                    .push(DsonOperation::ArrayEnd { path: current_path });

                Ok(current_idx)
            }
        }
    }

    /// Get a field value by path
    #[must_use]
    pub fn get_field(&self, path: &str) -> Option<&OperationValue> {
        self.field_values.get(path)
    }

    /// Get all extracted operations
    #[must_use]
    pub fn operations(&self) -> &[DsonOperation] {
        &self.operations
    }

    /// Get all field values
    #[must_use]
    pub const fn field_values(&self) -> &HashMap<String, OperationValue> {
        &self.field_values
    }

    /// Serialize operations back to JSON
    ///
    /// # Errors
    /// Returns an error if JSON serialization fails.
    pub fn serialize_to_json(&self) -> Result<String, String> {
        let mut root = serde_json::Map::new();

        for op in &self.operations {
            match op {
                DsonOperation::FieldAdd { path, value }
                | DsonOperation::FieldModify { path, value } => {
                    Self::set_value_at_path(&mut root, path, value);
                }
                _ => {}
            }
        }

        serde_json::to_string(&serde_json::Value::Object(root))
            .map_err(|e| format!("Serialization error: {e}"))
    }

    /// Set a value at a given path in the JSON structure
    fn set_value_at_path(
        root: &mut serde_json::Map<String, serde_json::Value>,
        path: &str,
        value: &OperationValue,
    ) {
        let parts: Vec<&str> = path.split('.').collect();
        let json_value = Self::operation_value_to_json(value);

        if parts.is_empty() {
            return;
        }

        let mut current = root;

        for (i, part) in parts.iter().enumerate() {
            if i == parts.len() - 1 {
                // Handle array notation in path like "items[0]"
                if let Some(bracket_pos) = part.find('[') {
                    let field_name = &part[..bracket_pos];
                    // For arrays, just set the field (simplified)
                    current.insert(field_name.to_string(), json_value.clone());
                } else {
                    current.insert(part.to_string(), json_value.clone());
                }
            } else {
                // Navigate or create intermediate objects
                if !current.contains_key(*part) {
                    current.insert(
                        part.to_string(),
                        serde_json::Value::Object(serde_json::Map::new()),
                    );
                }
                if let Some(serde_json::Value::Object(obj)) = current.get_mut(*part) {
                    current = obj;
                } else {
                    return;
                }
            }
        }
    }

    /// Convert `OperationValue` to `serde_json::Value`
    fn operation_value_to_json(value: &OperationValue) -> serde_json::Value {
        match value {
            OperationValue::StringRef(s) => serde_json::Value::String(s.clone()),
            OperationValue::NumberRef(n) => n.parse::<i64>().map_or_else(
                |_| {
                    n.parse::<f64>().map_or_else(
                        |_| serde_json::Value::String(n.clone()),
                        |num| {
                            serde_json::Number::from_f64(num).map_or_else(
                                || serde_json::Value::String(n.clone()),
                                serde_json::Value::Number,
                            )
                        },
                    )
                },
                |num| serde_json::Value::Number(num.into()),
            ),
            OperationValue::BoolRef(b) => serde_json::Value::Bool(*b),
            OperationValue::Null => serde_json::Value::Null,
            OperationValue::ObjectRef { .. } => serde_json::Value::Object(serde_json::Map::new()),
            OperationValue::ArrayRef { .. } => serde_json::Value::Array(Vec::new()),
        }
    }
}

impl Default for TapeDsonProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_operations() {
        let mut processor = TapeDsonProcessor::new();
        let json = r#"{"name": "Alice", "age": 30}"#;

        let tape = DsonTape::parse(json).unwrap();
        let ops = processor.extract_operations(&tape).unwrap();

        // Should have extracted operations for name and age
        assert!(!ops.is_empty());
    }

    #[test]
    fn test_nested_object() {
        let mut processor = TapeDsonProcessor::new();
        let json = r#"{"user": {"name": "Bob", "active": true}}"#;

        let tape = DsonTape::parse(json).unwrap();
        processor.extract_operations(&tape).unwrap();

        // Should have extracted nested field values
        assert!(processor.get_field("user.name").is_some());
        assert!(processor.get_field("user.active").is_some());
    }

    #[test]
    fn test_array_processing() {
        let mut processor = TapeDsonProcessor::new();
        let json = r#"{"items": [1, 2, 3]}"#;

        let tape = DsonTape::parse(json).unwrap();
        let ops = processor.extract_operations(&tape).unwrap();

        // Should have array start/end and element operations
        let has_array_start = ops
            .iter()
            .any(|op| matches!(op, DsonOperation::ArrayStart { .. }));
        assert!(has_array_start);
    }

    #[test]
    fn test_serialize_to_json() {
        let mut processor = TapeDsonProcessor::new();
        let json = r#"{"name": "Alice", "age": 30}"#;

        let tape = DsonTape::parse(json).unwrap();
        processor.extract_operations(&tape).unwrap();

        let result = processor.serialize_to_json().unwrap();
        assert!(result.contains("Alice"));
        assert!(result.contains("30"));
    }

    // Additional tests for coverage

    #[test]
    fn test_default() {
        let processor = TapeDsonProcessor::default();
        assert!(processor.operations().is_empty());
        assert!(processor.field_values().is_empty());
    }

    #[test]
    fn test_empty_tape() {
        let mut processor = TapeDsonProcessor::new();
        let json = r"{}";

        let tape = DsonTape::parse(json).unwrap();
        let ops = processor.extract_operations(&tape).unwrap();

        // Empty object should still have ObjectStart and ObjectEnd
        assert!(!ops.is_empty());
    }

    #[test]
    fn test_operations_getter() {
        let mut processor = TapeDsonProcessor::new();
        let json = r#"{"x": 1}"#;

        let tape = DsonTape::parse(json).unwrap();
        processor.extract_operations(&tape).unwrap();

        let ops = processor.operations();
        assert!(!ops.is_empty());
    }

    #[test]
    fn test_field_values_getter() {
        let mut processor = TapeDsonProcessor::new();
        let json = r#"{"x": 1}"#;

        let tape = DsonTape::parse(json).unwrap();
        processor.extract_operations(&tape).unwrap();

        let values = processor.field_values();
        assert!(values.contains_key("x"));
    }

    #[test]
    fn test_get_field_nonexistent() {
        let processor = TapeDsonProcessor::new();
        assert!(processor.get_field("nonexistent").is_none());
    }

    #[test]
    fn test_null_value() {
        let mut processor = TapeDsonProcessor::new();
        let json = r#"{"value": null}"#;

        let tape = DsonTape::parse(json).unwrap();
        processor.extract_operations(&tape).unwrap();

        let value = processor.get_field("value");
        assert!(value.is_some());
        assert!(matches!(value.unwrap(), OperationValue::Null));
    }

    #[test]
    fn test_bool_value() {
        let mut processor = TapeDsonProcessor::new();
        let json = r#"{"active": true, "deleted": false}"#;

        let tape = DsonTape::parse(json).unwrap();
        processor.extract_operations(&tape).unwrap();

        let active = processor.get_field("active");
        assert!(matches!(active.unwrap(), OperationValue::BoolRef(true)));

        let deleted = processor.get_field("deleted");
        assert!(matches!(deleted.unwrap(), OperationValue::BoolRef(false)));
    }

    #[test]
    fn test_float_value() {
        let mut processor = TapeDsonProcessor::new();
        let json = r#"{"pi": 1.5}"#;

        let tape = DsonTape::parse(json).unwrap();
        processor.extract_operations(&tape).unwrap();

        let value = processor.get_field("pi");
        assert!(value.is_some());
    }

    #[test]
    fn test_negative_number() {
        let mut processor = TapeDsonProcessor::new();
        let json = r#"{"temp": -10}"#;

        let tape = DsonTape::parse(json).unwrap();
        processor.extract_operations(&tape).unwrap();

        let value = processor.get_field("temp");
        assert!(value.is_some());
    }

    #[test]
    fn test_deeply_nested() {
        let mut processor = TapeDsonProcessor::new();
        let json = r#"{"a": {"b": {"c": {"d": 42}}}}"#;

        let tape = DsonTape::parse(json).unwrap();
        processor.extract_operations(&tape).unwrap();

        let value = processor.get_field("a.b.c.d");
        assert!(value.is_some());
    }

    #[test]
    fn test_array_of_objects() {
        let mut processor = TapeDsonProcessor::new();
        let json = r#"{"users": [{"name": "Alice"}, {"name": "Bob"}]}"#;

        let tape = DsonTape::parse(json).unwrap();
        let ops = processor.extract_operations(&tape).unwrap();

        let has_array_start = ops
            .iter()
            .any(|op| matches!(op, DsonOperation::ArrayStart { .. }));
        let has_array_end = ops
            .iter()
            .any(|op| matches!(op, DsonOperation::ArrayEnd { .. }));
        assert!(has_array_start);
        assert!(has_array_end);
    }

    #[test]
    fn test_nested_arrays() {
        let mut processor = TapeDsonProcessor::new();
        let json = r#"{"matrix": [[1, 2], [3, 4]]}"#;

        let tape = DsonTape::parse(json).unwrap();
        let ops = processor.extract_operations(&tape).unwrap();

        assert!(!ops.is_empty());
    }

    #[test]
    fn test_serialize_nested_object() {
        let mut processor = TapeDsonProcessor::new();
        let json = r#"{"user": {"name": "Alice"}}"#;

        let tape = DsonTape::parse(json).unwrap();
        processor.extract_operations(&tape).unwrap();

        let result = processor.serialize_to_json().unwrap();
        assert!(result.contains("user"));
        assert!(result.contains("name"));
        assert!(result.contains("Alice"));
    }

    #[test]
    fn test_serialize_boolean() {
        let mut processor = TapeDsonProcessor::new();
        let json = r#"{"active": true}"#;

        let tape = DsonTape::parse(json).unwrap();
        processor.extract_operations(&tape).unwrap();

        let result = processor.serialize_to_json().unwrap();
        assert!(result.contains("true"));
    }

    #[test]
    fn test_serialize_null() {
        let mut processor = TapeDsonProcessor::new();
        let json = r#"{"value": null}"#;

        let tape = DsonTape::parse(json).unwrap();
        processor.extract_operations(&tape).unwrap();

        let result = processor.serialize_to_json().unwrap();
        assert!(result.contains("null"));
    }

    #[test]
    fn test_serialize_float() {
        let mut processor = TapeDsonProcessor::new();
        let json = r#"{"pi": 1.23}"#;

        let tape = DsonTape::parse(json).unwrap();
        processor.extract_operations(&tape).unwrap();

        let result = processor.serialize_to_json().unwrap();
        assert!(result.contains("3.14"));
    }

    #[test]
    fn test_extract_operations_clears_previous() {
        let mut processor = TapeDsonProcessor::new();

        // First extraction
        let json1 = r#"{"first": 1}"#;
        let tape1 = DsonTape::parse(json1).unwrap();
        processor.extract_operations(&tape1).unwrap();
        assert!(processor.get_field("first").is_some());

        // Second extraction should clear previous
        let json2 = r#"{"second": 2}"#;
        let tape2 = DsonTape::parse(json2).unwrap();
        processor.extract_operations(&tape2).unwrap();

        // Previous field should be gone
        assert!(processor.get_field("first").is_none());
        assert!(processor.get_field("second").is_some());
    }

    #[test]
    fn test_operation_value_to_json_object_ref() {
        let value = OperationValue::ObjectRef { start: 0, end: 10 };
        let json = TapeDsonProcessor::operation_value_to_json(&value);
        assert!(json.is_object());
    }

    #[test]
    fn test_operation_value_to_json_array_ref() {
        let value = OperationValue::ArrayRef { start: 0, end: 10 };
        let json = TapeDsonProcessor::operation_value_to_json(&value);
        assert!(json.is_array());
    }

    #[test]
    fn test_operation_value_to_json_invalid_float() {
        // NaN or infinity that can't be represented in JSON
        let value = OperationValue::NumberRef("NaN".to_string());
        let json = TapeDsonProcessor::operation_value_to_json(&value);
        // Should fall back to string representation
        assert!(json.is_string());
    }

    #[test]
    fn test_large_integer() {
        let mut processor = TapeDsonProcessor::new();
        let json = r#"{"big": 9223372036854775807}"#;

        let tape = DsonTape::parse(json).unwrap();
        processor.extract_operations(&tape).unwrap();

        let value = processor.get_field("big");
        assert!(value.is_some());
    }

    #[test]
    fn test_string_with_special_chars() {
        let mut processor = TapeDsonProcessor::new();
        let json = r#"{"msg": "hello\nworld"}"#;

        let tape = DsonTape::parse(json).unwrap();
        processor.extract_operations(&tape).unwrap();

        let value = processor.get_field("msg");
        assert!(value.is_some());
    }

    #[test]
    fn test_array_with_mixed_types() {
        let mut processor = TapeDsonProcessor::new();
        let json = r#"{"mixed": [1, "two", true, null]}"#;

        let tape = DsonTape::parse(json).unwrap();
        let ops = processor.extract_operations(&tape).unwrap();

        assert!(!ops.is_empty());
    }

    #[test]
    fn test_serialize_with_array_notation_path() {
        let mut processor = TapeDsonProcessor::new();

        // Manually add an operation with array notation in path
        processor.operations.push(DsonOperation::FieldAdd {
            path: "items[0]".to_string(),
            value: OperationValue::NumberRef("1".to_string()),
        });

        let result = processor.serialize_to_json().unwrap();
        // Should handle the array notation
        assert!(result.contains("items"));
    }
}
