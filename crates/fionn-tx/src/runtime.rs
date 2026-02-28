// SPDX-License-Identifier: MIT OR Apache-2.0
//! Transaction runtime — user-facing API for transactional operations

use fionn_core::{DsonOperation, OperationValue, Result};
use fionn_crdt::strict::StrictDsonProcessor;

use super::event::{TxEvent, TxEventKind, TxPayload, TxVerdict};
use super::modes::calvin::CalvinProtocol;
use super::modes::escrow::EscrowProtocol;
use super::modes::materialized_views::MaterializedViewsProtocol;
use super::modes::psi::PsiProtocol;
use super::modes::ramp::RampProtocol;
use super::modes::rola::RolaProtocol;
use super::modes::saga::SagaProtocol;
use super::modes::ssi::SsiProtocol;
use super::modes::tcb::TcbProtocol;
use super::protocol::TxProtocol;
use super::types::{TxEnvelope, TxId, TxMode, TxState};

/// Transaction options.
#[derive(Debug, Clone)]
pub struct TxOpts {
    /// Protocol mode.
    pub mode: TxMode,
    /// Optional timeout in milliseconds.
    pub timeout_ms: Option<u64>,
    /// Maximum retry count.
    pub max_retries: u32,
}

/// Handle to an active transaction. Operations are buffered until commit.
pub struct TxHandle<'a> {
    runtime: &'a mut TxRuntime,
    envelope: TxEnvelope,
}

impl TxHandle<'_> {
    /// Set a value at the given path within this transaction.
    ///
    /// # Errors
    /// Returns an error if the transaction is not active.
    pub fn set(&mut self, path: &str, value: impl Into<OperationValue>) -> Result<()> {
        self.check_active()?;
        self.envelope.write_set.push(DsonOperation::FieldAdd {
            path: path.to_string(),
            value: value.into(),
        });
        Ok(())
    }

    /// Read a value at the given path within this transaction's snapshot.
    ///
    /// # Errors
    /// Returns an error if the transaction is not active.
    pub fn get(&mut self, path: &str) -> Result<Option<OperationValue>> {
        self.check_active()?;
        self.envelope.read_set.push(path.to_string());
        Ok(self.runtime.processor.get(path).cloned())
    }

    /// Delete a field at the given path.
    ///
    /// # Errors
    /// Returns an error if the transaction is not active.
    pub fn delete(&mut self, path: &str) -> Result<()> {
        self.check_active()?;
        self.envelope.write_set.push(DsonOperation::FieldDelete {
            path: path.to_string(),
        });
        Ok(())
    }

    /// Insert a value into an array at the given path and index.
    ///
    /// # Errors
    /// Returns an error if the transaction is not active.
    pub fn array_insert(
        &mut self,
        path: &str,
        index: usize,
        value: impl Into<OperationValue>,
    ) -> Result<()> {
        self.check_active()?;
        // Encode array insert as a FieldAdd with index-encoded path
        self.envelope.write_set.push(DsonOperation::FieldAdd {
            path: format!("{path}[{index}]"),
            value: value.into(),
        });
        Ok(())
    }

    /// Commit this transaction, applying all buffered operations.
    ///
    /// # Errors
    /// Returns an error if validation or commit fails.
    pub fn commit(self) -> Result<Vec<TxEvent>> {
        let TxHandle {
            runtime,
            mut envelope,
        } = self;
        let protocol = runtime.get_protocol(envelope.mode);

        let verdict = protocol.validate(&envelope)?;
        match verdict {
            TxVerdict::Commit => {
                let events = protocol.commit(&mut envelope)?;
                // Apply operations to the processor
                for op in &envelope.write_set {
                    runtime.processor.apply_op(op);
                }
                Ok(events)
            }
            TxVerdict::Abort(reason) => {
                envelope.state = TxState::Aborted;
                Err(fionn_core::DsonError::InvalidOperation(format!(
                    "transaction aborted: {reason}"
                )))
            }
        }
    }

    /// Abort this transaction, discarding all buffered operations.
    ///
    /// # Errors
    /// Returns an error if the abort process fails.
    pub fn abort(self) -> Result<Vec<TxEvent>> {
        let TxHandle {
            runtime,
            mut envelope,
        } = self;
        let protocol = runtime.get_protocol(envelope.mode);
        protocol.abort(&mut envelope)
    }

    fn check_active(&self) -> Result<()> {
        if self.envelope.state != TxState::Active {
            return Err(fionn_core::DsonError::InvalidOperation(
                "transaction is not active".to_string(),
            ));
        }
        Ok(())
    }
}

/// Transaction runtime — manages the CRDT processor and transaction protocols.
pub struct TxRuntime {
    /// The underlying strict DSON processor.
    processor: StrictDsonProcessor,
    /// Next transaction id.
    next_tx_id: TxId,
    // Protocol instances (lazily used)
    ramp: RampProtocol,
    rola: RolaProtocol,
    psi: PsiProtocol,
    tcb: TcbProtocol,
    calvin: CalvinProtocol,
    saga: SagaProtocol,
    escrow: EscrowProtocol,
    ssi: SsiProtocol,
    materialized_views: MaterializedViewsProtocol,
}

impl TxRuntime {
    /// Create a new transaction runtime with the given processor.
    #[must_use]
    pub fn new(processor: StrictDsonProcessor) -> Self {
        Self {
            rola: RolaProtocol::new(processor.causal_context().clone()),
            tcb: TcbProtocol::new(processor.causal_context().clone()),
            processor,
            next_tx_id: 1,
            ramp: RampProtocol::new(),
            psi: PsiProtocol::new(),
            calvin: CalvinProtocol::new(),
            saga: SagaProtocol::new(),
            escrow: EscrowProtocol::new(),
            ssi: SsiProtocol::new(),
            materialized_views: MaterializedViewsProtocol::new(),
        }
    }

    /// Begin a new transaction with the given options.
    pub fn begin_tx(&mut self, opts: &TxOpts) -> TxHandle<'_> {
        let tx_id = self.next_tx_id;
        self.next_tx_id += 1;
        let snapshot = self.processor.causal_context().clone();
        let replica_id = self.processor.replica_id().to_string();
        let mut envelope = TxEnvelope::new(tx_id, opts.mode, snapshot, &replica_id);
        envelope.metadata.retry_count = 0;

        // Begin on the appropriate protocol
        let _ = self.get_protocol(opts.mode).begin(&mut envelope);

        TxHandle {
            runtime: self,
            envelope,
        }
    }

    /// Apply a sequence of transaction events (e.g. from a remote replica).
    ///
    /// # Errors
    /// Returns an error if any event cannot be applied.
    pub fn apply_events(&mut self, events: &[TxEvent]) -> Result<()> {
        for event in events {
            if event.kind == TxEventKind::Commit
                && let TxPayload::Operations(ops) = &event.payload
            {
                for op in ops {
                    self.processor.apply_op(op);
                }
            }
        }
        Ok(())
    }

    /// Replay a stream of events to reconstruct state.
    ///
    /// # Errors
    /// Returns an error if any event cannot be replayed.
    pub fn replay(&mut self, events: impl Iterator<Item = TxEvent>) -> Result<()> {
        for event in events {
            if event.kind == TxEventKind::Commit
                && let TxPayload::Operations(ops) = &event.payload
            {
                for op in ops {
                    self.processor.apply_op(op);
                }
            }
        }
        Ok(())
    }

    /// Get mutable reference to the appropriate protocol for a mode.
    fn get_protocol(&mut self, mode: TxMode) -> &mut dyn TxProtocol {
        match mode {
            TxMode::Ramp => &mut self.ramp,
            TxMode::Rola => &mut self.rola,
            TxMode::Psi => &mut self.psi,
            TxMode::Tcb => &mut self.tcb,
            TxMode::Calvin => &mut self.calvin,
            TxMode::Saga => &mut self.saga,
            TxMode::EscrowCounters => &mut self.escrow,
            TxMode::SsiLite => &mut self.ssi,
            TxMode::MaterializedViews => &mut self.materialized_views,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_tx_commit() {
        let processor = StrictDsonProcessor::new(1);
        let mut runtime = TxRuntime::new(processor);

        let mut tx = runtime.begin_tx(&TxOpts {
            mode: TxMode::Ramp,
            timeout_ms: None,
            max_retries: 0,
        });
        tx.set("name", OperationValue::StringRef("Alice".into()))
            .unwrap();
        let events = tx.commit().unwrap();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn test_tx_read() {
        let mut processor = StrictDsonProcessor::new(1);
        processor.set("x", OperationValue::StringRef("hello".into()));
        let mut runtime = TxRuntime::new(processor);

        let mut tx = runtime.begin_tx(&TxOpts {
            mode: TxMode::Ramp,
            timeout_ms: None,
            max_retries: 0,
        });
        let val = tx.get("x").unwrap();
        assert_eq!(val, Some(OperationValue::StringRef("hello".into())));
    }

    #[test]
    fn test_tx_abort() {
        let processor = StrictDsonProcessor::new(1);
        let mut runtime = TxRuntime::new(processor);

        let tx = runtime.begin_tx(&TxOpts {
            mode: TxMode::Ramp,
            timeout_ms: None,
            max_retries: 0,
        });
        let events = tx.abort().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, TxEventKind::Abort);
    }

    #[test]
    fn test_replay() {
        let processor = StrictDsonProcessor::new(1);
        let mut runtime = TxRuntime::new(processor);

        let mut tx = runtime.begin_tx(&TxOpts {
            mode: TxMode::Ramp,
            timeout_ms: None,
            max_retries: 0,
        });
        tx.set("k", OperationValue::StringRef("v".into())).unwrap();
        let events = tx.commit().unwrap();

        // Replay on a fresh runtime
        let processor2 = StrictDsonProcessor::new(2);
        let mut runtime2 = TxRuntime::new(processor2);
        runtime2.apply_events(&events).unwrap();
    }
}
