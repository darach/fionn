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

/// Unified tape format for multi-format DSON/CRDT operations
pub mod unified_tape;

/// ISONL batch processor (requires `ison` feature)
#[cfg(feature = "ison")]
pub mod isonl;

/// CSV batch processor (requires `csv` feature)
#[cfg(feature = "csv")]
pub mod csv_batch;

/// YAML batch processor (requires `yaml` feature)
#[cfg(feature = "yaml")]
pub mod yaml_batch;

/// TOML batch processor (requires `toml` feature)
#[cfg(feature = "toml")]
pub mod toml_batch;

/// TOON batch processor (requires `toon` feature)
#[cfg(feature = "toon")]
pub mod toon_batch;

/// Re-export main types for convenience
pub use error::SkipTapeError;
pub use jsonl::{PreScanMode, SimdJsonlProcessor};
pub use processor::SkipTapeProcessor;
pub use schema::CompiledSchema;
pub use tape::SkipTape;

/// Re-export ISONL processor
#[cfg(feature = "ison")]
pub use isonl::SimdIsonlBatchProcessor;

/// Re-export CSV processor
#[cfg(feature = "csv")]
pub use csv_batch::SimdCsvBatchProcessor;

/// Re-export YAML processor
#[cfg(feature = "yaml")]
pub use yaml_batch::SimdYamlBatchProcessor;

/// Re-export TOML processor
#[cfg(feature = "toml")]
pub use toml_batch::SimdTomlBatchProcessor;

/// Re-export TOON processor
#[cfg(feature = "toon")]
pub use toon_batch::SimdToonBatchProcessor;

/// Re-export unified tape types
pub use unified_tape::{
    ExtendedNodeType, IsonFieldType, IsonRefKind, NewlineStyle, NodeFlags, OriginalSyntax,
    TapeSegment, UnifiedNode, UnifiedStringArena, UnifiedTape, UnifiedTapeMetadata,
};
