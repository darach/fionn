// SPDX-License-Identifier: MIT OR Apache-2.0
//! Join-semilattice trait and implementations
//!
//! Provides the [`Lattice`] trait for commutative, associative, idempotent merge
//! operations — the algebraic foundation for CRDTs in Strict DSON mode.

use super::dot_store::{CausalContext, CausalDotStore, DotStore, VecDotStore};

/// Join-semilattice: commutative, associative, idempotent merge
///
/// Any type implementing this trait guarantees:
/// - **Commutativity**: `a.join(b) == b.join(a)`
/// - **Associativity**: `a.join(b).join(c) == a.join(b.join(c))`
/// - **Idempotency**: `a.join(a) == a`
pub trait Lattice: Clone {
    /// Merge another value into this one (least upper bound).
    fn join(&mut self, other: &Self);

    /// Partial ordering: returns `true` if `self ≤ other` in the lattice.
    fn partial_le(&self, other: &Self) -> bool;
}

impl Lattice for CausalContext {
    fn join(&mut self, other: &Self) {
        self.merge(other.clone());
    }

    fn partial_le(&self, other: &Self) -> bool {
        self.happened_before(other)
    }
}

impl Lattice for VecDotStore {
    fn join(&mut self, other: &Self) {
        self.union(other.clone());
    }

    fn partial_le(&self, other: &Self) -> bool {
        // self ≤ other iff every dot in self is also in other
        let other_dots = other.dots();
        self.dots().iter().all(|d| other_dots.contains(d))
    }
}

impl<T: DotStore + Clone> Lattice for CausalDotStore<T> {
    fn join(&mut self, other: &Self) {
        self.context.merge(other.context.clone());
        self.store.union(other.store.clone());
    }

    fn partial_le(&self, other: &Self) -> bool {
        // self ≤ other iff context ≤ other.context and store ⊆ other.store
        let other_dots = other.store.dots();
        self.context.happened_before(&other.context)
            && self.store.dots().iter().all(|d| other_dots.contains(d))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dot_store::Dot;

    #[test]
    fn test_causal_context_lattice_join() {
        let mut a = CausalContext::new();
        a.observe(Dot::new(1, 5));
        let mut b = CausalContext::new();
        b.observe(Dot::new(2, 3));

        a.join(&b);
        assert!(a.has_observed(Dot::new(1, 5)));
        assert!(a.has_observed(Dot::new(2, 3)));
    }

    #[test]
    fn test_causal_context_lattice_idempotent() {
        let mut a = CausalContext::new();
        a.observe(Dot::new(1, 5));
        let before = a.clone();
        a.join(&before);
        assert!(a.has_observed(Dot::new(1, 5)));
    }

    #[test]
    fn test_vec_dot_store_lattice_join() {
        let mut a = VecDotStore::new();
        a.add_dot(Dot::new(1, 1));
        let mut b = VecDotStore::new();
        b.add_dot(Dot::new(2, 1));

        a.join(&b);
        assert_eq!(a.dots().len(), 2);
    }

    #[test]
    fn test_vec_dot_store_partial_le() {
        let mut a = VecDotStore::new();
        a.add_dot(Dot::new(1, 1));
        let mut b = VecDotStore::new();
        b.add_dot(Dot::new(1, 1));
        b.add_dot(Dot::new(2, 1));

        assert!(a.partial_le(&b));
        assert!(!b.partial_le(&a));
    }

    #[test]
    fn test_causal_dot_store_lattice_join() {
        let mut a = CausalDotStore::new(VecDotStore::new());
        a.store.add_dot(Dot::new(1, 1));
        a.context.observe(Dot::new(1, 1));

        let mut b = CausalDotStore::new(VecDotStore::new());
        b.store.add_dot(Dot::new(2, 1));
        b.context.observe(Dot::new(2, 1));

        Lattice::join(&mut a, &b);
        assert_eq!(a.store.dots().len(), 2);
        assert!(a.context.has_observed(Dot::new(1, 1)));
        assert!(a.context.has_observed(Dot::new(2, 1)));
    }
}
