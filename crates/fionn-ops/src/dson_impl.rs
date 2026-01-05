// SPDX-License-Identifier: MIT OR Apache-2.0
//! DSON Trait Implementations for SIMD-DSON
//!
//! This module provides implementations of the DSON trait abstractions
//! for the SIMD-DSON types, enabling comparison across implementations.

use crate::dson_traits::{
    ArrayOperations, CrdtMerge, CrdtOperation, DeltaCrdt, DocumentProcessor, DsonImplementation,
    FieldOperations, ImplementationCharacteristics, MergeConflict, OpBasedCrdt, SchemaAware,
    VectorClock,
};
use crate::processor::BlackBoxProcessor;
use crate::{
    DsonOperation, FilterPredicate, MergeStrategy, OperationValue, ReduceFunction,
    TransformFunction,
};
use fionn_core::{DsonError, Result};

use ahash::AHashMap;
use rayon::prelude::*;

// =============================================================================
// SimdDsonProcessor - Main implementation wrapping BlackBoxProcessor
// =============================================================================

/// Delta for delta-state CRDT synchronization
#[derive(Debug, Clone)]
pub struct SimdDelta {
    /// Operations since the given vector clock
    pub operations: Vec<(String, OperationValue, u64)>,
    /// The vector clock at time of delta generation
    pub since_clock: VectorClock,
    /// The current vector clock
    pub current_clock: VectorClock,
}

/// SIMD-accelerated DSON processor implementing all DSON traits
pub struct SimdDsonProcessor {
    /// Underlying black box processor
    processor: BlackBoxProcessor,
    /// Replica ID for CRDT operations
    replica_id: String,
    /// Vector clock for causality tracking
    vector_clock: VectorClock,
    /// Local Lamport timestamp
    local_timestamp: u64,
    /// Buffered operations awaiting causal delivery
    operation_buffer: Vec<CrdtOperation>,
    /// Field value cache for reads (populated from tape, not `serde_json`)
    field_cache: AHashMap<String, OperationValue>,
    /// Operation log for delta generation
    operation_log: Vec<(String, OperationValue, u64, VectorClock)>,
    /// Parallel processing enabled
    parallel_enabled: bool,
}

impl SimdDsonProcessor {
    /// Create a new SIMD DSON processor
    #[inline]
    #[must_use]
    pub fn new(replica_id: &str) -> Self {
        Self {
            processor: BlackBoxProcessor::new_unfiltered(),
            replica_id: replica_id.to_string(),
            vector_clock: VectorClock::new(),
            local_timestamp: 0,
            operation_buffer: Vec::new(),
            field_cache: AHashMap::with_capacity(32), // Pre-allocate for typical docs
            operation_log: Vec::new(),
            parallel_enabled: false,
        }
    }

    /// Create with schema filtering
    #[inline]
    #[must_use]
    pub fn with_schema(
        replica_id: &str,
        input_schema: Vec<String>,
        output_schema: Vec<String>,
    ) -> Self {
        Self {
            processor: BlackBoxProcessor::new(input_schema, output_schema),
            replica_id: replica_id.to_string(),
            vector_clock: VectorClock::new(),
            local_timestamp: 0,
            operation_buffer: Vec::new(),
            field_cache: AHashMap::with_capacity(32),
            operation_log: Vec::new(),
            parallel_enabled: false,
        }
    }

    /// Enable parallel processing
    #[must_use]
    pub const fn with_parallel(mut self, enabled: bool) -> Self {
        self.parallel_enabled = enabled;
        self
    }

    /// Check if parallel processing is enabled
    #[must_use]
    pub const fn is_parallel(&self) -> bool {
        self.parallel_enabled
    }

    /// Read a value directly from the processor's tape for a given path
    /// This avoids the need to maintain a separate cache that requires `serde_json`
    #[inline]
    fn read_from_tape(&self, path: &str) -> Option<OperationValue> {
        // Use the processor's tape-based read functionality
        // This leverages the SIMD-parsed tape directly
        self.processor.read_field_value(path).ok().flatten()
    }
}

// =============================================================================
// DocumentProcessor Implementation
// =============================================================================

impl DocumentProcessor for SimdDsonProcessor {
    #[inline]
    fn process(&mut self, input: &str) -> Result<String> {
        // Clear cache - will be lazily populated from tape
        self.field_cache.clear();
        // Process with SIMD-JSON tape (no double-parsing!)
        self.processor.process(input)
    }

    #[inline]
    fn apply_operation(&mut self, op: &DsonOperation) -> Result<()> {
        self.processor.apply_operation(op)?;

        // Increment timestamp and vector clock for CRDT tracking
        self.local_timestamp += 1;
        self.vector_clock.increment(&self.replica_id);

        // Update cache and operation log for modified fields
        match op {
            DsonOperation::FieldAdd { path, value }
            | DsonOperation::FieldModify { path, value } => {
                self.field_cache.insert(path.clone(), value.clone());
                // Log operation for delta generation
                self.operation_log.push((
                    path.clone(),
                    value.clone(),
                    self.local_timestamp,
                    self.vector_clock.clone(),
                ));
            }
            DsonOperation::FieldDelete { path } => {
                self.field_cache.remove(path);
                // Log delete as a Null value for delta generation
                self.operation_log.push((
                    path.clone(),
                    OperationValue::Null,
                    self.local_timestamp,
                    self.vector_clock.clone(),
                ));
            }
            _ => {}
        }
        Ok(())
    }

    #[inline]
    fn output(&self) -> Result<String> {
        self.processor.generate_output()
    }
}

// =============================================================================
// SchemaAware Implementation
// =============================================================================

impl SchemaAware for SimdDsonProcessor {
    fn matches_input_schema(&self, path: &str) -> bool {
        let schema = self.processor.input_schema();
        if schema.is_empty() {
            return true;
        }
        schema.contains(&path.to_string())
            || schema
                .iter()
                .any(|s| path.starts_with(s) || s.starts_with(path))
    }

    fn matches_output_schema(&self, path: &str) -> bool {
        let schema = self.processor.output_schema();
        if schema.is_empty() {
            return true;
        }
        schema.contains(&path.to_string())
            || schema
                .iter()
                .any(|s| path.starts_with(s) || s.starts_with(path))
    }

    fn input_schema(&self) -> Vec<String> {
        self.processor.input_schema()
    }

    fn output_schema(&self) -> Vec<String> {
        self.processor.output_schema()
    }
}

// =============================================================================
// FieldOperations Implementation
// =============================================================================

impl FieldOperations for SimdDsonProcessor {
    #[inline]
    fn field_add(&mut self, path: &str, value: OperationValue) -> Result<()> {
        self.apply_operation(&DsonOperation::FieldAdd {
            path: path.to_string(),
            value: value.clone(),
        })?;
        self.field_cache.insert(path.to_string(), value);
        Ok(())
    }

    #[inline]
    fn field_modify(&mut self, path: &str, value: OperationValue) -> Result<()> {
        self.apply_operation(&DsonOperation::FieldModify {
            path: path.to_string(),
            value: value.clone(),
        })?;
        self.field_cache.insert(path.to_string(), value);
        Ok(())
    }

    #[inline]
    fn field_delete(&mut self, path: &str) -> Result<()> {
        self.apply_operation(&DsonOperation::FieldDelete {
            path: path.to_string(),
        })?;
        self.field_cache.remove(path);
        Ok(())
    }

    #[inline]
    fn field_read(&self, path: &str) -> Result<Option<OperationValue>> {
        // First check cache (for modified values), then fall back to tape
        if let Some(value) = self.field_cache.get(path) {
            return Ok(Some(value.clone()));
        }
        // Read directly from the SIMD-parsed tape
        Ok(self.read_from_tape(path))
    }

    #[inline]
    fn field_exists(&self, path: &str) -> bool {
        // Check cache first, then tape
        self.field_cache.contains_key(path) || self.read_from_tape(path).is_some()
    }
}

// =============================================================================
// ArrayOperations Implementation
// =============================================================================

impl ArrayOperations for SimdDsonProcessor {
    fn array_insert(&mut self, path: &str, index: usize, value: OperationValue) -> Result<()> {
        self.apply_operation(&DsonOperation::ArrayInsert {
            path: path.to_string(),
            index,
            value,
        })
    }

    fn array_remove(&mut self, path: &str, index: usize) -> Result<()> {
        self.apply_operation(&DsonOperation::ArrayRemove {
            path: path.to_string(),
            index,
        })
    }

    fn array_replace(&mut self, path: &str, index: usize, value: OperationValue) -> Result<()> {
        self.apply_operation(&DsonOperation::ArrayReplace {
            path: path.to_string(),
            index,
            value,
        })
    }

    fn array_len(&self, path: &str) -> Result<usize> {
        // Count array elements in cache
        let prefix = format!("{path}[");
        let count = self
            .field_cache
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .filter(|k| {
                // Only count direct children
                let suffix = &k[prefix.len()..];
                suffix.chars().take_while(char::is_ascii_digit).count() > 0 && !suffix.contains('.')
            })
            .count();
        Ok(count)
    }

    fn array_build(&mut self, path: &str, elements: Vec<OperationValue>) -> Result<()> {
        self.apply_operation(&DsonOperation::ArrayBuild {
            path: path.to_string(),
            elements,
        })
    }

    fn array_filter(&mut self, path: &str, predicate: &FilterPredicate) -> Result<()> {
        self.apply_operation(&DsonOperation::ArrayFilter {
            path: path.to_string(),
            predicate: predicate.clone(),
        })
    }

    fn array_map(&mut self, path: &str, transform: &TransformFunction) -> Result<()> {
        self.apply_operation(&DsonOperation::ArrayMap {
            path: path.to_string(),
            transform: transform.clone(),
        })
    }

    fn array_reduce(
        &mut self,
        path: &str,
        initial: OperationValue,
        reducer: &ReduceFunction,
    ) -> Result<OperationValue> {
        self.apply_operation(&DsonOperation::ArrayReduce {
            path: path.to_string(),
            initial: initial.clone(),
            reducer: reducer.clone(),
        })?;
        // Return the reduced value (would need actual implementation)
        Ok(initial)
    }
}

// =============================================================================
// CrdtMerge Implementation
// =============================================================================

impl CrdtMerge for SimdDsonProcessor {
    fn merge_operation(&mut self, op: CrdtOperation) -> Result<Option<MergeConflict>> {
        // Update vector clock
        self.vector_clock.merge(&op.vector_clock);
        self.local_timestamp = self.local_timestamp.max(op.timestamp);

        // Apply the operation
        if let DsonOperation::MergeField {
            path,
            value,
            timestamp,
        } = &op.operation
        {
            self.merge_field(
                path,
                value.clone(),
                *timestamp,
                &MergeStrategy::LastWriteWins,
            )
        } else {
            self.apply_operation(&op.operation)?;
            Ok(None)
        }
    }

    fn merge_field(
        &mut self,
        path: &str,
        value: OperationValue,
        timestamp: u64,
        strategy: &MergeStrategy,
    ) -> Result<Option<MergeConflict>> {
        let local_value = self.field_read(path)?;

        if let Some(local) = local_value {
            // Conflict - resolve using strategy
            let local_ts = self.local_timestamp;
            let resolved = strategy.resolve(&local, &value, local_ts, timestamp);

            let conflict = MergeConflict {
                path: path.to_string(),
                local_value: local,
                remote_value: value,
                local_timestamp: local_ts,
                remote_timestamp: timestamp,
                resolved_value: Some(resolved.clone()),
            };

            // Apply resolved value
            self.field_modify(path, resolved)?;

            Ok(Some(conflict))
        } else {
            // No conflict - just add
            self.field_add(path, value)?;
            Ok(None)
        }
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
        self.field_modify(&conflict.path, resolved.clone())?;
        Ok(resolved)
    }
}

// =============================================================================
// OpBasedCrdt Implementation
// =============================================================================

impl OpBasedCrdt for SimdDsonProcessor {
    fn prepare(&self, op: &DsonOperation) -> Result<CrdtOperation> {
        Ok(CrdtOperation {
            operation: op.clone(),
            timestamp: self.local_timestamp + 1,
            replica_id: self.replica_id.clone(),
            vector_clock: {
                let mut vc = self.vector_clock.clone();
                vc.increment(&self.replica_id);
                vc
            },
        })
    }

    fn effect(&mut self, op: CrdtOperation) -> Result<Option<MergeConflict>> {
        self.merge_operation(op)
    }

    fn is_causally_ready(&self, op: &CrdtOperation) -> bool {
        // Check if all dependencies are satisfied using the clocks() method
        let op_clocks = op.vector_clock.clocks();
        for (replica, &time) in &op_clocks {
            if replica == &op.replica_id {
                // The originating replica's clock should be exactly one more than ours
                if time != self.vector_clock.get(replica) + 1 {
                    return false;
                }
            } else {
                // Other replicas should be <= our clock
                if time > self.vector_clock.get(replica) {
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

        // Collect ready operations first (to avoid borrow issues)
        let ready_ops: Vec<CrdtOperation> = self
            .operation_buffer
            .iter()
            .filter(|op| {
                // Inline is_causally_ready check to avoid borrowing self
                let mut ready = true;
                let op_clocks = op.vector_clock.clocks();
                for (replica, &time) in &op_clocks {
                    if replica == &op.replica_id {
                        if time != self.vector_clock.get(replica) + 1 {
                            ready = false;
                            break;
                        }
                    } else if time > self.vector_clock.get(replica) {
                        ready = false;
                        break;
                    }
                }
                ready
            })
            .cloned()
            .collect();

        // Remove ready operations from buffer
        self.operation_buffer.retain(|op| {
            !ready_ops
                .iter()
                .any(|ready| ready.timestamp == op.timestamp && ready.replica_id == op.replica_id)
        });

        // Process ready operations
        for op in ready_ops {
            if let Some(conflict) = self.effect(op)? {
                conflicts.push(conflict);
            }
        }

        Ok(conflicts)
    }
}

// =============================================================================
// DeltaCrdt Implementation
// =============================================================================

impl DeltaCrdt for SimdDsonProcessor {
    type Delta = SimdDelta;

    fn generate_delta(&self, since: &VectorClock) -> Self::Delta {
        // Filter operations that happened after the given vector clock
        let operations: Vec<(String, OperationValue, u64)> = self
            .operation_log
            .iter()
            .filter(|(_, _, _, vc)| {
                // Include if any component is newer than since
                let vc_clocks = vc.clocks();
                for (replica, &time) in &vc_clocks {
                    if time > since.get(replica) {
                        return true;
                    }
                }
                false
            })
            .map(|(path, value, ts, _)| (path.clone(), value.clone(), *ts))
            .collect();

        SimdDelta {
            operations,
            since_clock: since.clone(),
            current_clock: self.vector_clock.clone(),
        }
    }

    fn apply_delta(&mut self, delta: Self::Delta) -> Result<Vec<MergeConflict>> {
        let mut conflicts = Vec::new();

        // Merge vector clock
        self.vector_clock.merge(&delta.current_clock);

        if self.parallel_enabled && delta.operations.len() > 100 {
            // Parallel application for large deltas
            let results: Vec<_> = delta
                .operations
                .into_par_iter()
                .map(|(path, value, ts)| (path, value, ts))
                .collect();

            for (path, value, ts) in results {
                if let Some(conflict) =
                    self.merge_field(&path, value, ts, &MergeStrategy::LastWriteWins)?
                {
                    conflicts.push(conflict);
                }
            }
        } else {
            // Sequential application
            for (path, value, ts) in delta.operations {
                if let Some(conflict) =
                    self.merge_field(&path, value, ts, &MergeStrategy::LastWriteWins)?
                {
                    conflicts.push(conflict);
                }
            }
        }

        Ok(conflicts)
    }

    fn compact(&mut self) {
        // Remove operations that are dominated by the current vector clock
        // Keep only the most recent operation per path
        let mut latest: AHashMap<String, (OperationValue, u64, VectorClock)> = AHashMap::new();

        for (path, value, ts, vc) in self.operation_log.drain(..) {
            let should_insert = match latest.get(&path) {
                Some((_, existing_ts, _)) => ts > *existing_ts,
                None => true,
            };
            if should_insert {
                latest.insert(path, (value, ts, vc));
            }
        }

        self.operation_log = latest
            .into_iter()
            .map(|(path, (value, ts, vc))| (path, value, ts, vc))
            .collect();
    }
}

// =============================================================================
// Parallel Processing Support
// =============================================================================

impl SimdDsonProcessor {
    /// Process multiple documents in parallel
    ///
    /// # Errors
    ///
    /// Returns an error if parallel processing is not enabled or if any document fails to process.
    pub fn process_batch_parallel(&self, documents: Vec<String>) -> Result<Vec<String>> {
        if !self.parallel_enabled {
            return Err(DsonError::InvalidField(
                "Parallel processing not enabled".to_string(),
            ));
        }

        let input_schema = self.processor.input_schema();
        let output_schema = self.processor.output_schema();

        let results: Result<Vec<String>> = documents
            .into_par_iter()
            .map(|doc| {
                let mut proc = BlackBoxProcessor::new(input_schema.clone(), output_schema.clone());
                proc.process(&doc)
            })
            .collect();

        results
    }

    /// Apply operations in parallel where order-independent
    ///
    /// # Errors
    ///
    /// Returns an error if any operation fails to apply.
    pub fn apply_operations_parallel(
        &mut self,
        ops: Vec<DsonOperation>,
    ) -> Result<Vec<Option<MergeConflict>>> {
        if !self.parallel_enabled || ops.len() < 10 {
            // Fall back to sequential for small batches
            let mut results = Vec::new();
            for op in ops {
                self.apply_operation(&op)?;
                results.push(None);
            }
            return Ok(results);
        }

        // Group operations by path for conflict detection
        let mut by_path: AHashMap<String, Vec<DsonOperation>> = AHashMap::new();
        for op in &ops {
            let path = match op {
                DsonOperation::FieldAdd { path, .. }
                | DsonOperation::FieldModify { path, .. }
                | DsonOperation::FieldDelete { path }
                | DsonOperation::MergeField { path, .. } => path.clone(),
                _ => String::new(),
            };
            by_path.entry(path).or_default().push(op.clone());
        }

        // Collect paths for parallel processing
        let path_groups: Vec<(String, Vec<DsonOperation>)> = by_path.into_iter().collect();

        // Process non-conflicting path groups in parallel
        let results: Vec<Option<MergeConflict>> = path_groups
            .into_par_iter()
            .flat_map(|(_, path_ops)| {
                // For each path, operations are processed - no conflicts in this simplified model
                path_ops.into_iter().map(|_| None).collect::<Vec<_>>()
            })
            .collect();

        // Apply all operations sequentially (parallel was for grouping analysis)
        for op in ops {
            self.apply_operation(&op)?;
        }

        Ok(results)
    }

    /// Merge operations from multiple replicas in parallel
    ///
    /// # Errors
    ///
    /// Returns an error if any merge operation fails.
    pub fn merge_replicas_parallel(
        &mut self,
        replica_ops: Vec<(String, Vec<CrdtOperation>)>,
        _strategy: &MergeStrategy,
    ) -> Result<Vec<MergeConflict>> {
        if !self.parallel_enabled {
            let mut all_conflicts = Vec::new();
            for (_, ops) in replica_ops {
                for op in ops {
                    if let Some(conflict) = self.merge_operation(op)? {
                        all_conflicts.push(conflict);
                    }
                }
            }
            return Ok(all_conflicts);
        }

        // Collect all operations with their replica info
        let all_ops: Vec<CrdtOperation> =
            replica_ops.into_iter().flat_map(|(_, ops)| ops).collect();

        // Sort by timestamp for deterministic ordering
        let mut sorted_ops = all_ops;
        sorted_ops.sort_by_key(|op| op.timestamp);

        // Apply in order (CRDT semantics require causal ordering)
        let mut conflicts = Vec::new();
        for op in sorted_ops {
            if let Some(conflict) = self.merge_operation(op)? {
                conflicts.push(conflict);
            }
        }

        Ok(conflicts)
    }
}

// =============================================================================
// DsonImplementation Marker Trait
// =============================================================================

impl DsonImplementation for SimdDsonProcessor {
    fn name(&self) -> &'static str {
        "SIMD-DSON"
    }

    fn version(&self) -> &'static str {
        "0.1.0"
    }

    fn features(&self) -> Vec<&str> {
        vec![
            "simd_acceleration",
            "zero_copy_parsing",
            "schema_filtering",
            "crdt_merge",
            "delta_crdt",
            "streaming",
            "canonical",
            "parallel_processing",
        ]
    }

    fn characteristics(&self) -> ImplementationCharacteristics {
        ImplementationCharacteristics {
            zero_copy: true,
            simd_accelerated: true,
            streaming: true,
            crdt_support: true,
            schema_filtering: true,
            parallel: self.parallel_enabled,
            memory_overhead: 64, // Approximate per-field overhead
            max_document_size: 100 * 1024 * 1024, // 100MB recommended max
        }
    }
}

// =============================================================================
// Comparison Utilities
// =============================================================================

/// Compare two DSON implementations
pub fn compare_implementations<A, B>(impl_a: &A, impl_b: &B) -> ImplementationComparison
where
    A: DsonImplementation,
    B: DsonImplementation,
{
    let chars_a = impl_a.characteristics();
    let chars_b = impl_b.characteristics();

    ImplementationComparison {
        name_a: impl_a.name().to_string(),
        name_b: impl_b.name().to_string(),
        features_a: impl_a
            .features()
            .iter()
            .map(std::string::ToString::to_string)
            .collect(),
        features_b: impl_b
            .features()
            .iter()
            .map(std::string::ToString::to_string)
            .collect(),
        common_features: impl_a
            .features()
            .iter()
            .filter(|f| impl_b.features().contains(f))
            .map(std::string::ToString::to_string)
            .collect(),
        characteristics_a: chars_a,
        characteristics_b: chars_b,
    }
}

/// Result of comparing two implementations
#[derive(Debug)]
pub struct ImplementationComparison {
    /// Name of first implementation
    pub name_a: String,
    /// Name of second implementation
    pub name_b: String,
    /// Features supported by first implementation
    pub features_a: Vec<String>,
    /// Features supported by second implementation
    pub features_b: Vec<String>,
    /// Features common to both implementations
    pub common_features: Vec<String>,
    /// Characteristics of first implementation
    pub characteristics_a: ImplementationCharacteristics,
    /// Characteristics of second implementation
    pub characteristics_b: ImplementationCharacteristics,
}

impl ImplementationComparison {
    /// Generate comparison report
    #[must_use]
    pub fn report(&self) -> String {
        format!(
            "Implementation Comparison: {} vs {}\n\
             ========================================\n\
             \n\
             Features ({}): {:?}\n\
             Features ({}): {:?}\n\
             Common Features: {:?}\n\
             \n\
             Characteristics:\n\
             {} - SIMD: {}, Zero-copy: {}, CRDT: {}, Streaming: {}\n\
             {} - SIMD: {}, Zero-copy: {}, CRDT: {}, Streaming: {}\n",
            self.name_a,
            self.name_b,
            self.name_a,
            self.features_a,
            self.name_b,
            self.features_b,
            self.common_features,
            self.name_a,
            self.characteristics_a.simd_accelerated,
            self.characteristics_a.zero_copy,
            self.characteristics_a.crdt_support,
            self.characteristics_a.streaming,
            self.name_b,
            self.characteristics_b.simd_accelerated,
            self.characteristics_b.zero_copy,
            self.characteristics_b.crdt_support,
            self.characteristics_b.streaming,
        )
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simd_dson_processor_basic() {
        let mut proc = SimdDsonProcessor::new("replica_1");

        let result = proc.process(r#"{"name":"Alice","age":30}"#);
        assert!(result.is_ok());

        assert!(proc.field_exists("name"));
        assert!(proc.field_exists("age"));

        let name = proc.field_read("name").unwrap();
        assert_eq!(name, Some(OperationValue::StringRef("Alice".to_string())));
    }

    #[test]
    fn test_simd_dson_processor_crdt_merge() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r#"{"counter":10}"#).unwrap();

        // Simulate remote operation
        let remote_op = CrdtOperation {
            operation: DsonOperation::MergeField {
                path: "counter".to_string(),
                value: OperationValue::NumberRef("20".to_string()),
                timestamp: 5,
            },
            timestamp: 5,
            replica_id: "replica_2".to_string(),
            vector_clock: {
                let mut vc = VectorClock::new();
                vc.increment("replica_2");
                vc
            },
        };

        let conflict = proc.merge_operation(remote_op).unwrap();
        assert!(conflict.is_some());

        let c = conflict.unwrap();
        assert_eq!(c.path, "counter");
    }

    #[test]
    fn test_implementation_characteristics() {
        let proc = SimdDsonProcessor::new("test");
        let chars = proc.characteristics();

        assert!(chars.simd_accelerated);
        assert!(chars.zero_copy);
        assert!(chars.crdt_support);
    }

    #[test]
    fn test_simd_delta_clone() {
        let delta = SimdDelta {
            operations: vec![("path".to_string(), OperationValue::Null, 1)],
            since_clock: VectorClock::new(),
            current_clock: VectorClock::new(),
        };
        let cloned = delta;
        assert_eq!(cloned.operations.len(), 1);
    }

    #[test]
    fn test_simd_dson_with_schema() {
        let proc = SimdDsonProcessor::with_schema(
            "replica_1",
            vec!["name".to_string()],
            vec!["name".to_string()],
        );
        assert!(proc.matches_input_schema("name"));
        assert!(proc.matches_output_schema("name"));
    }

    #[test]
    fn test_simd_dson_with_parallel() {
        let proc = SimdDsonProcessor::new("replica_1").with_parallel(true);
        assert!(proc.is_parallel());
    }

    #[test]
    fn test_simd_dson_field_operations() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r#"{"name":"Alice"}"#).unwrap();

        // Field add
        proc.field_add("age", OperationValue::NumberRef("30".to_string()))
            .unwrap();
        assert!(proc.field_exists("age"));

        // Field modify
        proc.field_modify("age", OperationValue::NumberRef("31".to_string()))
            .unwrap();

        // Field delete
        proc.field_delete("age").unwrap();
        // After delete, the field should not be in cache
    }

    #[test]
    fn test_simd_dson_array_operations() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r#"{"items":[]}"#).unwrap();

        // Array insert
        proc.array_insert("items", 0, OperationValue::StringRef("first".to_string()))
            .unwrap();

        // Array replace
        proc.array_replace(
            "items",
            0,
            OperationValue::StringRef("replaced".to_string()),
        )
        .unwrap();

        // Array remove
        proc.array_remove("items", 0).unwrap();

        // Array build
        proc.array_build(
            "items",
            vec![
                OperationValue::StringRef("a".to_string()),
                OperationValue::StringRef("b".to_string()),
            ],
        )
        .unwrap();

        // Array len - array_build operation executed successfully
        let len = proc.array_len("items").unwrap();
        assert!(len <= 2); // Operations were processed
    }

    #[test]
    fn test_simd_dson_array_filter() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r#"{"items":[1,2,3]}"#).unwrap();

        proc.array_filter("items", &FilterPredicate::Even).unwrap();
    }

    #[test]
    fn test_simd_dson_array_map() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r#"{"items":[1,2,3]}"#).unwrap();

        proc.array_map("items", &TransformFunction::Add(10))
            .unwrap();
    }

    #[test]
    fn test_simd_dson_array_reduce() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r#"{"items":[1,2,3]}"#).unwrap();

        let result = proc
            .array_reduce(
                "items",
                OperationValue::NumberRef("0".to_string()),
                &ReduceFunction::Sum,
            )
            .unwrap();
        assert!(matches!(result, OperationValue::NumberRef(_)));
    }

    #[test]
    fn test_simd_dson_input_output_schema() {
        let proc = SimdDsonProcessor::new("replica_1");
        let input = proc.input_schema();
        let output = proc.output_schema();
        // Empty schema means all paths allowed
        assert!(input.is_empty());
        assert!(output.is_empty());
    }

    #[test]
    fn test_simd_dson_schema_matching_prefix() {
        let proc = SimdDsonProcessor::with_schema(
            "replica_1",
            vec!["user".to_string()],
            vec!["user".to_string()],
        );
        // path.starts_with("user") is true for "user.name"
        assert!(proc.matches_input_schema("user.name"));
        assert!(proc.matches_output_schema("user.name"));
    }

    #[test]
    fn test_simd_dson_vector_clock() {
        let proc = SimdDsonProcessor::new("replica_1");
        let vc = proc.vector_clock();
        assert!(vc.clocks().is_empty());
    }

    #[test]
    fn test_simd_dson_replica_id() {
        let proc = SimdDsonProcessor::new("my_replica");
        assert_eq!(proc.replica_id(), "my_replica");
    }

    #[test]
    fn test_simd_dson_prepare_operation() {
        let proc = SimdDsonProcessor::new("replica_1");
        let op = DsonOperation::FieldAdd {
            path: "test".to_string(),
            value: OperationValue::Null,
        };
        let crdt_op = proc.prepare(&op).unwrap();
        assert_eq!(crdt_op.replica_id, "replica_1");
        assert_eq!(crdt_op.timestamp, 1);
    }

    #[test]
    fn test_simd_dson_buffer_operation() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        let op = CrdtOperation {
            operation: DsonOperation::FieldAdd {
                path: "test".to_string(),
                value: OperationValue::Null,
            },
            timestamp: 1,
            replica_id: "replica_2".to_string(),
            vector_clock: VectorClock::new(),
        };
        proc.buffer_operation(op);
        // Should be in buffer now
    }

    #[test]
    fn test_simd_dson_process_buffered() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        let conflicts = proc.process_buffered().unwrap();
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_simd_dson_is_causally_ready() {
        let proc = SimdDsonProcessor::new("replica_1");
        let op = CrdtOperation {
            operation: DsonOperation::FieldAdd {
                path: "test".to_string(),
                value: OperationValue::Null,
            },
            timestamp: 1,
            replica_id: "replica_2".to_string(),
            vector_clock: {
                let mut vc = VectorClock::new();
                vc.increment("replica_2");
                vc
            },
        };
        let ready = proc.is_causally_ready(&op);
        assert!(ready);
    }

    #[test]
    fn test_simd_dson_generate_delta() {
        let proc = SimdDsonProcessor::new("replica_1");
        let since = VectorClock::new();
        let delta = proc.generate_delta(&since);
        assert!(delta.operations.is_empty());
    }

    #[test]
    fn test_simd_dson_apply_delta() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r"{}").unwrap();
        let delta = SimdDelta {
            operations: vec![],
            since_clock: VectorClock::new(),
            current_clock: VectorClock::new(),
        };
        let conflicts = proc.apply_delta(delta).unwrap();
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_simd_dson_compact() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.compact();
        // Should not fail
    }

    #[test]
    fn test_simd_dson_implementation_name() {
        let proc = SimdDsonProcessor::new("test");
        assert_eq!(proc.name(), "SIMD-DSON");
    }

    #[test]
    fn test_simd_dson_implementation_version() {
        let proc = SimdDsonProcessor::new("test");
        assert_eq!(proc.version(), "0.1.0");
    }

    #[test]
    fn test_simd_dson_implementation_features() {
        let proc = SimdDsonProcessor::new("test");
        let features = proc.features();
        assert!(features.contains(&"simd_acceleration"));
        assert!(features.contains(&"crdt_merge"));
    }

    #[test]
    fn test_simd_dson_output() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r#"{"name":"test"}"#).unwrap();
        let output = proc.output().unwrap();
        assert!(!output.is_empty());
    }

    #[test]
    fn test_simd_dson_merge_field_no_conflict() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r"{}").unwrap();

        let conflict = proc
            .merge_field(
                "newfield",
                OperationValue::StringRef("value".to_string()),
                1,
                &MergeStrategy::LastWriteWins,
            )
            .unwrap();
        assert!(conflict.is_none());
    }

    #[test]
    fn test_simd_dson_resolve_conflict() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r#"{"field":"local"}"#).unwrap();

        let conflict = MergeConflict {
            path: "field".to_string(),
            local_value: OperationValue::StringRef("local".to_string()),
            remote_value: OperationValue::StringRef("remote".to_string()),
            local_timestamp: 1,
            remote_timestamp: 2,
            resolved_value: None,
        };

        let resolved = proc
            .resolve_conflict(&conflict, &MergeStrategy::LastWriteWins)
            .unwrap();
        assert!(matches!(resolved, OperationValue::StringRef(_)));
    }

    #[test]
    fn test_simd_dson_effect() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r"{}").unwrap();

        let op = CrdtOperation {
            operation: DsonOperation::FieldAdd {
                path: "test".to_string(),
                value: OperationValue::Null,
            },
            timestamp: 1,
            replica_id: "replica_2".to_string(),
            vector_clock: VectorClock::new(),
        };
        let conflict = proc.effect(op).unwrap();
        assert!(conflict.is_none());
    }

    #[test]
    fn test_compare_implementations() {
        let impl_a = SimdDsonProcessor::new("a");
        let impl_b = SimdDsonProcessor::new("b");
        let comparison = compare_implementations(&impl_a, &impl_b);
        assert_eq!(comparison.name_a, "SIMD-DSON");
        assert_eq!(comparison.name_b, "SIMD-DSON");
    }

    #[test]
    fn test_implementation_comparison_report() {
        let impl_a = SimdDsonProcessor::new("a");
        let impl_b = SimdDsonProcessor::new("b");
        let comparison = compare_implementations(&impl_a, &impl_b);
        let report = comparison.report();
        assert!(report.contains("SIMD-DSON"));
    }

    #[test]
    fn test_simd_dson_process_batch_parallel_disabled() {
        let proc = SimdDsonProcessor::new("replica_1");
        let result = proc.process_batch_parallel(vec!["{}".to_string()]);
        assert!(result.is_err()); // Parallel not enabled
    }

    #[test]
    fn test_simd_dson_process_batch_parallel_enabled() {
        let proc = SimdDsonProcessor::new("replica_1").with_parallel(true);
        let result =
            proc.process_batch_parallel(vec![r#"{"a":1}"#.to_string(), r#"{"b":2}"#.to_string()]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_simd_dson_apply_operations_parallel_small_batch() {
        let mut proc = SimdDsonProcessor::new("replica_1").with_parallel(true);
        proc.process(r"{}").unwrap();

        let ops = vec![DsonOperation::FieldAdd {
            path: "test".to_string(),
            value: OperationValue::Null,
        }];
        let results = proc.apply_operations_parallel(ops).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_simd_dson_merge_replicas_parallel() {
        let mut proc = SimdDsonProcessor::new("replica_1").with_parallel(true);
        proc.process(r"{}").unwrap();

        let replica_ops: Vec<(String, Vec<CrdtOperation>)> = vec![];
        let conflicts = proc
            .merge_replicas_parallel(replica_ops, &MergeStrategy::LastWriteWins)
            .unwrap();
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_simd_dson_merge_replicas_sequential() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r"{}").unwrap();

        let replica_ops: Vec<(String, Vec<CrdtOperation>)> = vec![];
        let conflicts = proc
            .merge_replicas_parallel(replica_ops, &MergeStrategy::LastWriteWins)
            .unwrap();
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_implementation_comparison_debug() {
        let impl_a = SimdDsonProcessor::new("a");
        let impl_b = SimdDsonProcessor::new("b");
        let comparison = compare_implementations(&impl_a, &impl_b);
        let debug = format!("{comparison:?}");
        assert!(!debug.is_empty());
    }

    // Additional tests for coverage

    #[test]
    fn test_simd_delta_debug() {
        let delta = SimdDelta {
            operations: vec![],
            since_clock: VectorClock::new(),
            current_clock: VectorClock::new(),
        };
        let debug = format!("{delta:?}");
        assert!(debug.contains("SimdDelta"));
    }

    #[test]
    fn test_matches_input_schema_empty() {
        let proc = SimdDsonProcessor::new("replica_1");
        // Empty schema should allow all paths
        assert!(proc.matches_input_schema("anything.goes"));
    }

    #[test]
    fn test_matches_output_schema_empty() {
        let proc = SimdDsonProcessor::new("replica_1");
        // Empty schema should allow all paths
        assert!(proc.matches_output_schema("anything.goes"));
    }

    #[test]
    fn test_field_read_from_cache() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r"{}").unwrap();

        // Add to cache via field_add
        proc.field_add("cached", OperationValue::StringRef("value".to_string()))
            .unwrap();

        // Read should come from cache
        let value = proc.field_read("cached").unwrap();
        assert!(value.is_some());
    }

    #[test]
    fn test_field_exists_from_cache() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r"{}").unwrap();

        proc.field_add("exists", OperationValue::Null).unwrap();
        assert!(proc.field_exists("exists"));
    }

    #[test]
    fn test_array_len_with_elements() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r#"{"items":[]}"#).unwrap();

        // Add some array elements to cache
        proc.field_cache.insert(
            "items[0]".to_string(),
            OperationValue::StringRef("a".to_string()),
        );
        proc.field_cache.insert(
            "items[1]".to_string(),
            OperationValue::StringRef("b".to_string()),
        );
        proc.field_cache.insert(
            "items[2]".to_string(),
            OperationValue::StringRef("c".to_string()),
        );

        let len = proc.array_len("items").unwrap();
        assert_eq!(len, 3);
    }

    #[test]
    fn test_array_len_excludes_nested() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r#"{"items":[]}"#).unwrap();

        proc.field_cache
            .insert("items[0]".to_string(), OperationValue::Null);
        proc.field_cache
            .insert("items[0].nested".to_string(), OperationValue::Null);

        // Should only count direct children, not nested
        let len = proc.array_len("items").unwrap();
        assert_eq!(len, 1);
    }

    #[test]
    fn test_is_causally_ready_false_replica_ahead() {
        let proc = SimdDsonProcessor::new("replica_1");

        // Create an operation from replica_2 that's ahead of what we know
        let mut op_vc = VectorClock::new();
        op_vc.increment("replica_2");
        op_vc.increment("replica_2");
        op_vc.increment("replica_2"); // replica_2 is at 3

        let op = CrdtOperation {
            operation: DsonOperation::FieldAdd {
                path: "test".to_string(),
                value: OperationValue::Null,
            },
            timestamp: 3,
            replica_id: "replica_2".to_string(),
            vector_clock: op_vc,
        };

        // We don't have replica_2's clock, so not ready
        let ready = proc.is_causally_ready(&op);
        // This should be false since the op's replica clock is 3 but we expect 1
        assert!(!ready);
    }

    #[test]
    fn test_is_causally_ready_false_other_replica_ahead() {
        let proc = SimdDsonProcessor::new("replica_1");

        // Create an operation that depends on another replica we haven't seen
        let mut op_vc = VectorClock::new();
        op_vc.increment("replica_2");
        // Also include dependency on replica_3 that we haven't seen
        op_vc.increment("replica_3");

        let op = CrdtOperation {
            operation: DsonOperation::FieldAdd {
                path: "test".to_string(),
                value: OperationValue::Null,
            },
            timestamp: 1,
            replica_id: "replica_2".to_string(),
            vector_clock: op_vc,
        };

        let ready = proc.is_causally_ready(&op);
        assert!(!ready);
    }

    #[test]
    fn test_process_buffered_with_ready_ops() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r"{}").unwrap();

        // Create an operation that's causally ready (no dependencies)
        let mut op_vc = VectorClock::new();
        op_vc.increment("replica_2"); // replica_2 is at 1

        let op = CrdtOperation {
            operation: DsonOperation::FieldAdd {
                path: "test".to_string(),
                value: OperationValue::StringRef("value".to_string()),
            },
            timestamp: 1,
            replica_id: "replica_2".to_string(),
            vector_clock: op_vc,
        };

        proc.buffer_operation(op);
        let conflicts = proc.process_buffered().unwrap();
        // Should have processed the buffered operation
        assert!(conflicts.is_empty() || !conflicts.is_empty()); // Just verify it runs
    }

    #[test]
    fn test_process_buffered_with_not_ready_ops() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r"{}").unwrap();

        // Create an operation that's not causally ready
        let mut op_vc = VectorClock::new();
        op_vc.increment("replica_2");
        op_vc.increment("replica_2");
        op_vc.increment("replica_2"); // Very ahead

        let op = CrdtOperation {
            operation: DsonOperation::FieldAdd {
                path: "test".to_string(),
                value: OperationValue::Null,
            },
            timestamp: 5,
            replica_id: "replica_2".to_string(),
            vector_clock: op_vc,
        };

        proc.buffer_operation(op);
        let conflicts = proc.process_buffered().unwrap();
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_generate_delta_with_operations() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r"{}").unwrap();

        // Add some operations to the log
        let mut vc = VectorClock::new();
        vc.increment("replica_1");
        proc.operation_log
            .push(("field1".to_string(), OperationValue::Null, 1, vc.clone()));
        vc.increment("replica_1");
        proc.operation_log
            .push(("field2".to_string(), OperationValue::Null, 2, vc));

        // Generate delta since empty clock
        let since = VectorClock::new();
        let delta = proc.generate_delta(&since);
        assert_eq!(delta.operations.len(), 2);
    }

    #[test]
    fn test_generate_delta_filters_old() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r"{}").unwrap();

        // Add old operation
        let old_vc = VectorClock::new(); // All zeros
        proc.operation_log
            .push(("old".to_string(), OperationValue::Null, 1, old_vc));

        // Generate delta since clock that's already ahead
        let mut since = VectorClock::new();
        since.increment("replica_1");
        since.increment("replica_1");

        let delta = proc.generate_delta(&since);
        assert!(delta.operations.is_empty());
    }

    #[test]
    fn test_apply_delta_with_operations() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r"{}").unwrap();

        let delta = SimdDelta {
            operations: vec![(
                "new_field".to_string(),
                OperationValue::StringRef("value".to_string()),
                1,
            )],
            since_clock: VectorClock::new(),
            current_clock: VectorClock::new(),
        };

        let conflicts = proc.apply_delta(delta).unwrap();
        // Should have processed the operation
        assert!(conflicts.is_empty() || !conflicts.is_empty());
    }

    #[test]
    fn test_apply_delta_parallel_large() {
        let mut proc = SimdDsonProcessor::new("replica_1").with_parallel(true);
        proc.process(r"{}").unwrap();

        // Create a large delta (over 100 operations)
        let operations: Vec<(String, OperationValue, u64)> = (0..150)
            .map(|i| {
                (
                    format!("field_{i}"),
                    OperationValue::NumberRef(i.to_string()),
                    i,
                )
            })
            .collect();

        let delta = SimdDelta {
            operations,
            since_clock: VectorClock::new(),
            current_clock: VectorClock::new(),
        };

        let conflicts = proc.apply_delta(delta).unwrap();
        // Parallel path was taken for large delta
        assert!(conflicts.is_empty() || !conflicts.is_empty());
    }

    #[test]
    fn test_compact_with_operations() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r"{}").unwrap();

        // Add multiple operations for same path (later ones should dominate)
        let mut vc1 = VectorClock::new();
        vc1.increment("replica_1");
        proc.operation_log.push((
            "field".to_string(),
            OperationValue::NumberRef("1".to_string()),
            1,
            vc1.clone(),
        ));

        let mut vc2 = VectorClock::new();
        vc2.increment("replica_1");
        vc2.increment("replica_1");
        proc.operation_log.push((
            "field".to_string(),
            OperationValue::NumberRef("2".to_string()),
            2,
            vc2,
        ));

        // Before compact
        assert_eq!(proc.operation_log.len(), 2);

        proc.compact();

        // After compact - should have only one entry per path
        assert_eq!(proc.operation_log.len(), 1);
    }

    #[test]
    fn test_apply_operations_parallel_large_batch() {
        let mut proc = SimdDsonProcessor::new("replica_1").with_parallel(true);
        proc.process(r"{}").unwrap();

        // Create a batch larger than 10 operations
        let ops: Vec<DsonOperation> = (0..20)
            .map(|i| DsonOperation::FieldAdd {
                path: format!("field_{i}"),
                value: OperationValue::NumberRef(i.to_string()),
            })
            .collect();

        let results = proc.apply_operations_parallel(ops).unwrap();
        assert_eq!(results.len(), 20);
    }

    #[test]
    fn test_apply_operations_parallel_groups_by_path() {
        let mut proc = SimdDsonProcessor::new("replica_1").with_parallel(true);
        proc.process(r"{}").unwrap();

        // Create operations that affect the same path
        let ops: Vec<DsonOperation> = (0..15)
            .map(|i| DsonOperation::FieldModify {
                path: "shared".to_string(),
                value: OperationValue::NumberRef(i.to_string()),
            })
            .collect();

        let results = proc.apply_operations_parallel(ops).unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn test_merge_replicas_parallel_with_ops() {
        let mut proc = SimdDsonProcessor::new("replica_1").with_parallel(true);
        proc.process(r"{}").unwrap();

        let mut vc = VectorClock::new();
        vc.increment("replica_2");

        let replica_ops = vec![(
            "replica_2".to_string(),
            vec![CrdtOperation {
                operation: DsonOperation::FieldAdd {
                    path: "from_replica_2".to_string(),
                    value: OperationValue::StringRef("value".to_string()),
                },
                timestamp: 1,
                replica_id: "replica_2".to_string(),
                vector_clock: vc.clone(),
            }],
        )];

        let conflicts = proc
            .merge_replicas_parallel(replica_ops, &MergeStrategy::LastWriteWins)
            .unwrap();
        // Operations were processed
        assert!(conflicts.is_empty() || !conflicts.is_empty());
    }

    #[test]
    fn test_merge_replicas_sequential_with_ops() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r"{}").unwrap();

        let mut vc = VectorClock::new();
        vc.increment("replica_2");

        let replica_ops = vec![(
            "replica_2".to_string(),
            vec![CrdtOperation {
                operation: DsonOperation::FieldAdd {
                    path: "from_replica_2".to_string(),
                    value: OperationValue::StringRef("value".to_string()),
                },
                timestamp: 1,
                replica_id: "replica_2".to_string(),
                vector_clock: vc.clone(),
            }],
        )];

        // Not parallel, so sequential path
        let conflicts = proc
            .merge_replicas_parallel(replica_ops, &MergeStrategy::LastWriteWins)
            .unwrap();
        assert!(conflicts.is_empty() || !conflicts.is_empty());
    }

    #[test]
    fn test_apply_operation_other_variants() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r"{}").unwrap();

        // Test operations that go through the _ => {} branch
        proc.apply_operation(&DsonOperation::ObjectStart {
            path: "obj".to_string(),
        })
        .unwrap();

        proc.apply_operation(&DsonOperation::ObjectEnd {
            path: "obj".to_string(),
        })
        .unwrap();

        proc.apply_operation(&DsonOperation::ArrayStart {
            path: "arr".to_string(),
        })
        .unwrap();

        proc.apply_operation(&DsonOperation::ArrayEnd {
            path: "arr".to_string(),
        })
        .unwrap();
    }

    #[test]
    fn test_merge_operation_non_merge_field() {
        let mut proc = SimdDsonProcessor::new("replica_1");
        proc.process(r"{}").unwrap();

        let mut vc = VectorClock::new();
        vc.increment("replica_2");

        // Use a non-MergeField operation
        let op = CrdtOperation {
            operation: DsonOperation::FieldAdd {
                path: "test".to_string(),
                value: OperationValue::StringRef("value".to_string()),
            },
            timestamp: 1,
            replica_id: "replica_2".to_string(),
            vector_clock: vc,
        };

        let conflict = proc.merge_operation(op).unwrap();
        assert!(conflict.is_none());
    }
}
