// SPDX-License-Identifier: MIT OR Apache-2.0
//! Core types, error handling, and foundational types for fionn
//!
//! This crate provides the foundational types used across the fionn ecosystem:
//!
//! - [`error`] - Error types and Result alias
//! - [`path`] - JSON path parsing utilities
//! - [`schema`] - Schema-based filtering
//! - [`value`] - Operation value types
//! - [`operations`] - DSON operation types

#![deny(missing_docs)]
#![deny(rust_2018_idioms)]
#![deny(clippy::pedantic)]
#![deny(clippy::nursery)]
#![deny(clippy::cargo)]

/// Error types for fionn operations
pub mod error;
/// Core operation types
pub mod operations;
/// JSON path parsing utilities with SIMD acceleration
pub mod path;
/// Schema-based filtering for DOMless processing
pub mod schema;
/// Operation value types for DSON operations
pub mod value;
// Re-exports for convenience
pub use error::{DsonError, Result};
pub use operations::{DsonOperation, MergeStrategy};
pub use path::{
    ParsedPath, PathCache, PathComponent, PathComponentRange, PathComponentRef, parse_simd,
    parse_simd_ref_into,
};
pub use schema::{CompiledSchema, MatchType, SchemaFilter, SchemaPattern};
pub use value::OperationValue;
