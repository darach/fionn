// SPDX-License-Identifier: MIT OR Apache-2.0
//! SIMD-JSONL Skip Tape - A high-performance JSON Lines processor with schema-aware filtering
//!
//! This crate provides SIMD-accelerated JSON Lines processing that integrates schema filtering
//! directly into the parsing phase, producing compact "skip tapes" containing only
//! schema-matching data.

pub mod error;
pub mod jsonl;
pub mod processor;
pub mod schema;
pub mod simd_ops;
pub mod tape;

/// Re-export main types for convenience
pub use error::SkipTapeError;
#[cfg(feature = "wgpu")]
pub use jsonl::{PreScanMode, SimdJsonlProcessor};
pub use processor::SkipTapeProcessor;
pub use schema::CompiledSchema;
pub use tape::SkipTape;
