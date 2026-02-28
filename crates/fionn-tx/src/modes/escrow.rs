// SPDX-License-Identifier: MIT OR Apache-2.0
//! Escrow Counters
//!
//! Pre-allocated numeric budgets with bounded invariants. Each replica has a
//! local escrow budget; operations that would exceed the budget are rejected.

use std::collections::HashMap;

use fionn_core::Result;

use crate::event::{TxEvent, TxEventKind, TxPayload, TxVerdict};
use crate::protocol::TxProtocol;
use crate::types::{TxEnvelope, TxState};

/// Escrow counter protocol implementation.
#[derive(Debug)]
pub struct EscrowProtocol {
    /// Current counter values per path.
    counters: HashMap<String, i64>,
    /// Reserved floor per path (counter must not go below this).
    floors: HashMap<String, i64>,
    /// Local escrow budget per path.
    budgets: HashMap<String, i64>,
    next_seq: u64,
}

impl EscrowProtocol {
    /// Create a new escrow protocol.
    #[must_use]
    pub fn new() -> Self {
        Self {
            counters: HashMap::new(),
            floors: HashMap::new(),
            budgets: HashMap::new(),
            next_seq: 0,
        }
    }

    /// Set the floor (minimum value) for a counter path.
    pub fn set_floor(&mut self, path: &str, floor: i64) {
        self.floors.insert(path.to_string(), floor);
    }

    /// Allocate escrow budget for a path.
    pub fn allocate_budget(&mut self, path: &str, budget: i64) {
        *self.budgets.entry(path.to_string()).or_insert(0) += budget;
    }

    /// Get current counter value.
    #[must_use]
    pub fn get_counter(&self, path: &str) -> i64 {
        self.counters.get(path).copied().unwrap_or(0)
    }

    /// Check if a decrement would violate the floor invariant.
    #[must_use]
    pub fn can_decrement(&self, path: &str, amount: i64) -> bool {
        let current = self.get_counter(path);
        let floor = self.floors.get(path).copied().unwrap_or(i64::MIN);
        let budget = self.budgets.get(path).copied().unwrap_or(0);
        // Must not go below floor, and decrement must be within budget
        current - amount >= floor && amount <= budget
    }

    const fn next_seq(&mut self) -> u64 {
        let s = self.next_seq;
        self.next_seq += 1;
        s
    }
}

impl Default for EscrowProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl TxProtocol for EscrowProtocol {
    fn begin(&mut self, envelope: &mut TxEnvelope) -> Result<()> {
        envelope.state = TxState::Active;
        Ok(())
    }

    fn validate(&self, envelope: &TxEnvelope) -> Result<TxVerdict> {
        // Check that all counter operations stay within budget and above floor
        for op in &envelope.write_set {
            if let fionn_core::DsonOperation::FieldModify {
                path, new_value, ..
            } = op
                && let fionn_core::OperationValue::NumberRef(n) = new_value
                && let Ok(delta) = n.parse::<i64>()
                && delta < 0
                && !self.can_decrement(path, -delta)
            {
                return Ok(TxVerdict::Abort(format!(
                    "escrow bound violation on path: {path}"
                )));
            }
        }
        if envelope.write_set.is_empty() {
            return Ok(TxVerdict::Abort("empty write set".to_string()));
        }
        Ok(TxVerdict::Commit)
    }

    fn commit(&mut self, envelope: &mut TxEnvelope) -> Result<Vec<TxEvent>> {
        // Apply counter modifications
        for op in &envelope.write_set {
            if let fionn_core::DsonOperation::FieldModify {
                path, new_value, ..
            } = op
                && let fionn_core::OperationValue::NumberRef(n) = new_value
                && let Ok(delta) = n.parse::<i64>()
            {
                *self.counters.entry(path.clone()).or_insert(0) += delta;
            }
        }
        envelope.state = TxState::Committed;
        let seq = self.next_seq();
        Ok(vec![TxEvent {
            stream_id: 0,
            tx_id: envelope.tx_id,
            seq,
            kind: TxEventKind::Commit,
            envelope: envelope.clone(),
            payload: TxPayload::Operations(envelope.write_set.clone()),
        }])
    }

    fn abort(&mut self, envelope: &mut TxEnvelope) -> Result<Vec<TxEvent>> {
        envelope.state = TxState::Aborted;
        let seq = self.next_seq();
        Ok(vec![TxEvent {
            stream_id: 0,
            tx_id: envelope.tx_id,
            seq,
            kind: TxEventKind::Abort,
            envelope: envelope.clone(),
            payload: TxPayload::Verdict(TxVerdict::Abort("aborted".to_string())),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fionn_core::{DsonOperation, OperationValue};
    use fionn_crdt::dot_store::CausalContext;

    #[test]
    fn test_escrow_within_budget() {
        let mut proto = EscrowProtocol::new();
        proto.counters.insert("balance".into(), 100);
        proto.set_floor("balance", 0);
        proto.allocate_budget("balance", 50);

        assert!(proto.can_decrement("balance", 50));
        assert!(!proto.can_decrement("balance", 51)); // exceeds budget
    }

    #[test]
    fn test_escrow_floor_violation() {
        let mut proto = EscrowProtocol::new();
        proto.counters.insert("stock".into(), 5);
        proto.set_floor("stock", 0);
        proto.allocate_budget("stock", 100);

        assert!(!proto.can_decrement("stock", 6)); // would go below floor
    }

    #[test]
    fn test_escrow_commit() {
        let mut proto = EscrowProtocol::new();
        proto.counters.insert("x".into(), 100);
        proto.set_floor("x", 0);
        proto.allocate_budget("x", 100);

        let mut env = TxEnvelope::new(
            1,
            crate::types::TxMode::EscrowCounters,
            CausalContext::new(),
            "r1",
        );
        proto.begin(&mut env).unwrap();
        env.write_set.push(DsonOperation::FieldModify {
            path: "x".into(),
            old_value: None,
            new_value: OperationValue::NumberRef("-10".into()),
        });
        assert!(matches!(proto.validate(&env).unwrap(), TxVerdict::Commit));
        proto.commit(&mut env).unwrap();
        assert_eq!(proto.get_counter("x"), 90);
    }
}
