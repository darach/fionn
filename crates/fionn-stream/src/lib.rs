// SPDX-License-Identifier: MIT OR Apache-2.0
//! Stream processing for fionn
//!
//! Provides streaming and JSONL processing capabilities:
//! - [`streaming`] - Streaming data pipeline processing
//! - [`skiptape`] - SIMD-JSONL skip tape processing
//! - [`jsonl_dson`] - JSONL-DSON integration
//! - [`format_dson`] - Format-agnostic DSON processor
//! - [`format_crdt`] - Format-aware CRDT processor

#![deny(missing_docs)]
#![deny(rust_2018_idioms)]
#![deny(clippy::pedantic)]
#![deny(clippy::nursery)]

/// Streaming data pipeline processing
pub mod streaming;

/// SIMD-JSONL Skip Tape
pub mod skiptape;

/// JSONL-DSON Integration
pub mod jsonl_dson;

/// GPU processing support
pub mod gpu;

/// Format-agnostic DSON processor
pub mod format_dson;

/// Format-aware CRDT processor
pub mod format_crdt;

// Format-specific DSON processors (feature-gated by format)

/// ISONL-DSON Integration
#[cfg(feature = "ison")]
pub mod isonl_dson;

/// CSV-DSON Integration
#[cfg(feature = "csv")]
pub mod csv_dson;

/// YAML-DSON Integration
#[cfg(feature = "yaml")]
pub mod yaml_dson;

/// TOML-DSON Integration
#[cfg(feature = "toml")]
pub mod toml_dson;

/// TOON-DSON Integration
#[cfg(feature = "toon")]
pub mod toon_dson;
