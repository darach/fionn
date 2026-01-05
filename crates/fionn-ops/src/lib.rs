// SPDX-License-Identifier: MIT OR Apache-2.0
//! Operations and processors for fionn
//!
//! This crate provides:
//! - [`operations`] - Operation types and canonical processing
//! - [`processor`] - Black box and streaming processors
//! - [`dson_traits`] - DSON trait abstractions
//! - [`dson_impl`] - SIMD-DSON implementation

#![deny(missing_docs)]
#![deny(rust_2018_idioms)]
#![deny(clippy::pedantic)]
#![deny(clippy::nursery)]

/// Canonical operations and optimization
pub mod operations;

/// Processing engines
pub mod processor;

/// DSON trait abstractions
pub mod dson_traits;

/// DSON trait implementations
pub mod dson_impl;

// Re-exports for convenience
pub use operations::{
    CanonicalOperationProcessor, DsonOperation, FilterPredicate, MergeStrategy, OperationOptimizer,
    OperationValue, ReduceFunction, StreamGenerator, TransformFunction,
};
pub use processor::{BlackBoxProcessor, SimdDsonProcessor, StreamingProcessor};
