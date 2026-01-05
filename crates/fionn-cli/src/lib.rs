// SPDX-License-Identifier: MIT OR Apache-2.0
//! # fionn-cli
//!
//! Command-line interface for fionn - a Swiss Army knife for JSON with SIMD acceleration.
//!
//! ## Installation
//!
//! ```bash
//! cargo install fionn-cli
//! ```
//!
//! ## Usage
//!
//! ```bash
//! # Convert JSON to greppable format
//! fionn gron data.json
//!
//! # Convert back to JSON
//! fionn gron -u data.gron
//!
//! # Diff two JSON files
//! fionn diff old.json new.json
//!
//! # Apply a JSON patch
//! fionn patch data.json changes.patch
//!
//! # Merge JSON files
//! fionn merge base.json overlay.json
//!
//! # Query JSON
//! fionn query '.users[0].name' data.json
//!
//! # Format JSON
//! fionn format data.json
//! fionn format -c data.json  # compact
//!
//! # Validate JSON
//! fionn validate data.json
//!
//! # Infer schema
//! fionn schema data.json
//! ```
//!
//! ## Subcommands
//!
//! | Command | Description |
//! |---------|-------------|
//! | `gron` | Convert JSON to greppable line format |
//! | `diff` | Compute RFC 6902 diff between two JSON files |
//! | `patch` | Apply JSON Patch to a file |
//! | `merge` | Merge multiple JSON files (RFC 7396) |
//! | `query` | Query JSON with path expressions |
//! | `format` | Pretty-print or compact JSON |
//! | `validate` | Check JSON validity |
//! | `schema` | Infer JSON schema from data |
//! | `stream` | Process JSONL streams |
//! | `bench` | Run basic benchmarks |
//!
//! ## Library Usage
//!
//! This crate is primarily a CLI tool. For programmatic access to fionn
//! functionality, use the constituent library crates directly:
//!
//! - [`fionn`](https://docs.rs/fionn) - Umbrella crate with all functionality
//! - [`fionn-gron`](https://docs.rs/fionn-gron) - Greppable JSON transformation
//! - [`fionn-diff`](https://docs.rs/fionn-diff) - JSON diff, patch, merge
//! - [`fionn-tape`](https://docs.rs/fionn-tape) - Tape-based JSON representation
//! - [`fionn-simd`](https://docs.rs/fionn-simd) - SIMD acceleration primitives
//! - [`fionn-core`](https://docs.rs/fionn-core) - Core types and traits

#![doc(html_root_url = "https://docs.rs/fionn-cli/0.1.0")]
#![warn(missing_docs)]

/// Re-export of fionn-gron for gron functionality.
pub use fionn_gron as gron;

/// Re-export of fionn-diff for diff/patch/merge functionality.
pub use fionn_diff as diff;

/// Re-export of fionn-core for core types.
pub use fionn_core as core;
