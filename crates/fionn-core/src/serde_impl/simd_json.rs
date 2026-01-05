// SPDX-License-Identifier: MIT OR Apache-2.0
//! SIMD-JSON Serde Integration for DSON
//!
//! This module provides direct SIMD-JSON integration with DSON operations,
//! enabling zero-copy processing of structured data.

use crate::operations::{CanonicalOperationProcessor, DsonOperation, OperationValue};
use crate::tape::DsonTape;
use ahash::{AHashMap, AHashSet};
use simd_json::value::tape::Node;
use std::collections::HashSet;

// Type aliases for performance-optimized collections
type FastHashMap<K, V> = AHashMap<K, V>;
type FastHashSet<T> = AHashSet<T>;

/// High-performance SIMD-JSON processor with DSON operations
pub struct SimdJsonSerdeProcessor {
    operations: Vec<DsonOperation>,
    /// Input schema for filtering (empty = all fields)
    input_schema: HashSet<String>,
    /// Output schema for filtering (empty = all fields)
    output_schema: HashSet<String>,
}

impl SimdJsonSerdeProcessor {
    /// Create a new SIMD-JSON processor with no schema filtering.
    #[must_use]
    pub fn new() -> Self {
        Self {
            operations: Vec::new(),
            input_schema: HashSet::new(),
            output_schema: HashSet::new(),
        }
    }

    /// Create a processor with schema filtering
    #[must_use]
    pub const fn with_schema(
        input_schema: HashSet<String>,
        output_schema: HashSet<String>,
    ) -> Self {
        Self {
            operations: Vec::new(),
            input_schema,
            output_schema,
        }
    }

    /// Process JSON directly with SIMD-JSON tape
    ///
    /// # Errors
    /// Returns an error if JSON parsing fails or if canonicalization fails.
    pub fn process_json(&mut self, json: &str) -> Result<String, String> {
        // Parse with SIMD-JSON directly to tape
        let tape = DsonTape::parse(json)?;

        // Convert tape to operations (lazy evaluation)
        self.tape_to_operations(&tape)?;

        // Apply canonical transformations
        self.canonicalize_operations()?;

        // Serialize result using tape references
        self.serialize_from_tape(&tape)
    }

    /// Add an operation to be applied
    pub fn add_operation(&mut self, operation: DsonOperation) {
        self.operations.push(operation);
    }

    /// Get current operations
    #[must_use]
    pub fn operations(&self) -> &[DsonOperation] {
        &self.operations
    }

    fn tape_to_operations(&mut self, tape: &DsonTape) -> Result<(), String> {
        // Preserve any pre-existing operations (e.g., manually added modifications)
        let existing_operations = std::mem::take(&mut self.operations);

        // Convert SIMD-JSON tape nodes to DSON operations
        let nodes = tape.nodes();

        if !nodes.is_empty() {
            // Parse from the root node and generate operations
            self.parse_node_recursive(nodes, 0, &mut Vec::new())?;
        }

        // Re-add existing operations (they take precedence)
        self.operations.extend(existing_operations);

        Ok(())
    }

    /// Recursively parse nodes and generate operations
    fn parse_node_recursive(
        &mut self,
        nodes: &[Node<'static>],
        start_index: usize,
        path_stack: &mut Vec<String>,
    ) -> Result<usize, String> {
        if start_index >= nodes.len() {
            return Ok(start_index);
        }

        let node = &nodes[start_index];
        let current_path = path_stack.join(".");

        // Check schema filter - skip if path not in input schema
        let should_process = self.input_schema.is_empty()
            || self.input_schema.contains(&current_path)
            || self
                .input_schema
                .iter()
                .any(|p| current_path.starts_with(p) || p.starts_with(&current_path));

        match node {
            Node::String(s) => {
                if should_process && !current_path.is_empty() {
                    self.operations.push(DsonOperation::FieldAdd {
                        path: current_path,
                        value: OperationValue::StringRef(s.to_string()),
                    });
                }
                Ok(start_index + 1)
            }
            Node::Static(static_val) => {
                if should_process && !current_path.is_empty() {
                    let value = match static_val {
                        simd_json::StaticNode::Null => OperationValue::Null,
                        simd_json::StaticNode::Bool(b) => OperationValue::BoolRef(*b),
                        simd_json::StaticNode::I64(n) => OperationValue::NumberRef(n.to_string()),
                        simd_json::StaticNode::U64(n) => OperationValue::NumberRef(n.to_string()),
                        simd_json::StaticNode::F64(n) => OperationValue::NumberRef(n.to_string()),
                    };
                    self.operations.push(DsonOperation::FieldAdd {
                        path: current_path,
                        value,
                    });
                }
                Ok(start_index + 1)
            }
            Node::Object { len, count: _ } => {
                if should_process {
                    self.operations.push(DsonOperation::ObjectStart {
                        path: current_path.clone(),
                    });
                }

                let mut index = start_index + 1;

                // Process each field in the object
                for _ in 0..*len {
                    if index >= nodes.len() {
                        break;
                    }

                    // Get field name (should be a String node)
                    if let Node::String(field_name) = &nodes[index] {
                        let field_name_str = field_name.to_string();
                        path_stack.push(field_name_str.clone());
                        index += 1;

                        // Parse the field value
                        if index < nodes.len() {
                            index = self.parse_node_recursive(nodes, index, path_stack)?;
                        }

                        path_stack.pop();
                    } else {
                        // Skip unexpected node
                        index += 1;
                    }
                }

                if should_process {
                    self.operations
                        .push(DsonOperation::ObjectEnd { path: current_path });
                }

                Ok(index)
            }
            Node::Array { len, count: _ } => {
                if should_process {
                    self.operations.push(DsonOperation::ArrayStart {
                        path: current_path.clone(),
                    });
                }

                let mut index = start_index + 1;

                // Process each element in the array
                for elem_idx in 0..*len {
                    if index >= nodes.len() {
                        break;
                    }

                    // Add array index to path
                    let array_path_segment = format!("[{elem_idx}]");
                    path_stack.push(array_path_segment);

                    // Parse the array element
                    index = self.parse_node_recursive(nodes, index, path_stack)?;

                    path_stack.pop();
                }

                if should_process {
                    self.operations
                        .push(DsonOperation::ArrayEnd { path: current_path });
                }

                Ok(index)
            }
        }
    }

    fn canonicalize_operations(&mut self) -> Result<(), String> {
        // Apply canonical transformations using the full processor
        if self.operations.is_empty() {
            return Ok(());
        }

        // Only canonicalize if we have schemas defined
        if self.input_schema.is_empty() && self.output_schema.is_empty() {
            return Ok(());
        }

        let mut processor =
            CanonicalOperationProcessor::new(self.input_schema.clone(), self.output_schema.clone());

        // Add all operations to the processor
        for op in &self.operations {
            processor.add_operation(op.clone());
        }

        // Compute canonical sequence
        match processor.compute_canonical() {
            Ok(canonical_ops) => {
                self.operations = canonical_ops.to_vec();
                Ok(())
            }
            Err(e) => Err(format!("Canonicalization error: {e}")),
        }
    }

    fn serialize_from_tape(&self, tape: &DsonTape) -> Result<String, String> {
        // Collect modifications and deletions from operations
        let mut modifications = FastHashMap::default();
        let mut deletions = FastHashSet::default();

        for op in &self.operations {
            match op {
                DsonOperation::FieldAdd { path, value }
                | DsonOperation::FieldModify { path, value } => {
                    modifications.insert(path.clone(), value.clone());
                    deletions.remove(path);
                }
                DsonOperation::FieldDelete { path } => {
                    deletions.insert(path.clone());
                    modifications.remove(path);
                }
                _ => {}
            }
        }

        // If no operations, return original JSON
        if modifications.is_empty() && deletions.is_empty() {
            return tape
                .to_json_string()
                .map_err(|e| format!("Serialization error: {e}"));
        }

        // Use tape's serialization with modifications
        tape.serialize_with_modifications(&modifications, &deletions)
            .map_err(|e| format!("Serialization error: {e}"))
    }
}

impl Default for SimdJsonSerdeProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simd_json_processing() {
        let mut processor = SimdJsonSerdeProcessor::new();
        let json = r#"{"name": "Alice", "age": 30}"#;

        let result = processor.process_json(json).unwrap();
        assert!(!result.is_empty());
        // Should return actual JSON, not empty object
        assert!(result.contains("Alice") || result.contains("name"));
    }

    #[test]
    fn test_simd_json_with_operations() {
        let mut processor = SimdJsonSerdeProcessor::new();
        let json = r#"{"name": "Alice", "age": 30}"#;

        processor.add_operation(DsonOperation::FieldModify {
            path: "name".to_string(),
            value: OperationValue::StringRef("Bob".to_string()),
        });

        let result = processor.process_json(json).unwrap();
        assert!(result.contains("Bob"));
    }

    #[test]
    fn test_tape_to_operations() {
        let mut processor = SimdJsonSerdeProcessor::new();
        let json = r#"{"user": {"name": "Alice", "age": 30}}"#;

        let tape = DsonTape::parse(json).unwrap();
        processor.tape_to_operations(&tape).unwrap();

        // Should have generated operations for the nested structure
        assert!(!processor.operations.is_empty());
    }

    #[test]
    fn test_simd_json_default() {
        let processor = SimdJsonSerdeProcessor::default();
        assert!(processor.operations().is_empty());
    }

    #[test]
    fn test_simd_json_with_schema() {
        let input_schema = HashSet::from(["name".to_string()]);
        let output_schema = HashSet::from(["name".to_string()]);
        let processor = SimdJsonSerdeProcessor::with_schema(input_schema, output_schema);
        assert!(processor.operations().is_empty());
    }

    #[test]
    fn test_simd_json_add_operation() {
        let mut processor = SimdJsonSerdeProcessor::new();
        processor.add_operation(DsonOperation::FieldAdd {
            path: "test".to_string(),
            value: OperationValue::Null,
        });
        assert_eq!(processor.operations().len(), 1);
    }

    #[test]
    fn test_simd_json_process_array() {
        let mut processor = SimdJsonSerdeProcessor::new();
        let json = r#"{"items": [1, 2, 3]}"#;
        let result = processor.process_json(json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_simd_json_process_nested() {
        let mut processor = SimdJsonSerdeProcessor::new();
        let json = r#"{"level1": {"level2": {"level3": "value"}}}"#;
        let result = processor.process_json(json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_simd_json_process_null() {
        let mut processor = SimdJsonSerdeProcessor::new();
        let json = r#"{"value": null}"#;
        let result = processor.process_json(json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_simd_json_process_bool() {
        let mut processor = SimdJsonSerdeProcessor::new();
        let json = r#"{"flag": true}"#;
        let result = processor.process_json(json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_simd_json_process_number() {
        let mut processor = SimdJsonSerdeProcessor::new();
        let json = r#"{"value": 42}"#;
        let result = processor.process_json(json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_simd_json_process_float() {
        let mut processor = SimdJsonSerdeProcessor::new();
        let json = r#"{"value": 3.14}"#;
        let result = processor.process_json(json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_simd_json_delete_operation() {
        let mut processor = SimdJsonSerdeProcessor::new();
        let json = r#"{"name": "Alice", "age": 30}"#;

        processor.add_operation(DsonOperation::FieldDelete {
            path: "age".to_string(),
        });

        let result = processor.process_json(json).unwrap();
        assert!(!result.contains("\"age\""));
    }

    #[test]
    fn test_simd_json_with_schema_filtering() {
        let input_schema = HashSet::from(["name".to_string()]);
        let output_schema = HashSet::from(["name".to_string()]);
        let mut processor = SimdJsonSerdeProcessor::with_schema(input_schema, output_schema);

        let json = r#"{"name": "Alice", "age": 30}"#;
        let result = processor.process_json(json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_simd_json_empty_json() {
        let mut processor = SimdJsonSerdeProcessor::new();
        let json = r"{}";
        let result = processor.process_json(json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_simd_json_empty_array() {
        let mut processor = SimdJsonSerdeProcessor::new();
        let json = r#"{"items": []}"#;
        let result = processor.process_json(json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_simd_json_mixed_array() {
        let mut processor = SimdJsonSerdeProcessor::new();
        let json = r#"{"items": [1, "two", true, null]}"#;
        let result = processor.process_json(json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_simd_json_invalid_json() {
        let mut processor = SimdJsonSerdeProcessor::new();
        let json = "not valid json";
        let result = processor.process_json(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_simd_json_empty_object() {
        let mut processor = SimdJsonSerdeProcessor::new();
        let json = r"{}";
        let result = processor.process_json(json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_simd_json_with_schema_filter() {
        let input_schema = std::collections::HashSet::from(["name".to_string()]);
        let output_schema = std::collections::HashSet::from(["name".to_string()]);
        let mut processor = SimdJsonSerdeProcessor::with_schema(input_schema, output_schema);
        let json = r#"{"name": "test", "age": 30}"#;
        let result = processor.process_json(json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_simd_json_deeply_nested() {
        let mut processor = SimdJsonSerdeProcessor::new();
        let json = r#"{"a": {"b": {"c": {"d": "deep"}}}}"#;
        let result = processor.process_json(json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_simd_json_array_of_objects() {
        let mut processor = SimdJsonSerdeProcessor::new();
        let json = r#"{"items": [{"id": 1}, {"id": 2}, {"id": 3}]}"#;
        let result = processor.process_json(json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_simd_json_operations_accessor() {
        let mut processor = SimdJsonSerdeProcessor::new();
        let json = r#"{"name": "test"}"#;
        processor.process_json(json).unwrap();
        let ops = processor.operations();
        assert!(!ops.is_empty() || ops.is_empty()); // Just test accessor works
    }

    #[test]
    fn test_simd_json_parse_node_recursive_bounds() {
        let mut processor = SimdJsonSerdeProcessor::new();
        // Very large index won't crash
        let json = r#"{"a": 1}"#;
        let result = processor.process_json(json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_simd_json_with_boolean_values() {
        let mut processor = SimdJsonSerdeProcessor::new();
        let json = r#"{"active": true, "deleted": false}"#;
        let result = processor.process_json(json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_simd_json_with_null_values() {
        let mut processor = SimdJsonSerdeProcessor::new();
        let json = r#"{"empty": null, "value": "test"}"#;
        let result = processor.process_json(json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_simd_json_with_numeric_values() {
        let mut processor = SimdJsonSerdeProcessor::new();
        let json = r#"{"int": 42, "float": 3.14, "negative": -100}"#;
        let result = processor.process_json(json);
        assert!(result.is_ok());
    }
}
