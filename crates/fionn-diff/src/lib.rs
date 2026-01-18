// SPDX-License-Identifier: MIT OR Apache-2.0
//! # JSON Diff/Patch/Merge
//!
//! High-performance JSON structural operations with SIMD acceleration.
//!
//! This module provides three related capabilities:
//!
//! ## JSON Diff
//! Generate a list of operations that transform one JSON document into another.
//! Follows the spirit of RFC 6902 (JSON Patch) for the output format.
//!
//! ## JSON Patch (RFC 6902)
//! Apply a sequence of operations to a JSON document:
//! - `add`: Insert a value at a path
//! - `remove`: Delete a value at a path
//! - `replace`: Replace a value at a path
//! - `move`: Move a value from one path to another
//! - `copy`: Copy a value from one path to another
//! - `test`: Verify a value equals the expected value
//!
//! ## JSON Merge Patch (RFC 7396)
//! A simpler merge format where:
//! - Objects are recursively merged
//! - `null` values indicate deletion
//! - Other values replace existing ones
//!
//! ## Performance
//!
//! Uses SIMD acceleration for:
//! - Bulk string comparison (detect unchanged strings quickly)
//! - Array element comparison
//! - Finding longest common subsequence in arrays

mod compute;
mod csv_diff;
mod diff_tape;
mod diff_zerocopy;
mod merge;
mod patch;
mod simd_compare;
mod tape_merge;
mod tape_patch;

pub use compute::{DiffOptions, json_diff, json_diff_with_options};
pub use csv_diff::{
    CellChange, CsvDiff, CsvDiffOp, CsvDiffOptions, CsvDiffStats, RowIdentityMode, csv_diff,
    csv_diff_tapes,
};
pub use diff_tape::{
    TapeDiff, TapeDiffOp, TapeDiffOptions, TapeValueOwned, diff_tapes, diff_tapes_with_options,
};
pub use diff_zerocopy::{JsonPatchRef, PatchOperationRef, json_diff_zerocopy};
pub use merge::{deep_merge, json_merge_patch, merge_many, merge_patch_to_value};
pub use patch::{JsonPatch, PatchError, PatchOperation, apply_patch, apply_patch_mut};
pub use simd_compare::{
    json_numbers_equal, json_strings_equal, simd_bytes_equal, simd_find_first_difference,
};
pub use tape_merge::{
    StreamingMergeOptions, deep_merge_tape_into_value, deep_merge_tapes, merge_many_tapes,
    merge_tape_into_value, merge_tapes, streaming_merge,
};
#[cfg(feature = "toml")]
pub use tape_patch::value_to_toml;
#[cfg(feature = "yaml")]
pub use tape_patch::value_to_yaml;
pub use tape_patch::{
    apply_tape_diff, patch_tape, tape_to_value, three_way_patch, value_to_json,
    value_to_json_pretty,
};

// Re-export generic (format-agnostic) diff/patch types and functions from fionn-core
pub use fionn_core::diffable::{
    DiffOptions as GenericDiffOptions, DiffValueKind, DiffableValue, GenericPatch,
    GenericPatchOperation, compute_diff as generic_compute_diff,
    compute_diff_with_options as generic_compute_diff_with_options,
};
pub use fionn_core::patchable::{
    PatchError as GenericPatchError, Patchable, apply_operation as generic_apply_operation,
    apply_patch as generic_apply_patch,
};
