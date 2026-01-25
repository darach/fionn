// SPDX-License-Identifier: MIT OR Apache-2.0
//! Extended fionn features (fionn.ext)
//!
//! Features beyond orjson compatibility:
//! - JSONL streaming (matches sonic-rs performance)
//! - ISONL streaming (11.9x faster than JSONL)
//! - Multi-format parsing (YAML, TOML, CSV, ISON, TOON)
//! - Gron path-based exploration
//! - Diff/Patch/Merge operations
//! - CRDT conflict-free merging
//! - Zero-copy tape API

mod crdt;
mod diff;
mod formats;
mod gron;
mod isonl;
mod jsonl;
mod pipeline;
mod tape;

use pyo3::prelude::*;

/// Register the ext submodule
pub fn register_ext_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Version and description
    m.add(
        "__doc__",
        "Extended fionn features - streaming, multi-format, gron, diff, CRDT",
    )?;

    // ==========================================================================
    // JSONL Streaming (matches sonic-rs performance)
    // ==========================================================================
    m.add_class::<jsonl::JsonlReader>()?;
    m.add_class::<jsonl::JsonlWriter>()?;
    m.add_function(wrap_pyfunction!(jsonl::parse_jsonl, m)?)?;
    m.add_function(wrap_pyfunction!(jsonl::to_jsonl, m)?)?;

    // ==========================================================================
    // ISONL Streaming (11.9x faster than JSONL - KEY DIFFERENTIATOR)
    // ==========================================================================
    m.add_class::<isonl::IsonlReader>()?;
    m.add_class::<isonl::IsonlWriter>()?;
    m.add_function(wrap_pyfunction!(isonl::parse_isonl, m)?)?;
    m.add_function(wrap_pyfunction!(isonl::to_isonl, m)?)?;
    m.add_function(wrap_pyfunction!(isonl::jsonl_to_isonl, m)?)?;

    // ==========================================================================
    // Multi-Format Parsing
    // ==========================================================================
    m.add_function(wrap_pyfunction!(formats::parse, m)?)?;
    m.add_function(wrap_pyfunction!(formats::parse_yaml, m)?)?;
    m.add_function(wrap_pyfunction!(formats::parse_toml, m)?)?;
    m.add_function(wrap_pyfunction!(formats::parse_csv, m)?)?;
    m.add_function(wrap_pyfunction!(formats::parse_ison, m)?)?;
    m.add_function(wrap_pyfunction!(formats::parse_toon, m)?)?;
    m.add_function(wrap_pyfunction!(formats::to_yaml, m)?)?;
    m.add_function(wrap_pyfunction!(formats::to_toml, m)?)?;
    m.add_function(wrap_pyfunction!(formats::to_csv, m)?)?;
    m.add_function(wrap_pyfunction!(formats::to_ison, m)?)?;
    m.add_function(wrap_pyfunction!(formats::to_toon, m)?)?;

    // ==========================================================================
    // Gron Operations
    // ==========================================================================
    m.add_function(wrap_pyfunction!(gron::gron, m)?)?;
    m.add_function(wrap_pyfunction!(gron::ungron, m)?)?;
    m.add_function(wrap_pyfunction!(gron::gron_query, m)?)?;
    m.add_function(wrap_pyfunction!(gron::gron_bytes, m)?)?;

    // ==========================================================================
    // Diff/Patch/Merge
    // ==========================================================================
    m.add_function(wrap_pyfunction!(diff::diff, m)?)?;
    m.add_function(wrap_pyfunction!(diff::patch, m)?)?;
    m.add_function(wrap_pyfunction!(diff::merge, m)?)?;
    m.add_function(wrap_pyfunction!(diff::deep_merge, m)?)?;
    m.add_function(wrap_pyfunction!(diff::three_way_merge, m)?)?;
    m.add_function(wrap_pyfunction!(diff::diff_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(diff::batch_diff, m)?)?;

    // ==========================================================================
    // CRDT Operations
    // ==========================================================================
    m.add_class::<crdt::CrdtDocument>()?;
    m.add_class::<crdt::MergeStrategy>()?;
    m.add_function(wrap_pyfunction!(crdt::crdt_lww_merge, m)?)?;
    m.add_function(wrap_pyfunction!(crdt::crdt_max_merge, m)?)?;
    m.add_function(wrap_pyfunction!(crdt::crdt_min_merge, m)?)?;
    m.add_function(wrap_pyfunction!(crdt::crdt_additive_merge, m)?)?;
    m.add_function(wrap_pyfunction!(crdt::crdt_batch_merge, m)?)?;

    // ==========================================================================
    // Pipeline Processing
    // ==========================================================================
    m.add_class::<pipeline::Pipeline>()?;

    // ==========================================================================
    // Advanced Tape API (zero-copy SIMD-accelerated access)
    // ==========================================================================
    m.add_class::<tape::Tape>()?;
    m.add_class::<tape::TapePool>()?;
    m.add_class::<tape::Schema>()?;
    m.add_function(wrap_pyfunction!(tape::parse_tape, m)?)?;
    m.add_function(wrap_pyfunction!(tape::batch_resolve, m)?)?;
    m.add_function(wrap_pyfunction!(tape::batch_query, m)?)?;

    Ok(())
}
