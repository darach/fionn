// SPDX-License-Identifier: MIT OR Apache-2.0
//! Diff/Patch/Merge operations
//!
//! RFC 6902 JSON Patch and RFC 7396 Merge Patch support.

use fionn_diff::{
    JsonPatch, PatchOperation, apply_patch as rust_apply_patch, deep_merge as rust_deep_merge,
    json_diff, json_merge_patch,
};
use pyo3::prelude::*;
use pyo3::types::PyList;

use crate::compat::options::DumpOptions;
use crate::exceptions::JSONDecodeError;
use crate::types::{json_to_py, py_to_json};

/// Compute diff between two documents (RFC 6902 JSON Patch).
///
/// Returns a list of patch operations that transform `source` into `target`.
///
/// # Arguments
/// * `source` - Original document (dict or list)
/// * `target` - Modified document (dict or list)
///
/// # Returns
/// List of patch operations: [{"op": "add", "path": "/x", "value": 1}, ...]
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// source = {"name": "Alice", "age": 30}
/// target = {"name": "Alice", "age": 31, "city": "NYC"}
///
/// patch = fx.diff(source, target)
/// # [{"op": "replace", "path": "/age", "value": 31},
/// #  {"op": "add", "path": "/city", "value": "NYC"}]
/// ```
#[pyfunction]
pub fn diff(
    py: Python<'_>,
    source: &Bound<'_, PyAny>,
    target: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let source_val = py_to_json(py, source, None, &DumpOptions::default())?;
    let target_val = py_to_json(py, target, None, &DumpOptions::default())?;

    // Release GIL during diff computation (returns JsonPatch directly)
    let patch: JsonPatch = py.detach(|| json_diff(&source_val, &target_val));

    // Convert patch to serde_json::Value for Python conversion
    let patch_json = serde_json::to_value(&patch)
        .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Serialize error: {e}")))?;

    json_to_py(py, &patch_json)
}

/// Apply patch to document (RFC 6902).
///
/// # Arguments
/// * `document` - Document to patch (dict or list)
/// * `patch` - List of patch operations
///
/// # Returns
/// New document with patch applied
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// doc = {"name": "Alice", "age": 30}
/// patch = [{"op": "replace", "path": "/age", "value": 31}]
///
/// result = fx.patch(doc, patch)
/// # {"name": "Alice", "age": 31}
/// ```
#[pyfunction]
pub fn patch(
    py: Python<'_>,
    document: &Bound<'_, PyAny>,
    patch_ops: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let doc_val = py_to_json(py, document, None, &DumpOptions::default())?;
    let patch_val = py_to_json(py, patch_ops, None, &DumpOptions::default())?;

    // Parse patch operations
    let json_patch: JsonPatch = serde_json::from_value(patch_val)
        .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Invalid patch: {e}")))?;

    // Release GIL during patch application
    let result = py
        .detach(|| rust_apply_patch(&doc_val, &json_patch))
        .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Patch error: {e}")))?;

    json_to_py(py, &result)
}

/// Merge two documents (RFC 7396 Merge Patch).
///
/// Overlays `overlay` onto `base` using merge-patch semantics:
/// - null values in overlay delete keys
/// - objects are merged recursively
/// - other values replace base values
///
/// # Arguments
/// * `base` - Base document
/// * `overlay` - Overlay/patch document
///
/// # Returns
/// Merged document
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// base = {"name": "Alice", "age": 30, "city": "LA"}
/// overlay = {"age": 31, "city": None}  # None deletes "city"
///
/// result = fx.merge(base, overlay)
/// # {"name": "Alice", "age": 31}
/// ```
#[pyfunction]
pub fn merge(
    py: Python<'_>,
    base: &Bound<'_, PyAny>,
    overlay: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let base_val = py_to_json(py, base, None, &DumpOptions::default())?;
    let overlay_val = py_to_json(py, overlay, None, &DumpOptions::default())?;

    // Release GIL during merge (returns Value directly)
    let result = py.detach(|| json_merge_patch(&base_val, &overlay_val));

    json_to_py(py, &result)
}

/// Deep merge two documents.
///
/// Unlike RFC 7396 merge, this preserves both values when types differ
/// and recursively merges objects and arrays.
///
/// # Arguments
/// * `base` - Base document
/// * `overlay` - Overlay document
///
/// # Returns
/// Deep-merged document
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// base = {"user": {"name": "Alice", "prefs": {"theme": "dark"}}}
/// overlay = {"user": {"prefs": {"lang": "en"}}}
///
/// result = fx.deep_merge(base, overlay)
/// # {"user": {"name": "Alice", "prefs": {"theme": "dark", "lang": "en"}}}
/// ```
#[pyfunction]
pub fn deep_merge(
    py: Python<'_>,
    base: &Bound<'_, PyAny>,
    overlay: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let base_val = py_to_json(py, base, None, &DumpOptions::default())?;
    let overlay_val = py_to_json(py, overlay, None, &DumpOptions::default())?;

    // Release GIL during deep merge (returns Value directly)
    let result = py.detach(|| rust_deep_merge(&base_val, &overlay_val));

    json_to_py(py, &result)
}

/// Helper to get path from a `PatchOperation`
fn get_op_path(op: &PatchOperation) -> &str {
    match op {
        PatchOperation::Add { path, .. } => path,
        PatchOperation::Remove { path } => path,
        PatchOperation::Replace { path, .. } => path,
        PatchOperation::Move { path, .. } => path,
        PatchOperation::Copy { path, .. } => path,
        PatchOperation::Test { path, .. } => path,
    }
}

/// Three-way merge for concurrent edits.
///
/// Merges `ours` and `theirs` relative to common ancestor `base`.
/// Detects conflicts where both sides modified the same path differently.
///
/// # Arguments
/// * `base` - Common ancestor document
/// * `ours` - Our modified version
/// * `theirs` - Their modified version
///
/// # Returns
/// Tuple of (`merged_document`, `list_of_conflicts`)
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// base = {"name": "Alice", "age": 30}
/// ours = {"name": "Alice", "age": 31}  # We changed age
/// theirs = {"name": "Bob", "age": 30}  # They changed name
///
/// result, conflicts = fx.three_way_merge(base, ours, theirs)
/// # result: {"name": "Bob", "age": 31}  # Both changes merged
/// # conflicts: []  # No conflicts - different fields
/// ```
#[pyfunction]
pub fn three_way_merge(
    py: Python<'_>,
    base: &Bound<'_, PyAny>,
    ours: &Bound<'_, PyAny>,
    theirs: &Bound<'_, PyAny>,
) -> PyResult<(Py<PyAny>, Py<PyAny>)> {
    let base_val = py_to_json(py, base, None, &DumpOptions::default())?;
    let ours_val = py_to_json(py, ours, None, &DumpOptions::default())?;
    let theirs_val = py_to_json(py, theirs, None, &DumpOptions::default())?;

    // Compute patches from base (these return JsonPatch directly)
    let ours_patch = json_diff(&base_val, &ours_val);
    let theirs_patch = json_diff(&base_val, &theirs_val);

    // Apply our changes first
    let mut result = rust_apply_patch(&base_val, &ours_patch)
        .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Patch error (ours): {e}")))?;

    // Detect conflicts
    let mut conflicts: Vec<serde_json::Value> = Vec::new();

    for their_op in &theirs_patch.operations {
        let path = get_op_path(their_op);
        let conflict = ours_patch
            .operations
            .iter()
            .any(|our_op| get_op_path(our_op) == path);

        if conflict {
            conflicts.push(serde_json::json!({
                "path": path,
                "ours": ours_val.pointer(path),
                "theirs": theirs_val.pointer(path),
            }));
        }
    }

    // Apply their changes (last-writer-wins for conflicts)
    result = rust_apply_patch(&result, &theirs_patch)
        .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Patch error (theirs): {e}")))?;

    let result_py = json_to_py(py, &result)?;
    let conflicts_py = json_to_py(py, &serde_json::Value::Array(conflicts))?;

    Ok((result_py, conflicts_py))
}

/// Compute diff and return as bytes (zero-copy optimization).
#[pyfunction]
pub fn diff_bytes(
    py: Python<'_>,
    source: &Bound<'_, PyAny>,
    target: &Bound<'_, PyAny>,
) -> PyResult<Py<pyo3::types::PyBytes>> {
    let source_val = py_to_json(py, source, None, &DumpOptions::default())?;
    let target_val = py_to_json(py, target, None, &DumpOptions::default())?;

    let patch = py.detach(|| json_diff(&source_val, &target_val));

    let bytes = serde_json::to_vec(&patch)
        .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Serialize error: {e}")))?;

    Ok(pyo3::types::PyBytes::new(py, &bytes).into())
}

/// Batch diff multiple document pairs.
///
/// Efficiently diffs multiple (source, target) pairs in a single call.
///
/// # Arguments
/// * `pairs` - List of (source, target) tuples
///
/// # Returns
/// List of patches
#[pyfunction]
pub fn batch_diff(py: Python<'_>, pairs: &Bound<'_, PyList>) -> PyResult<Py<PyAny>> {
    let mut results: Vec<serde_json::Value> = Vec::with_capacity(pairs.len());

    for item in pairs.iter() {
        let tuple = item.cast::<pyo3::types::PyTuple>()?;
        if tuple.len() != 2 {
            return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                "Each pair must be a (source, target) tuple",
            ));
        }

        let source = tuple.get_item(0)?;
        let target = tuple.get_item(1)?;

        let source_val = py_to_json(py, &source, None, &DumpOptions::default())?;
        let target_val = py_to_json(py, &target, None, &DumpOptions::default())?;

        let patch = json_diff(&source_val, &target_val);

        let patch_json = serde_json::to_value(&patch)
            .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Serialize error: {e}")))?;

        results.push(patch_json);
    }

    json_to_py(py, &serde_json::Value::Array(results))
}
