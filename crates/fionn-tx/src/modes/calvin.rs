// SPDX-License-Identifier: MIT OR Apache-2.0
//! Calvin — Deterministic Execution
//!
//! Transactions agree on execution order first, then execute without
//! coordination. Once order is established, execution is deterministic.

use fionn_core::Result;

use crate::event::{TxEvent, TxEventKind, TxPayload, TxVerdict};
use crate::protocol::TxProtocol;
use crate::types::{TxEnvelope, TxId, TxState};

/// Calvin protocol implementation.
#[derive(Debug, Default)]
pub struct CalvinProtocol {
    /// Global log of committed transaction ids (execution order).
    committed_order: Vec<TxId>,
    next_seq: u64,
}

impl CalvinProtocol {
    /// Create a new Calvin protocol instance.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    const fn next_seq(&mut self) -> u64 {
        let s = self.next_seq;
        self.next_seq += 1;
        s
    }
}

impl TxProtocol for CalvinProtocol {
    fn begin(&mut self, envelope: &mut TxEnvelope) -> Result<()> {
        envelope.state = TxState::Active;
        Ok(())
    }

    fn validate(&self, envelope: &TxEnvelope) -> Result<TxVerdict> {
        // Calvin: deterministic — if the tx is in the agreed order, it commits.
        // In a single-node stub, always commit if non-empty.
        if envelope.write_set.is_empty() {
            return Ok(TxVerdict::Abort("empty write set".to_string()));
        }
        Ok(TxVerdict::Commit)
    }

    fn commit(&mut self, envelope: &mut TxEnvelope) -> Result<Vec<TxEvent>> {
        self.committed_order.push(envelope.tx_id);
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
    fn test_calvin_deterministic_commit() {
        let mut proto = CalvinProtocol::new();
        let mut env = TxEnvelope::new(1, crate::types::TxMode::Calvin, CausalContext::new(), "r1");
        proto.begin(&mut env).unwrap();
        env.write_set.push(DsonOperation::FieldAdd {
            path: "x".into(),
            value: OperationValue::Null,
        });
        proto.commit(&mut env).unwrap();
        assert_eq!(proto.committed_order, vec![1]);
    }
}
