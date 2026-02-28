// SPDX-License-Identifier: MIT OR Apache-2.0
//! Replicated Growable Array (RGA) for Strict DSON mode
//!
//! Provides identity-preserving array semantics where each element has a unique
//! [`ElementId`]. Concurrent inserts at the same position are deterministically
//! ordered by `ElementId`, and deletes use tombstones so that positions are stable.

use fionn_core::OperationValue;

use super::lattice::Lattice;

/// Unique identifier for an array element, never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ElementId {
    /// Replica that created this element
    pub replica_id: u64,
    /// Monotonically increasing sequence within the replica
    pub sequence: u64,
}

impl ElementId {
    /// Create a new element id.
    #[must_use]
    pub const fn new(replica_id: u64, sequence: u64) -> Self {
        Self {
            replica_id,
            sequence,
        }
    }
}

/// A node in the RGA linked list.
#[derive(Debug, Clone)]
pub struct RgaNode {
    /// Unique identity of this element
    pub id: ElementId,
    /// `None` means tombstoned (deleted)
    pub value: Option<OperationValue>,
    /// Parent element (the element this was inserted after). `None` = head.
    pub parent: Option<ElementId>,
}

/// Replicated Growable Array — identity-preserving ordered sequence CRDT.
#[derive(Debug, Clone)]
pub struct Rga {
    /// The flat list of nodes (including tombstones), in document order.
    nodes: Vec<RgaNode>,
    /// Next sequence number for this replica.
    next_seq: u64,
    /// Replica id for generating new elements.
    replica_id: u64,
}

impl Rga {
    /// Create a new empty RGA for the given replica.
    #[must_use]
    pub const fn new(replica_id: u64) -> Self {
        Self {
            nodes: Vec::new(),
            next_seq: 1,
            replica_id,
        }
    }

    /// Insert a value after the given parent element. Returns the new element's id.
    ///
    /// Pass `None` for `parent` to insert at the head of the array.
    pub fn insert_after(&mut self, parent: Option<ElementId>, value: OperationValue) -> ElementId {
        let id = ElementId::new(self.replica_id, self.next_seq);
        self.next_seq += 1;

        let node = RgaNode {
            id,
            value: Some(value),
            parent,
        };

        let insert_pos = self.find_insert_position(parent, id);
        self.nodes.insert(insert_pos, node);
        id
    }

    /// Tombstone-delete the element with the given id.
    ///
    /// Returns `true` if the element was found and deleted, `false` if not found
    /// or already deleted.
    pub fn delete(&mut self, id: ElementId) -> bool {
        for node in &mut self.nodes {
            if node.id == id && node.value.is_some() {
                node.value = None;
                return true;
            }
        }
        false
    }

    /// Return the live (non-tombstoned) values in document order.
    #[must_use]
    pub fn to_vec(&self) -> Vec<&OperationValue> {
        self.nodes.iter().filter_map(|n| n.value.as_ref()).collect()
    }

    /// Number of live elements.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.iter().filter(|n| n.value.is_some()).count()
    }

    /// Check if the array has no live elements.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Merge another RGA into this one.
    ///
    /// Nodes from `other` that are not present in `self` are inserted at the correct
    /// position. If both contain the same `ElementId`, the tombstone wins (delete
    /// propagates).
    pub fn merge(&mut self, other: &Self) {
        for other_node in &other.nodes {
            if let Some(existing) = self.nodes.iter_mut().find(|n| n.id == other_node.id) {
                // If either side tombstoned, propagate the tombstone
                if other_node.value.is_none() {
                    existing.value = None;
                }
            } else {
                // Insert at the correct position respecting parent + id ordering
                let pos = self.find_insert_position(other_node.parent, other_node.id);
                self.nodes.insert(pos, other_node.clone());
            }
        }
        // Update next_seq to avoid id collisions on future inserts
        for node in &other.nodes {
            if node.id.replica_id == self.replica_id && node.id.sequence >= self.next_seq {
                self.next_seq = node.id.sequence + 1;
            }
        }
    }

    /// Find the correct insertion index for a new node with the given parent and id.
    ///
    /// Strategy: find the parent, then scan right past any existing children that have
    /// a *larger* `ElementId` (higher priority = inserted first).
    fn find_insert_position(&self, parent: Option<ElementId>, new_id: ElementId) -> usize {
        let start = parent.map_or(0, |pid| {
            self.nodes
                .iter()
                .position(|n| n.id == pid)
                .map_or(self.nodes.len(), |i| i + 1)
        });

        // Skip past siblings with larger ElementId (they sort before us)
        let mut pos = start;
        while pos < self.nodes.len() {
            let n = &self.nodes[pos];
            // Stop if this node has a different parent or a smaller id
            if n.parent != parent || n.id < new_id {
                break;
            }
            pos += 1;
        }
        pos
    }
}

impl Lattice for Rga {
    fn join(&mut self, other: &Self) {
        self.merge(other);
    }

    fn partial_le(&self, other: &Self) -> bool {
        // self ≤ other iff every node in self exists in other with same or more deleted state
        self.nodes.iter().all(|self_node| {
            other.nodes.iter().any(|other_node| {
                #[allow(clippy::suspicious_operation_groupings)]
                // Correct: if self is live (is_some via !is_none), other must also be live
                {
                    other_node.id == self_node.id
                        && (self_node.value.is_none() || other_node.value.is_some())
                }
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_read() {
        let mut rga = Rga::new(1);
        rga.insert_after(None, OperationValue::StringRef("a".into()));
        rga.insert_after(None, OperationValue::StringRef("b".into()));

        assert_eq!(rga.len(), 2);
        assert!(!rga.is_empty());
    }

    #[test]
    fn test_delete() {
        let mut rga = Rga::new(1);
        let id = rga.insert_after(None, OperationValue::StringRef("a".into()));
        assert_eq!(rga.len(), 1);

        assert!(rga.delete(id));
        assert_eq!(rga.len(), 0);
        assert!(rga.is_empty());

        // Double delete returns false
        assert!(!rga.delete(id));
    }

    #[test]
    fn test_ordered_insert() {
        let mut rga = Rga::new(1);
        let a = rga.insert_after(None, OperationValue::StringRef("a".into()));
        let _b = rga.insert_after(Some(a), OperationValue::StringRef("b".into()));
        let _c = rga.insert_after(Some(a), OperationValue::StringRef("c".into()));

        assert_eq!(rga.to_vec().len(), 3);
    }

    #[test]
    fn test_merge_convergence() {
        // Two replicas insert concurrently after head
        let mut r1 = Rga::new(1);
        let mut r2 = Rga::new(2);

        r1.insert_after(None, OperationValue::StringRef("from_r1".into()));
        r2.insert_after(None, OperationValue::StringRef("from_r2".into()));

        let mut merged_a = r1.clone();
        merged_a.merge(&r2);

        let mut merged_b = r2.clone();
        merged_b.merge(&r1);

        // Both merge orders should produce the same sequence
        let a_vals: Vec<_> = merged_a.to_vec().iter().map(|v| format!("{v:?}")).collect();
        let b_vals: Vec<_> = merged_b.to_vec().iter().map(|v| format!("{v:?}")).collect();
        assert_eq!(a_vals, b_vals);
    }

    #[test]
    fn test_merge_tombstone_propagation() {
        let mut r1 = Rga::new(1);
        let id = r1.insert_after(None, OperationValue::StringRef("x".into()));

        let mut r2 = r1.clone();
        r2.delete(id);

        r1.merge(&r2);
        assert!(r1.is_empty());
    }

    #[test]
    fn test_lattice_idempotent() {
        let mut rga = Rga::new(1);
        rga.insert_after(None, OperationValue::StringRef("a".into()));
        let before = rga.clone();
        rga.join(&before);
        assert_eq!(rga.len(), 1);
    }
}
