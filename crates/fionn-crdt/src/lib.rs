// SPDX-License-Identifier: MIT OR Apache-2.0
//! CRDT (Conflict-free Replicated Data Type) implementations
//!
//! This module provides CRDT types for distributed document synchronization:
//!
//! - [`dot_store`] - Dot store for causal contexts
//! - [`observed_remove`] - Observed-remove semantics
//! - [`merge`] - Optimized merge strategies

pub mod dot_store;
pub mod merge;
pub mod observed_remove;

// Re-exports for convenience
pub use dot_store::*;
pub use merge::*;
pub use observed_remove::*;
