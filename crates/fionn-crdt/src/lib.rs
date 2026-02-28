// SPDX-License-Identifier: MIT OR Apache-2.0
//! CRDT (Conflict-free Replicated Data Type) implementations
//!
//! This module provides CRDT types for distributed document synchronization:
//!
//! - [`dot_store`] - Dot store for causal contexts
//! - [`observed_remove`] - Observed-remove semantics
//! - [`merge`] - Optimized merge strategies
//! - [`lattice`] - Join-semilattice trait for strict DSON mode
//! - [`rga`] - Replicated Growable Array for identity-preserving arrays
//! - [`dot_observed_remove`] - Dot-level observed-remove map
//! - [`strict`] - Strict DSON processor (full Rinberg et al. compliance)

pub mod dot_store;
pub mod merge;
pub mod observed_remove;

pub mod dot_observed_remove;
pub mod lattice;
pub mod rga;
pub mod strict;

// Re-exports for convenience
pub use dot_store::*;
pub use merge::*;
pub use observed_remove::*;

// Strict-mode re-exports
pub use dot_observed_remove::DotObservedRemoveMap;
pub use lattice::Lattice;
pub use rga::{ElementId, Rga};
pub use strict::{StrictDelta, StrictDsonProcessor};
