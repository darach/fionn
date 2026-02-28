// SPDX-License-Identifier: MIT OR Apache-2.0
//! ROLA — RAMP + Compare-And-Swap
//!
//! Extends RAMP with a version vector check before commit. If the version
//! vector has advanced since the transaction's read snapshot, the CAS fails
//! and the transaction must abort.

use fionn_core::Result;

use crate::event::{TxEvent, TxEventKind, TxPayload, TxVerdict};
use crate::protocol::TxProtocol;
use crate::types::{TxEnvelope, TxState};

use fionn_crdt::dot_store::CausalContext;

/// ROLA protocol implementation.
#[derive(Debug)]
pub struct RolaProtocol {
    /// Current global causal context (version vector).
    current_context: CausalContext,
    next_seq: u64,
}

impl RolaProtocol {
    /// Create a new ROLA protocol with the given initial context.
    #[must_use]
    pub const fn new(context: CausalContext) -> Self {
        Self {
            current_context: context,
            next_seq: 0,
        }
    }

    const fn next_seq(&mut self) -> u64 {
        let s = self.next_seq;
        self.next_seq += 1;
        s
    }
}

impl TxProtocol for RolaProtocol {
    fn begin(&mut self, envelope: &mut TxEnvelope) -> Result<()> {
        envelope.state = TxState::Active;
        envelope.read_snapshot = self.current_context.clone();
        Ok(())
    }

    fn validate(&self, envelope: &TxEnvelope) -> Result<TxVerdict> {
        if envelope.write_set.is_empty() {
            return Ok(TxVerdict::Abort("empty write set".to_string()));
        }
        // CAS check: current context must not have advanced beyond our snapshot.
        // If current_context ≤ snapshot, nothing changed. If current_context has
        // entries not in the snapshot, someone committed since we began.
        if !self
            .current_context
            .happened_before(&envelope.read_snapshot)
        {
            return Ok(TxVerdict::Abort(
                "CAS failure: version advanced".to_string(),
            ));
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
    use fionn_crdt::Dot;

    #[test]
    fn test_rola_cas_success() {
        let ctx = CausalContext::new();
        let mut proto = RolaProtocol::new(ctx);
        let mut env = TxEnvelope::new(1, crate::types::TxMode::Rola, CausalContext::new(), "r1");
        proto.begin(&mut env).unwrap();
        env.write_set.push(DsonOperation::FieldAdd {
            path: "x".into(),
            value: OperationValue::Null,
        });
        assert!(matches!(proto.validate(&env).unwrap(), TxVerdict::Commit));
    }

    #[test]
    fn test_rola_cas_failure() {
        let mut ctx = CausalContext::new();
        ctx.observe(Dot::new(1, 1));
        let mut proto = RolaProtocol::new(CausalContext::new());
        let mut env = TxEnvelope::new(1, crate::types::TxMode::Rola, CausalContext::new(), "r1");
        proto.begin(&mut env).unwrap();
        // Advance the global context after begin
        proto.current_context = ctx;
        env.write_set.push(DsonOperation::FieldAdd {
            path: "x".into(),
            value: OperationValue::Null,
        });
        assert!(matches!(proto.validate(&env).unwrap(), TxVerdict::Abort(_)));
    }
}
