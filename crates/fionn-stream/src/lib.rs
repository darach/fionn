// SPDX-License-Identifier: MIT OR Apache-2.0
//! Stream processing for fionn
//!
//! Provides streaming and JSONL processing capabilities:
//! - [`streaming`] - Streaming data pipeline processing
//! - [`skiptape`] - SIMD-JSONL skip tape processing
//! - [`jsonl_dson`] - JSONL-DSON integration

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
