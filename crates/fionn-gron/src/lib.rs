// SPDX-License-Identifier: MIT OR Apache-2.0
//! # fionn-gron - SIMD-accelerated gron implementation
//!
//! This module provides a high-performance implementation of the gron transformation,
//! which converts JSON into greppable, line-oriented output format.
//!
//! ## Features
//!
//! - **SIMD-accelerated parsing**: Uses fionn's tape for efficient JSON parsing
//! - **Efficient path building**: Stack-based incremental path construction
//! - **Extended path syntax**: Supports bracket notation for special field names
//! - **Ungron support**: Reconstruct JSON from gron output
//! - **JSONL support**: Process newline-delimited JSON files
//! - **Query mode**: Filter output with JSONPath-like queries
//!
//! ## Example
//!
//! ```rust,ignore
//! use fionn::gron::{gron, GronOptions};
//!
//! let json = r#"{"name": "Alice", "age": 30}"#;
//! let output = gron(json, &GronOptions::default())?;
//! // Output:
//! // json = {};
//! // json.name = "Alice";
//! // json.age = 30;
//! ```
//!
//! ## JSONL Processing
//!
//! ```rust,ignore
//! use fionn::gron::{gron_jsonl, GronJsonlOptions};
//!
//! let jsonl = b"{\"a\":1}\n{\"b\":2}";
//! let output = gron_jsonl(jsonl, &GronJsonlOptions::default())?;
//! // json[0].a = 1;
//! // json[1].b = 2;
//! ```
//!
//! ## Query Mode
//!
//! ```rust,ignore
//! use fionn::gron::{gron_query, Query, GronOptions};
//!
//! let json = r#"{"users": [{"name": "Alice"}, {"name": "Bob"}]}"#;
//! let query = Query::parse(".users[*].name")?;
//! let output = gron_query(json, &query, &GronOptions::default())?;
//! // json.users[0].name = "Alice";
//! // json.users[1].name = "Bob";
//! ```

mod gron_core;
mod gron_generic;
mod gron_jsonl;
mod gron_parallel;
mod gron_query;
mod gron_zerocopy;
mod path_builder;
mod path_extended;
mod query;
mod simd_escape;
mod simd_unescape;
mod simd_utils;
mod ungron;
mod ungron_generic;

pub use gron_core::{GronOptions, GronOutput, gron, gron_to_writer};
pub use gron_jsonl::{
    ErrorMode, GronJsonlOptions, IndexFormat, JsonlStats, gron_jsonl, gron_jsonl_streaming,
    gron_jsonl_to_writer,
};
pub use gron_parallel::{GronParallelOptions, gron_parallel};
pub use gron_query::{GronQueryOptions, gron_query, gron_query_to_writer};
pub use gron_zerocopy::{GronLine, GronOutput as GronOutputZc, gron_zerocopy};
pub use path_builder::PathBuilder;
pub use path_extended::{
    ExtendedPathComponent, ParsedExtendedPath, parse_extended_path, parse_extended_path_ref,
};
pub use query::{MatchPotential, Query, QueryError, QuerySegment};
pub use simd_escape::{escape_json_string_simd, escape_json_to_string};
pub use simd_unescape::{UnescapeError, unescape_json_string_simd, unescape_json_to_string};
pub use simd_utils::{escape_json_string, needs_escape, needs_quoting};
pub use ungron::{ungron, ungron_to_value};

// Generic (format-agnostic) gron operations
pub use gron_generic::{gron_from_tape, gron_from_tape_to_writer};
pub use ungron_generic::{ungron_to_json, ungron_with_builder};
