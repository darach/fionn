// SPDX-License-Identifier: MIT OR Apache-2.0
//! # fionn
//!
//! A Swiss Army knife for JSON with SIMD acceleration.
//!
//! This crate re-exports all fionn sub-crates for convenience.

#![deny(missing_docs)]
#![deny(rust_2018_idioms)]
#![deny(clippy::pedantic)]
#![deny(clippy::nursery)]

// Core types
pub use fionn_core as core;
pub use fionn_core::error;
pub use fionn_core::path;
pub use fionn_core::schema;
pub use fionn_core::{DsonError, DsonOperation, MergeStrategy, OperationValue, Result};

// Tape
pub use fionn_tape as tape;

// Operations and processors
pub use fionn_ops as ops;
pub use fionn_ops::operations;
pub use fionn_ops::processor;
pub use fionn_ops::{BlackBoxProcessor, CanonicalOperationProcessor, SimdDsonProcessor};

// Streaming
pub use fionn_stream as stream;
pub use fionn_stream::jsonl_dson;
pub use fionn_stream::skiptape;
pub use fionn_stream::streaming;

// CRDT
pub use fionn_crdt as crdt;

// Gron
pub use fionn_gron as gron;

// Diff
pub use fionn_diff as diff;

// Pool
pub use fionn_pool as pool;

// SIMD utilities
pub use fionn_simd as simd;
