// SPDX-License-Identifier: MIT OR Apache-2.0
//! Saga — Compensating Transactions
//!
//! A saga is a sequence of steps, each with a compensating action. If step N
//! fails, steps 1..N-1 are compensated in reverse order.

use fionn_core::{DsonOperation, Result};

use crate::event::{TxEvent, TxEventKind, TxPayload, TxVerdict};
use crate::protocol::TxProtocol;
use crate::types::{TxEnvelope, TxState};

/// A single saga step with its compensation.
#[derive(Debug, Clone)]
pub struct SagaStep {
    /// Forward operation.
    pub operation: DsonOperation,
    /// Compensating operation (undo).
    pub compensation: DsonOperation,
}

/// Saga protocol implementation.
#[derive(Debug, Default)]
pub struct SagaProtocol {
    /// Steps registered for the current saga.
    steps: Vec<SagaStep>,
    /// Index of last successfully executed step.
    executed_up_to: usize,
    next_seq: u64,
}

impl SagaProtocol {
    /// Create a new Saga protocol.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a saga step with its compensation.
    pub fn add_step(&mut self, step: SagaStep) {
        self.steps.push(step);
    }

    /// Get the compensation operations for steps 0..n in reverse order.
    #[must_use]
    pub fn compensations_for(&self, up_to: usize) -> Vec<DsonOperation> {
        self.steps[..up_to]
            .iter()
            .rev()
            .map(|s| s.compensation.clone())
            .collect()
    }

    const fn next_seq(&mut self) -> u64 {
        let s = self.next_seq;
        self.next_seq += 1;
        s
    }
}

impl TxProtocol for SagaProtocol {
    fn begin(&mut self, envelope: &mut TxEnvelope) -> Result<()> {
        envelope.state = TxState::Active;
        self.executed_up_to = 0;
        self.steps.clear();
        Ok(())
    }

    fn validate(&self, envelope: &TxEnvelope) -> Result<TxVerdict> {
        if envelope.write_set.is_empty() && self.steps.is_empty() {
            return Ok(TxVerdict::Abort("no saga steps".to_string()));
        }
        Ok(TxVerdict::Commit)
    }

    fn commit(&mut self, envelope: &mut TxEnvelope) -> Result<Vec<TxEvent>> {
        self.executed_up_to = self.steps.len();
        envelope.state = TxState::Committed;
        let seq = self.next_seq();
        let ops: Vec<DsonOperation> = self.steps.iter().map(|s| s.operation.clone()).collect();
        Ok(vec![TxEvent {
            stream_id: 0,
            tx_id: envelope.tx_id,
            seq,
            kind: TxEventKind::Commit,
            envelope: envelope.clone(),
            payload: TxPayload::Operations(ops),
        }])
    }

    fn abort(&mut self, envelope: &mut TxEnvelope) -> Result<Vec<TxEvent>> {
        let compensations = self.compensations_for(self.executed_up_to);
        envelope.state = TxState::Aborted;
        let seq = self.next_seq();
        Ok(vec![TxEvent {
            stream_id: 0,
            tx_id: envelope.tx_id,
            seq,
            kind: TxEventKind::Compensate,
            envelope: envelope.clone(),
            payload: TxPayload::Compensation(compensations),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fionn_core::OperationValue;
    use fionn_crdt::dot_store::CausalContext;

    #[test]
    fn test_saga_compensations() {
        let mut proto = SagaProtocol::new();
        let mut env = TxEnvelope::new(1, crate::types::TxMode::Saga, CausalContext::new(), "r1");
        proto.begin(&mut env).unwrap();

        proto.add_step(SagaStep {
            operation: DsonOperation::FieldAdd {
                path: "a".into(),
                value: OperationValue::Null,
            },
            compensation: DsonOperation::FieldDelete { path: "a".into() },
        });
        proto.add_step(SagaStep {
            operation: DsonOperation::FieldAdd {
                path: "b".into(),
                value: OperationValue::Null,
            },
            compensation: DsonOperation::FieldDelete { path: "b".into() },
        });

        // Simulate failure after step 1
        proto.executed_up_to = 1;
        let events = proto.abort(&mut env).unwrap();
        assert_eq!(events.len(), 1);
        if let TxPayload::Compensation(comps) = &events[0].payload {
            assert_eq!(comps.len(), 1); // Only step 0 compensated
        } else {
            panic!("expected compensation payload");
        }
    }
}
