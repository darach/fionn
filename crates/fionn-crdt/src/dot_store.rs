// SPDX-License-Identifier: MIT OR Apache-2.0
//! Causal Dot Store Integration for SIMD-DSON
//!
//! This module implements causal dot store concepts from DSON in the context
//! of SIMD-DSON's skip tape architecture. It enables CRDT semantics while
//! maintaining SIMD-DSON's performance advantages.

use fionn_core::Result;
use std::collections::HashMap;

/// A dot represents a unique event identifier in a causal context
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Dot {
    /// Replica identifier
    pub replica_id: u64,
    /// Sequence number within the replica
    pub sequence: u64,
}

impl Dot {
    /// Create a new dot
    #[must_use]
    pub const fn new(replica_id: u64, sequence: u64) -> Self {
        Self {
            replica_id,
            sequence,
        }
    }
}

/// Causal context tracks observed events across replicas
#[derive(Debug, Clone, Default)]
pub struct CausalContext {
    /// Maximum sequence number observed for each replica
    context: HashMap<u64, u64>,
}

impl CausalContext {
    /// Create a new empty causal context
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a dot has been observed
    #[must_use]
    pub fn has_observed(&self, dot: Dot) -> bool {
        self.context
            .get(&dot.replica_id)
            .is_some_and(|&max_seq| dot.sequence <= max_seq)
    }

    /// Record observation of a dot
    pub fn observe(&mut self, dot: Dot) {
        let entry = self.context.entry(dot.replica_id).or_insert(0);
        *entry = (*entry).max(dot.sequence);
    }

    /// Check if this context happened before another
    #[must_use]
    pub fn happened_before(&self, other: &Self) -> bool {
        // self ≺ other if for every replica, self's max seq <= other's max seq
        self.context.iter().all(|(replica, &self_seq)| {
            other
                .context
                .get(replica)
                .is_some_and(|&other_seq| self_seq <= other_seq)
        })
    }
    /// Merge another causal context into this one
    pub fn merge(&mut self, other: Self) {
        for (replica, seq) in other.context {
            let entry = self.context.entry(replica).or_insert(0);
            *entry = (*entry).max(seq);
        }
    }
}

/// Dot store trait for tracking event identifiers
pub trait DotStore {
    /// Get all dots in this store
    fn dots(&self) -> Vec<Dot>;

    /// Check if the store is empty (⊥)
    fn is_bottom(&self) -> bool;

    /// Union with another dot store
    fn union(&mut self, other: Self);
}

/// Basic dot store implementation using a vector
#[derive(Debug, Clone)]
pub struct VecDotStore {
    dots: Vec<Dot>,
}

impl VecDotStore {
    /// Create a new empty dot store.
    #[must_use]
    pub const fn new() -> Self {
        Self { dots: Vec::new() }
    }

    /// Add a dot to the store if not already present.
    pub fn add_dot(&mut self, dot: Dot) {
        if !self.dots.contains(&dot) {
            self.dots.push(dot);
        }
    }
}

impl Default for VecDotStore {
    fn default() -> Self {
        Self::new()
    }
}

impl DotStore for VecDotStore {
    fn dots(&self) -> Vec<Dot> {
        self.dots.clone()
    }

    fn is_bottom(&self) -> bool {
        self.dots.is_empty()
    }

    fn union(&mut self, other: Self) {
        for dot in other.dots {
            self.add_dot(dot);
        }
    }
}

/// Causal dot store combining dot store with causal context
#[derive(Debug, Clone)]
pub struct CausalDotStore<T: DotStore> {
    /// The underlying dot store
    pub store: T,
    /// Causal context for tracking causality
    pub context: CausalContext,
}

impl<T: DotStore> CausalDotStore<T> {
    /// Create a new causal dot store with the given store.
    pub fn new(store: T) -> Self {
        Self {
            store,
            context: CausalContext::new(),
        }
    }

    /// Check if the store is empty (bottom element).
    pub fn is_bottom(&self) -> bool {
        self.store.is_bottom() && self.context.context.is_empty()
    }
}

/// Join operation for causal dot stores
impl<T: DotStore> CausalDotStore<T> {
    /// Join two causal dot stores, merging their contents.
    ///
    /// # Errors
    /// Returns error if the join operation fails.
    pub fn join(mut self, other: Self) -> Result<Self> {
        // 1. Merge the causal contexts (take max for each replica)
        self.context.merge(other.context);

        // 2. Union the dot stores
        self.store.union(other.store);

        // Note: A full compacting implementation might discard dots that are
        // now covered by the causal context, but that is an optimization.
        // For correctness, union of stores + merge of contexts is sufficient.

        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_causal_context_basic() {
        let mut ctx = CausalContext::new();

        let dot1 = Dot::new(1, 5);
        let dot2 = Dot::new(1, 10);
        let _dot3 = Dot::new(2, 3);

        assert!(!ctx.has_observed(dot1));

        ctx.observe(dot1);
        assert!(ctx.has_observed(dot1));
        assert!(!ctx.has_observed(dot2));

        ctx.observe(dot2);
        assert!(ctx.has_observed(dot2));
    }

    #[test]
    fn test_causal_ordering() {
        let mut ctx1 = CausalContext::new();
        let mut ctx2 = CausalContext::new();

        // ctx1 observes some events
        ctx1.observe(Dot::new(1, 5));
        ctx1.observe(Dot::new(2, 3));

        // ctx2 observes ctx1's events plus more
        ctx2.observe(Dot::new(1, 5));
        ctx2.observe(Dot::new(2, 3));
        ctx2.observe(Dot::new(1, 10));

        assert!(ctx1.happened_before(&ctx2));
        assert!(!ctx2.happened_before(&ctx1));
    }

    #[test]
    fn test_vec_dot_store() {
        let mut store = VecDotStore::new();
        assert!(store.is_bottom());

        let dot = Dot::new(1, 5);
        store.add_dot(dot);

        assert!(!store.is_bottom());
        assert_eq!(store.dots(), vec![dot]);
    }

    #[test]
    fn test_dot_equality() {
        let dot1 = Dot::new(1, 5);
        let dot2 = Dot::new(1, 5);
        let dot3 = Dot::new(1, 6);
        assert_eq!(dot1, dot2);
        assert_ne!(dot1, dot3);
    }

    #[test]
    fn test_dot_clone() {
        let dot = Dot::new(1, 5);
        let cloned = dot;
        assert_eq!(dot, cloned);
    }

    #[test]
    fn test_dot_debug() {
        let dot = Dot::new(1, 5);
        let debug = format!("{dot:?}");
        assert!(debug.contains("Dot"));
    }

    #[test]
    fn test_causal_context_default() {
        let ctx = CausalContext::default();
        let dot = Dot::new(1, 1);
        assert!(!ctx.has_observed(dot));
    }

    #[test]
    fn test_causal_context_merge() {
        let mut ctx1 = CausalContext::new();
        let mut ctx2 = CausalContext::new();

        ctx1.observe(Dot::new(1, 5));
        ctx2.observe(Dot::new(2, 10));

        ctx1.merge(ctx2);
        assert!(ctx1.has_observed(Dot::new(1, 5)));
        assert!(ctx1.has_observed(Dot::new(2, 10)));
    }

    #[test]
    fn test_causal_context_clone() {
        let mut ctx = CausalContext::new();
        ctx.observe(Dot::new(1, 5));
        let cloned = ctx.clone();
        assert!(cloned.has_observed(Dot::new(1, 5)));
    }

    #[test]
    fn test_vec_dot_store_default() {
        let store = VecDotStore::default();
        assert!(store.is_bottom());
    }

    #[test]
    fn test_vec_dot_store_add_duplicate() {
        let mut store = VecDotStore::new();
        let dot = Dot::new(1, 5);
        store.add_dot(dot);
        store.add_dot(dot);
        assert_eq!(store.dots().len(), 1);
    }

    #[test]
    fn test_vec_dot_store_union() {
        let mut store1 = VecDotStore::new();
        let mut store2 = VecDotStore::new();

        store1.add_dot(Dot::new(1, 5));
        store2.add_dot(Dot::new(2, 10));

        store1.union(store2);
        assert_eq!(store1.dots().len(), 2);
    }

    #[test]
    fn test_causal_dot_store_new() {
        let store = VecDotStore::new();
        let causal = CausalDotStore::new(store);
        assert!(causal.is_bottom());
    }

    #[test]
    fn test_causal_dot_store_is_bottom() {
        let mut store = VecDotStore::new();
        store.add_dot(Dot::new(1, 5));
        let causal = CausalDotStore::new(store);
        assert!(!causal.is_bottom());
    }

    #[test]
    fn test_causal_dot_store_join() {
        let store1 = VecDotStore::new();
        let mut causal1 = CausalDotStore::new(store1);
        causal1.store.add_dot(Dot::new(1, 5));
        causal1.context.observe(Dot::new(1, 5));

        let store2 = VecDotStore::new();
        let mut causal2 = CausalDotStore::new(store2);
        causal2.store.add_dot(Dot::new(2, 10));
        causal2.context.observe(Dot::new(2, 10));

        let joined = causal1.join(causal2).unwrap();
        assert!(!joined.is_bottom());
        assert_eq!(joined.store.dots().len(), 2);
    }

    #[test]
    fn test_causal_dot_store_clone() {
        let store = VecDotStore::new();
        let causal = CausalDotStore::new(store);
        let cloned = causal;
        assert!(cloned.is_bottom());
    }

    #[test]
    fn test_dot_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        let dot1 = Dot::new(1, 5);
        let dot2 = Dot::new(1, 5);
        set.insert(dot1);
        set.insert(dot2);
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn test_causal_context_happened_before_empty() {
        let ctx1 = CausalContext::new();
        let ctx2 = CausalContext::new();
        assert!(ctx1.happened_before(&ctx2));
    }

    #[test]
    fn test_causal_context_happened_before_concurrent() {
        let mut ctx1 = CausalContext::new();
        let mut ctx2 = CausalContext::new();

        ctx1.observe(Dot::new(1, 5));
        ctx2.observe(Dot::new(2, 5));

        // Neither happened before the other (concurrent)
        assert!(!ctx1.happened_before(&ctx2));
        assert!(!ctx2.happened_before(&ctx1));
    }
}
