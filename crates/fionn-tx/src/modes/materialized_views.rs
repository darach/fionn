// SPDX-License-Identifier: MIT OR Apache-2.0
//! Materialized Views — Atomic base + derived view updates
//!
//! Ensures that updates to a base document and its derived materialized views
//! are applied atomically within a single transaction.

use std::collections::HashMap;

use fionn_core::{DsonOperation, Result};

use crate::event::{TxEvent, TxEventKind, TxPayload, TxVerdict};
use crate::protocol::TxProtocol;
use crate::types::{TxEnvelope, TxState};

/// View derivation function type.
pub type ViewDeriveFn = Box<dyn Fn(&[DsonOperation]) -> Vec<DsonOperation> + Send + Sync>;

/// Materialized views protocol implementation.
pub struct MaterializedViewsProtocol {
    /// Registered view derivations: `view_name` → derivation function.
    views: HashMap<String, ViewDeriveFn>,
    next_seq: u64,
}

impl MaterializedViewsProtocol {
    /// Create a new materialized views protocol.
    #[must_use]
    pub fn new() -> Self {
        Self {
            views: HashMap::new(),
            next_seq: 0,
        }
    }

    /// Register a view derivation.
    pub fn register_view(&mut self, name: &str, derive_fn: ViewDeriveFn) {
        self.views.insert(name.to_string(), derive_fn);
    }

    /// Derive all view operations from the base operations.
    #[must_use]
    pub fn derive_views(&self, base_ops: &[DsonOperation]) -> Vec<DsonOperation> {
        let mut all_view_ops = Vec::new();
        for derive_fn in self.views.values() {
            all_view_ops.extend(derive_fn(base_ops));
        }
        all_view_ops
    }

    const fn next_seq(&mut self) -> u64 {
        let s = self.next_seq;
        self.next_seq += 1;
        s
    }
}

impl Default for MaterializedViewsProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MaterializedViewsProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MaterializedViewsProtocol")
            .field("views", &self.views.keys().collect::<Vec<_>>())
            .field("next_seq", &self.next_seq)
            .finish()
    }
}

impl TxProtocol for MaterializedViewsProtocol {
    fn begin(&mut self, envelope: &mut TxEnvelope) -> Result<()> {
        envelope.state = TxState::Active;
        Ok(())
    }

    fn validate(&self, envelope: &TxEnvelope) -> Result<TxVerdict> {
        if envelope.write_set.is_empty() {
            return Ok(TxVerdict::Abort("empty write set".to_string()));
        }
        Ok(TxVerdict::Commit)
    }

    fn commit(&mut self, envelope: &mut TxEnvelope) -> Result<Vec<TxEvent>> {
        // Derive view operations atomically
        let view_ops = self.derive_views(&envelope.write_set);
        let mut all_ops = envelope.write_set.clone();
        all_ops.extend(view_ops);

        envelope.state = TxState::Committed;
        let seq = self.next_seq();
        Ok(vec![TxEvent {
            stream_id: 0,
            tx_id: envelope.tx_id,
            seq,
            kind: TxEventKind::Commit,
            envelope: envelope.clone(),
            payload: TxPayload::Operations(all_ops),
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
    use fionn_core::OperationValue;
    use fionn_crdt::dot_store::CausalContext;

    #[test]
    fn test_materialized_views_derive() {
        let mut proto = MaterializedViewsProtocol::new();
        proto.register_view(
            "count_view",
            Box::new(|ops: &[DsonOperation]| {
                vec![DsonOperation::FieldModify {
                    path: "_views.count".into(),
                    old_value: None,
                    new_value: OperationValue::NumberRef(ops.len().to_string()),
                }]
            }),
        );

        let mut env = TxEnvelope::new(
            1,
            crate::types::TxMode::MaterializedViews,
            CausalContext::new(),
            "r1",
        );
        proto.begin(&mut env).unwrap();
        env.write_set.push(DsonOperation::FieldAdd {
            path: "item".into(),
            value: OperationValue::StringRef("new".into()),
        });

        let events = proto.commit(&mut env).unwrap();
        if let TxPayload::Operations(ops) = &events[0].payload {
            assert_eq!(ops.len(), 2); // base op + view op
        } else {
            panic!("expected operations payload");
        }
    }
}
