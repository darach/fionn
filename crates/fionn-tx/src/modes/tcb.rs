// SPDX-License-Identifier: MIT OR Apache-2.0
//! TCB — Transactional Causal Broadcast
//!
//! Buffers operations until all causal dependencies are met, then delivers
//! the transaction atomically.

use fionn_core::Result;
use fionn_crdt::dot_store::CausalContext;

use crate::event::{TxEvent, TxEventKind, TxPayload, TxVerdict};
use crate::protocol::TxProtocol;
use crate::types::{TxEnvelope, TxState};

/// TCB protocol implementation.
#[derive(Debug)]
pub struct TcbProtocol {
    /// Known delivered causal context.
    delivered_context: CausalContext,
    next_seq: u64,
}

impl TcbProtocol {
    /// Create a new TCB protocol with the given initial delivered context.
    #[must_use]
    pub const fn new(delivered: CausalContext) -> Self {
        Self {
            delivered_context: delivered,
            next_seq: 0,
        }
    }

    const fn next_seq(&mut self) -> u64 {
        let s = self.next_seq;
        self.next_seq += 1;
        s
    }
}

impl TxProtocol for TcbProtocol {
    fn begin(&mut self, envelope: &mut TxEnvelope) -> Result<()> {
        envelope.state = TxState::Active;
        envelope.read_snapshot = self.delivered_context.clone();
        Ok(())
    }

    fn validate(&self, envelope: &TxEnvelope) -> Result<TxVerdict> {
        // Causal dependency check: all deps must be delivered
        if !envelope
            .read_snapshot
            .happened_before(&self.delivered_context)
        {
            return Ok(TxVerdict::Abort(
                "causal dependencies not yet delivered".to_string(),
            ));
        }
        if envelope.write_set.is_empty() {
            return Ok(TxVerdict::Abort("empty write set".to_string()));
        }
        Ok(TxVerdict::Commit)
    }

    fn commit(&mut self, envelope: &mut TxEnvelope) -> Result<Vec<TxEvent>> {
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

    #[test]
    fn test_tcb_causal_deps_met() {
        let ctx = CausalContext::new();
        let mut proto = TcbProtocol::new(ctx);
        let mut env = TxEnvelope::new(1, crate::types::TxMode::Tcb, CausalContext::new(), "r1");
        proto.begin(&mut env).unwrap();
        env.write_set.push(DsonOperation::FieldAdd {
            path: "x".into(),
            value: OperationValue::Null,
        });
        assert!(matches!(proto.validate(&env).unwrap(), TxVerdict::Commit));
    }
}
