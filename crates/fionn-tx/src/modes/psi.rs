// SPDX-License-Identifier: MIT OR Apache-2.0
//! PSI — Parallel Snapshot Isolation
//!
//! Transactions read from a consistent snapshot. Write-write conflicts on the
//! same path are detected at validation time — exactly one transaction wins.

use std::collections::HashSet;

use fionn_core::Result;

use crate::event::{TxEvent, TxEventKind, TxPayload, TxVerdict};
use crate::protocol::TxProtocol;
use crate::types::{TxEnvelope, TxState};

/// PSI protocol implementation.
#[derive(Debug, Default)]
pub struct PsiProtocol {
    /// Paths written by committed transactions (for write-write detection).
    committed_writes: HashSet<String>,
    next_seq: u64,
}

impl PsiProtocol {
    /// Create a new PSI protocol instance.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    const fn next_seq(&mut self) -> u64 {
        let s = self.next_seq;
        self.next_seq += 1;
        s
    }

    fn write_paths(envelope: &TxEnvelope) -> HashSet<String> {
        envelope
            .write_set
            .iter()
            .filter_map(|op| match op {
                fionn_core::DsonOperation::FieldAdd { path, .. }
                | fionn_core::DsonOperation::FieldModify { path, .. }
                | fionn_core::DsonOperation::FieldDelete { path } => Some(path.clone()),
                _ => None,
            })
            .collect()
    }
}

impl TxProtocol for PsiProtocol {
    fn begin(&mut self, envelope: &mut TxEnvelope) -> Result<()> {
        envelope.state = TxState::Active;
        Ok(())
    }

    fn validate(&self, envelope: &TxEnvelope) -> Result<TxVerdict> {
        // Write-write conflict detection
        let tx_writes = Self::write_paths(envelope);
        for path in &tx_writes {
            if self.committed_writes.contains(path) {
                return Ok(TxVerdict::Abort(format!(
                    "write-write conflict on path: {path}"
                )));
            }
        }
        if envelope.write_set.is_empty() {
            return Ok(TxVerdict::Abort("empty write set".to_string()));
        }
        Ok(TxVerdict::Commit)
    }

    fn commit(&mut self, envelope: &mut TxEnvelope) -> Result<Vec<TxEvent>> {
        // Record committed write paths
        let tx_writes = Self::write_paths(envelope);
        self.committed_writes.extend(tx_writes);

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
    fn test_psi_no_conflict() {
        let mut proto = PsiProtocol::new();
        let mut env = TxEnvelope::new(1, crate::types::TxMode::Psi, CausalContext::new(), "r1");
        proto.begin(&mut env).unwrap();
        env.write_set.push(DsonOperation::FieldAdd {
            path: "a".into(),
            value: OperationValue::Null,
        });
        assert!(matches!(proto.validate(&env).unwrap(), TxVerdict::Commit));
        proto.commit(&mut env).unwrap();
    }

    #[test]
    fn test_psi_write_write_conflict() {
        let mut proto = PsiProtocol::new();

        // First tx commits on path "x"
        let mut env1 = TxEnvelope::new(1, crate::types::TxMode::Psi, CausalContext::new(), "r1");
        proto.begin(&mut env1).unwrap();
        env1.write_set.push(DsonOperation::FieldAdd {
            path: "x".into(),
            value: OperationValue::Null,
        });
        proto.commit(&mut env1).unwrap();

        // Second tx tries to write to same path
        let mut env2 = TxEnvelope::new(2, crate::types::TxMode::Psi, CausalContext::new(), "r1");
        proto.begin(&mut env2).unwrap();
        env2.write_set.push(DsonOperation::FieldAdd {
            path: "x".into(),
            value: OperationValue::StringRef("conflict".into()),
        });
        assert!(matches!(
            proto.validate(&env2).unwrap(),
            TxVerdict::Abort(_)
        ));
    }
}
