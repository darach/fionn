// SPDX-License-Identifier: MIT OR Apache-2.0
//! Transaction event streaming model

use fionn_core::DsonOperation;
use fionn_crdt::dot_store::CausalContext;

use super::types::{StreamId, TxEnvelope, TxId};

/// Kind of transaction event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxEventKind {
    /// Transaction started
    Begin,
    /// Operation applied within the transaction
    Op,
    /// Entering validation phase
    Prepare,
    /// Transaction committed
    Commit,
    /// Transaction aborted
    Abort,
    /// Compensation applied (Saga mode)
    Compensate,
}

/// A single event in the transaction event stream.
#[derive(Debug, Clone)]
pub struct TxEvent {
    /// Stream this event belongs to.
    pub stream_id: StreamId,
    /// Transaction this event belongs to.
    pub tx_id: TxId,
    /// Sequence number within the transaction (monotonically increasing).
    pub seq: u64,
    /// Kind of event.
    pub kind: TxEventKind,
    /// Snapshot of the envelope at event time.
    pub envelope: TxEnvelope,
    /// Event payload.
    pub payload: TxPayload,
}

/// Payload carried by a transaction event.
#[derive(Debug, Clone)]
pub enum TxPayload {
    /// One or more operations.
    Operations(Vec<DsonOperation>),
    /// Causal snapshot.
    Snapshot(CausalContext),
    /// Commit/abort verdict.
    Verdict(TxVerdict),
    /// Compensating operations (Saga).
    Compensation(Vec<DsonOperation>),
    /// No payload.
    Empty,
}

/// Commit or abort verdict with optional reason.
#[derive(Debug, Clone)]
pub enum TxVerdict {
    /// Transaction committed successfully.
    Commit,
    /// Transaction aborted with a reason.
    Abort(String),
}
