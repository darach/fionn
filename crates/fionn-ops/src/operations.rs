// SPDX-License-Identifier: MIT OR Apache-2.0
//! Canonical DSON Operations and Zero-Allocation Processing
//!
//! This module defines the complete set of operations for schema-aware DSON processing.
//! Operations are designed for zero-allocation streaming with JSON path-based filtering.

use fionn_core::{DsonError, Result};
use std::collections::{HashMap, HashSet};

/// Complete set of DSON operations
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
        /// New value
        value: OperationValue,
    },
    /// Delete field at path
    FieldDelete {
        /// Target path
        path: String,
    },

    /// Insert element into array at index
    ArrayInsert {
        /// Array path
        path: String,
        /// Insert position
        index: usize,
        /// Value to insert
        value: OperationValue,
    },
    /// Remove element from array at index
    ArrayRemove {
        /// Array path
        path: String,
        /// Remove position
        index: usize,
    },
    /// Replace element in array at index
    ArrayReplace {
        /// Array path
        path: String,
        /// Replace position
        index: usize,
        /// New value
        value: OperationValue,
    },

    /// Check that path exists
    CheckPresence {
        /// Path to check
        path: String,
    },
    /// Check that path does not exist
    CheckAbsence {
        /// Path to check
        path: String,
    },
    /// Check that value at path is null
    CheckNull {
        /// Path to check
        path: String,
    },
    /// Check that value at path is not null
    CheckNotNull {
        /// Path to check
        path: String,
    },

    /// Merge field with CRDT semantics
    MergeField {
        /// Target path
        path: String,
        /// Value to merge
        value: OperationValue,
        /// Operation timestamp
        timestamp: u64,
    },
    /// Resolve conflict using strategy
    ConflictResolve {
        /// Conflict path
        path: String,
        /// Resolution strategy
        strategy: MergeStrategy,
    },

    /// Build array from elements
    ArrayBuild {
        /// Array path
        path: String,
        /// Array elements
        elements: Vec<OperationValue>,
    },
    /// Filter array elements
    ArrayFilter {
        /// Array path
        path: String,
        /// Filter predicate
        predicate: FilterPredicate,
    },
    /// Transform array elements
    ArrayMap {
        /// Array path
        path: String,
        /// Transform function
        transform: TransformFunction,
    },
    /// Reduce array to single value
    ArrayReduce {
        /// Array path
        path: String,
        /// Initial accumulator
        initial: OperationValue,
        /// Reducer function
        reducer: ReduceFunction,
    },

    /// Build stream from generator
    StreamBuild {
        /// Stream path
        path: String,
        /// Value generator
        generator: StreamGenerator,
    },
    /// Filter stream elements
    StreamFilter {
        /// Stream path
        path: String,
        /// Filter predicate
        predicate: FilterPredicate,
    },
    /// Transform stream elements
    StreamMap {
        /// Stream path
        path: String,
        /// Transform function
        transform: TransformFunction,
    },
    /// Emit stream batch
    StreamEmit {
        /// Stream path
        path: String,
        /// Batch size
        batch_size: usize,
    },

    /// Execute batch of operations
    BatchExecute {
        /// Operations to execute
        operations: Vec<Self>,
    },
}

/// Values that can be operated on (re-exported from fionn-core)
pub use fionn_core::OperationValue;

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

/// Filter predicates for array/stream filtering
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterPredicate {
    /// Keep every nth element
    EveryNth(usize),
    /// Keep alternating elements (1st, 3rd, 5th...)
    Alternate,
    /// Keep even indices
    Even,
    /// Keep odd indices
    Odd,
    /// Keep values greater than threshold
    GreaterThan(i64),
    /// Keep values less than threshold
    LessThan(i64),
    /// Keep values equal to target
    Equals(OperationValue),
    /// Custom predicate expression
    Custom(String),
}

/// Transform functions for mapping operations
#[derive(Debug, Clone)]
pub enum TransformFunction {
    /// Add to numeric values
    Add(i64),
    /// Multiply numeric values
    Multiply(i64),
    /// Convert string to uppercase
    ToUppercase,
    /// Convert string to lowercase
    ToLowercase,
    /// Append suffix to strings
    Append(String),
    /// Prepend prefix to strings
    Prepend(String),
    /// Custom transform expression
    Custom(String),
}

impl PartialEq for TransformFunction {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Add(a), Self::Add(b)) | (Self::Multiply(a), Self::Multiply(b)) => a == b,
            (Self::ToUppercase, Self::ToUppercase) | (Self::ToLowercase, Self::ToLowercase) => true,
            (Self::Append(a), Self::Append(b))
            | (Self::Prepend(a), Self::Prepend(b))
            | (Self::Custom(a), Self::Custom(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for TransformFunction {}

/// Reduce functions for aggregation
#[derive(Debug, Clone)]
pub enum ReduceFunction {
    /// Sum numeric values
    Sum,
    /// Product of numeric values
    Product,
    /// Minimum value
    Min,
    /// Maximum value
    Max,
    /// Count elements
    Count,
    /// Concatenate strings
    Concat,
    /// Custom reduction expression
    Custom(String),
}

impl PartialEq for ReduceFunction {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Sum, Self::Sum)
            | (Self::Product, Self::Product)
            | (Self::Min, Self::Min)
            | (Self::Max, Self::Max)
            | (Self::Count, Self::Count)
            | (Self::Concat, Self::Concat) => true,
            (Self::Custom(a), Self::Custom(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for ReduceFunction {}

/// Stream generators for streaming operations
#[derive(Debug, Clone)]
pub enum StreamGenerator {
    /// Generate numeric range
    Range {
        /// Start value
        start: i64,
        /// End value (exclusive)
        end: i64,
        /// Step between values
        step: i64,
    },
    /// Repeat value N times
    Repeat(OperationValue, usize),
    /// Generate fibonacci sequence
    Fibonacci(usize),
    /// Custom generator expression
    Custom(String),
}

impl PartialEq for StreamGenerator {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Range {
                    start: s1,
                    end: e1,
                    step: st1,
                },
                Self::Range {
                    start: s2,
                    end: e2,
                    step: st2,
                },
            ) => s1 == s2 && e1 == e2 && st1 == st2,
            (Self::Repeat(v1, c1), Self::Repeat(v2, c2)) => v1 == v2 && c1 == c2,
            (Self::Fibonacci(f1), Self::Fibonacci(f2)) => f1 == f2,
            (Self::Custom(c1), Self::Custom(c2)) => c1 == c2,
            _ => false,
        }
    }
}

impl Eq for StreamGenerator {}

/// Canonical operation processor that optimizes and filters operations
pub struct CanonicalOperationProcessor {
    input_schema: HashSet<String>,             // Allowed input paths
    output_schema: HashSet<String>,            // Allowed output paths
    operations: Vec<DsonOperation>,            // Original operations
    canonical_ops: Vec<DsonOperation>,         // Optimized canonical sequence
    field_states: HashMap<String, FieldState>, // Track field lifecycle
}

#[derive(Debug, Clone)]
enum FieldState {
    NotPresent,
    Present,
    Deleted,
    Modified,
}

impl CanonicalOperationProcessor {
    /// Create a new canonical operation processor with input and output schemas.
    #[must_use]
    pub fn new(input_schema: HashSet<String>, output_schema: HashSet<String>) -> Self {
        Self {
            input_schema,
            output_schema,
            operations: Vec::new(),
            canonical_ops: Vec::new(),
            field_states: HashMap::new(),
        }
    }

    /// Add an operation to be processed
    pub fn add_operation(&mut self, op: DsonOperation) {
        self.operations.push(op);
    }

    /// Compute canonical operation sequence
    ///
    /// # Errors
    ///
    /// Returns an error if an operation is invalid, such as:
    /// - Modifying a non-existent field
    /// - Modifying a deleted field
    pub fn compute_canonical(&mut self) -> Result<&[DsonOperation]> {
        // Reset state
        self.canonical_ops.clear();
        self.field_states.clear();

        // Process each operation
        let operations = self.operations.clone(); // Clone to avoid borrowing issues
        for op in operations {
            self.process_operation(op)?;
        }

        // Filter for output schema
        self.filter_output_schema();

        Ok(&self.canonical_ops)
    }

    fn process_operation(&mut self, op: DsonOperation) -> Result<()> {
        match op {
            // Field operations - track state and optimize
            DsonOperation::FieldAdd { path, value } => {
                self.process_field_add(path, value);
            }
            DsonOperation::FieldModify { path, value } => {
                self.process_field_modify(path, value)?;
            }
            DsonOperation::FieldDelete { path } => {
                self.process_field_delete(path);
            }

            // Presence operations - always pass through (they don't modify)
            DsonOperation::CheckPresence { .. }
            | DsonOperation::CheckAbsence { .. }
            | DsonOperation::CheckNull { .. }
            | DsonOperation::CheckNotNull { .. } => {
                self.canonical_ops.push(op);
            }

            // CRDT MergeField needs special handling
            DsonOperation::MergeField {
                path,
                value,
                timestamp,
            } => {
                self.process_merge_field(path, value, timestamp);
            }

            // Batch operations - recurse into the batch
            DsonOperation::BatchExecute { operations } => {
                for batch_op in operations {
                    self.process_operation(batch_op)?;
                }
            }

            // All other operations with paths - filter by input schema
            DsonOperation::ObjectStart { ref path }
            | DsonOperation::ObjectEnd { ref path }
            | DsonOperation::ArrayStart { ref path }
            | DsonOperation::ArrayEnd { ref path }
            | DsonOperation::ArrayInsert { ref path, .. }
            | DsonOperation::ArrayRemove { ref path, .. }
            | DsonOperation::ArrayReplace { ref path, .. }
            | DsonOperation::ConflictResolve { ref path, .. }
            | DsonOperation::ArrayBuild { ref path, .. }
            | DsonOperation::ArrayFilter { ref path, .. }
            | DsonOperation::ArrayMap { ref path, .. }
            | DsonOperation::ArrayReduce { ref path, .. }
            | DsonOperation::StreamBuild { ref path, .. }
            | DsonOperation::StreamFilter { ref path, .. }
            | DsonOperation::StreamMap { ref path, .. }
            | DsonOperation::StreamEmit { ref path, .. } => {
                if self.should_process_path(path) {
                    self.canonical_ops.push(op);
                }
            }
        }
        Ok(())
    }

    fn process_field_add(&mut self, path: String, value: OperationValue) {
        if !self.should_process_path(&path) {
            return; // Skip if not in input schema
        }

        let current_state = self
            .field_states
            .get(&path)
            .cloned()
            .unwrap_or(FieldState::NotPresent);

        match current_state {
            FieldState::NotPresent => {
                // Normal add
                self.field_states.insert(path.clone(), FieldState::Present);
                self.canonical_ops
                    .push(DsonOperation::FieldAdd { path, value });
            }
            FieldState::Deleted => {
                // Add after delete - becomes modify
                self.field_states.insert(path.clone(), FieldState::Present);
                self.canonical_ops
                    .push(DsonOperation::FieldModify { path, value });
            }
            FieldState::Present | FieldState::Modified => {
                // Add after existing - becomes modify
                self.field_states.insert(path.clone(), FieldState::Modified);
                self.canonical_ops
                    .push(DsonOperation::FieldModify { path, value });
            }
        }
    }

    fn process_field_modify(&mut self, path: String, value: OperationValue) -> Result<()> {
        if !self.should_process_path(&path) {
            return Ok(()); // Skip if not in input schema
        }

        let current_state = self
            .field_states
            .get(&path)
            .cloned()
            .unwrap_or(FieldState::NotPresent);

        match current_state {
            FieldState::NotPresent => {
                // Modify of non-existent field - error unless checking absence
                return Err(DsonError::InvalidOperation(format!(
                    "Cannot modify non-existent field: {path}"
                )));
            }
            FieldState::Deleted => {
                // Modify after delete - invalid
                return Err(DsonError::InvalidOperation(format!(
                    "Cannot modify deleted field: {path}"
                )));
            }
            FieldState::Present | FieldState::Modified => {
                // Normal modify
                self.field_states.insert(path.clone(), FieldState::Modified);
                self.canonical_ops
                    .push(DsonOperation::FieldModify { path, value });
            }
        }
        Ok(())
    }

    fn process_field_delete(&mut self, path: String) {
        if !self.should_process_path(&path) {
            return; // Skip if not in input schema
        }

        let current_state = self
            .field_states
            .get(&path)
            .cloned()
            .unwrap_or(FieldState::NotPresent);

        match current_state {
            FieldState::NotPresent => {
                // Delete of non-existent field - mark as deleted for future operations
                self.field_states.insert(path, FieldState::Deleted);
                // Don't add the delete operation since it was a no-op, but track the state
            }
            FieldState::Deleted => {
                // Already deleted - no-op
            }
            FieldState::Present | FieldState::Modified => {
                // Normal delete
                self.field_states.insert(path.clone(), FieldState::Deleted);
                self.canonical_ops.push(DsonOperation::FieldDelete { path });
            }
        }
    }

    fn process_merge_field(&mut self, path: String, value: OperationValue, timestamp: u64) {
        if !self.should_process_path(&path) {
            return; // Skip if not in input schema
        }

        // For CRDT merges, we always add the operation
        // Conflict resolution happens at merge time
        self.canonical_ops.push(DsonOperation::MergeField {
            path,
            value,
            timestamp,
        });
    }

    fn should_process_path(&self, path: &str) -> bool {
        // Check if path is in input schema (allows wildcards)
        self.input_schema.contains(path)
            || self.input_schema.iter().any(|schema_path| {
                schema_path.ends_with(".*")
                    && path.starts_with(&schema_path[..schema_path.len() - 2])
            })
    }

    fn filter_output_schema(&mut self) {
        // Remove operations for fields not in output schema
        self.canonical_ops.retain(|op| {
            match op {
                DsonOperation::FieldAdd { path, .. }
                | DsonOperation::FieldModify { path, .. }
                | DsonOperation::FieldDelete { path }
                | DsonOperation::ArrayInsert { path, .. }
                | DsonOperation::ArrayRemove { path, .. }
                | DsonOperation::ArrayReplace { path, .. }
                | DsonOperation::MergeField { path, .. }
                | DsonOperation::ConflictResolve { path, .. } => {
                    self.output_schema.contains(path)
                        || self.output_schema.iter().any(|schema_path| {
                            schema_path.ends_with(".*")
                                && path.starts_with(&schema_path[..schema_path.len() - 2])
                        })
                }
                // Structural and presence operations always pass through
                _ => true,
            }
        });
    }
}

/// Operation sequence optimizer
pub struct OperationOptimizer {
    operations: Vec<DsonOperation>,
}

impl OperationOptimizer {
    /// Create a new operation optimizer with the given operations.
    #[must_use]
    pub const fn new(operations: Vec<DsonOperation>) -> Self {
        Self { operations }
    }

    /// Optimize operation sequence for zero-allocation processing
    #[must_use]
    pub fn optimize(mut self) -> Vec<DsonOperation> {
        // Phase 1: Coalesce adjacent operations
        self.coalesce_operations();

        // Phase 2: Reorder for efficiency
        self.reorder_for_efficiency();

        // Phase 3: Remove redundant operations
        self.remove_redundant_ops();

        self.operations
    }

    fn remove_redundant_ops(&mut self) {
        let mut i = 0;
        while i < self.operations.len() {
            if self.is_redundant_operation(i) {
                self.operations.remove(i);
            } else {
                i += 1;
            }
        }
    }

    fn is_redundant_operation(&self, index: usize) -> bool {
        let op = &self.operations[index];

        match op {
            DsonOperation::FieldAdd { path, .. } => {
                // Check if this field is deleted later without being re-added
                self.is_field_deleted_after_without_readd(path, index)
            }
            DsonOperation::FieldDelete { path } => {
                // Check if this field is added/modified after (making the delete redundant)
                self.is_field_modified_after(path, index)
            }
            // All other operations (including FieldModify) are never redundant
            _ => false,
        }
    }

    fn is_field_deleted_after_without_readd(&self, path: &str, start_index: usize) -> bool {
        let mut saw_delete = false;
        for i in start_index + 1..self.operations.len() {
            match &self.operations[i] {
                DsonOperation::FieldDelete { path: del_path } if del_path == path => {
                    saw_delete = true;
                }
                DsonOperation::FieldAdd { path: add_path, .. } if add_path == path => {
                    // If we see an add after a delete, it's not redundant
                    if saw_delete {
                        return false;
                    }
                }
                _ => {}
            }
        }
        saw_delete
    }

    fn is_field_modified_after(&self, path: &str, start_index: usize) -> bool {
        for i in start_index + 1..self.operations.len() {
            match &self.operations[i] {
                DsonOperation::FieldAdd { path: add_path, .. } if add_path == path => {
                    return true;
                }
                DsonOperation::FieldModify { path: mod_path, .. } if mod_path == path => {
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    fn reorder_for_efficiency(&mut self) {
        // Reorder operations to minimize tape navigation
        // Group operations by path prefix for locality
        let mut operations_with_paths: Vec<(String, DsonOperation)> = self
            .operations
            .drain(..)
            .map(|op| {
                let path = Self::get_operation_path(&op);
                // For sorting, append a high character to End operations so they come after children
                let sort_key = match op {
                    DsonOperation::ObjectEnd { .. } | DsonOperation::ArrayEnd { .. } => {
                        format!("{path}\u{10FFFF}")
                    }
                    _ => path,
                };
                (sort_key, op)
            })
            .collect();

        operations_with_paths.sort_by(|(a_key, _), (b_key, _)| a_key.cmp(b_key));

        self.operations = operations_with_paths
            .into_iter()
            .map(|(_, op)| op)
            .collect();
    }

    fn get_operation_path(op: &DsonOperation) -> String {
        match op {
            DsonOperation::ObjectStart { path }
            | DsonOperation::ObjectEnd { path }
            | DsonOperation::ArrayStart { path }
            | DsonOperation::ArrayEnd { path }
            | DsonOperation::FieldAdd { path, .. }
            | DsonOperation::FieldModify { path, .. }
            | DsonOperation::FieldDelete { path }
            | DsonOperation::ArrayInsert { path, .. }
            | DsonOperation::ArrayRemove { path, .. }
            | DsonOperation::ArrayReplace { path, .. }
            | DsonOperation::CheckPresence { path }
            | DsonOperation::CheckAbsence { path }
            | DsonOperation::CheckNull { path }
            | DsonOperation::CheckNotNull { path }
            | DsonOperation::MergeField { path, .. }
            | DsonOperation::ConflictResolve { path, .. }
            | DsonOperation::ArrayBuild { path, .. }
            | DsonOperation::ArrayFilter { path, .. }
            | DsonOperation::ArrayMap { path, .. }
            | DsonOperation::ArrayReduce { path, .. }
            | DsonOperation::StreamBuild { path, .. }
            | DsonOperation::StreamFilter { path, .. }
            | DsonOperation::StreamMap { path, .. }
            | DsonOperation::StreamEmit { path, .. } => path.clone(),
            DsonOperation::BatchExecute { .. } => "batch".to_string(),
        }
    }

    fn coalesce_operations(&mut self) {
        let mut i = 0;
        while i + 1 < self.operations.len() {
            if self.can_coalesce(i, i + 1) {
                self.coalesce_pair(i, i + 1);
                // Remove the second operation (now coalesced)
                self.operations.remove(i + 1);
            } else {
                i += 1;
            }
        }
    }

    fn can_coalesce(&self, i: usize, j: usize) -> bool {
        match (&self.operations[i], &self.operations[j]) {
            (
                DsonOperation::FieldModify { path: p1, .. },
                DsonOperation::FieldModify { path: p2, .. },
            ) => {
                p1 == p2 // Multiple modifies on same field can be coalesced to last one
            }
            _ => false,
        }
    }

    fn coalesce_pair(&mut self, i: usize, j: usize) {
        // For multiple modifies, keep the last one
        let (left, right) = self.operations.split_at_mut(j);
        if let DsonOperation::FieldModify { value, .. } = &right[0]
            && let DsonOperation::FieldModify {
                value: old_value, ..
            } = &mut left[i]
        {
            *old_value = value.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonical_delete_then_add() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["field1".to_string()]),
            HashSet::from(["field1".to_string()]),
        );

        // Delete then add - should become modify
        processor.add_operation(DsonOperation::FieldDelete {
            path: "field1".to_string(),
        });
        processor.add_operation(DsonOperation::FieldAdd {
            path: "field1".to_string(),
            value: OperationValue::StringRef("value".to_string()),
        });

        let canonical = processor.compute_canonical().unwrap();

        assert_eq!(canonical.len(), 1);
        match &canonical[0] {
            DsonOperation::FieldModify { path, .. } => assert_eq!(path, "field1"),
            _ => panic!("Expected FieldModify"),
        }
    }

    #[test]
    fn test_input_schema_filtering() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["allowed".to_string()]), // Only "allowed" in input schema
            HashSet::from(["allowed".to_string()]), // Only "allowed" in output schema
        );

        processor.add_operation(DsonOperation::FieldAdd {
            path: "allowed".to_string(),
            value: OperationValue::StringRef("ok".to_string()),
        });
        processor.add_operation(DsonOperation::FieldAdd {
            path: "not_allowed".to_string(),
            value: OperationValue::StringRef("filtered".to_string()),
        });

        let canonical = processor.compute_canonical().unwrap();

        // Only the allowed operation should remain
        assert_eq!(canonical.len(), 1);
        match &canonical[0] {
            DsonOperation::FieldAdd { path, .. } => assert_eq!(path, "allowed"),
            _ => panic!("Expected FieldAdd for allowed field"),
        }
    }

    #[test]
    fn test_operation_optimizer() {
        let operations = vec![
            DsonOperation::FieldModify {
                path: "field1".to_string(),
                value: OperationValue::StringRef("first".to_string()),
            },
            DsonOperation::FieldModify {
                path: "field1".to_string(),
                value: OperationValue::StringRef("second".to_string()),
            },
            DsonOperation::FieldDelete {
                path: "field1".to_string(),
            },
        ];

        let optimizer = OperationOptimizer::new(operations);
        let ops = optimizer.optimize();

        // Should coalesce the two modifies and keep the delete
        assert_eq!(ops.len(), 2);
        match &ops[0] {
            DsonOperation::FieldModify { value, .. } => match value {
                OperationValue::StringRef(s) => assert_eq!(s, "second"),
                _ => panic!("Expected StringRef with 'second'"),
            },
            _ => panic!("Expected FieldModify"),
        }
        match &ops[1] {
            DsonOperation::FieldDelete { path } => assert_eq!(path, "field1"),
            _ => panic!("Expected FieldDelete"),
        }
    }

    #[test]
    fn test_operation_value_equality() {
        assert_eq!(OperationValue::Null, OperationValue::Null);
        assert_eq!(OperationValue::BoolRef(true), OperationValue::BoolRef(true));
        assert_ne!(
            OperationValue::BoolRef(true),
            OperationValue::BoolRef(false)
        );
        assert_eq!(
            OperationValue::StringRef("test".to_string()),
            OperationValue::StringRef("test".to_string())
        );
        assert_eq!(
            OperationValue::NumberRef("42".to_string()),
            OperationValue::NumberRef("42".to_string())
        );
        assert_eq!(
            OperationValue::ObjectRef { start: 0, end: 10 },
            OperationValue::ObjectRef { start: 0, end: 10 }
        );
        assert_eq!(
            OperationValue::ArrayRef { start: 0, end: 5 },
            OperationValue::ArrayRef { start: 0, end: 5 }
        );
    }

    #[test]
    fn test_merge_strategy_equality() {
        assert_eq!(MergeStrategy::LastWriteWins, MergeStrategy::LastWriteWins);
        assert_eq!(MergeStrategy::Additive, MergeStrategy::Additive);
        assert_eq!(MergeStrategy::Max, MergeStrategy::Max);
        assert_eq!(MergeStrategy::Min, MergeStrategy::Min);
        assert_eq!(MergeStrategy::Union, MergeStrategy::Union);
        assert_eq!(
            MergeStrategy::Custom("test".to_string()),
            MergeStrategy::Custom("test".to_string())
        );
    }

    #[test]
    fn test_filter_predicate_equality() {
        assert_eq!(FilterPredicate::Even, FilterPredicate::Even);
        assert_eq!(FilterPredicate::Odd, FilterPredicate::Odd);
        assert_eq!(FilterPredicate::Alternate, FilterPredicate::Alternate);
        assert_eq!(FilterPredicate::EveryNth(3), FilterPredicate::EveryNth(3));
        assert_eq!(
            FilterPredicate::GreaterThan(10),
            FilterPredicate::GreaterThan(10)
        );
        assert_eq!(FilterPredicate::LessThan(5), FilterPredicate::LessThan(5));
        assert_eq!(
            FilterPredicate::Equals(OperationValue::Null),
            FilterPredicate::Equals(OperationValue::Null)
        );
        assert_eq!(
            FilterPredicate::Custom("custom".to_string()),
            FilterPredicate::Custom("custom".to_string())
        );
    }

    #[test]
    fn test_transform_function_equality() {
        assert_eq!(TransformFunction::Add(5), TransformFunction::Add(5));
        assert_ne!(TransformFunction::Add(5), TransformFunction::Add(10));
        assert_eq!(
            TransformFunction::Multiply(2),
            TransformFunction::Multiply(2)
        );
        assert_eq!(
            TransformFunction::ToUppercase,
            TransformFunction::ToUppercase
        );
        assert_eq!(
            TransformFunction::ToLowercase,
            TransformFunction::ToLowercase
        );
        assert_eq!(
            TransformFunction::Append("x".to_string()),
            TransformFunction::Append("x".to_string())
        );
        assert_eq!(
            TransformFunction::Prepend("y".to_string()),
            TransformFunction::Prepend("y".to_string())
        );
        assert_eq!(
            TransformFunction::Custom("c".to_string()),
            TransformFunction::Custom("c".to_string())
        );
        assert_ne!(TransformFunction::Add(1), TransformFunction::Multiply(1));
    }

    #[test]
    fn test_reduce_function_equality() {
        assert_eq!(ReduceFunction::Sum, ReduceFunction::Sum);
        assert_eq!(ReduceFunction::Product, ReduceFunction::Product);
        assert_eq!(ReduceFunction::Min, ReduceFunction::Min);
        assert_eq!(ReduceFunction::Max, ReduceFunction::Max);
        assert_eq!(ReduceFunction::Count, ReduceFunction::Count);
        assert_eq!(ReduceFunction::Concat, ReduceFunction::Concat);
        assert_eq!(
            ReduceFunction::Custom("r".to_string()),
            ReduceFunction::Custom("r".to_string())
        );
        assert_ne!(ReduceFunction::Sum, ReduceFunction::Product);
    }

    #[test]
    fn test_stream_generator_equality() {
        assert_eq!(
            StreamGenerator::Range {
                start: 0,
                end: 10,
                step: 1
            },
            StreamGenerator::Range {
                start: 0,
                end: 10,
                step: 1
            }
        );
        assert_ne!(
            StreamGenerator::Range {
                start: 0,
                end: 10,
                step: 1
            },
            StreamGenerator::Range {
                start: 0,
                end: 20,
                step: 1
            }
        );
        assert_eq!(
            StreamGenerator::Repeat(OperationValue::Null, 5),
            StreamGenerator::Repeat(OperationValue::Null, 5)
        );
        assert_eq!(
            StreamGenerator::Fibonacci(10),
            StreamGenerator::Fibonacci(10)
        );
        assert_eq!(
            StreamGenerator::Custom("g".to_string()),
            StreamGenerator::Custom("g".to_string())
        );
    }

    #[test]
    fn test_dson_operation_clone() {
        let op = DsonOperation::FieldAdd {
            path: "test".to_string(),
            value: OperationValue::Null,
        };
        let cloned = op.clone();
        assert_eq!(op, cloned);
    }

    #[test]
    fn test_dson_operation_debug() {
        let op = DsonOperation::FieldAdd {
            path: "test".to_string(),
            value: OperationValue::Null,
        };
        let debug = format!("{op:?}");
        assert!(debug.contains("FieldAdd"));
    }

    #[test]
    fn test_canonical_processor_structural_ops() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["obj".to_string()]),
            HashSet::from(["obj".to_string()]),
        );

        processor.add_operation(DsonOperation::ObjectStart {
            path: "obj".to_string(),
        });
        processor.add_operation(DsonOperation::ObjectEnd {
            path: "obj".to_string(),
        });

        let canonical = processor.compute_canonical().unwrap();
        assert_eq!(canonical.len(), 2);
    }

    #[test]
    fn test_canonical_processor_array_structural() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["arr".to_string()]),
            HashSet::from(["arr".to_string()]),
        );

        processor.add_operation(DsonOperation::ArrayStart {
            path: "arr".to_string(),
        });
        processor.add_operation(DsonOperation::ArrayEnd {
            path: "arr".to_string(),
        });

        let canonical = processor.compute_canonical().unwrap();
        assert_eq!(canonical.len(), 2);
    }

    #[test]
    fn test_canonical_processor_array_operations() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["items".to_string()]),
            HashSet::from(["items".to_string()]),
        );

        processor.add_operation(DsonOperation::ArrayInsert {
            path: "items".to_string(),
            index: 0,
            value: OperationValue::Null,
        });
        processor.add_operation(DsonOperation::ArrayRemove {
            path: "items".to_string(),
            index: 0,
        });
        processor.add_operation(DsonOperation::ArrayReplace {
            path: "items".to_string(),
            index: 0,
            value: OperationValue::Null,
        });

        let canonical = processor.compute_canonical().unwrap();
        assert_eq!(canonical.len(), 3);
    }

    #[test]
    fn test_canonical_processor_presence_ops() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["field".to_string()]),
            HashSet::from(["field".to_string()]),
        );

        processor.add_operation(DsonOperation::CheckPresence {
            path: "field".to_string(),
        });
        processor.add_operation(DsonOperation::CheckAbsence {
            path: "field".to_string(),
        });
        processor.add_operation(DsonOperation::CheckNull {
            path: "field".to_string(),
        });
        processor.add_operation(DsonOperation::CheckNotNull {
            path: "field".to_string(),
        });

        let canonical = processor.compute_canonical().unwrap();
        assert_eq!(canonical.len(), 4);
    }

    #[test]
    fn test_canonical_processor_merge_field() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["field".to_string()]),
            HashSet::from(["field".to_string()]),
        );

        processor.add_operation(DsonOperation::MergeField {
            path: "field".to_string(),
            value: OperationValue::Null,
            timestamp: 1,
        });

        let canonical = processor.compute_canonical().unwrap();
        assert_eq!(canonical.len(), 1);
    }

    #[test]
    fn test_canonical_processor_conflict_resolve() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["field".to_string()]),
            HashSet::from(["field".to_string()]),
        );

        processor.add_operation(DsonOperation::ConflictResolve {
            path: "field".to_string(),
            strategy: MergeStrategy::LastWriteWins,
        });

        let canonical = processor.compute_canonical().unwrap();
        assert_eq!(canonical.len(), 1);
    }

    #[test]
    fn test_canonical_processor_advanced_array_ops() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["arr".to_string()]),
            HashSet::from(["arr".to_string()]),
        );

        processor.add_operation(DsonOperation::ArrayBuild {
            path: "arr".to_string(),
            elements: vec![OperationValue::Null],
        });
        processor.add_operation(DsonOperation::ArrayFilter {
            path: "arr".to_string(),
            predicate: FilterPredicate::Even,
        });
        processor.add_operation(DsonOperation::ArrayMap {
            path: "arr".to_string(),
            transform: TransformFunction::Add(1),
        });
        processor.add_operation(DsonOperation::ArrayReduce {
            path: "arr".to_string(),
            initial: OperationValue::NumberRef("0".to_string()),
            reducer: ReduceFunction::Sum,
        });

        let canonical = processor.compute_canonical().unwrap();
        assert_eq!(canonical.len(), 4);
    }

    #[test]
    fn test_canonical_processor_streaming_ops() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["stream".to_string()]),
            HashSet::from(["stream".to_string()]),
        );

        processor.add_operation(DsonOperation::StreamBuild {
            path: "stream".to_string(),
            generator: StreamGenerator::Fibonacci(10),
        });
        processor.add_operation(DsonOperation::StreamFilter {
            path: "stream".to_string(),
            predicate: FilterPredicate::Even,
        });
        processor.add_operation(DsonOperation::StreamMap {
            path: "stream".to_string(),
            transform: TransformFunction::Multiply(2),
        });
        processor.add_operation(DsonOperation::StreamEmit {
            path: "stream".to_string(),
            batch_size: 10,
        });

        let canonical = processor.compute_canonical().unwrap();
        assert_eq!(canonical.len(), 4);
    }

    #[test]
    fn test_canonical_processor_batch_execute() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["field".to_string()]),
            HashSet::from(["field".to_string()]),
        );

        processor.add_operation(DsonOperation::BatchExecute {
            operations: vec![DsonOperation::FieldAdd {
                path: "field".to_string(),
                value: OperationValue::Null,
            }],
        });

        let canonical = processor.compute_canonical().unwrap();
        assert_eq!(canonical.len(), 1);
    }

    #[test]
    fn test_canonical_processor_wildcard_schema() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["user.*".to_string()]),
            HashSet::from(["user.*".to_string()]),
        );

        processor.add_operation(DsonOperation::FieldAdd {
            path: "user.name".to_string(),
            value: OperationValue::StringRef("test".to_string()),
        });

        let canonical = processor.compute_canonical().unwrap();
        assert_eq!(canonical.len(), 1);
    }

    #[test]
    fn test_canonical_processor_add_then_add() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["field".to_string()]),
            HashSet::from(["field".to_string()]),
        );

        processor.add_operation(DsonOperation::FieldAdd {
            path: "field".to_string(),
            value: OperationValue::StringRef("first".to_string()),
        });
        processor.add_operation(DsonOperation::FieldAdd {
            path: "field".to_string(),
            value: OperationValue::StringRef("second".to_string()),
        });

        let canonical = processor.compute_canonical().unwrap();
        // Second add becomes modify
        assert_eq!(canonical.len(), 2);
    }

    #[test]
    fn test_operation_optimizer_remove_redundant() {
        let operations = vec![
            DsonOperation::FieldAdd {
                path: "field".to_string(),
                value: OperationValue::Null,
            },
            DsonOperation::FieldDelete {
                path: "field".to_string(),
            },
        ];

        let optimizer = OperationOptimizer::new(operations);
        let ops = optimizer.optimize();
        // Add followed by delete - add is redundant
        assert_eq!(ops.len(), 1);
    }

    #[test]
    fn test_operation_optimizer_delete_then_add() {
        let operations = vec![
            DsonOperation::FieldDelete {
                path: "field".to_string(),
            },
            DsonOperation::FieldAdd {
                path: "field".to_string(),
                value: OperationValue::Null,
            },
        ];

        let optimizer = OperationOptimizer::new(operations);
        let ops = optimizer.optimize();
        // Delete followed by add - delete is redundant
        assert!(ops.len() <= 2);
    }

    #[test]
    fn test_operation_value_clone() {
        let val = OperationValue::StringRef("test".to_string());
        let cloned = val.clone();
        assert_eq!(val, cloned);
    }

    #[test]
    fn test_merge_strategy_clone() {
        let strategy = MergeStrategy::LastWriteWins;
        let cloned = strategy.clone();
        assert_eq!(strategy, cloned);
    }

    #[test]
    fn test_filter_predicate_clone() {
        let pred = FilterPredicate::Even;
        let cloned = pred.clone();
        assert_eq!(pred, cloned);
    }

    #[test]
    fn test_transform_function_clone() {
        let transform = TransformFunction::Add(5);
        let cloned = transform.clone();
        assert_eq!(transform, cloned);
    }

    #[test]
    fn test_reduce_function_clone() {
        let reduce = ReduceFunction::Sum;
        let cloned = reduce.clone();
        assert_eq!(reduce, cloned);
    }

    #[test]
    fn test_stream_generator_clone() {
        let generator = StreamGenerator::Fibonacci(10);
        let cloned = generator.clone();
        assert_eq!(generator, cloned);
    }

    #[test]
    fn test_field_state_clone() {
        let state = FieldState::Present;
        let cloned = state;
        assert!(matches!(cloned, FieldState::Present));
    }

    #[test]
    fn test_transform_function_debug() {
        let transform = TransformFunction::Add(5);
        let debug = format!("{transform:?}");
        assert!(debug.contains("Add"));
    }

    #[test]
    fn test_reduce_function_debug() {
        let reduce = ReduceFunction::Sum;
        let debug = format!("{reduce:?}");
        assert!(debug.contains("Sum"));
    }

    #[test]
    fn test_stream_generator_debug() {
        let generator = StreamGenerator::Fibonacci(10);
        let debug = format!("{generator:?}");
        assert!(debug.contains("Fibonacci"));
    }

    #[test]
    fn test_canonical_processor_modify_nonexistent_field() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["field".to_string()]),
            HashSet::from(["field".to_string()]),
        );

        processor.add_operation(DsonOperation::FieldModify {
            path: "field".to_string(),
            value: OperationValue::StringRef("value".to_string()),
        });

        let result = processor.compute_canonical();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(format!("{err:?}").contains("non-existent"));
    }

    #[test]
    fn test_canonical_processor_modify_deleted_field() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["field".to_string()]),
            HashSet::from(["field".to_string()]),
        );

        // First add then delete the field
        processor.add_operation(DsonOperation::FieldAdd {
            path: "field".to_string(),
            value: OperationValue::StringRef("initial".to_string()),
        });
        processor.add_operation(DsonOperation::FieldDelete {
            path: "field".to_string(),
        });
        processor.add_operation(DsonOperation::FieldModify {
            path: "field".to_string(),
            value: OperationValue::StringRef("modified".to_string()),
        });

        let result = processor.compute_canonical();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(format!("{err:?}").contains("deleted"));
    }

    #[test]
    fn test_canonical_processor_delete_already_deleted() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["field".to_string()]),
            HashSet::from(["field".to_string()]),
        );

        // Add, delete, then delete again
        processor.add_operation(DsonOperation::FieldAdd {
            path: "field".to_string(),
            value: OperationValue::StringRef("value".to_string()),
        });
        processor.add_operation(DsonOperation::FieldDelete {
            path: "field".to_string(),
        });
        processor.add_operation(DsonOperation::FieldDelete {
            path: "field".to_string(),
        });

        let canonical = processor.compute_canonical().unwrap();
        // Should have add and one delete only (second delete is no-op)
        assert_eq!(canonical.len(), 2);
    }

    #[test]
    fn test_canonical_processor_output_schema_filtering() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["input".to_string(), "output".to_string()]),
            HashSet::from(["output".to_string()]), // Only "output" in output schema
        );

        processor.add_operation(DsonOperation::FieldAdd {
            path: "input".to_string(),
            value: OperationValue::StringRef("input_val".to_string()),
        });
        processor.add_operation(DsonOperation::FieldAdd {
            path: "output".to_string(),
            value: OperationValue::StringRef("output_val".to_string()),
        });

        let canonical = processor.compute_canonical().unwrap();
        // Only output field should remain
        assert_eq!(canonical.len(), 1);
        match &canonical[0] {
            DsonOperation::FieldAdd { path, .. } => assert_eq!(path, "output"),
            _ => panic!("Expected FieldAdd"),
        }
    }

    #[test]
    fn test_canonical_processor_output_wildcard_filtering() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["user.*".to_string()]),
            HashSet::from(["user.name".to_string()]), // Only user.name in output
        );

        processor.add_operation(DsonOperation::FieldAdd {
            path: "user.name".to_string(),
            value: OperationValue::StringRef("Alice".to_string()),
        });
        processor.add_operation(DsonOperation::FieldAdd {
            path: "user.email".to_string(),
            value: OperationValue::StringRef("alice@example.com".to_string()),
        });

        let canonical = processor.compute_canonical().unwrap();
        // Only user.name should remain
        assert_eq!(canonical.len(), 1);
    }

    #[test]
    fn test_canonical_processor_merge_field_filtered() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["field".to_string()]),
            HashSet::from([]), // Empty output schema
        );

        processor.add_operation(DsonOperation::MergeField {
            path: "field".to_string(),
            value: OperationValue::Null,
            timestamp: 1,
        });

        let canonical = processor.compute_canonical().unwrap();
        assert_eq!(canonical.len(), 0); // Filtered out by output schema
    }

    #[test]
    fn test_canonical_processor_conflict_resolve_filtered() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["field".to_string()]),
            HashSet::from([]), // Empty output schema
        );

        processor.add_operation(DsonOperation::ConflictResolve {
            path: "field".to_string(),
            strategy: MergeStrategy::Max,
        });

        let canonical = processor.compute_canonical().unwrap();
        assert_eq!(canonical.len(), 0); // Filtered out by output schema
    }

    #[test]
    fn test_canonical_processor_array_insert_filtered() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["arr".to_string()]),
            HashSet::from([]), // Empty output schema
        );

        processor.add_operation(DsonOperation::ArrayInsert {
            path: "arr".to_string(),
            index: 0,
            value: OperationValue::Null,
        });

        let canonical = processor.compute_canonical().unwrap();
        assert_eq!(canonical.len(), 0); // Filtered out by output schema
    }

    #[test]
    fn test_canonical_processor_array_remove_filtered() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["arr".to_string()]),
            HashSet::from([]), // Empty output schema
        );

        processor.add_operation(DsonOperation::ArrayRemove {
            path: "arr".to_string(),
            index: 0,
        });

        let canonical = processor.compute_canonical().unwrap();
        assert_eq!(canonical.len(), 0); // Filtered out by output schema
    }

    #[test]
    fn test_canonical_processor_array_replace_filtered() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["arr".to_string()]),
            HashSet::from([]), // Empty output schema
        );

        processor.add_operation(DsonOperation::ArrayReplace {
            path: "arr".to_string(),
            index: 0,
            value: OperationValue::Null,
        });

        let canonical = processor.compute_canonical().unwrap();
        assert_eq!(canonical.len(), 0); // Filtered out by output schema
    }

    #[test]
    fn test_field_state_not_present_clone() {
        let state = FieldState::NotPresent;
        let cloned = state;
        assert!(matches!(cloned, FieldState::NotPresent));
    }

    #[test]
    fn test_field_state_deleted_clone() {
        let state = FieldState::Deleted;
        let cloned = state;
        assert!(matches!(cloned, FieldState::Deleted));
    }

    #[test]
    fn test_field_state_modified_clone() {
        let state = FieldState::Modified;
        let cloned = state;
        assert!(matches!(cloned, FieldState::Modified));
    }

    #[test]
    fn test_field_state_debug() {
        assert!(format!("{:?}", FieldState::NotPresent).contains("NotPresent"));
        assert!(format!("{:?}", FieldState::Present).contains("Present"));
        assert!(format!("{:?}", FieldState::Deleted).contains("Deleted"));
        assert!(format!("{:?}", FieldState::Modified).contains("Modified"));
    }

    #[test]
    fn test_operation_value_debug() {
        assert!(format!("{:?}", OperationValue::Null).contains("Null"));
        assert!(format!("{:?}", OperationValue::BoolRef(true)).contains("BoolRef"));
        assert!(format!("{:?}", OperationValue::StringRef("s".to_string())).contains("StringRef"));
        assert!(format!("{:?}", OperationValue::NumberRef("1".to_string())).contains("NumberRef"));
        assert!(
            format!("{:?}", OperationValue::ObjectRef { start: 0, end: 1 }).contains("ObjectRef")
        );
        assert!(
            format!("{:?}", OperationValue::ArrayRef { start: 0, end: 1 }).contains("ArrayRef")
        );
    }

    #[test]
    fn test_merge_strategy_debug() {
        assert!(format!("{:?}", MergeStrategy::LastWriteWins).contains("LastWriteWins"));
        assert!(format!("{:?}", MergeStrategy::Additive).contains("Additive"));
        assert!(format!("{:?}", MergeStrategy::Max).contains("Max"));
        assert!(format!("{:?}", MergeStrategy::Min).contains("Min"));
        assert!(format!("{:?}", MergeStrategy::Union).contains("Union"));
        assert!(format!("{:?}", MergeStrategy::Custom("c".to_string())).contains("Custom"));
    }

    #[test]
    fn test_filter_predicate_debug() {
        assert!(format!("{:?}", FilterPredicate::EveryNth(2)).contains("EveryNth"));
        assert!(format!("{:?}", FilterPredicate::Alternate).contains("Alternate"));
        assert!(format!("{:?}", FilterPredicate::Even).contains("Even"));
        assert!(format!("{:?}", FilterPredicate::Odd).contains("Odd"));
        assert!(format!("{:?}", FilterPredicate::GreaterThan(5)).contains("GreaterThan"));
        assert!(format!("{:?}", FilterPredicate::LessThan(5)).contains("LessThan"));
        assert!(format!("{:?}", FilterPredicate::Equals(OperationValue::Null)).contains("Equals"));
        assert!(format!("{:?}", FilterPredicate::Custom("c".to_string())).contains("Custom"));
    }

    #[test]
    fn test_transform_function_inequality() {
        // Test cross-variant inequality
        assert_ne!(
            TransformFunction::ToUppercase,
            TransformFunction::ToLowercase
        );
        assert_ne!(TransformFunction::Add(1), TransformFunction::ToUppercase);
        assert_ne!(
            TransformFunction::Multiply(1),
            TransformFunction::ToLowercase
        );
        assert_ne!(
            TransformFunction::Append("a".to_string()),
            TransformFunction::Prepend("a".to_string())
        );
        assert_ne!(
            TransformFunction::Custom("a".to_string()),
            TransformFunction::Add(1)
        );
    }

    #[test]
    fn test_reduce_function_inequality() {
        // Test cross-variant inequality
        assert_ne!(ReduceFunction::Sum, ReduceFunction::Min);
        assert_ne!(ReduceFunction::Product, ReduceFunction::Max);
        assert_ne!(ReduceFunction::Count, ReduceFunction::Concat);
        assert_ne!(ReduceFunction::Custom("a".to_string()), ReduceFunction::Sum);
    }

    #[test]
    fn test_stream_generator_inequality() {
        // Test cross-variant inequality
        assert_ne!(
            StreamGenerator::Fibonacci(10),
            StreamGenerator::Custom("c".to_string())
        );
        assert_ne!(
            StreamGenerator::Range {
                start: 0,
                end: 10,
                step: 1
            },
            StreamGenerator::Repeat(OperationValue::Null, 5)
        );
        assert_ne!(
            StreamGenerator::Repeat(OperationValue::Null, 5),
            StreamGenerator::Fibonacci(5)
        );
    }

    #[test]
    fn test_operation_optimizer_add_delete_readd() {
        let operations = vec![
            DsonOperation::FieldAdd {
                path: "field".to_string(),
                value: OperationValue::StringRef("first".to_string()),
            },
            DsonOperation::FieldDelete {
                path: "field".to_string(),
            },
            DsonOperation::FieldAdd {
                path: "field".to_string(),
                value: OperationValue::StringRef("second".to_string()),
            },
        ];

        let optimizer = OperationOptimizer::new(operations);
        let ops = optimizer.optimize();
        // Add, delete, add - add is not redundant because there's a readd after delete
        assert!(ops.len() >= 2);
    }

    #[test]
    fn test_operation_optimizer_delete_then_modify() {
        let operations = vec![
            DsonOperation::FieldDelete {
                path: "field".to_string(),
            },
            DsonOperation::FieldModify {
                path: "field".to_string(),
                value: OperationValue::Null,
            },
        ];

        let optimizer = OperationOptimizer::new(operations);
        let ops = optimizer.optimize();
        // Delete followed by modify - delete is redundant
        assert_eq!(ops.len(), 1);
    }

    #[test]
    fn test_operation_optimizer_reorder_paths() {
        let operations = vec![
            DsonOperation::FieldAdd {
                path: "z.field".to_string(),
                value: OperationValue::Null,
            },
            DsonOperation::FieldAdd {
                path: "a.field".to_string(),
                value: OperationValue::Null,
            },
        ];

        let optimizer = OperationOptimizer::new(operations);
        let ops = optimizer.optimize();
        // Should be reordered by path
        assert_eq!(ops.len(), 2);
        match &ops[0] {
            DsonOperation::FieldAdd { path, .. } => assert_eq!(path, "a.field"),
            _ => panic!("Expected FieldAdd"),
        }
    }

    #[test]
    fn test_operation_optimizer_batch_execute_path() {
        let operations = vec![DsonOperation::BatchExecute { operations: vec![] }];

        let optimizer = OperationOptimizer::new(operations);
        let ops = optimizer.optimize();
        assert_eq!(ops.len(), 1);
    }

    #[test]
    fn test_operation_optimizer_object_end_ordering() {
        let operations = vec![
            DsonOperation::ObjectEnd {
                path: "obj".to_string(),
            },
            DsonOperation::FieldAdd {
                path: "obj.child".to_string(),
                value: OperationValue::Null,
            },
        ];

        let optimizer = OperationOptimizer::new(operations);
        let ops = optimizer.optimize();
        // Object end should come after its children
        assert_eq!(ops.len(), 2);
    }

    #[test]
    fn test_operation_optimizer_array_end_ordering() {
        let operations = vec![
            DsonOperation::ArrayEnd {
                path: "arr".to_string(),
            },
            DsonOperation::ArrayInsert {
                path: "arr[0]".to_string(),
                index: 0,
                value: OperationValue::Null,
            },
        ];

        let optimizer = OperationOptimizer::new(operations);
        let ops = optimizer.optimize();
        assert_eq!(ops.len(), 2);
    }

    #[test]
    fn test_canonical_processor_not_in_input_schema() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["allowed".to_string()]), // Only "allowed" in input schema
            HashSet::from(["allowed".to_string(), "not_allowed".to_string()]),
        );

        // Try structural operations on a path not in input schema
        processor.add_operation(DsonOperation::ObjectStart {
            path: "not_allowed".to_string(),
        });
        processor.add_operation(DsonOperation::ObjectEnd {
            path: "not_allowed".to_string(),
        });
        processor.add_operation(DsonOperation::ArrayStart {
            path: "not_allowed".to_string(),
        });
        processor.add_operation(DsonOperation::ArrayEnd {
            path: "not_allowed".to_string(),
        });

        let canonical = processor.compute_canonical().unwrap();
        // None should be added because they're not in input schema
        assert_eq!(canonical.len(), 0);
    }

    #[test]
    fn test_canonical_processor_array_ops_not_in_input() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["allowed".to_string()]),
            HashSet::from(["allowed".to_string(), "arr".to_string()]),
        );

        processor.add_operation(DsonOperation::ArrayInsert {
            path: "arr".to_string(),
            index: 0,
            value: OperationValue::Null,
        });
        processor.add_operation(DsonOperation::ArrayRemove {
            path: "arr".to_string(),
            index: 0,
        });
        processor.add_operation(DsonOperation::ArrayReplace {
            path: "arr".to_string(),
            index: 0,
            value: OperationValue::Null,
        });

        let canonical = processor.compute_canonical().unwrap();
        // None should be added because arr is not in input schema
        assert_eq!(canonical.len(), 0);
    }

    #[test]
    fn test_canonical_processor_conflict_resolve_not_in_input() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from([]), // Empty input schema
            HashSet::from(["field".to_string()]),
        );

        processor.add_operation(DsonOperation::ConflictResolve {
            path: "field".to_string(),
            strategy: MergeStrategy::Max,
        });

        let canonical = processor.compute_canonical().unwrap();
        assert_eq!(canonical.len(), 0);
    }

    #[test]
    fn test_canonical_processor_advanced_array_not_in_input() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from([]), // Empty input schema
            HashSet::from(["arr".to_string()]),
        );

        processor.add_operation(DsonOperation::ArrayBuild {
            path: "arr".to_string(),
            elements: vec![],
        });
        processor.add_operation(DsonOperation::ArrayFilter {
            path: "arr".to_string(),
            predicate: FilterPredicate::Even,
        });
        processor.add_operation(DsonOperation::ArrayMap {
            path: "arr".to_string(),
            transform: TransformFunction::Add(1),
        });
        processor.add_operation(DsonOperation::ArrayReduce {
            path: "arr".to_string(),
            initial: OperationValue::Null,
            reducer: ReduceFunction::Sum,
        });

        let canonical = processor.compute_canonical().unwrap();
        assert_eq!(canonical.len(), 0);
    }

    #[test]
    fn test_canonical_processor_streaming_not_in_input() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from([]), // Empty input schema
            HashSet::from(["stream".to_string()]),
        );

        processor.add_operation(DsonOperation::StreamBuild {
            path: "stream".to_string(),
            generator: StreamGenerator::Fibonacci(10),
        });
        processor.add_operation(DsonOperation::StreamFilter {
            path: "stream".to_string(),
            predicate: FilterPredicate::Even,
        });
        processor.add_operation(DsonOperation::StreamMap {
            path: "stream".to_string(),
            transform: TransformFunction::Add(1),
        });
        processor.add_operation(DsonOperation::StreamEmit {
            path: "stream".to_string(),
            batch_size: 10,
        });

        let canonical = processor.compute_canonical().unwrap();
        assert_eq!(canonical.len(), 0);
    }

    #[test]
    #[allow(clippy::too_many_lines)] // Comprehensive variant coverage test
    fn test_dson_operation_all_variants_equality() {
        // Test equality for all DsonOperation variants
        assert_eq!(
            DsonOperation::ObjectStart {
                path: "a".to_string()
            },
            DsonOperation::ObjectStart {
                path: "a".to_string()
            }
        );
        assert_eq!(
            DsonOperation::ObjectEnd {
                path: "a".to_string()
            },
            DsonOperation::ObjectEnd {
                path: "a".to_string()
            }
        );
        assert_eq!(
            DsonOperation::ArrayStart {
                path: "a".to_string()
            },
            DsonOperation::ArrayStart {
                path: "a".to_string()
            }
        );
        assert_eq!(
            DsonOperation::ArrayEnd {
                path: "a".to_string()
            },
            DsonOperation::ArrayEnd {
                path: "a".to_string()
            }
        );
        assert_eq!(
            DsonOperation::ArrayInsert {
                path: "a".to_string(),
                index: 0,
                value: OperationValue::Null
            },
            DsonOperation::ArrayInsert {
                path: "a".to_string(),
                index: 0,
                value: OperationValue::Null
            }
        );
        assert_eq!(
            DsonOperation::ArrayRemove {
                path: "a".to_string(),
                index: 0
            },
            DsonOperation::ArrayRemove {
                path: "a".to_string(),
                index: 0
            }
        );
        assert_eq!(
            DsonOperation::ArrayReplace {
                path: "a".to_string(),
                index: 0,
                value: OperationValue::Null
            },
            DsonOperation::ArrayReplace {
                path: "a".to_string(),
                index: 0,
                value: OperationValue::Null
            }
        );
        assert_eq!(
            DsonOperation::CheckPresence {
                path: "a".to_string()
            },
            DsonOperation::CheckPresence {
                path: "a".to_string()
            }
        );
        assert_eq!(
            DsonOperation::CheckAbsence {
                path: "a".to_string()
            },
            DsonOperation::CheckAbsence {
                path: "a".to_string()
            }
        );
        assert_eq!(
            DsonOperation::CheckNull {
                path: "a".to_string()
            },
            DsonOperation::CheckNull {
                path: "a".to_string()
            }
        );
        assert_eq!(
            DsonOperation::CheckNotNull {
                path: "a".to_string()
            },
            DsonOperation::CheckNotNull {
                path: "a".to_string()
            }
        );
        assert_eq!(
            DsonOperation::MergeField {
                path: "a".to_string(),
                value: OperationValue::Null,
                timestamp: 1
            },
            DsonOperation::MergeField {
                path: "a".to_string(),
                value: OperationValue::Null,
                timestamp: 1
            }
        );
        assert_eq!(
            DsonOperation::ConflictResolve {
                path: "a".to_string(),
                strategy: MergeStrategy::Max
            },
            DsonOperation::ConflictResolve {
                path: "a".to_string(),
                strategy: MergeStrategy::Max
            }
        );
        assert_eq!(
            DsonOperation::ArrayBuild {
                path: "a".to_string(),
                elements: vec![]
            },
            DsonOperation::ArrayBuild {
                path: "a".to_string(),
                elements: vec![]
            }
        );
        assert_eq!(
            DsonOperation::ArrayFilter {
                path: "a".to_string(),
                predicate: FilterPredicate::Even
            },
            DsonOperation::ArrayFilter {
                path: "a".to_string(),
                predicate: FilterPredicate::Even
            }
        );
        assert_eq!(
            DsonOperation::ArrayMap {
                path: "a".to_string(),
                transform: TransformFunction::Add(1)
            },
            DsonOperation::ArrayMap {
                path: "a".to_string(),
                transform: TransformFunction::Add(1)
            }
        );
        assert_eq!(
            DsonOperation::ArrayReduce {
                path: "a".to_string(),
                initial: OperationValue::Null,
                reducer: ReduceFunction::Sum
            },
            DsonOperation::ArrayReduce {
                path: "a".to_string(),
                initial: OperationValue::Null,
                reducer: ReduceFunction::Sum
            }
        );
        assert_eq!(
            DsonOperation::StreamBuild {
                path: "a".to_string(),
                generator: StreamGenerator::Fibonacci(5)
            },
            DsonOperation::StreamBuild {
                path: "a".to_string(),
                generator: StreamGenerator::Fibonacci(5)
            }
        );
        assert_eq!(
            DsonOperation::StreamFilter {
                path: "a".to_string(),
                predicate: FilterPredicate::Odd
            },
            DsonOperation::StreamFilter {
                path: "a".to_string(),
                predicate: FilterPredicate::Odd
            }
        );
        assert_eq!(
            DsonOperation::StreamMap {
                path: "a".to_string(),
                transform: TransformFunction::ToUppercase
            },
            DsonOperation::StreamMap {
                path: "a".to_string(),
                transform: TransformFunction::ToUppercase
            }
        );
        assert_eq!(
            DsonOperation::StreamEmit {
                path: "a".to_string(),
                batch_size: 10
            },
            DsonOperation::StreamEmit {
                path: "a".to_string(),
                batch_size: 10
            }
        );
        assert_eq!(
            DsonOperation::BatchExecute { operations: vec![] },
            DsonOperation::BatchExecute { operations: vec![] }
        );
    }

    #[test]
    fn test_operation_value_inequality() {
        assert_ne!(OperationValue::Null, OperationValue::BoolRef(true));
        assert_ne!(
            OperationValue::BoolRef(true),
            OperationValue::StringRef("s".to_string())
        );
        assert_ne!(
            OperationValue::StringRef("a".to_string()),
            OperationValue::NumberRef("1".to_string())
        );
        assert_ne!(
            OperationValue::ObjectRef { start: 0, end: 1 },
            OperationValue::ArrayRef { start: 0, end: 1 }
        );
    }

    #[test]
    fn test_merge_strategy_inequality() {
        assert_ne!(MergeStrategy::LastWriteWins, MergeStrategy::Additive);
        assert_ne!(MergeStrategy::Max, MergeStrategy::Min);
        assert_ne!(MergeStrategy::Union, MergeStrategy::Custom("c".to_string()));
    }

    #[test]
    fn test_filter_predicate_inequality() {
        assert_ne!(FilterPredicate::Even, FilterPredicate::Odd);
        assert_ne!(FilterPredicate::EveryNth(2), FilterPredicate::EveryNth(3));
        assert_ne!(
            FilterPredicate::GreaterThan(5),
            FilterPredicate::LessThan(5)
        );
        assert_ne!(FilterPredicate::Alternate, FilterPredicate::Even);
        assert_ne!(
            FilterPredicate::Custom("a".to_string()),
            FilterPredicate::Custom("b".to_string())
        );
    }
}
