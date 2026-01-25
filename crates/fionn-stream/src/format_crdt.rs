// SPDX-License-Identifier: MIT OR Apache-2.0
//! Format-Aware CRDT Processor
//!
//! This module provides a CRDT-enabled processor that wraps `FormatDsonProcessor`,
//! enabling conflict-free replicated document processing across distributed systems.

use crate::format_dson::{FormatBatchProcessor, FormatBatchResult, FormatDsonProcessor};
use crate::skiptape::CompiledSchema;
use fionn_core::Result;
use fionn_core::format::FormatKind;
use fionn_ops::dson_traits::{
    CrdtMerge, CrdtOperation, DeltaCrdt, MergeConflict, OpBasedCrdt, VectorClock,
};
use fionn_ops::{DsonOperation, MergeStrategy, OperationValue};
use smallvec::SmallVec;
use std::collections::HashMap;

// =============================================================================
// CRDT Delta Types
// =============================================================================

/// Delta state for format-aware CRDT synchronization
#[derive(Debug, Clone)]
pub struct FormatDelta {
    /// Operations included in this delta
    pub operations: Vec<CrdtOperation>,
    /// Vector clock at delta generation
    pub clock: VectorClock,
    /// Format kind this delta applies to
    pub format_kind: FormatKind,
}

impl FormatDelta {
    /// Create a new empty delta
    #[must_use]
    pub fn new(format_kind: FormatKind) -> Self {
        Self {
            operations: Vec::new(),
            clock: VectorClock::new(),
            format_kind,
        }
    }

    /// Create a delta with operations
    #[must_use]
    pub const fn with_operations(
        operations: Vec<CrdtOperation>,
        clock: VectorClock,
        format_kind: FormatKind,
    ) -> Self {
        Self {
            operations,
            clock,
            format_kind,
        }
    }

    /// Check if delta is empty
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    /// Get number of operations
    #[must_use]
    pub const fn len(&self) -> usize {
        self.operations.len()
    }
}

// =============================================================================
// Document State
// =============================================================================

/// State for a single document in the CRDT
#[derive(Debug, Clone)]
struct DocumentState {
    /// Current document content (JSON)
    content: String,
    /// Last write timestamp for each field path
    field_timestamps: HashMap<String, u64>,
}

impl DocumentState {
    fn new(content: String) -> Self {
        Self {
            content,
            field_timestamps: HashMap::new(),
        }
    }
}

// =============================================================================
// FormatCrdtProcessor
// =============================================================================

/// CRDT-enabled processor for any format implementing `FormatBatchProcessor`
///
/// This wraps a `FormatDsonProcessor` and adds CRDT semantics:
/// - Vector clock tracking for causality
/// - Last-Writer-Wins (LWW) register semantics per field
/// - Delta-state synchronization
/// - Operation buffering for causal ordering
pub struct FormatCrdtProcessor<P: FormatBatchProcessor> {
    /// The underlying DSON processor
    dson_processor: FormatDsonProcessor<P>,
    /// Replica identifier
    replica_id: String,
    /// Vector clock for causality tracking
    vector_clock: VectorClock,
    /// Lamport timestamp counter
    lamport_timestamp: u64,
    /// Default merge strategy
    default_strategy: MergeStrategy,
    /// Document states indexed by document ID or index
    document_states: HashMap<String, DocumentState>,
    /// Operation history for delta generation
    operation_history: Vec<CrdtOperation>,
    /// Buffered operations waiting for causal delivery
    operation_buffer: SmallVec<[CrdtOperation; 16]>,
}

impl<P: FormatBatchProcessor> FormatCrdtProcessor<P> {
    /// Create a new CRDT processor with a replica ID
    #[must_use]
    pub fn new(batch_processor: P, replica_id: impl Into<String>) -> Self {
        Self {
            dson_processor: FormatDsonProcessor::new(batch_processor),
            replica_id: replica_id.into(),
            vector_clock: VectorClock::new(),
            lamport_timestamp: 0,
            default_strategy: MergeStrategy::LastWriteWins,
            document_states: HashMap::new(),
            operation_history: Vec::new(),
            operation_buffer: SmallVec::new(),
        }
    }

    /// Create with a specific merge strategy
    #[must_use]
    pub fn with_strategy(mut self, strategy: MergeStrategy) -> Self {
        self.default_strategy = strategy;
        self
    }

    /// Get the format kind
    #[must_use]
    pub fn format_kind(&self) -> FormatKind {
        self.dson_processor.format_kind()
    }

    /// Get the replica ID
    #[must_use]
    pub fn replica_id(&self) -> &str {
        &self.replica_id
    }

    /// Get the current vector clock
    #[must_use]
    pub const fn vector_clock(&self) -> &VectorClock {
        &self.vector_clock
    }

    /// Get the current Lamport timestamp
    #[must_use]
    pub const fn lamport_timestamp(&self) -> u64 {
        self.lamport_timestamp
    }

    /// Process data with schema filtering and track as CRDT state
    ///
    /// # Errors
    /// Returns an error if processing fails
    pub fn process(&mut self, data: &[u8], schema: &CompiledSchema) -> Result<FormatBatchResult> {
        let result = self
            .dson_processor
            .process_with_operations(data, schema, &[])?;

        // Track documents in CRDT state
        for (idx, doc) in result.documents.iter().enumerate() {
            let doc_id = format!("doc_{idx}");
            self.document_states
                .insert(doc_id, DocumentState::new(doc.clone()));
        }

        // Increment local clock
        self.vector_clock.increment(&self.replica_id);
        self.lamport_timestamp += 1;

        Ok(result)
    }

    /// Process with DSON operations and CRDT tracking
    ///
    /// # Errors
    /// Returns an error if processing fails
    pub fn process_with_operations(
        &mut self,
        data: &[u8],
        schema: &CompiledSchema,
        operations: &[DsonOperation],
    ) -> Result<FormatBatchResult> {
        let result = self
            .dson_processor
            .process_with_operations(data, schema, operations)?;

        // Track operations for delta generation
        for op in operations {
            let crdt_op = self.prepare_operation(op);
            self.operation_history.push(crdt_op);
        }

        // Track documents
        for (idx, doc) in result.documents.iter().enumerate() {
            let doc_id = format!("doc_{idx}");
            self.document_states
                .insert(doc_id, DocumentState::new(doc.clone()));
        }

        self.vector_clock.increment(&self.replica_id);
        self.lamport_timestamp += 1;

        Ok(result)
    }

    /// Process unfiltered data with CRDT tracking
    ///
    /// # Errors
    /// Returns an error if processing fails
    pub fn process_unfiltered(&mut self, data: &[u8]) -> Result<FormatBatchResult> {
        let result = self
            .dson_processor
            .process_unfiltered_with_operations(data, &[])?;

        for (idx, doc) in result.documents.iter().enumerate() {
            let doc_id = format!("doc_{idx}");
            self.document_states
                .insert(doc_id, DocumentState::new(doc.clone()));
        }

        self.vector_clock.increment(&self.replica_id);
        self.lamport_timestamp += 1;

        Ok(result)
    }

    /// Apply a local operation and prepare it for replication
    fn prepare_operation(&mut self, op: &DsonOperation) -> CrdtOperation {
        self.lamport_timestamp += 1;
        self.vector_clock.increment(&self.replica_id);

        CrdtOperation {
            operation: op.clone(),
            timestamp: self.lamport_timestamp,
            replica_id: self.replica_id.clone(),
            vector_clock: self.vector_clock.clone(),
        }
    }

    /// Get a document by ID
    #[must_use]
    pub fn get_document(&self, doc_id: &str) -> Option<&str> {
        self.document_states.get(doc_id).map(|s| s.content.as_str())
    }

    /// Get all document IDs
    #[must_use]
    pub fn document_ids(&self) -> Vec<&str> {
        self.document_states.keys().map(String::as_str).collect()
    }

    /// Get number of tracked documents
    #[must_use]
    pub fn document_count(&self) -> usize {
        self.document_states.len()
    }

    /// Apply a field value to a document with LWW semantics
    fn apply_field_value(
        &mut self,
        doc_id: &str,
        path: &str,
        value: &OperationValue,
        timestamp: u64,
    ) -> Option<MergeConflict> {
        let state = self.document_states.get_mut(doc_id)?;

        let current_ts = state.field_timestamps.get(path).copied().unwrap_or(0);

        match timestamp.cmp(&current_ts) {
            std::cmp::Ordering::Greater => {
                // Remote wins - update the field
                state.field_timestamps.insert(path.to_string(), timestamp);

                // Apply the update to the document content
                if let Ok(mut json_value) =
                    serde_json::from_str::<serde_json::Value>(&state.content)
                {
                    Self::set_json_path(&mut json_value, path, value);
                    if let Ok(new_content) = serde_json::to_string(&json_value) {
                        state.content = new_content;
                    }
                }

                None
            }
            std::cmp::Ordering::Equal => {
                // Same timestamp - use replica_id as tiebreaker
                // For now, just report conflict
                Some(MergeConflict {
                    path: path.to_string(),
                    local_value: OperationValue::StringRef("current".to_string()),
                    remote_value: value.clone(),
                    local_timestamp: current_ts,
                    remote_timestamp: timestamp,
                    resolved_value: None,
                })
            }
            std::cmp::Ordering::Less => {
                // Local wins - no change
                None
            }
        }
    }

    /// Set a value at a JSON path
    fn set_json_path(json: &mut serde_json::Value, path: &str, value: &OperationValue) {
        let parts: Vec<&str> = path.split('.').collect();
        let mut current = json;

        for (i, part) in parts.iter().enumerate() {
            if i == parts.len() - 1 {
                // Final part - set the value
                if let serde_json::Value::Object(obj) = current {
                    obj.insert((*part).to_string(), Self::operation_value_to_json(value));
                }
            } else {
                // Navigate down
                if let serde_json::Value::Object(obj) = current {
                    current = obj
                        .entry((*part).to_string())
                        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
                }
            }
        }
    }

    /// Convert `OperationValue` to JSON
    ///
    /// Note: `ArrayRef` and `ObjectRef` are tape position ranges in the actual implementation,
    /// so we represent them as placeholder values. In practice, these should be resolved
    /// from the tape before calling this function.
    fn operation_value_to_json(value: &OperationValue) -> serde_json::Value {
        match value {
            OperationValue::Null => serde_json::Value::Null,
            OperationValue::BoolRef(b) => serde_json::Value::Bool(*b),
            OperationValue::NumberRef(n) => n
                .parse::<i64>()
                .map(|i| serde_json::Value::Number(i.into()))
                .or_else(|_| {
                    n.parse::<f64>().map(|f| {
                        serde_json::Value::Number(
                            serde_json::Number::from_f64(f).unwrap_or_else(|| 0.into()),
                        )
                    })
                })
                .unwrap_or_else(|_| serde_json::Value::String(n.clone())),
            OperationValue::StringRef(s) => serde_json::Value::String(s.clone()),
            // ArrayRef and ObjectRef are tape position ranges - represent as empty
            // In practice, CRDT operations work on scalar values
            OperationValue::ArrayRef { .. } => serde_json::Value::Array(Vec::new()),
            OperationValue::ObjectRef { .. } => serde_json::Value::Object(serde_json::Map::new()),
        }
    }

    /// Reset the processor
    pub fn reset(&mut self) {
        self.dson_processor.reset();
        self.document_states.clear();
        self.operation_history.clear();
        self.operation_buffer.clear();
    }

    /// Get reference to underlying DSON processor
    #[must_use]
    pub const fn dson_processor(&self) -> &FormatDsonProcessor<P> {
        &self.dson_processor
    }

    /// Get mutable reference to underlying DSON processor
    #[allow(clippy::missing_const_for_fn)] // Cannot be const: returns &mut
    pub fn dson_processor_mut(&mut self) -> &mut FormatDsonProcessor<P> {
        &mut self.dson_processor
    }
}

// =============================================================================
// CrdtMerge Implementation
// =============================================================================

impl<P: FormatBatchProcessor> CrdtMerge for FormatCrdtProcessor<P> {
    fn merge_operation(&mut self, op: CrdtOperation) -> Result<Option<MergeConflict>> {
        // Update Lamport timestamp
        self.lamport_timestamp = self.lamport_timestamp.max(op.timestamp) + 1;

        // Merge vector clocks
        self.vector_clock.merge(&op.vector_clock);
        self.vector_clock.increment(&self.replica_id);

        // Apply the operation based on type
        match &op.operation {
            DsonOperation::FieldAdd { path, value }
            | DsonOperation::FieldModify { path, value } => {
                // Apply to all documents (or specific document if path indicates)
                let mut conflict = None;
                let doc_ids: Vec<_> = self.document_states.keys().cloned().collect();
                for doc_id in doc_ids {
                    if let Some(c) = self.apply_field_value(&doc_id, path, value, op.timestamp) {
                        conflict = Some(c);
                    }
                }
                Ok(conflict)
            }
            DsonOperation::FieldDelete { path } => {
                // Remove field from all documents
                let doc_ids: Vec<_> = self.document_states.keys().cloned().collect();
                for doc_id in doc_ids {
                    if let Some(state) = self.document_states.get_mut(&doc_id)
                        && let Ok(mut json_value) =
                            serde_json::from_str::<serde_json::Value>(&state.content)
                    {
                        Self::delete_json_path(&mut json_value, path);
                        if let Ok(new_content) = serde_json::to_string(&json_value) {
                            state.content = new_content;
                        }
                    }
                }
                Ok(None)
            }
            _ => {
                // Other operations - record in history
                self.operation_history.push(op);
                Ok(None)
            }
        }
    }

    fn merge_field(
        &mut self,
        path: &str,
        value: OperationValue,
        timestamp: u64,
        strategy: &MergeStrategy,
    ) -> Result<Option<MergeConflict>> {
        let mut conflict = None;
        let doc_ids: Vec<_> = self.document_states.keys().cloned().collect();

        for doc_id in doc_ids {
            if let Some(state) = self.document_states.get(&doc_id) {
                let current_ts = state.field_timestamps.get(path).copied().unwrap_or(0);

                if current_ts == timestamp {
                    // Concurrent writes - need to resolve
                    let local_value = self
                        .get_field_value(&doc_id, path)
                        .unwrap_or(OperationValue::Null);

                    let resolved = strategy.resolve(&local_value, &value, current_ts, timestamp);

                    conflict = Some(MergeConflict {
                        path: path.to_string(),
                        local_value,
                        remote_value: value.clone(),
                        local_timestamp: current_ts,
                        remote_timestamp: timestamp,
                        resolved_value: Some(resolved.clone()),
                    });

                    // Apply resolved value
                    self.apply_field_value(&doc_id, path, &resolved, timestamp.max(current_ts) + 1);
                } else {
                    self.apply_field_value(&doc_id, path, &value, timestamp);
                }
            }
        }

        Ok(conflict)
    }

    fn vector_clock(&self) -> &VectorClock {
        &self.vector_clock
    }

    fn replica_id(&self) -> &str {
        &self.replica_id
    }

    fn resolve_conflict(
        &mut self,
        conflict: &MergeConflict,
        strategy: &MergeStrategy,
    ) -> Result<OperationValue> {
        let resolved = strategy.resolve(
            &conflict.local_value,
            &conflict.remote_value,
            conflict.local_timestamp,
            conflict.remote_timestamp,
        );

        // Apply the resolution
        let doc_ids: Vec<_> = self.document_states.keys().cloned().collect();
        for doc_id in doc_ids {
            self.apply_field_value(
                &doc_id,
                &conflict.path,
                &resolved,
                conflict.local_timestamp.max(conflict.remote_timestamp) + 1,
            );
        }

        Ok(resolved)
    }
}

impl<P: FormatBatchProcessor> FormatCrdtProcessor<P> {
    /// Delete a value at a JSON path
    fn delete_json_path(json: &mut serde_json::Value, path: &str) {
        let parts: Vec<&str> = path.split('.').collect();
        let mut current = json;

        for (i, part) in parts.iter().enumerate() {
            if i == parts.len() - 1 {
                // Final part - remove the value
                if let serde_json::Value::Object(obj) = current {
                    obj.remove(*part);
                }
            } else {
                // Navigate down
                if let serde_json::Value::Object(obj) = current {
                    match obj.get_mut(*part) {
                        Some(v) => current = v,
                        None => return,
                    }
                } else {
                    return;
                }
            }
        }
    }

    /// Get field value from a document
    fn get_field_value(&self, doc_id: &str, path: &str) -> Option<OperationValue> {
        let state = self.document_states.get(doc_id)?;
        let json_value: serde_json::Value = serde_json::from_str(&state.content).ok()?;

        let parts: Vec<&str> = path.split('.').collect();
        let mut current = &json_value;

        for part in &parts {
            match current {
                serde_json::Value::Object(obj) => {
                    current = obj.get(*part)?;
                }
                _ => return None,
            }
        }

        Some(Self::json_to_operation_value(current))
    }

    /// Convert JSON to `OperationValue`
    ///
    /// Note: Arrays and objects are represented as JSON strings in `StringRef`,
    /// since ArrayRef/ObjectRef require tape positions which we don't have here.
    fn json_to_operation_value(json: &serde_json::Value) -> OperationValue {
        match json {
            serde_json::Value::Null => OperationValue::Null,
            serde_json::Value::Bool(b) => OperationValue::BoolRef(*b),
            serde_json::Value::Number(n) => OperationValue::NumberRef(n.to_string()),
            serde_json::Value::String(s) => OperationValue::StringRef(s.clone()),
            // Arrays and objects are serialized to JSON strings
            // since ArrayRef/ObjectRef use tape positions
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                OperationValue::StringRef(json.to_string())
            }
        }
    }
}

// =============================================================================
// DeltaCrdt Implementation
// =============================================================================

impl<P: FormatBatchProcessor> DeltaCrdt for FormatCrdtProcessor<P> {
    type Delta = FormatDelta;

    fn generate_delta(&self, since: &VectorClock) -> Self::Delta {
        // Collect operations that happened after the given vector clock
        let ops: Vec<CrdtOperation> = self
            .operation_history
            .iter()
            .filter(|op| !op.vector_clock.happened_before(since))
            .cloned()
            .collect();

        FormatDelta::with_operations(ops, self.vector_clock.clone(), self.format_kind())
    }

    fn apply_delta(&mut self, delta: Self::Delta) -> Result<Vec<MergeConflict>> {
        let mut conflicts = Vec::new();

        for op in delta.operations {
            if !self.is_causally_ready(&op) {
                // Buffer for later
                self.buffer_operation(op);
            } else if let Some(conflict) = self.merge_operation(op)? {
                conflicts.push(conflict);
            }
        }

        // Process any buffered operations that are now ready
        conflicts.extend(self.process_buffered()?);

        // Merge the delta clock
        self.vector_clock.merge(&delta.clock);

        Ok(conflicts)
    }

    fn compact(&mut self) {
        // Keep only recent operations (e.g., last 1000)
        const MAX_HISTORY: usize = 1000;
        if self.operation_history.len() > MAX_HISTORY {
            let drain_count = self.operation_history.len() - MAX_HISTORY;
            self.operation_history.drain(0..drain_count);
        }
    }
}

// =============================================================================
// OpBasedCrdt Implementation
// =============================================================================

impl<P: FormatBatchProcessor> OpBasedCrdt for FormatCrdtProcessor<P> {
    fn prepare(&self, op: &DsonOperation) -> Result<CrdtOperation> {
        let mut clock = self.vector_clock.clone();
        clock.increment(&self.replica_id);

        Ok(CrdtOperation {
            operation: op.clone(),
            timestamp: self.lamport_timestamp + 1,
            replica_id: self.replica_id.clone(),
            vector_clock: clock,
        })
    }

    fn effect(&mut self, op: CrdtOperation) -> Result<Option<MergeConflict>> {
        self.merge_operation(op)
    }

    fn is_causally_ready(&self, op: &CrdtOperation) -> bool {
        // Check if all causal dependencies are satisfied
        // For each replica in op's clock, our clock should be >= op's clock - 1 for that replica
        // (we need to have seen all operations before this one)
        for (replica, &time) in &op.vector_clock.clocks() {
            if replica == &op.replica_id {
                // For the originating replica, we expect time - 1
                if time > 1 && self.vector_clock.get(replica) < time - 1 {
                    return false;
                }
            } else {
                // For other replicas, we need to have seen at least that many
                if self.vector_clock.get(replica) < time {
                    return false;
                }
            }
        }
        true
    }

    fn buffer_operation(&mut self, op: CrdtOperation) {
        self.operation_buffer.push(op);
    }

    fn process_buffered(&mut self) -> Result<Vec<MergeConflict>> {
        let mut conflicts = Vec::new();
        let mut processed = Vec::new();

        // Keep trying to process buffered operations until no progress
        loop {
            let mut made_progress = false;

            for (i, op) in self.operation_buffer.iter().enumerate() {
                if self.is_causally_ready(op) {
                    processed.push(i);
                    made_progress = true;
                }
            }

            if !made_progress {
                break;
            }

            // Process in reverse order to maintain indices
            for &idx in processed.iter().rev() {
                let op = self.operation_buffer.remove(idx);
                if let Some(conflict) = self.merge_operation(op)? {
                    conflicts.push(conflict);
                }
            }

            processed.clear();
        }

        Ok(conflicts)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format_dson::BatchStatistics;

    // Mock batch processor for testing
    struct MockBatchProcessor;

    impl FormatBatchProcessor for MockBatchProcessor {
        fn format_kind(&self) -> FormatKind {
            FormatKind::Json
        }

        fn process_batch(
            &mut self,
            _data: &[u8],
            _schema: &CompiledSchema,
        ) -> Result<FormatBatchResult> {
            Ok(FormatBatchResult {
                documents: vec![r#"{"name":"test","value":42}"#.to_string()],
                errors: vec![],
                statistics: BatchStatistics::default(),
            })
        }

        fn process_batch_unfiltered(&mut self, _data: &[u8]) -> Result<FormatBatchResult> {
            self.process_batch(&[], &CompiledSchema::compile(&[]).unwrap())
        }

        fn reset(&mut self) {}
    }

    #[test]
    fn test_crdt_processor_creation() {
        let processor = FormatCrdtProcessor::new(MockBatchProcessor, "replica_1");
        assert_eq!(processor.replica_id(), "replica_1");
        assert_eq!(processor.format_kind(), FormatKind::Json);
    }

    #[test]
    fn test_crdt_processor_with_strategy() {
        let processor =
            FormatCrdtProcessor::new(MockBatchProcessor, "r1").with_strategy(MergeStrategy::Max);
        assert_eq!(processor.replica_id(), "r1");
    }

    #[test]
    fn test_process_increments_clock() {
        let mut processor = FormatCrdtProcessor::new(MockBatchProcessor, "r1");
        let schema = CompiledSchema::compile(&[]).unwrap();

        let initial_ts = processor.lamport_timestamp();
        processor.process(b"{}", &schema).unwrap();

        assert!(processor.lamport_timestamp() > initial_ts);
        assert_eq!(processor.vector_clock().get("r1"), 1);
    }

    #[test]
    fn test_process_tracks_documents() {
        let mut processor = FormatCrdtProcessor::new(MockBatchProcessor, "r1");
        let schema = CompiledSchema::compile(&[]).unwrap();

        processor.process(b"{}", &schema).unwrap();

        assert_eq!(processor.document_count(), 1);
        assert!(processor.get_document("doc_0").is_some());
    }

    #[test]
    fn test_format_delta_creation() {
        let delta = FormatDelta::new(FormatKind::Json);
        assert!(delta.is_empty());
        assert_eq!(delta.len(), 0);
    }

    #[test]
    fn test_format_delta_with_operations() {
        let op = CrdtOperation {
            operation: DsonOperation::FieldAdd {
                path: "test".to_string(),
                value: OperationValue::StringRef("value".to_string()),
            },
            timestamp: 1,
            replica_id: "r1".to_string(),
            vector_clock: VectorClock::new(),
        };

        let delta = FormatDelta::with_operations(vec![op], VectorClock::new(), FormatKind::Json);
        assert!(!delta.is_empty());
        assert_eq!(delta.len(), 1);
    }

    #[test]
    fn test_merge_operation() {
        let mut processor = FormatCrdtProcessor::new(MockBatchProcessor, "r1");
        let schema = CompiledSchema::compile(&[]).unwrap();
        processor.process(b"{}", &schema).unwrap();

        let op = CrdtOperation {
            operation: DsonOperation::FieldAdd {
                path: "new_field".to_string(),
                value: OperationValue::StringRef("new_value".to_string()),
            },
            timestamp: 10,
            replica_id: "r2".to_string(),
            vector_clock: VectorClock::new(),
        };

        let conflict = processor.merge_operation(op).unwrap();
        // No conflict expected for new field
        assert!(conflict.is_none());
    }

    #[test]
    fn test_generate_and_apply_delta() {
        let mut processor1 = FormatCrdtProcessor::new(MockBatchProcessor, "r1");
        let schema = CompiledSchema::compile(&[]).unwrap();
        processor1.process(b"{}", &schema).unwrap();

        // Generate delta from empty clock
        let delta = processor1.generate_delta(&VectorClock::new());

        let mut processor2 = FormatCrdtProcessor::new(MockBatchProcessor, "r2");
        processor2.process(b"{}", &schema).unwrap();

        // Apply delta
        let conflicts = processor2.apply_delta(delta).unwrap();
        // Should be no conflicts for initial sync
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_prepare_operation() {
        let processor = FormatCrdtProcessor::new(MockBatchProcessor, "r1");
        let op = DsonOperation::FieldAdd {
            path: "test".to_string(),
            value: OperationValue::Null,
        };

        let crdt_op = processor.prepare(&op).unwrap();
        assert_eq!(crdt_op.replica_id, "r1");
        assert!(crdt_op.timestamp > 0);
    }

    #[test]
    fn test_is_causally_ready() {
        let processor = FormatCrdtProcessor::new(MockBatchProcessor, "r1");

        // Operation with empty clock is always ready
        let op = CrdtOperation {
            operation: DsonOperation::FieldAdd {
                path: "test".to_string(),
                value: OperationValue::Null,
            },
            timestamp: 1,
            replica_id: "r2".to_string(),
            vector_clock: VectorClock::new(),
        };

        assert!(processor.is_causally_ready(&op));
    }

    #[test]
    fn test_buffer_and_process() {
        let mut processor = FormatCrdtProcessor::new(MockBatchProcessor, "r1");

        let op = CrdtOperation {
            operation: DsonOperation::FieldAdd {
                path: "test".to_string(),
                value: OperationValue::Null,
            },
            timestamp: 1,
            replica_id: "r2".to_string(),
            vector_clock: VectorClock::new(),
        };

        processor.buffer_operation(op);
        assert_eq!(processor.operation_buffer.len(), 1);

        let conflicts = processor.process_buffered().unwrap();
        // Empty vector clock means causally ready
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_compact() {
        let mut processor = FormatCrdtProcessor::new(MockBatchProcessor, "r1");

        // Add many operations
        for i in 0..2000_u64 {
            processor.operation_history.push(CrdtOperation {
                operation: DsonOperation::FieldAdd {
                    path: format!("field_{i}"),
                    value: OperationValue::Null,
                },
                timestamp: i,
                replica_id: "r1".to_string(),
                vector_clock: VectorClock::new(),
            });
        }

        processor.compact();
        assert!(processor.operation_history.len() <= 1000);
    }

    #[test]
    fn test_reset() {
        let mut processor = FormatCrdtProcessor::new(MockBatchProcessor, "r1");
        let schema = CompiledSchema::compile(&[]).unwrap();
        processor.process(b"{}", &schema).unwrap();

        assert!(processor.document_count() > 0);

        processor.reset();
        assert_eq!(processor.document_count(), 0);
    }

    #[test]
    fn test_document_ids() {
        let mut processor = FormatCrdtProcessor::new(MockBatchProcessor, "r1");
        let schema = CompiledSchema::compile(&[]).unwrap();
        processor.process(b"{}", &schema).unwrap();

        let ids = processor.document_ids();
        assert!(!ids.is_empty());
    }

    #[test]
    fn test_merge_field_with_conflict() {
        let mut processor = FormatCrdtProcessor::new(MockBatchProcessor, "r1");
        let schema = CompiledSchema::compile(&[]).unwrap();
        processor.process(b"{}", &schema).unwrap();

        // First, set a field
        processor.apply_field_value(
            "doc_0",
            "test_field",
            &OperationValue::StringRef("value1".to_string()),
            1,
        );

        // Now merge with same timestamp (conflict)
        let conflict = processor
            .merge_field(
                "test_field",
                OperationValue::StringRef("value2".to_string()),
                1, // Same timestamp
                &MergeStrategy::LastWriteWins,
            )
            .unwrap();

        // Should report conflict for same timestamp
        assert!(conflict.is_some());
    }

    #[test]
    fn test_resolve_conflict() {
        let mut processor = FormatCrdtProcessor::new(MockBatchProcessor, "r1");
        let schema = CompiledSchema::compile(&[]).unwrap();
        processor.process(b"{}", &schema).unwrap();

        let conflict = MergeConflict {
            path: "test".to_string(),
            local_value: OperationValue::NumberRef("10".to_string()),
            remote_value: OperationValue::NumberRef("20".to_string()),
            local_timestamp: 1,
            remote_timestamp: 2,
            resolved_value: None,
        };

        let resolved = processor
            .resolve_conflict(&conflict, &MergeStrategy::Max)
            .unwrap();

        // Max should pick 20
        match resolved {
            OperationValue::NumberRef(n) => assert_eq!(n, "20"),
            _ => panic!("Expected NumberRef"),
        }
    }

    #[test]
    fn test_operation_value_to_json_all_types() {
        // Test all OperationValue variants
        let null_json = FormatCrdtProcessor::<MockBatchProcessor>::operation_value_to_json(
            &OperationValue::Null,
        );
        assert!(null_json.is_null());

        let bool_json = FormatCrdtProcessor::<MockBatchProcessor>::operation_value_to_json(
            &OperationValue::BoolRef(true),
        );
        assert_eq!(bool_json, serde_json::Value::Bool(true));

        let int_json = FormatCrdtProcessor::<MockBatchProcessor>::operation_value_to_json(
            &OperationValue::NumberRef("42".to_string()),
        );
        assert_eq!(int_json, serde_json::json!(42));

        let float_json = FormatCrdtProcessor::<MockBatchProcessor>::operation_value_to_json(
            &OperationValue::NumberRef("3.14".to_string()),
        );
        assert!(float_json.is_number());

        let string_json = FormatCrdtProcessor::<MockBatchProcessor>::operation_value_to_json(
            &OperationValue::StringRef("hello".to_string()),
        );
        assert_eq!(string_json, serde_json::Value::String("hello".to_string()));

        // ArrayRef and ObjectRef use tape positions, so they return empty containers
        let array_json = FormatCrdtProcessor::<MockBatchProcessor>::operation_value_to_json(
            &OperationValue::ArrayRef { start: 0, end: 0 },
        );
        assert!(array_json.is_array());

        let obj_json = FormatCrdtProcessor::<MockBatchProcessor>::operation_value_to_json(
            &OperationValue::ObjectRef { start: 0, end: 0 },
        );
        assert!(obj_json.is_object());
    }

    #[test]
    fn test_json_to_operation_value_all_types() {
        let null_val = FormatCrdtProcessor::<MockBatchProcessor>::json_to_operation_value(
            &serde_json::Value::Null,
        );
        assert!(matches!(null_val, OperationValue::Null));

        let bool_val = FormatCrdtProcessor::<MockBatchProcessor>::json_to_operation_value(
            &serde_json::Value::Bool(false),
        );
        assert!(matches!(bool_val, OperationValue::BoolRef(false)));

        let num_val = FormatCrdtProcessor::<MockBatchProcessor>::json_to_operation_value(
            &serde_json::json!(123),
        );
        assert!(matches!(num_val, OperationValue::NumberRef(_)));

        let str_val = FormatCrdtProcessor::<MockBatchProcessor>::json_to_operation_value(
            &serde_json::Value::String("test".to_string()),
        );
        assert!(matches!(str_val, OperationValue::StringRef(_)));

        // Arrays and objects are serialized to JSON strings in StringRef
        let arr_val = FormatCrdtProcessor::<MockBatchProcessor>::json_to_operation_value(
            &serde_json::json!([1, 2]),
        );
        assert!(matches!(arr_val, OperationValue::StringRef(_)));

        let obj_val = FormatCrdtProcessor::<MockBatchProcessor>::json_to_operation_value(
            &serde_json::json!({"a": 1}),
        );
        assert!(matches!(obj_val, OperationValue::StringRef(_)));
    }

    #[test]
    fn test_dson_processor_access() {
        let mut processor = FormatCrdtProcessor::new(MockBatchProcessor, "r1");
        let _ = processor.dson_processor();
        let _ = processor.dson_processor_mut();
    }

    #[test]
    fn test_field_delete_operation() {
        let mut processor = FormatCrdtProcessor::new(MockBatchProcessor, "r1");
        let schema = CompiledSchema::compile(&[]).unwrap();
        processor.process(b"{}", &schema).unwrap();

        let op = CrdtOperation {
            operation: DsonOperation::FieldDelete {
                path: "name".to_string(),
            },
            timestamp: 10,
            replica_id: "r2".to_string(),
            vector_clock: VectorClock::new(),
        };

        let conflict = processor.merge_operation(op).unwrap();
        assert!(conflict.is_none());
    }
}
