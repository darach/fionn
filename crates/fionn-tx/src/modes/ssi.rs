// SPDX-License-Identifier: MIT OR Apache-2.0
//! SSI-Lite — Serializable Snapshot Isolation
//!
//! Extends snapshot isolation with read-write dependency tracking. If a
//! dangerous structure (RW anti-dependency cycle) is detected, one of the
//! involved transactions is aborted to maintain serializability.

use std::collections::{HashMap, HashSet};

use fionn_core::Result;

use crate::event::{TxEvent, TxEventKind, TxPayload, TxVerdict};
use crate::protocol::TxProtocol;
use crate::types::{TxEnvelope, TxId, TxState};

/// SSI-Lite protocol implementation.
#[derive(Debug, Default)]
pub struct SsiProtocol {
    /// For each committed tx: the set of paths it wrote.
    committed_writes: HashMap<TxId, HashSet<String>>,
    /// For each committed tx: the set of paths it read.
    committed_reads: HashMap<TxId, HashSet<String>>,
    next_seq: u64,
}

impl SsiProtocol {
    /// Create a new SSI-Lite protocol instance.
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

    /// Detect RW anti-dependency: did a committed tx write to a path that this
    /// tx reads, AND does this tx write to a path that another committed tx reads?
    fn has_dangerous_structure(&self, envelope: &TxEnvelope) -> bool {
        let tx_reads: HashSet<&str> = envelope.read_set.iter().map(String::as_str).collect();
        let tx_writes = Self::write_paths(envelope);

        for (other_id, other_writes) in &self.committed_writes {
            // Check: other wrote something we read (RW from other → us)
            let rw_in = other_writes.iter().any(|p| tx_reads.contains(p.as_str()));
            if rw_in {
                // Check: we write something another committed tx read (RW from us → other)
                if let Some(other_reads) = self.committed_reads.get(other_id) {
                    let rw_out = tx_writes.iter().any(|p| other_reads.contains(p));
                    if rw_out {
                        return true;
                    }
                }
            }
        }
        false
    }
}

impl TxProtocol for SsiProtocol {
    fn begin(&mut self, envelope: &mut TxEnvelope) -> Result<()> {
        envelope.state = TxState::Active;
        Ok(())
    }

    fn validate(&self, envelope: &TxEnvelope) -> Result<TxVerdict> {
        if envelope.write_set.is_empty() {
            return Ok(TxVerdict::Abort("empty write set".to_string()));
        }
        if self.has_dangerous_structure(envelope) {
            return Ok(TxVerdict::Abort(
                "SSI: dangerous RW anti-dependency structure detected".to_string(),
            ));
        }
        Ok(TxVerdict::Commit)
    }

    fn commit(&mut self, envelope: &mut TxEnvelope) -> Result<Vec<TxEvent>> {
        let writes = Self::write_paths(envelope);
        let reads: HashSet<String> = envelope.read_set.iter().cloned().collect();
        self.committed_writes.insert(envelope.tx_id, writes);
        self.committed_reads.insert(envelope.tx_id, reads);

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
    fn test_ssi_no_anomaly() {
        let mut proto = SsiProtocol::new();
        let mut env = TxEnvelope::new(1, crate::types::TxMode::SsiLite, CausalContext::new(), "r1");
        proto.begin(&mut env).unwrap();
        env.write_set.push(DsonOperation::FieldAdd {
            path: "a".into(),
            value: OperationValue::Null,
        });
        assert!(matches!(proto.validate(&env).unwrap(), TxVerdict::Commit));
    }

    #[test]
    fn test_ssi_dangerous_structure() {
        let mut proto = SsiProtocol::new();

        // Tx1: reads "x", writes "y"
        let mut env1 =
            TxEnvelope::new(1, crate::types::TxMode::SsiLite, CausalContext::new(), "r1");
        proto.begin(&mut env1).unwrap();
        env1.read_set.push("x".into());
        env1.write_set.push(DsonOperation::FieldAdd {
            path: "y".into(),
            value: OperationValue::Null,
        });
        proto.commit(&mut env1).unwrap();

        // Tx2: reads "y", writes "x" — forms RW cycle with Tx1
        let mut env2 =
            TxEnvelope::new(2, crate::types::TxMode::SsiLite, CausalContext::new(), "r1");
        proto.begin(&mut env2).unwrap();
        env2.read_set.push("y".into());
        env2.write_set.push(DsonOperation::FieldAdd {
            path: "x".into(),
            value: OperationValue::Null,
        });
        assert!(matches!(
            proto.validate(&env2).unwrap(),
            TxVerdict::Abort(_)
        ));
    }
}
