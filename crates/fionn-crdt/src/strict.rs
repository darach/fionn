// SPDX-License-Identifier: MIT OR Apache-2.0
//! Strict DSON Processor
//!
//! Implements full Rinberg et al. (PVLDB 2022) compliance using dot-level
//! observed-remove semantics, identity-preserving arrays (RGA), and lattice
//! join for convergence.
//!
//! This is a **separate** state representation from the weak-mode
//! `SimdDsonProcessor` — they are not interchangeable at runtime.

use std::collections::HashMap;

use fionn_core::{DsonOperation, OperationValue, Result};

use super::dot_observed_remove::DotObservedRemoveMap;
use super::dot_store::{CausalContext, Dot};
use super::lattice::Lattice;
use super::rga::{ElementId, Rga};

/// Delta state for efficient synchronisation.
#[derive(Debug, Clone)]
pub struct StrictDelta {
    /// Object field changes since the snapshot.
    pub fields: DotObservedRemoveMap,
    /// Array changes since the snapshot (per-path RGA states).
    pub arrays: HashMap<String, Rga>,
    /// Causal context at the time the delta was generated.
    pub context: CausalContext,
}

/// Strict DSON processor state.
///
/// Holds the full CRDT state for Strict mode: dot-level OR map for object
/// fields, per-path RGA for arrays, and a global causal context.
#[derive(Debug, Clone)]
pub struct StrictDsonProcessor {
    /// Replica id for this processor.
    replica_id: u64,
    /// Next sequence number for dot generation.
    next_seq: u64,
    /// Object fields: dot-level observed-remove map.
    fields: DotObservedRemoveMap,
    /// Arrays keyed by their JSON path, each an RGA.
    arrays: HashMap<String, Rga>,
    /// Global causal context.
    context: CausalContext,
}

impl StrictDsonProcessor {
    /// Create a new strict processor for the given replica.
    #[must_use]
    pub fn new(replica_id: u64) -> Self {
        Self {
            replica_id,
            next_seq: 1,
            fields: DotObservedRemoveMap::new(),
            arrays: HashMap::new(),
            context: CausalContext::new(),
        }
    }

    /// Get the replica id.
    #[must_use]
    pub const fn replica_id(&self) -> u64 {
        self.replica_id
    }

    /// Generate the next dot and advance the sequence counter.
    fn next_dot(&mut self) -> Dot {
        let dot = Dot::new(self.replica_id, self.next_seq);
        self.next_seq += 1;
        self.context.observe(dot);
        dot
    }

    /// Apply a single DSON operation with an explicit dot.
    ///
    /// Use this when replaying operations from a remote replica.
    pub fn apply_op_with_dot(&mut self, op: &DsonOperation, dot: Dot) {
        self.context.observe(dot);
        match op {
            DsonOperation::FieldAdd { path, value }
            | DsonOperation::FieldModify {
                path,
                new_value: value,
                ..
            } => {
                self.fields.add(path, value.clone(), dot);
            }
            DsonOperation::FieldDelete { path } => {
                self.fields.remove(path);
            }
            DsonOperation::ArrayStart { path } => {
                self.arrays
                    .entry(path.clone())
                    .or_insert_with(|| Rga::new(self.replica_id));
            }
            DsonOperation::BatchExecute { operations } => {
                for sub_op in operations {
                    self.apply_op_with_dot(sub_op, dot);
                }
            }
            // ObjectStart/ObjectEnd/ArrayEnd are structural markers — no CRDT state change
            DsonOperation::ObjectStart { .. }
            | DsonOperation::ObjectEnd { .. }
            | DsonOperation::ArrayEnd { .. } => {}
        }
    }

    /// Apply a DSON operation, generating a new dot for it.
    ///
    /// Returns the dot assigned to this operation.
    pub fn apply_op(&mut self, op: &DsonOperation) -> Dot {
        let dot = self.next_dot();
        self.apply_op_with_dot(op, dot);
        dot
    }

    /// Set a field value (convenience wrapper).
    pub fn set(&mut self, path: &str, value: OperationValue) -> Dot {
        self.apply_op(&DsonOperation::FieldAdd {
            path: path.to_string(),
            value,
        })
    }

    /// Delete a field (convenience wrapper).
    pub fn delete(&mut self, path: &str) -> Dot {
        self.apply_op(&DsonOperation::FieldDelete {
            path: path.to_string(),
        })
    }

    /// Get the value at a path (single deterministic winner if conflicted).
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&OperationValue> {
        self.fields.get_one(path)
    }

    /// Insert into an array at the given path, after the specified parent element.
    ///
    /// Returns the new `ElementId`.
    pub fn array_insert(
        &mut self,
        path: &str,
        parent: Option<ElementId>,
        value: OperationValue,
    ) -> ElementId {
        let rga = self
            .arrays
            .entry(path.to_string())
            .or_insert_with(|| Rga::new(self.replica_id));
        rga.insert_after(parent, value)
    }

    /// Delete an array element.
    pub fn array_delete(&mut self, path: &str, id: ElementId) -> bool {
        self.arrays.get_mut(path).is_some_and(|rga| rga.delete(id))
    }

    /// Get array contents at a path.
    #[must_use]
    pub fn array_get(&self, path: &str) -> Vec<&OperationValue> {
        self.arrays.get(path).map(Rga::to_vec).unwrap_or_default()
    }

    /// Generate a delta of changes since the given causal context snapshot.
    ///
    /// The returned delta contains all state that the receiver (who has `since`)
    /// has not yet seen.
    #[must_use]
    pub fn generate_delta(&self, _since: &CausalContext) -> StrictDelta {
        // For now, return the full state as delta (a correct but non-optimal
        // implementation; an optimised version would filter to only unseen dots).
        StrictDelta {
            fields: self.fields.clone(),
            arrays: self.arrays.clone(),
            context: self.context.clone(),
        }
    }

    /// Apply a delta from a remote replica.
    ///
    /// # Errors
    ///
    /// Returns an error if the delta cannot be applied.
    pub fn apply_delta(&mut self, delta: &StrictDelta) -> Result<()> {
        self.fields.join(&delta.fields);
        for (path, rga) in &delta.arrays {
            self.arrays
                .entry(path.clone())
                .or_insert_with(|| Rga::new(self.replica_id))
                .merge(rga);
        }
        self.context.merge(delta.context.clone());
        Ok(())
    }

    /// Produce a JSON value representing the current document state.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let mut root = serde_json::Map::new();
        for (path, value) in self.fields.iter() {
            let json_val = op_value_to_json(value);
            root.insert(path.to_string(), json_val);
        }
        for (path, rga) in &self.arrays {
            let arr: Vec<serde_json::Value> =
                rga.to_vec().into_iter().map(op_value_to_json).collect();
            root.insert(path.clone(), serde_json::Value::Array(arr));
        }
        serde_json::Value::Object(root)
    }

    /// Get a reference to the global causal context.
    #[must_use]
    pub const fn causal_context(&self) -> &CausalContext {
        &self.context
    }
}

impl Lattice for StrictDsonProcessor {
    fn join(&mut self, other: &Self) {
        self.fields.join(&other.fields);
        for (path, rga) in &other.arrays {
            self.arrays
                .entry(path.clone())
                .or_insert_with(|| Rga::new(self.replica_id))
                .merge(rga);
        }
        self.context.merge(other.context.clone());
        // Advance next_seq so we don't collide
        if other.replica_id == self.replica_id && other.next_seq > self.next_seq {
            self.next_seq = other.next_seq;
        }
    }

    fn partial_le(&self, other: &Self) -> bool {
        self.fields.partial_le(&other.fields) && self.context.happened_before(&other.context)
    }
}

/// Convert an `OperationValue` to a `serde_json::Value`.
fn op_value_to_json(v: &OperationValue) -> serde_json::Value {
    match v {
        OperationValue::StringRef(s) => serde_json::Value::String(s.clone()),
        OperationValue::NumberRef(n) => n.parse::<f64>().map_or_else(
            |_| serde_json::Value::String(n.clone()),
            |f| {
                serde_json::Value::Number(
                    serde_json::Number::from_f64(f).unwrap_or_else(|| serde_json::Number::from(0)),
                )
            },
        ),
        OperationValue::BoolRef(b) => serde_json::Value::Bool(*b),
        OperationValue::Null => serde_json::Value::Null,
        OperationValue::ObjectRef { .. } | OperationValue::ArrayRef { .. } => {
            serde_json::Value::Null
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_set_get() {
        let mut proc = StrictDsonProcessor::new(1);
        proc.set("name", OperationValue::StringRef("Alice".into()));
        assert_eq!(
            proc.get("name"),
            Some(&OperationValue::StringRef("Alice".into()))
        );
    }

    #[test]
    fn test_delete() {
        let mut proc = StrictDsonProcessor::new(1);
        proc.set("x", OperationValue::Null);
        assert!(proc.get("x").is_some());
        proc.delete("x");
        assert!(proc.get("x").is_none());
    }

    #[test]
    fn test_merge_convergence() {
        let mut r1 = StrictDsonProcessor::new(1);
        let mut r2 = StrictDsonProcessor::new(2);

        r1.set("a", OperationValue::StringRef("r1".into()));
        r2.set("b", OperationValue::StringRef("r2".into()));

        let mut m1 = r1.clone();
        m1.join(&r2);

        let mut m2 = r2.clone();
        m2.join(&r1);

        assert_eq!(m1.to_json(), m2.to_json());
    }

    #[test]
    fn test_delta_apply() {
        let mut r1 = StrictDsonProcessor::new(1);
        r1.set("k", OperationValue::StringRef("v".into()));

        let empty_ctx = CausalContext::new();
        let delta = r1.generate_delta(&empty_ctx);

        let mut r2 = StrictDsonProcessor::new(2);
        r2.apply_delta(&delta).unwrap();
        assert_eq!(r2.get("k"), Some(&OperationValue::StringRef("v".into())));
    }

    #[test]
    fn test_to_json() {
        let mut proc = StrictDsonProcessor::new(1);
        proc.set("name", OperationValue::StringRef("Bob".into()));
        proc.set("age", OperationValue::NumberRef("30".into()));

        let json = proc.to_json();
        assert_eq!(json["name"], "Bob");
    }

    #[test]
    fn test_array_operations() {
        let mut proc = StrictDsonProcessor::new(1);
        let id1 = proc.array_insert("arr", None, OperationValue::StringRef("a".into()));
        let _id2 = proc.array_insert("arr", Some(id1), OperationValue::StringRef("b".into()));

        let values = proc.array_get("arr");
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn test_array_delete() {
        let mut proc = StrictDsonProcessor::new(1);
        let id = proc.array_insert("arr", None, OperationValue::StringRef("x".into()));
        assert!(proc.array_delete("arr", id));
        assert!(proc.array_get("arr").is_empty());
    }
}
