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
mod diff_zerocopy;
mod merge;
mod patch;
mod simd_compare;

pub use compute::{DiffOptions, json_diff, json_diff_with_options};
pub use diff_zerocopy::{JsonPatchRef, PatchOperationRef, json_diff_zerocopy};
pub use merge::{deep_merge, json_merge_patch, merge_many, merge_patch_to_value};
pub use patch::{JsonPatch, PatchError, PatchOperation, apply_patch, apply_patch_mut};
pub use simd_compare::{
    json_numbers_equal, json_strings_equal, simd_bytes_equal, simd_find_first_difference,
};
