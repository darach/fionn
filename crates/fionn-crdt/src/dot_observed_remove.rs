// SPDX-License-Identifier: MIT OR Apache-2.0
//! Dot-level Observed-Remove Map for Strict DSON mode
//!
//! Extends OR semantics from path-level to dot-level: each value is tagged with
//! its creation [`Dot`], and deletes only remove dots observed at delete time.
//! Under concurrency, adds win (add-wins semantics).

use std::collections::HashMap;

use fionn_core::OperationValue;

use super::dot_store::{CausalContext, Dot};
use super::lattice::Lattice;

/// A single value entry tagged with its creation dot.
#[derive(Debug, Clone)]
struct DotValue {
    dot: Dot,
    value: OperationValue,
}

/// Dot-level Observed-Remove Map.
///
/// Maps string paths to sets of dot-tagged values. Concurrent adds to the same
/// path are all retained; a delete only removes dots that were observed at
/// delete time (add-wins under concurrency).
#[derive(Debug, Clone)]
pub struct DotObservedRemoveMap {
    /// Path → set of dot-tagged values
    entries: HashMap<String, Vec<DotValue>>,
    /// Global causal context
    context: CausalContext,
}

impl DotObservedRemoveMap {
    /// Create a new empty map.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            context: CausalContext::new(),
        }
    }

    /// Add a value at the given path, tagged with a new dot.
    pub fn add(&mut self, path: &str, value: OperationValue, dot: Dot) {
        self.context.observe(dot);
        self.entries
            .entry(path.to_string())
            .or_default()
            .push(DotValue { dot, value });
    }

    /// Remove all values at the given path that are in the current causal context.
    ///
    /// Values added concurrently (not yet observed) are preserved (add-wins).
    pub fn remove(&mut self, path: &str) {
        if let Some(values) = self.entries.get_mut(path) {
            values.retain(|dv| !self.context.has_observed(dv.dot));
            if values.is_empty() {
                self.entries.remove(path);
            }
        }
    }

    /// Get the current value(s) at the given path.
    ///
    /// In a converged state there is typically one value per path, but during
    /// concurrent operation there may be multiple (conflict set).
    #[must_use]
    pub fn get(&self, path: &str) -> Vec<&OperationValue> {
        self.entries
            .get(path)
            .map(|vs| vs.iter().map(|dv| &dv.value).collect())
            .unwrap_or_default()
    }

    /// Get the single value at a path, or `None` if absent / conflicted.
    ///
    /// When multiple concurrent values exist, returns the one with the highest
    /// dot (deterministic tiebreak).
    #[must_use]
    pub fn get_one(&self, path: &str) -> Option<&OperationValue> {
        self.entries.get(path).and_then(|vs| {
            vs.iter()
                .max_by_key(|dv| (dv.dot.sequence, dv.dot.replica_id))
                .map(|dv| &dv.value)
        })
    }

    /// Check if a path has any value.
    #[must_use]
    pub fn contains(&self, path: &str) -> bool {
        self.entries.get(path).is_some_and(|vs| !vs.is_empty())
    }

    /// Iterate over all paths and their latest values.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &OperationValue)> {
        self.entries.iter().filter_map(|(path, vs)| {
            vs.iter()
                .max_by_key(|dv| (dv.dot.sequence, dv.dot.replica_id))
                .map(|dv| (path.as_str(), &dv.value))
        })
    }

    /// Get a reference to the causal context.
    #[must_use]
    pub const fn context(&self) -> &CausalContext {
        &self.context
    }

    /// Lattice join: merge another map into this one.
    ///
    /// For each path, keep dots that are:
    /// - in both stores (keep value from either, they're identical), OR
    /// - in self's store but NOT in other's context (other hasn't seen this add), OR
    /// - in other's store but NOT in self's context (self hasn't seen this add)
    ///
    /// This implements the standard add-wins OR-set join.
    fn join_impl(&mut self, other: &Self) {
        let mut merged: HashMap<String, Vec<DotValue>> = HashMap::new();

        // Collect all paths from both maps
        let all_paths: std::collections::HashSet<&str> = self
            .entries
            .keys()
            .chain(other.entries.keys())
            .map(String::as_str)
            .collect();

        for path in all_paths {
            let mut result_values = Vec::new();
            let self_values = self.entries.get(path);
            let other_values = other.entries.get(path);

            // Keep dots from self that other hasn't observed (or that other also has)
            if let Some(svs) = self_values {
                for dv in svs {
                    if !other.context.has_observed(dv.dot)
                        || other_values.is_some_and(|ovs| ovs.iter().any(|odv| odv.dot == dv.dot))
                    {
                        result_values.push(dv.clone());
                    }
                }
            }

            // Keep dots from other that self hasn't observed (and aren't already added)
            if let Some(ovs) = other_values {
                for dv in ovs {
                    if !self.context.has_observed(dv.dot)
                        && !result_values.iter().any(|rv| rv.dot == dv.dot)
                    {
                        result_values.push(dv.clone());
                    }
                }
            }

            if !result_values.is_empty() {
                merged.insert(path.to_string(), result_values);
            }
        }

        self.entries = merged;
        self.context.merge(other.context.clone());
    }
}

impl Default for DotObservedRemoveMap {
    fn default() -> Self {
        Self::new()
    }
}

impl Lattice for DotObservedRemoveMap {
    fn join(&mut self, other: &Self) {
        self.join_impl(other);
    }

    fn partial_le(&self, other: &Self) -> bool {
        // self ≤ other iff self.context ≤ other.context and every dot-value
        // in self is also in other
        if !self.context.happened_before(&other.context) {
            return false;
        }
        self.entries.iter().all(|(path, svs)| {
            other
                .entries
                .get(path)
                .is_some_and(|ovs| svs.iter().all(|sv| ovs.iter().any(|ov| ov.dot == sv.dot)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_get() {
        let mut map = DotObservedRemoveMap::new();
        map.add(
            "a.b",
            OperationValue::StringRef("hello".into()),
            Dot::new(1, 1),
        );
        assert_eq!(
            map.get_one("a.b"),
            Some(&OperationValue::StringRef("hello".into()))
        );
    }

    #[test]
    fn test_remove_observed() {
        let mut map = DotObservedRemoveMap::new();
        map.add("x", OperationValue::Null, Dot::new(1, 1));
        assert!(map.contains("x"));

        map.remove("x");
        assert!(!map.contains("x"));
    }

    #[test]
    fn test_add_wins_concurrent() {
        // Replica 1 adds, replica 2 deletes concurrently (doesn't know about the add)
        let mut r1 = DotObservedRemoveMap::new();
        let mut r2 = DotObservedRemoveMap::new();

        // Both observe an initial value
        let initial_dot = Dot::new(1, 1);
        r1.add("key", OperationValue::StringRef("old".into()), initial_dot);
        r2.add("key", OperationValue::StringRef("old".into()), initial_dot);

        // r1 adds a new value concurrently
        r1.add(
            "key",
            OperationValue::StringRef("new".into()),
            Dot::new(1, 2),
        );

        // r2 removes (only sees dot (1,1))
        r2.remove("key");

        // Merge: r1's new add (1,2) should survive because r2 didn't observe it
        r1.join(&r2);
        assert!(r1.contains("key"));
    }

    #[test]
    fn test_join_commutativity() {
        let mut a = DotObservedRemoveMap::new();
        let mut b = DotObservedRemoveMap::new();

        a.add("x", OperationValue::StringRef("a".into()), Dot::new(1, 1));
        b.add("x", OperationValue::StringRef("b".into()), Dot::new(2, 1));

        let mut ab = a.clone();
        ab.join(&b);

        let mut ba = b.clone();
        ba.join(&a);

        // Both should contain the same dots
        let ab_vals: Vec<_> = ab.get("x").into_iter().map(|v| format!("{v:?}")).collect();
        let ba_vals: Vec<_> = ba.get("x").into_iter().map(|v| format!("{v:?}")).collect();
        // Sort for comparison since order may differ
        let mut ab_sorted = ab_vals;
        ab_sorted.sort();
        let mut ba_sorted = ba_vals;
        ba_sorted.sort();
        assert_eq!(ab_sorted, ba_sorted);
    }

    #[test]
    fn test_join_idempotent() {
        let mut map = DotObservedRemoveMap::new();
        map.add("k", OperationValue::BoolRef(true), Dot::new(1, 1));
        let before = map.clone();
        map.join(&before);
        assert_eq!(map.get("k").len(), 1);
    }

    #[test]
    fn test_iter() {
        let mut map = DotObservedRemoveMap::new();
        map.add("a", OperationValue::Null, Dot::new(1, 1));
        map.add("b", OperationValue::Null, Dot::new(1, 2));

        assert_eq!(map.iter().count(), 2);
    }
}
