// SPDX-License-Identifier: MIT OR Apache-2.0
//! Processing engines for DSON operations
//!
//! This module provides various processor implementations:
//! - [`BlackBoxProcessor`] - DOMless processing with schema filtering
//! - [`StreamingProcessor`] - Streaming pipeline for large datasets
//! - [`SimdDsonProcessor`] - Full CRDT-enabled SIMD processor
//! - [`TapeDsonProcessor`](crate::processor::TapeDsonProcessor) - Tape-based operations

pub mod black_box;
pub mod simd_dson;
pub mod streaming;
pub mod tape_ops;

pub use black_box::{BlackBoxProcessor, JsonPathContext, ProcessingMode, SchemaFilter};
pub use simd_dson::{
    ImplementationComparison, SimdDelta, SimdDsonProcessor, compare_implementations,
};
pub use streaming::StreamingProcessor;
pub use tape_ops::TapeDsonProcessor;
