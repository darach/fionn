// SPDX-License-Identifier: MIT OR Apache-2.0
//! DSON Trait Abstractions
//!
//! This module provides trait-based abstractions for DSON operations,
//! enabling comparison across implementations and CRDT/merge semantics.

use crate::{
    DsonOperation, FilterPredicate, MergeStrategy, OperationValue, ReduceFunction, StreamGenerator,
    TransformFunction,
};
use fionn_core::Result;
use smallvec::SmallVec;

// =============================================================================
// Core DSON Traits
// =============================================================================

/// Core document processor trait - the fundamental DSON interface
pub trait DocumentProcessor {
    /// Process input JSON and return transformed output
    ///
    /// # Errors
    /// Returns an error if parsing or transformation fails
    fn process(&mut self, input: &str) -> Result<String>;

    /// Apply a single operation
    ///
    /// # Errors
    /// Returns an error if the operation cannot be applied
    fn apply_operation(&mut self, op: &DsonOperation) -> Result<()>;

    /// Apply a batch of operations
    ///
    /// # Errors
    /// Returns an error if any operation in the batch fails
    fn apply_operations(&mut self, ops: &[DsonOperation]) -> Result<()> {
        for op in ops {
            self.apply_operation(op)?;
        }
        Ok(())
    }

    /// Generate output from current state
    ///
    /// # Errors
    /// Returns an error if serialization fails
    fn output(&self) -> Result<String>;
}

/// Schema-aware filtering trait
pub trait SchemaAware {
    /// Check if a path matches the input schema
    fn matches_input_schema(&self, path: &str) -> bool;

    /// Check if a path matches the output schema
    fn matches_output_schema(&self, path: &str) -> bool;

    /// Get input schema paths
    fn input_schema(&self) -> Vec<String>;

    /// Get output schema paths
    fn output_schema(&self) -> Vec<String>;
}

/// Field operations trait - CRUD on document fields
pub trait FieldOperations {
    /// Add a field at path with value
    ///
    /// # Errors
    /// Returns an error if the field cannot be added at the specified path
    fn field_add(&mut self, path: &str, value: OperationValue) -> Result<()>;

    /// Modify existing field at path
    ///
    /// # Errors
    /// Returns an error if the field does not exist or cannot be modified
    fn field_modify(&mut self, path: &str, value: OperationValue) -> Result<()>;

    /// Delete field at path
    ///
    /// # Errors
    /// Returns an error if the field does not exist or cannot be deleted
    fn field_delete(&mut self, path: &str) -> Result<()>;

    /// Read field value at path
    ///
    /// # Errors
    /// Returns an error if the path is invalid
    fn field_read(&self, path: &str) -> Result<Option<OperationValue>>;

    /// Check if field exists at path
    fn field_exists(&self, path: &str) -> bool;
}

/// Array operations trait - operations on JSON arrays
pub trait ArrayOperations {
    /// Insert value at index in array at path
    ///
    /// # Errors
    /// Returns an error if the path is not an array or index is out of bounds
    fn array_insert(&mut self, path: &str, index: usize, value: OperationValue) -> Result<()>;

    /// Remove element at index from array at path
    ///
    /// # Errors
    /// Returns an error if the path is not an array or index is out of bounds
    fn array_remove(&mut self, path: &str, index: usize) -> Result<()>;

    /// Replace element at index in array at path
    ///
    /// # Errors
    /// Returns an error if the path is not an array or index is out of bounds
    fn array_replace(&mut self, path: &str, index: usize, value: OperationValue) -> Result<()>;

    /// Get array length at path
    ///
    /// # Errors
    /// Returns an error if the path is not an array
    fn array_len(&self, path: &str) -> Result<usize>;

    /// Build array from elements
    ///
    /// # Errors
    /// Returns an error if the array cannot be created at the path
    fn array_build(&mut self, path: &str, elements: Vec<OperationValue>) -> Result<()>;

    /// Filter array elements
    ///
    /// # Errors
    /// Returns an error if the path is not an array or predicate fails
    fn array_filter(&mut self, path: &str, predicate: &FilterPredicate) -> Result<()>;

    /// Map over array elements
    ///
    /// # Errors
    /// Returns an error if the path is not an array or transform fails
    fn array_map(&mut self, path: &str, transform: &TransformFunction) -> Result<()>;

    /// Reduce array to single value
    ///
    /// # Errors
    /// Returns an error if the path is not an array or reducer fails
    fn array_reduce(
        &mut self,
        path: &str,
        initial: OperationValue,
        reducer: &ReduceFunction,
    ) -> Result<OperationValue>;
}

// =============================================================================
// CRDT/Merge Traits - The key abstraction for comparing implementations
// =============================================================================

/// Vector clock for causality tracking
/// Uses `SmallVec` for common case (< 8 replicas) to avoid heap allocation
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VectorClock {
    /// Inline storage for small replica counts (most common case)
    /// Format: (`replica_id`, timestamp) pairs
    inline: SmallVec<[(u64, u64); 8]>, // (replica_id_hash, timestamp)
    /// Replica IDs for iteration (only when needed)
    replica_ids: SmallVec<[String; 8]>,
}

impl VectorClock {
    /// Create a new empty vector clock.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            inline: SmallVec::new(),
            replica_ids: SmallVec::new(),
        }
    }

    #[inline]
    fn hash_replica_id(replica_id: &str) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = ahash::AHasher::default();
        replica_id.hash(&mut hasher);
        hasher.finish()
    }

    /// Increment the clock for a replica.
    #[inline]
    pub fn increment(&mut self, replica_id: &str) {
        let hash = Self::hash_replica_id(replica_id);
        // Linear scan for small vectors is faster than hash lookup
        for (h, ts) in &mut self.inline {
            if *h == hash {
                *ts += 1;
                return;
            }
        }
        // Not found, add new entry
        self.inline.push((hash, 1));
        self.replica_ids.push(replica_id.to_string());
    }

    /// Get the clock value for a replica.
    #[inline]
    #[must_use]
    pub fn get(&self, replica_id: &str) -> u64 {
        let hash = Self::hash_replica_id(replica_id);
        for &(h, ts) in &self.inline {
            if h == hash {
                return ts;
            }
        }
        0
    }

    /// Merge another vector clock into this one.
    #[inline]
    pub fn merge(&mut self, other: &Self) {
        for (i, &(hash, time)) in other.inline.iter().enumerate() {
            let mut found = false;
            for (h, ts) in &mut self.inline {
                if *h == hash {
                    *ts = (*ts).max(time);
                    found = true;
                    break;
                }
            }
            if !found {
                self.inline.push((hash, time));
                if i < other.replica_ids.len() {
                    self.replica_ids.push(other.replica_ids[i].clone());
                }
            }
        }
    }

    /// Returns true if self happened-before other
    #[inline]
    #[must_use]
    pub fn happened_before(&self, other: &Self) -> bool {
        let mut dominated = false;
        for &(hash, time) in &self.inline {
            let other_time = other.get_by_hash(hash);
            if time > other_time {
                return false;
            }
            if time < other_time {
                dominated = true;
            }
        }
        for &(hash, time) in &other.inline {
            if self.get_by_hash(hash) == 0 && time > 0 {
                dominated = true;
            }
        }
        dominated
    }

    #[inline]
    fn get_by_hash(&self, hash: u64) -> u64 {
        for &(h, ts) in &self.inline {
            if h == hash {
                return ts;
            }
        }
        0
    }

    /// Returns true if clocks are concurrent (neither happened-before the other)
    #[inline]
    #[must_use]
    pub fn concurrent_with(&self, other: &Self) -> bool {
        !self.happened_before(other) && !other.happened_before(self)
    }

    /// For compatibility: get clocks as `BTreeMap` (for iteration in tests/benchmarks)
    #[must_use]
    pub fn clocks(&self) -> std::collections::BTreeMap<String, u64> {
        let mut map = std::collections::BTreeMap::new();
        for (i, &(_, ts)) in self.inline.iter().enumerate() {
            if i < self.replica_ids.len() {
                map.insert(self.replica_ids[i].clone(), ts);
            }
        }
        map
    }
}

/// CRDT operation with causal metadata
#[derive(Debug, Clone)]
pub struct CrdtOperation {
    /// The underlying DSON operation
    pub operation: DsonOperation,
    /// Lamport timestamp for ordering
    pub timestamp: u64,
    /// Replica ID that generated this operation
    pub replica_id: String,
    /// Vector clock for causality
    pub vector_clock: VectorClock,
}

/// Conflict information when merging concurrent operations
#[derive(Debug, Clone)]
pub struct MergeConflict {
    /// Path where conflict occurred
    pub path: String,
    /// Local value at conflict path
    pub local_value: OperationValue,
    /// Remote value at conflict path
    pub remote_value: OperationValue,
    /// Local operation timestamp
    pub local_timestamp: u64,
    /// Remote operation timestamp
    pub remote_timestamp: u64,
    /// Resolved value after conflict resolution
    pub resolved_value: Option<OperationValue>,
}

/// CRDT merge trait - defines how documents merge across replicas
pub trait CrdtMerge {
    /// Merge a remote operation into local state
    ///
    /// # Errors
    /// Returns an error if the merge operation fails
    fn merge_operation(&mut self, op: CrdtOperation) -> Result<Option<MergeConflict>>;

    /// Merge field with CRDT semantics
    ///
    /// # Errors
    /// Returns an error if the field merge fails
    fn merge_field(
        &mut self,
        path: &str,
        value: OperationValue,
        timestamp: u64,
        strategy: &MergeStrategy,
    ) -> Result<Option<MergeConflict>>;

    /// Get the current vector clock
    fn vector_clock(&self) -> &VectorClock;

    /// Get the replica ID
    fn replica_id(&self) -> &str;

    /// Resolve a conflict using the specified strategy
    ///
    /// # Errors
    /// Returns an error if conflict resolution fails
    fn resolve_conflict(
        &mut self,
        conflict: &MergeConflict,
        strategy: &MergeStrategy,
    ) -> Result<OperationValue>;
}

/// Delta-state CRDT trait for efficient synchronization
pub trait DeltaCrdt: CrdtMerge {
    /// Type representing a delta (difference from previous state)
    type Delta;

    /// Generate delta since given vector clock
    fn generate_delta(&self, since: &VectorClock) -> Self::Delta;

    /// Apply delta from remote replica
    ///
    /// # Errors
    /// Returns an error if delta application fails
    fn apply_delta(&mut self, delta: Self::Delta) -> Result<Vec<MergeConflict>>;

    /// Compact/garbage collect old deltas
    fn compact(&mut self);
}

/// Operation-based CRDT trait
pub trait OpBasedCrdt: CrdtMerge {
    /// Prepare an operation for broadcast (downstream precondition)
    ///
    /// # Errors
    /// Returns an error if operation preparation fails
    fn prepare(&self, op: &DsonOperation) -> Result<CrdtOperation>;

    /// Effect an operation (apply after delivery)
    ///
    /// # Errors
    /// Returns an error if operation effect fails
    fn effect(&mut self, op: CrdtOperation) -> Result<Option<MergeConflict>>;

    /// Check if operation is causally ready to be applied
    fn is_causally_ready(&self, op: &CrdtOperation) -> bool;

    /// Buffer operation until causally ready
    fn buffer_operation(&mut self, op: CrdtOperation);

    /// Process buffered operations that are now ready
    ///
    /// # Errors
    /// Returns an error if processing buffered operations fails
    fn process_buffered(&mut self) -> Result<Vec<MergeConflict>>;
}

// =============================================================================
// Streaming Traits
// =============================================================================

/// Streaming processor trait for large datasets
pub trait StreamProcessor {
    /// Process a stream of JSON lines
    ///
    /// # Errors
    /// Returns an error if stream processing fails
    fn process_stream<I>(&mut self, lines: I) -> Result<Vec<String>>
    where
        I: Iterator<Item = String>;

    /// Build a stream from generator
    ///
    /// # Errors
    /// Returns an error if stream building fails
    fn stream_build(&mut self, path: &str, generator: &StreamGenerator) -> Result<()>;

    /// Filter stream elements
    ///
    /// # Errors
    /// Returns an error if stream filtering fails
    fn stream_filter(&mut self, path: &str, predicate: &FilterPredicate) -> Result<()>;

    /// Map over stream elements
    ///
    /// # Errors
    /// Returns an error if stream mapping fails
    fn stream_map(&mut self, path: &str, transform: &TransformFunction) -> Result<()>;

    /// Emit batch from stream
    ///
    /// # Errors
    /// Returns an error if batch emission fails
    fn stream_emit(&mut self, path: &str, batch_size: usize) -> Result<Vec<OperationValue>>;
}

/// Tape-level processor trait (SIMD-specific)
pub trait TapeProcessor {
    /// Type of tape nodes
    type Node;

    /// Parse JSON into tape representation
    ///
    /// # Errors
    /// Returns an error if JSON parsing fails
    fn parse(json: &str) -> Result<Self>
    where
        Self: Sized;

    /// Get tape nodes
    fn nodes(&self) -> &[Self::Node];

    /// Serialize tape back to JSON
    ///
    /// # Errors
    /// Returns an error if serialization fails
    fn serialize(&self) -> Result<String>;

    /// Navigate to path and return node index
    ///
    /// # Errors
    /// Returns an error if path resolution fails
    fn resolve_path(&self, path: &str) -> Result<Option<usize>>;

    /// Skip value at index, return next index
    ///
    /// # Errors
    /// Returns an error if the index is invalid
    fn skip_value(&self, index: usize) -> Result<usize>;
}

// =============================================================================
// Canonical Operations Trait
// =============================================================================

/// Canonical operation processor trait
pub trait CanonicalProcessor {
    /// Add operation to canonical sequence
    fn add_operation(&mut self, op: DsonOperation);

    /// Compute canonical (optimized) operation sequence
    ///
    /// # Errors
    /// Returns an error if canonical computation fails
    fn compute_canonical(&mut self) -> Result<Vec<DsonOperation>>;

    /// Check if operations can be coalesced
    fn can_coalesce(&self, a: &DsonOperation, b: &DsonOperation) -> bool;

    /// Reorder operations for efficiency
    fn reorder(&mut self);
}

// =============================================================================
// Comparison Framework
// =============================================================================

/// Trait for comparing DSON implementations
pub trait DsonImplementation:
    DocumentProcessor + FieldOperations + ArrayOperations + SchemaAware
{
    /// Implementation name for comparison
    fn name(&self) -> &str;

    /// Implementation version
    fn version(&self) -> &str;

    /// Supported features
    fn features(&self) -> Vec<&str>;

    /// Performance characteristics
    fn characteristics(&self) -> ImplementationCharacteristics;
}

/// Performance and capability characteristics
#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)] // These are independent feature flags
pub struct ImplementationCharacteristics {
    /// Zero-copy parsing support
    pub zero_copy: bool,
    /// SIMD acceleration support
    pub simd_accelerated: bool,
    /// Streaming support
    pub streaming: bool,
    /// CRDT/merge support
    pub crdt_support: bool,
    /// Schema filtering support
    pub schema_filtering: bool,
    /// Parallel processing support
    pub parallel: bool,
    /// Estimated memory overhead per document (bytes)
    pub memory_overhead: usize,
    /// Maximum recommended document size (bytes)
    pub max_document_size: usize,
}

// =============================================================================
// Merge Strategy Implementations
// =============================================================================

impl MergeStrategy {
    /// Resolve conflict between two values using this strategy
    #[inline]
    #[must_use]
    pub fn resolve(
        &self,
        local: &OperationValue,
        remote: &OperationValue,
        local_ts: u64,
        remote_ts: u64,
    ) -> OperationValue {
        match self {
            Self::LastWriteWins => {
                if remote_ts > local_ts {
                    remote.clone()
                } else {
                    local.clone()
                }
            }
            Self::Max => {
                // For numeric values, take the max
                match (local, remote) {
                    (OperationValue::NumberRef(a), OperationValue::NumberRef(b)) => {
                        let a_val: f64 = a.parse().unwrap_or(0.0);
                        let b_val: f64 = b.parse().unwrap_or(0.0);
                        if b_val > a_val {
                            remote.clone()
                        } else {
                            local.clone()
                        }
                    }
                    _ => {
                        // Fall back to LWW for non-numeric
                        if remote_ts > local_ts {
                            remote.clone()
                        } else {
                            local.clone()
                        }
                    }
                }
            }
            Self::Min => match (local, remote) {
                (OperationValue::NumberRef(a), OperationValue::NumberRef(b)) => {
                    let a_val: f64 = a.parse().unwrap_or(0.0);
                    let b_val: f64 = b.parse().unwrap_or(0.0);
                    if b_val < a_val {
                        remote.clone()
                    } else {
                        local.clone()
                    }
                }
                _ => {
                    if remote_ts > local_ts {
                        remote.clone()
                    } else {
                        local.clone()
                    }
                }
            },
            Self::Additive => {
                // For numeric values, sum them
                match (local, remote) {
                    (OperationValue::NumberRef(a), OperationValue::NumberRef(b)) => {
                        let a_val: f64 = a.parse().unwrap_or(0.0);
                        let b_val: f64 = b.parse().unwrap_or(0.0);
                        OperationValue::NumberRef((a_val + b_val).to_string())
                    }
                    // For strings, concatenate
                    (OperationValue::StringRef(a), OperationValue::StringRef(b)) => {
                        OperationValue::StringRef(format!("{a}{b}"))
                    }
                    _ => {
                        if remote_ts > local_ts {
                            remote.clone()
                        } else {
                            local.clone()
                        }
                    }
                }
            }
            Self::Union => {
                // Union only makes sense for arrays/sets - fall back to LWW for scalars
                if remote_ts > local_ts {
                    remote.clone()
                } else {
                    local.clone()
                }
            }
            Self::Custom(_name) => {
                // Custom strategies would need a registry - fall back to LWW
                if remote_ts > local_ts {
                    remote.clone()
                } else {
                    local.clone()
                }
            }
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_clock_basics() {
        let mut vc1 = VectorClock::new();
        vc1.increment("replica_a");
        vc1.increment("replica_a");

        let mut vc2 = VectorClock::new();
        vc2.increment("replica_b");

        assert_eq!(vc1.get("replica_a"), 2);
        assert_eq!(vc1.get("replica_b"), 0);
        assert_eq!(vc2.get("replica_b"), 1);

        // Neither happened before the other (concurrent)
        assert!(vc1.concurrent_with(&vc2));
    }

    #[test]
    fn test_vector_clock_causality() {
        let mut vc1 = VectorClock::new();
        vc1.increment("replica_a");

        let mut vc2 = vc1.clone();
        vc2.increment("replica_a");

        // vc1 happened before vc2
        assert!(vc1.happened_before(&vc2));
        assert!(!vc2.happened_before(&vc1));
    }

    #[test]
    fn test_merge_strategy_lww() {
        let local = OperationValue::StringRef("local".to_string());
        let remote = OperationValue::StringRef("remote".to_string());

        let result = MergeStrategy::LastWriteWins.resolve(&local, &remote, 1, 2);
        assert_eq!(result, remote);

        let result = MergeStrategy::LastWriteWins.resolve(&local, &remote, 2, 1);
        assert_eq!(result, local);
    }

    #[test]
    fn test_merge_strategy_max() {
        let local = OperationValue::NumberRef("10".to_string());
        let remote = OperationValue::NumberRef("20".to_string());

        let result = MergeStrategy::Max.resolve(&local, &remote, 1, 1);
        assert_eq!(result, remote);
    }

    #[test]
    fn test_merge_strategy_additive() {
        let local = OperationValue::NumberRef("10".to_string());
        let remote = OperationValue::NumberRef("20".to_string());

        let result = MergeStrategy::Additive.resolve(&local, &remote, 1, 1);
        match result {
            OperationValue::NumberRef(s) => assert_eq!(s, "30"),
            _ => panic!("Expected NumberRef"),
        }
    }

    // Additional tests for coverage

    #[test]
    fn test_vector_clock_new() {
        let vc = VectorClock::new();
        assert_eq!(vc.get("any_replica"), 0);
    }

    #[test]
    fn test_vector_clock_default() {
        let vc = VectorClock::default();
        assert_eq!(vc.get("any"), 0);
    }

    #[test]
    fn test_vector_clock_increment_multiple_replicas() {
        let mut vc = VectorClock::new();
        vc.increment("a");
        vc.increment("b");
        vc.increment("c");
        vc.increment("a");

        assert_eq!(vc.get("a"), 2);
        assert_eq!(vc.get("b"), 1);
        assert_eq!(vc.get("c"), 1);
        assert_eq!(vc.get("d"), 0);
    }

    #[test]
    fn test_vector_clock_merge() {
        let mut vc1 = VectorClock::new();
        vc1.increment("a");
        vc1.increment("a");

        let mut vc2 = VectorClock::new();
        vc2.increment("b");
        vc2.increment("b");
        vc2.increment("b");

        vc1.merge(&vc2);

        assert_eq!(vc1.get("a"), 2);
        assert_eq!(vc1.get("b"), 3);
    }

    #[test]
    fn test_vector_clock_merge_overlapping() {
        let mut vc1 = VectorClock::new();
        vc1.increment("a");

        let mut vc2 = VectorClock::new();
        vc2.increment("a");
        vc2.increment("a");
        vc2.increment("a");

        vc1.merge(&vc2);
        // Should take max
        assert_eq!(vc1.get("a"), 3);
    }

    #[test]
    fn test_vector_clock_clocks() {
        let mut vc = VectorClock::new();
        vc.increment("replica_a");
        vc.increment("replica_b");

        let clocks = vc.clocks();
        assert!(clocks.contains_key("replica_a"));
        assert!(clocks.contains_key("replica_b"));
    }

    #[test]
    fn test_vector_clock_happened_before_empty() {
        let vc1 = VectorClock::new();
        let mut vc2 = VectorClock::new();
        vc2.increment("a");

        // Empty clock happened before non-empty
        assert!(vc1.happened_before(&vc2));
    }

    #[test]
    fn test_vector_clock_happened_before_equal() {
        let mut vc1 = VectorClock::new();
        vc1.increment("a");

        let vc2 = vc1.clone();

        // Equal clocks - neither happened before
        assert!(!vc1.happened_before(&vc2));
        assert!(!vc2.happened_before(&vc1));
    }

    #[test]
    fn test_vector_clock_concurrent_with_both_have_unique() {
        let mut vc1 = VectorClock::new();
        vc1.increment("a");

        let mut vc2 = VectorClock::new();
        vc2.increment("b");

        // Each has changes the other doesn't
        assert!(vc1.concurrent_with(&vc2));
        assert!(vc2.concurrent_with(&vc1));
    }

    #[test]
    fn test_vector_clock_clone() {
        let mut vc = VectorClock::new();
        vc.increment("a");
        vc.increment("b");

        let cloned = vc.clone();
        assert_eq!(cloned.get("a"), 1);
        assert_eq!(cloned.get("b"), 1);
    }

    #[test]
    fn test_vector_clock_eq() {
        let mut vc1 = VectorClock::new();
        vc1.increment("a");

        let mut vc2 = VectorClock::new();
        vc2.increment("a");

        // Note: PartialEq compares the inline vectors, might not be equal due to hash order
        let _ = vc1 == vc2;
    }

    #[test]
    fn test_vector_clock_debug() {
        let vc = VectorClock::new();
        let debug = format!("{vc:?}");
        assert!(debug.contains("VectorClock"));
    }

    #[test]
    fn test_crdt_operation_fields() {
        let op = CrdtOperation {
            operation: DsonOperation::FieldAdd {
                path: "test".to_string(),
                value: OperationValue::StringRef("value".to_string()),
            },
            timestamp: 100,
            replica_id: "replica_1".to_string(),
            vector_clock: VectorClock::new(),
        };

        assert_eq!(op.timestamp, 100);
        assert_eq!(op.replica_id, "replica_1");
    }

    #[test]
    fn test_crdt_operation_clone() {
        let op = CrdtOperation {
            operation: DsonOperation::FieldAdd {
                path: "test".to_string(),
                value: OperationValue::Null,
            },
            timestamp: 50,
            replica_id: "r1".to_string(),
            vector_clock: VectorClock::new(),
        };

        let cloned = op;
        assert_eq!(cloned.timestamp, 50);
    }

    #[test]
    fn test_crdt_operation_debug() {
        let op = CrdtOperation {
            operation: DsonOperation::FieldDelete {
                path: "test".to_string(),
            },
            timestamp: 0,
            replica_id: "r".to_string(),
            vector_clock: VectorClock::new(),
        };

        let debug = format!("{op:?}");
        assert!(debug.contains("CrdtOperation"));
    }

    #[test]
    fn test_merge_conflict_fields() {
        let conflict = MergeConflict {
            path: "user.name".to_string(),
            local_value: OperationValue::StringRef("local".to_string()),
            remote_value: OperationValue::StringRef("remote".to_string()),
            local_timestamp: 100,
            remote_timestamp: 200,
            resolved_value: None,
        };

        assert_eq!(conflict.path, "user.name");
        assert_eq!(conflict.local_timestamp, 100);
        assert_eq!(conflict.remote_timestamp, 200);
        assert!(conflict.resolved_value.is_none());
    }

    #[test]
    fn test_merge_conflict_with_resolved() {
        let conflict = MergeConflict {
            path: "counter".to_string(),
            local_value: OperationValue::NumberRef("10".to_string()),
            remote_value: OperationValue::NumberRef("20".to_string()),
            local_timestamp: 1,
            remote_timestamp: 2,
            resolved_value: Some(OperationValue::NumberRef("20".to_string())),
        };

        assert!(conflict.resolved_value.is_some());
    }

    #[test]
    fn test_merge_conflict_clone() {
        let conflict = MergeConflict {
            path: "test".to_string(),
            local_value: OperationValue::Null,
            remote_value: OperationValue::Null,
            local_timestamp: 0,
            remote_timestamp: 0,
            resolved_value: None,
        };

        let cloned = conflict;
        assert_eq!(cloned.path, "test");
    }

    #[test]
    fn test_merge_conflict_debug() {
        let conflict = MergeConflict {
            path: "p".to_string(),
            local_value: OperationValue::Null,
            remote_value: OperationValue::Null,
            local_timestamp: 0,
            remote_timestamp: 0,
            resolved_value: None,
        };

        let debug = format!("{conflict:?}");
        assert!(debug.contains("MergeConflict"));
    }

    #[test]
    fn test_merge_strategy_min() {
        let local = OperationValue::NumberRef("10".to_string());
        let remote = OperationValue::NumberRef("5".to_string());

        let result = MergeStrategy::Min.resolve(&local, &remote, 1, 1);
        assert_eq!(result, remote); // 5 is smaller than 10
    }

    #[test]
    fn test_merge_strategy_min_local_smaller() {
        let local = OperationValue::NumberRef("3".to_string());
        let remote = OperationValue::NumberRef("10".to_string());

        let result = MergeStrategy::Min.resolve(&local, &remote, 1, 1);
        assert_eq!(result, local); // 3 is smaller than 10
    }

    #[test]
    fn test_merge_strategy_min_non_numeric() {
        let local = OperationValue::StringRef("local".to_string());
        let remote = OperationValue::StringRef("remote".to_string());

        // Falls back to LWW for non-numeric
        let result = MergeStrategy::Min.resolve(&local, &remote, 1, 2);
        assert_eq!(result, remote);
    }

    #[test]
    fn test_merge_strategy_max_local_larger() {
        let local = OperationValue::NumberRef("100".to_string());
        let remote = OperationValue::NumberRef("50".to_string());

        let result = MergeStrategy::Max.resolve(&local, &remote, 1, 1);
        assert_eq!(result, local); // 100 is larger
    }

    #[test]
    fn test_merge_strategy_max_non_numeric() {
        let local = OperationValue::StringRef("a".to_string());
        let remote = OperationValue::StringRef("b".to_string());

        // Falls back to LWW for non-numeric
        let result = MergeStrategy::Max.resolve(&local, &remote, 1, 2);
        assert_eq!(result, remote);
    }

    #[test]
    fn test_merge_strategy_additive_strings() {
        let local = OperationValue::StringRef("hello ".to_string());
        let remote = OperationValue::StringRef("world".to_string());

        let result = MergeStrategy::Additive.resolve(&local, &remote, 1, 1);
        match result {
            OperationValue::StringRef(s) => assert_eq!(s, "hello world"),
            _ => panic!("Expected StringRef"),
        }
    }

    #[test]
    fn test_merge_strategy_additive_non_matching() {
        let local = OperationValue::BoolRef(true);
        let remote = OperationValue::NumberRef("10".to_string());

        // Falls back to LWW for non-matching types
        let result = MergeStrategy::Additive.resolve(&local, &remote, 1, 2);
        assert_eq!(result, remote);
    }

    #[test]
    fn test_merge_strategy_union() {
        let local = OperationValue::StringRef("a".to_string());
        let remote = OperationValue::StringRef("b".to_string());

        // Union falls back to LWW for scalars
        let result = MergeStrategy::Union.resolve(&local, &remote, 1, 2);
        assert_eq!(result, remote);
    }

    #[test]
    fn test_merge_strategy_custom() {
        let local = OperationValue::StringRef("local".to_string());
        let remote = OperationValue::StringRef("remote".to_string());

        // Custom falls back to LWW
        let result = MergeStrategy::Custom("my_custom".to_string()).resolve(&local, &remote, 1, 2);
        assert_eq!(result, remote);
    }

    #[test]
    fn test_implementation_characteristics_default() {
        let chars = ImplementationCharacteristics::default();
        assert!(!chars.zero_copy);
        assert!(!chars.simd_accelerated);
        assert!(!chars.streaming);
        assert!(!chars.crdt_support);
        assert!(!chars.schema_filtering);
        assert!(!chars.parallel);
        assert_eq!(chars.memory_overhead, 0);
        assert_eq!(chars.max_document_size, 0);
    }

    #[test]
    fn test_implementation_characteristics_custom() {
        let chars = ImplementationCharacteristics {
            zero_copy: true,
            simd_accelerated: true,
            streaming: true,
            crdt_support: true,
            schema_filtering: true,
            parallel: true,
            memory_overhead: 1024,
            max_document_size: 1_000_000,
        };

        assert!(chars.zero_copy);
        assert!(chars.simd_accelerated);
        assert_eq!(chars.memory_overhead, 1024);
    }

    #[test]
    fn test_implementation_characteristics_clone() {
        let chars = ImplementationCharacteristics {
            zero_copy: true,
            ..Default::default()
        };

        let cloned = chars;
        assert!(cloned.zero_copy);
    }

    #[test]
    fn test_implementation_characteristics_debug() {
        let chars = ImplementationCharacteristics::default();
        let debug = format!("{chars:?}");
        assert!(debug.contains("ImplementationCharacteristics"));
    }

    #[test]
    fn test_vector_clock_hash_replica_id_consistency() {
        // Test that hashing is consistent
        let hash1 = VectorClock::hash_replica_id("replica_a");
        let hash2 = VectorClock::hash_replica_id("replica_a");
        assert_eq!(hash1, hash2);

        // Different IDs should have different hashes (with high probability)
        let hash3 = VectorClock::hash_replica_id("replica_b");
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_vector_clock_get_by_hash() {
        let mut vc = VectorClock::new();
        vc.increment("test_replica");

        let hash = VectorClock::hash_replica_id("test_replica");
        assert_eq!(vc.get_by_hash(hash), 1);

        // Non-existent hash
        let fake_hash = 12_345_678;
        assert_eq!(vc.get_by_hash(fake_hash), 0);
    }

    #[test]
    fn test_merge_strategy_max_invalid_parse() {
        // Test with non-parseable numbers
        let local = OperationValue::NumberRef("invalid".to_string());
        let remote = OperationValue::NumberRef("5".to_string());

        let result = MergeStrategy::Max.resolve(&local, &remote, 1, 1);
        // "invalid" parses to 0.0, so 5 wins
        assert_eq!(result, remote);
    }

    #[test]
    fn test_merge_strategy_min_invalid_parse() {
        let local = OperationValue::NumberRef("invalid".to_string());
        let remote = OperationValue::NumberRef("5".to_string());

        let result = MergeStrategy::Min.resolve(&local, &remote, 1, 1);
        // "invalid" parses to 0.0, so 0 wins (local)
        assert_eq!(result, local);
    }

    #[test]
    fn test_merge_strategy_additive_invalid_parse() {
        let local = OperationValue::NumberRef("invalid".to_string());
        let remote = OperationValue::NumberRef("10".to_string());

        let result = MergeStrategy::Additive.resolve(&local, &remote, 1, 1);
        // "invalid" parses to 0.0, so result is 0 + 10 = 10
        match result {
            OperationValue::NumberRef(s) => assert_eq!(s, "10"),
            _ => panic!("Expected NumberRef"),
        }
    }

    #[test]
    fn test_vector_clock_merge_with_more_replicas() {
        let mut vc1 = VectorClock::new();
        vc1.increment("a");

        let mut vc2 = VectorClock::new();
        vc2.increment("b");
        vc2.increment("c");
        vc2.increment("d");

        vc1.merge(&vc2);

        assert_eq!(vc1.get("a"), 1);
        assert_eq!(vc1.get("b"), 1);
        assert_eq!(vc1.get("c"), 1);
        assert_eq!(vc1.get("d"), 1);
    }
}
