// SPDX-License-Identifier: MIT OR Apache-2.0
//! RAMP — Read-Atomic Multi-Partition
//!
//! Stamps writes with the transaction id so readers can collect all partitions
//! atomically. A reader that sees any write from a transaction will fetch all
//! of that transaction's writes before returning.

use fionn_core::Result;

use crate::event::{TxEvent, TxEventKind, TxPayload, TxVerdict};
use crate::protocol::TxProtocol;
use crate::types::{TxEnvelope, TxState};

/// RAMP protocol implementation.
#[derive(Debug, Default)]
pub struct RampProtocol {
    next_seq: u64,
}

impl RampProtocol {
    /// Create a new RAMP protocol instance.
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

impl TxProtocol for RampProtocol {
    fn begin(&mut self, envelope: &mut TxEnvelope) -> Result<()> {
        envelope.state = TxState::Active;
        Ok(())
    }

    fn validate(&self, envelope: &TxEnvelope) -> Result<TxVerdict> {
        // RAMP: no write-write conflict detection — always commits
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
    use fionn_crdt::dot_store::CausalContext;

    fn test_envelope() -> TxEnvelope {
        TxEnvelope::new(1, crate::types::TxMode::Ramp, CausalContext::new(), "r1")
    }

    #[test]
    fn test_ramp_commit() {
        let mut proto = RampProtocol::new();
        let mut env = test_envelope();
        proto.begin(&mut env).unwrap();
        env.write_set.push(DsonOperation::FieldAdd {
            path: "x".into(),
            value: OperationValue::Null,
        });
        let verdict = proto.validate(&env).unwrap();
        assert!(matches!(verdict, TxVerdict::Commit));
        let events = proto.commit(&mut env).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(env.state, TxState::Committed);
    }

    #[test]
    fn test_ramp_empty_aborts() {
        let proto = RampProtocol::new();
        let env = test_envelope();
        let verdict = proto.validate(&env).unwrap();
        assert!(matches!(verdict, TxVerdict::Abort(_)));
    }
}
