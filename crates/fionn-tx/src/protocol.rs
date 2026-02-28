// SPDX-License-Identifier: MIT OR Apache-2.0
//! Transaction protocol trait

use fionn_core::Result;

use super::event::{TxEvent, TxVerdict};
use super::types::TxEnvelope;

/// Protocol-specific transaction logic.
///
/// Each transaction mode implements this trait to provide its own
/// begin/validate/commit/abort semantics.
pub trait TxProtocol {
    /// Begin a new transaction.
    ///
    /// # Errors
    /// Returns an error if the transaction cannot be started.
    fn begin(&mut self, envelope: &mut TxEnvelope) -> Result<()>;

    /// Validate the transaction before commit.
    ///
    /// # Errors
    /// Returns an error if validation fails.
    fn validate(&self, envelope: &TxEnvelope) -> Result<TxVerdict>;

    /// Commit the transaction, producing a stream of events.
    ///
    /// # Errors
    /// Returns an error if commit fails.
    fn commit(&mut self, envelope: &mut TxEnvelope) -> Result<Vec<TxEvent>>;

    /// Abort the transaction, producing a stream of events.
    ///
    /// # Errors
    /// Returns an error if abort fails.
    fn abort(&mut self, envelope: &mut TxEnvelope) -> Result<Vec<TxEvent>>;
}
