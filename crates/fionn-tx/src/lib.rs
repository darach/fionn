// SPDX-License-Identifier: MIT OR Apache-2.0
//! Transactional envelopes for fionn CRDTs
//!
//! Provides 9 transaction protocol modes on top of the Strict DSON processor:
//!
//! - [`modes::ramp`] — Read-Atomic Multi-Partition
//! - [`modes::rola`] — RAMP + Compare-And-Swap
//! - [`modes::psi`] — Parallel Snapshot Isolation
//! - [`modes::tcb`] — Transactional Causal Broadcast
//! - [`modes::calvin`] — Deterministic Execution
//! - [`modes::saga`] — Compensating Transactions
//! - [`modes::escrow`] — Escrow Counters
//! - [`modes::ssi`] — Serializable Snapshot Isolation (Lite)
//! - [`modes::materialized_views`] — Atomic Materialized Views

pub mod event;
pub mod modes;
pub mod protocol;
pub mod runtime;
pub mod types;

// Re-exports
pub use event::{TxEvent, TxEventKind, TxPayload, TxVerdict};
pub use protocol::TxProtocol;
pub use runtime::{TxHandle, TxOpts, TxRuntime};
pub use types::{TxEnvelope, TxId, TxMetadata, TxMode, TxState};
