// SPDX-License-Identifier: MIT OR Apache-2.0
//! Black box serde integration for DSON
//!
//! This module provides serde-compatible serialization and deserialization
//! interfaces that integrate with the black box processor for schema-aware
//! JSON processing with CRDT semantics.

use crate::error::{DsonError, Result};
use crate::operations::{DsonOperation, OperationValue};
use crate::processor::BlackBoxProcessor;
use serde::Deserialize;

/// Black box serde processor for schema-aware JSON operations
pub struct BlackBoxSerde {
    processor: BlackBoxProcessor,
    operations: Vec<DsonOperation>,
}

impl BlackBoxSerde {
    /// Create a new black box serde processor
    #[must_use]
    pub fn new(schema_in: Vec<String>, schema_out: Vec<String>) -> Self {
        Self {
            processor: BlackBoxProcessor::new(schema_in, schema_out),
            operations: Vec::new(),
        }
    }

    /// Create a processor without schema filtering
    #[must_use]
    pub fn new_unfiltered() -> Self {
        Self {
            processor: BlackBoxProcessor::new_unfiltered(),
            operations: Vec::new(),
        }
    }

    /// Add an operation to be applied during processing
    pub fn add_operation(&mut self, operation: DsonOperation) {
        self.operations.push(operation);
    }

    /// Process JSON input with operations
    ///
    /// # Errors
    /// Returns an error if processing fails
    pub fn process_json(&mut self, input: &str) -> Result<String> {
        // Apply operations to the processor
        for operation in &self.operations {
            self.processor.apply_operation(operation)?;
        }

        // Process the JSON
        self.processor.process(input)
    }

    /// Deserialize JSON into operations
    ///
    /// # Errors
    /// Returns an error if deserialization fails
    pub fn deserialize_to_operations<T: for<'de> Deserialize<'de>>(
        &self,
        json: &str,
    ) -> Result<Vec<DsonOperation>> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|e| DsonError::SerdeError(e.to_string()))?;

        Self::value_to_operations(&value, "")
    }

    /// Serialize operations to JSON
    ///
    /// # Errors
    /// Returns an error if serialization fails
    pub fn serialize_operations(&self, operations: &[DsonOperation]) -> Result<String> {
        let json_value = Self::operations_to_value(operations);
        serde_json::to_string(&json_value).map_err(|e| DsonError::SerdeError(e.to_string()))
    }

    /// Convert a JSON value to DSON operations
    fn value_to_operations(value: &serde_json::Value, path: &str) -> Result<Vec<DsonOperation>> {
        let mut operations = Vec::new();

        match value {
            serde_json::Value::Object(obj) => {
                for (key, val) in obj {
                    let field_path = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    };

                    match val {
                        serde_json::Value::String(s) => {
                            operations.push(DsonOperation::FieldAdd {
                                path: field_path,
                                value: OperationValue::StringRef(s.clone()),
                            });
                        }
                        serde_json::Value::Number(n) => {
                            operations.push(DsonOperation::FieldAdd {
                                path: field_path,
                                value: OperationValue::NumberRef(n.to_string()),
                            });
                        }
                        serde_json::Value::Bool(b) => {
                            operations.push(DsonOperation::FieldAdd {
                                path: field_path,
                                value: OperationValue::BoolRef(*b),
                            });
                        }
                        serde_json::Value::Array(arr) => {
                            // Handle array operations
                            for (index, item) in arr.iter().enumerate() {
                                let item_path = format!("{field_path}[{index}]");
                                operations.extend(Self::value_to_operations(item, &item_path)?);
                            }
                        }
                        serde_json::Value::Object(_) => {
                            // Recursively handle nested objects
                            operations.extend(Self::value_to_operations(val, &field_path)?);
                        }
                        serde_json::Value::Null => {
                            operations.push(DsonOperation::FieldAdd {
                                path: field_path,
                                value: OperationValue::Null,
                            });
                        }
                    }
                }
            }
            _ => {
                // Handle non-object root values
                match value {
                    serde_json::Value::String(s) => {
                        operations.push(DsonOperation::FieldAdd {
                            path: path.to_string(),
                            value: OperationValue::StringRef(s.clone()),
                        });
                    }
                    serde_json::Value::Number(n) => {
                        operations.push(DsonOperation::FieldAdd {
                            path: path.to_string(),
                            value: OperationValue::NumberRef(n.to_string()),
                        });
                    }
                    serde_json::Value::Bool(b) => {
                        operations.push(DsonOperation::FieldAdd {
                            path: path.to_string(),
                            value: OperationValue::BoolRef(*b),
                        });
                    }
                    serde_json::Value::Null => {
                        operations.push(DsonOperation::FieldAdd {
                            path: path.to_string(),
                            value: OperationValue::Null,
                        });
                    }
                    _ => {} // Arrays and objects handled above
                }
            }
        }

        Ok(operations)
    }

    /// Convert DSON operations to JSON value
    fn operations_to_value(operations: &[DsonOperation]) -> serde_json::Value {
        let mut root = serde_json::Value::Object(serde_json::Map::new());

        for operation in operations {
            match operation {
                DsonOperation::FieldAdd { path, value }
                | DsonOperation::FieldModify { path, value } => {
                    Self::set_value_at_path(&mut root, path, value);
                }
                // Handle other operation types as needed
                _ => {} // For now, only handle add/modify
            }
        }

        root
    }

    /// Set a value at a given path in the JSON structure
    fn set_value_at_path(root: &mut serde_json::Value, path: &str, value: &OperationValue) {
        let parts: Vec<&str> = path.split('.').collect();
        let mut current = root;

        // Navigate to the parent object
        for (i, part) in parts.iter().enumerate() {
            if i == parts.len() - 1 {
                // Set the final value
                let json_value = Self::operation_value_to_json(value);
                if let serde_json::Value::Object(obj) = current {
                    obj.insert(part.to_string(), json_value);
                }
            } else {
                // Ensure intermediate objects exist
                if let serde_json::Value::Object(obj) = current {
                    if !obj.contains_key(*part) {
                        obj.insert(
                            part.to_string(),
                            serde_json::Value::Object(serde_json::Map::new()),
                        );
                    }
                    // Key guaranteed to exist: either pre-existing or just inserted above.
                    // Using expect() with invariant message as this is a logic guarantee.
                    current = obj
                        .get_mut(*part)
                        .expect("key exists after conditional insert");
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
            // Handle Null and all other variants (ObjectRef, ArrayRef, etc.)
            _ => serde_json::Value::Null,
        }
    }
}

// TODO: Implement serde deserializer for DSON operations
// This would require implementing the serde::Deserializer trait for DsonError
// and creating a proper streaming deserializer from operations.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_black_box_serde_new() {
        let serde = BlackBoxSerde::new(vec!["name".to_string()], vec!["name".to_string()]);
        assert!(serde.operations.is_empty());
    }

    #[test]
    fn test_black_box_serde_new_unfiltered() {
        let serde = BlackBoxSerde::new_unfiltered();
        assert!(serde.operations.is_empty());
    }

    #[test]
    fn test_black_box_serde_add_operation() {
        let mut serde = BlackBoxSerde::new_unfiltered();
        serde.add_operation(DsonOperation::FieldAdd {
            path: "test".to_string(),
            value: OperationValue::Null,
        });
        assert_eq!(serde.operations.len(), 1);
    }

    #[test]
    fn test_black_box_serde_process_json() {
        let mut serde = BlackBoxSerde::new_unfiltered();
        let result = serde.process_json(r#"{"name":"test"}"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_black_box_serde_deserialize_to_operations() {
        let serde = BlackBoxSerde::new_unfiltered();
        let result =
            serde.deserialize_to_operations::<serde_json::Value>(r#"{"name":"Alice","age":30}"#);
        assert!(result.is_ok());
        let ops = result.unwrap();
        assert!(!ops.is_empty());
    }

    #[test]
    fn test_black_box_serde_deserialize_nested() {
        let serde = BlackBoxSerde::new_unfiltered();
        let result =
            serde.deserialize_to_operations::<serde_json::Value>(r#"{"user":{"name":"Alice"}}"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_black_box_serde_deserialize_array() {
        let serde = BlackBoxSerde::new_unfiltered();
        let result = serde.deserialize_to_operations::<serde_json::Value>(r#"{"items":[1,2,3]}"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_black_box_serde_deserialize_bool() {
        let serde = BlackBoxSerde::new_unfiltered();
        let result = serde.deserialize_to_operations::<serde_json::Value>(r#"{"active":true}"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_black_box_serde_deserialize_null() {
        let serde = BlackBoxSerde::new_unfiltered();
        let result = serde.deserialize_to_operations::<serde_json::Value>(r#"{"empty":null}"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_black_box_serde_serialize_operations() {
        let serde = BlackBoxSerde::new_unfiltered();
        let ops = vec![DsonOperation::FieldAdd {
            path: "name".to_string(),
            value: OperationValue::StringRef("Alice".to_string()),
        }];
        let result = serde.serialize_operations(&ops);
        assert!(result.is_ok());
        let json = result.unwrap();
        assert!(json.contains("Alice"));
    }

    #[test]
    fn test_black_box_serde_serialize_nested() {
        let serde = BlackBoxSerde::new_unfiltered();
        let ops = vec![DsonOperation::FieldAdd {
            path: "user.name".to_string(),
            value: OperationValue::StringRef("Alice".to_string()),
        }];
        let result = serde.serialize_operations(&ops);
        assert!(result.is_ok());
    }

    #[test]
    fn test_black_box_serde_serialize_number() {
        let serde = BlackBoxSerde::new_unfiltered();
        let ops = vec![DsonOperation::FieldAdd {
            path: "age".to_string(),
            value: OperationValue::NumberRef("30".to_string()),
        }];
        let result = serde.serialize_operations(&ops);
        assert!(result.is_ok());
    }

    #[test]
    fn test_black_box_serde_serialize_float() {
        let serde = BlackBoxSerde::new_unfiltered();
        let ops = vec![DsonOperation::FieldAdd {
            path: "score".to_string(),
            value: OperationValue::NumberRef("3.14".to_string()),
        }];
        let result = serde.serialize_operations(&ops);
        assert!(result.is_ok());
    }

    #[test]
    fn test_black_box_serde_serialize_bool() {
        let serde = BlackBoxSerde::new_unfiltered();
        let ops = vec![DsonOperation::FieldAdd {
            path: "active".to_string(),
            value: OperationValue::BoolRef(true),
        }];
        let result = serde.serialize_operations(&ops);
        assert!(result.is_ok());
    }

    #[test]
    fn test_black_box_serde_serialize_null() {
        let serde = BlackBoxSerde::new_unfiltered();
        let ops = vec![DsonOperation::FieldAdd {
            path: "empty".to_string(),
            value: OperationValue::Null,
        }];
        let result = serde.serialize_operations(&ops);
        assert!(result.is_ok());
    }

    #[test]
    fn test_black_box_serde_serialize_modify() {
        let serde = BlackBoxSerde::new_unfiltered();
        let ops = vec![DsonOperation::FieldModify {
            path: "name".to_string(),
            value: OperationValue::StringRef("Bob".to_string()),
        }];
        let result = serde.serialize_operations(&ops);
        assert!(result.is_ok());
    }

    #[test]
    fn test_black_box_serde_serialize_invalid_number() {
        let serde = BlackBoxSerde::new_unfiltered();
        let ops = vec![DsonOperation::FieldAdd {
            path: "value".to_string(),
            value: OperationValue::NumberRef("not_a_number".to_string()),
        }];
        let result = serde.serialize_operations(&ops);
        assert!(result.is_ok());
    }

    #[test]
    fn test_black_box_serde_deserialize_invalid() {
        let serde = BlackBoxSerde::new_unfiltered();
        let result = serde.deserialize_to_operations::<serde_json::Value>("not valid json");
        assert!(result.is_err());
    }

    #[test]
    fn test_black_box_serde_deserialize_root_string() {
        let serde = BlackBoxSerde::new_unfiltered();
        let result = serde.deserialize_to_operations::<serde_json::Value>(r#""hello""#);
        assert!(result.is_ok());
        let ops = result.unwrap();
        assert_eq!(ops.len(), 1);
    }

    #[test]
    fn test_black_box_serde_deserialize_root_number() {
        let serde = BlackBoxSerde::new_unfiltered();
        let result = serde.deserialize_to_operations::<serde_json::Value>("42");
        assert!(result.is_ok());
        let ops = result.unwrap();
        assert_eq!(ops.len(), 1);
    }

    #[test]
    fn test_black_box_serde_deserialize_root_bool() {
        let serde = BlackBoxSerde::new_unfiltered();
        let result = serde.deserialize_to_operations::<serde_json::Value>("true");
        assert!(result.is_ok());
        let ops = result.unwrap();
        assert_eq!(ops.len(), 1);
    }

    #[test]
    fn test_black_box_serde_deserialize_root_null() {
        let serde = BlackBoxSerde::new_unfiltered();
        let result = serde.deserialize_to_operations::<serde_json::Value>("null");
        assert!(result.is_ok());
        let ops = result.unwrap();
        assert_eq!(ops.len(), 1);
    }

    #[test]
    fn test_black_box_serde_deserialize_root_array() {
        let serde = BlackBoxSerde::new_unfiltered();
        let result = serde.deserialize_to_operations::<serde_json::Value>("[1, 2, 3]");
        assert!(result.is_ok());
    }

    #[test]
    fn test_black_box_serde_serialize_object_ref() {
        let serde = BlackBoxSerde::new_unfiltered();
        let ops = vec![DsonOperation::FieldAdd {
            path: "obj".to_string(),
            value: OperationValue::ObjectRef { start: 0, end: 10 },
        }];
        let result = serde.serialize_operations(&ops);
        assert!(result.is_ok());
        // ObjectRef converts to null in this implementation
        let json = result.unwrap();
        assert!(json.contains("null"));
    }

    #[test]
    fn test_black_box_serde_serialize_array_ref() {
        let serde = BlackBoxSerde::new_unfiltered();
        let ops = vec![DsonOperation::FieldAdd {
            path: "arr".to_string(),
            value: OperationValue::ArrayRef { start: 0, end: 5 },
        }];
        let result = serde.serialize_operations(&ops);
        assert!(result.is_ok());
        // ArrayRef converts to null in this implementation
        let json = result.unwrap();
        assert!(json.contains("null"));
    }

    #[test]
    fn test_black_box_serde_serialize_other_operations() {
        let serde = BlackBoxSerde::new_unfiltered();
        // Test that other operations are ignored (like FieldDelete)
        let ops = vec![
            DsonOperation::FieldDelete {
                path: "removed".to_string(),
            },
            DsonOperation::ObjectStart {
                path: "obj".to_string(),
            },
            DsonOperation::ObjectEnd {
                path: "obj".to_string(),
            },
            DsonOperation::FieldAdd {
                path: "name".to_string(),
                value: OperationValue::StringRef("test".to_string()),
            },
        ];
        let result = serde.serialize_operations(&ops);
        assert!(result.is_ok());
        // Only the FieldAdd should be in the result
        let json = result.unwrap();
        assert!(json.contains("test"));
    }

    #[test]
    fn test_black_box_serde_serialize_deeply_nested() {
        let serde = BlackBoxSerde::new_unfiltered();
        let ops = vec![DsonOperation::FieldAdd {
            path: "level1.level2.level3.value".to_string(),
            value: OperationValue::StringRef("deep".to_string()),
        }];
        let result = serde.serialize_operations(&ops);
        assert!(result.is_ok());
        let json = result.unwrap();
        assert!(json.contains("deep"));
    }

    #[test]
    fn test_black_box_serde_serialize_nan_float() {
        let serde = BlackBoxSerde::new_unfiltered();
        // NaN and Infinity are not valid JSON numbers, should fall back to string
        let ops = vec![DsonOperation::FieldAdd {
            path: "value".to_string(),
            value: OperationValue::NumberRef("NaN".to_string()),
        }];
        let result = serde.serialize_operations(&ops);
        assert!(result.is_ok());
    }

    #[test]
    fn test_black_box_serde_serialize_infinity_float() {
        let serde = BlackBoxSerde::new_unfiltered();
        let ops = vec![DsonOperation::FieldAdd {
            path: "value".to_string(),
            value: OperationValue::NumberRef("Infinity".to_string()),
        }];
        let result = serde.serialize_operations(&ops);
        assert!(result.is_ok());
    }

    #[test]
    fn test_black_box_serde_process_json_with_operations() {
        let mut serde = BlackBoxSerde::new_unfiltered();
        serde.add_operation(DsonOperation::FieldModify {
            path: "name".to_string(),
            value: OperationValue::StringRef("modified".to_string()),
        });
        let result = serde.process_json(r#"{"name":"original"}"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_black_box_serde_deserialize_array_of_objects() {
        let serde = BlackBoxSerde::new_unfiltered();
        let result = serde.deserialize_to_operations::<serde_json::Value>(
            r#"{"users":[{"name":"Alice"},{"name":"Bob"}]}"#,
        );
        assert!(result.is_ok());
        let ops = result.unwrap();
        // Should have operations for each nested element
        assert!(ops.len() >= 2);
    }

    #[test]
    fn test_black_box_serde_empty_object() {
        let serde = BlackBoxSerde::new_unfiltered();
        let result = serde.deserialize_to_operations::<serde_json::Value>("{}");
        assert!(result.is_ok());
        let ops = result.unwrap();
        assert!(ops.is_empty());
    }

    #[test]
    fn test_black_box_serde_serialize_empty_operations() {
        let serde = BlackBoxSerde::new_unfiltered();
        let ops: Vec<DsonOperation> = vec![];
        let result = serde.serialize_operations(&ops);
        assert!(result.is_ok());
        let json = result.unwrap();
        assert_eq!(json, "{}");
    }
}
