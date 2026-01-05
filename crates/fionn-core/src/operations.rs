// SPDX-License-Identifier: MIT OR Apache-2.0
//! Core operation types for DSON processing
//!
//! This module defines the fundamental operation types used across the fionn crates.

use super::value::OperationValue;

/// Merge strategies for CRDT conflict resolution
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeStrategy {
    /// Most recent write wins
    LastWriteWins,
    /// Combine values additively
    Additive,
    /// Keep maximum value
    Max,
    /// Keep minimum value
    Min,
    /// Union of collections
    Union,
    /// Custom merge function
    Custom(String),
}

/// DSON operations for document manipulation
#[derive(Debug, Clone, PartialEq)]
pub enum DsonOperation {
    // Structural Operations
    /// Start a new object at the specified path
    ObjectStart {
        /// Path where the object should be created
        path: String,
    },
    /// End the current object at the specified path
    ObjectEnd {
        /// Path of the object being ended
        path: String,
    },
    /// Start a new array at the specified path
    ArrayStart {
        /// Path where the array should be created
        path: String,
    },
    /// End the current array at the specified path
    ArrayEnd {
        /// Path of the array being ended
        path: String,
    },

    /// Add a new field at path with value
    FieldAdd {
        /// Target path
        path: String,
        /// Value to add
        value: OperationValue,
    },
    /// Modify existing field at path
    FieldModify {
        /// Target path
        path: String,
        /// Old value (for CRDT)
        old_value: Option<OperationValue>,
        /// New value
        new_value: OperationValue,
    },
    /// Delete field at path
    FieldDelete {
        /// Target path
        path: String,
    },

    // Batch Operations
    /// Execute multiple operations in batch
    BatchExecute {
        /// Operations to execute
        operations: Vec<Self>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_strategy_clone() {
        let strategy = MergeStrategy::LastWriteWins;
        let cloned = strategy.clone();
        assert_eq!(strategy, cloned);
    }

    #[test]
    fn test_dson_operation_field_add() {
        let op = DsonOperation::FieldAdd {
            path: "user.name".to_string(),
            value: OperationValue::StringRef("Alice".to_string()),
        };
        if let DsonOperation::FieldAdd { path, value } = op {
            assert_eq!(path, "user.name");
            assert_eq!(value, OperationValue::StringRef("Alice".to_string()));
        } else {
            panic!("Expected FieldAdd");
        }
    }
}
