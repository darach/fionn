// SPDX-License-Identifier: MIT OR Apache-2.0
//! Core transaction types for `fionn-tx`

use fionn_core::DsonOperation;
use fionn_crdt::dot_store::CausalContext;

/// Unique transaction identifier (UUID-compatible).
pub type TxId = u128;

/// Stream identifier for event ordering.
pub type StreamId = u64;

/// Transaction protocol mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TxMode {
    /// Read-Atomic Multi-Partition
    Ramp,
    /// RAMP + Compare-And-Swap
    Rola,
    /// Parallel Snapshot Isolation
    Psi,
    /// Transactional Causal Broadcast
    Tcb,
    /// Deterministic execution (agree on order, then execute)
    Calvin,
    /// Compensating transactions
    Saga,
    /// Pre-allocated numeric budgets with bounded invariants
    EscrowCounters,
    /// Serializable Snapshot Isolation (lite)
    SsiLite,
    /// Atomic base + derived view updates
    MaterializedViews,
}

/// Transaction lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxState {
    /// Transaction is accepting operations
    Active,
    /// Validation in progress
    Preparing,
    /// Successfully committed
    Committed,
    /// Rolled back
    Aborted,
}

/// Transaction envelope — the unit of transactional work.
#[derive(Debug, Clone)]
pub struct TxEnvelope {
    /// Unique id of this transaction.
    pub tx_id: TxId,
    /// Protocol mode governing commit/abort semantics.
    pub mode: TxMode,
    /// Current lifecycle state.
    pub state: TxState,
    /// Paths read during this transaction.
    pub read_set: Vec<String>,
    /// Operations to apply on commit.
    pub write_set: Vec<DsonOperation>,
    /// Causal snapshot at transaction begin.
    pub read_snapshot: CausalContext,
    /// Causal clock at commit time (set during commit).
    pub commit_clock: Option<CausalContext>,
    /// Metadata.
    pub metadata: TxMetadata,
}

impl TxEnvelope {
    /// Create a new active transaction envelope.
    #[must_use]
    pub fn new(tx_id: TxId, mode: TxMode, snapshot: CausalContext, replica_id: &str) -> Self {
        Self {
            tx_id,
            mode,
            state: TxState::Active,
            read_set: Vec::new(),
            write_set: Vec::new(),
            read_snapshot: snapshot,
            commit_clock: None,
            metadata: TxMetadata {
                created_at: 0,
                replica_id: replica_id.to_string(),
                parent_tx: None,
                retry_count: 0,
            },
        }
    }
}

/// Transaction metadata.
#[derive(Debug, Clone)]
pub struct TxMetadata {
    /// Logical creation timestamp.
    pub created_at: u64,
    /// Replica that initiated the transaction.
    pub replica_id: String,
    /// Parent transaction (for Saga sub-steps).
    pub parent_tx: Option<TxId>,
    /// Number of retries attempted.
    pub retry_count: u32,
}
