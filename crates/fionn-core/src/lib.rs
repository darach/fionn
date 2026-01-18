// SPDX-License-Identifier: MIT OR Apache-2.0
//! Core types, error handling, and foundational types for fionn
//!
//! This crate provides the foundational types used across the fionn ecosystem:
//!
//! - [`error`] - Error types and Result alias
//! - [`format`](mod@format) - Format types and node kind classification
//! - [`path`] - JSON path parsing utilities
//! - [`schema`] - Schema-based filtering
//! - [`value`] - Operation value types
//! - [`operations`] - DSON operation types
//! - [`tape_source`] - Format-agnostic tape traversal abstraction
//! - [`value_builder`] - Format-agnostic value construction for ungron
//! - [`diffable`] - Format-agnostic diff computation traits
//! - [`patchable`] - Format-agnostic patch application traits

#![deny(missing_docs)]
#![deny(rust_2018_idioms)]
#![deny(clippy::pedantic)]
#![deny(clippy::nursery)]
#![deny(clippy::cargo)]
// Allow multiple crate versions - transitive dependency from indexmap/hashbrown
#![allow(clippy::multiple_crate_versions)]

/// Format-agnostic diff computation traits and algorithms
pub mod diffable;
/// Error types for fionn operations
pub mod error;
/// Format types and node kind classification for multi-format support
pub mod format;
/// Core operation types
pub mod operations;
/// Format-agnostic patch application traits
pub mod patchable;
/// JSON path parsing utilities with SIMD acceleration
pub mod path;
/// Kind predicates for query path filtering
pub mod predicate;
/// Schema-based filtering for DOMless processing
pub mod schema;
/// Format-agnostic tape traversal abstraction for multi-format support
pub mod tape_source;
/// Operation value types for DSON operations
pub mod value;
/// Format-agnostic value construction for ungron reconstruction
pub mod value_builder;

// Re-exports for convenience
pub use error::{DsonError, Result};
pub use format::{
    Confidence, DetectionResult, FormatKind, FormatSpecificKind, NodeKind, ParsingContext,
};
pub use operations::{DsonOperation, MergeStrategy};
pub use path::{
    ParsedPath, PathCache, PathComponent, PathComponentRange, PathComponentRef, parse_simd,
    parse_simd_ref_into,
};
pub use predicate::{
    ContextPredicate, FidelityAnnotation, KindPredicate, LossCategory, ParsedPredicate,
};
pub use schema::{CompiledSchema, MatchType, SchemaFilter, SchemaPattern};
pub use value::OperationValue;

// Tape abstraction re-exports
pub use tape_source::{TapeIterator, TapeNodeKind, TapeNodeRef, TapeSource, TapeValue};
pub use value_builder::{JsonBuilder, PathSegment, ValueBuilder, set_at_path_json};

// Format-specific builder re-exports
#[cfg(feature = "toml")]
pub use value_builder::TomlBuilder;
#[cfg(feature = "yaml")]
pub use value_builder::YamlBuilder;
#[cfg(feature = "csv")]
pub use value_builder::{CsvBuilder, CsvValue};
#[cfg(feature = "ison")]
pub use value_builder::{IsonBuilder, IsonValue};
#[cfg(feature = "toon")]
pub use value_builder::{ToonBuilder, ToonValue};

// Diff/patch abstraction re-exports
pub use diffable::{
    DiffOptions, DiffValueKind, DiffableValue, GenericPatch, GenericPatchOperation,
};
pub use patchable::{PatchError, Patchable};
