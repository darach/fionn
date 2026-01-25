// SPDX-License-Identifier: MIT OR Apache-2.0
//! Gron operations - path-based JSON exploration
//!
//! Convert JSON to gron format and back.

use fionn_gron::{
    GronOptions, GronQueryOptions, Query, gron as rust_gron, gron_query as rust_gron_query,
    ungron_to_value,
};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString};

use crate::exceptions::JSONDecodeError;
use crate::types::json_to_py;

/// Convert JSON to gron format.
///
/// Gron format represents JSON as discrete assignments, making it
/// grep-able and diff-friendly.
///
/// # Arguments
/// * `json` - JSON string to convert
/// * `prefix` - Optional root variable name (default: "json")
/// * `compact` - If True, omit spaces around `=`
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// result = fx.gron('{"a": {"b": 1}}')
/// # 'json = {};\njson.a = {};\njson.a.b = 1;\n'
///
/// # Compact mode
/// result = fx.gron('{"a": {"b": 1}}', compact=True)
/// # 'json={};\njson.a={};\njson.a.b=1;\n'
///
/// # Custom prefix
/// result = fx.gron('{"x": 1}', prefix="data")
/// # 'data = {};\ndata.x = 1;\n'
/// ```
#[pyfunction]
#[pyo3(signature = (json, prefix=None, compact=false))]
pub fn gron(
    py: Python<'_>,
    json: &Bound<'_, PyAny>,
    prefix: Option<&str>,
    compact: bool,
) -> PyResult<String> {
    // Accept both str and bytes
    let json_str: String = if let Ok(s) = json.cast::<PyString>() {
        s.to_string()
    } else if let Ok(b) = json.cast::<PyBytes>() {
        String::from_utf8(b.as_bytes().to_vec())
            .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Invalid UTF-8: {e}")))?
    } else {
        return Err(PyErr::new::<pyo3::exceptions::PyTypeError, _>(
            "json must be str or bytes",
        ));
    };

    // Build gron options
    let mut options = GronOptions::default();
    if let Some(p) = prefix {
        options.prefix = p.to_string();
    }
    if compact {
        options.compact = true;
    }

    // Release GIL during Rust processing
    py.detach(|| rust_gron(&json_str, &options))
        .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Gron error: {e}")))
}

/// Convert gron format back to JSON.
///
/// Reconstructs the original JSON structure from gron assignments.
///
/// # Arguments
/// * `gron_str` - Gron-formatted string
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// gron_data = '''json = {};
/// json.name = "Alice";
/// json.age = 30;
/// '''
/// result = fx.ungron(gron_data)
/// # {"name": "Alice", "age": 30}
/// ```
#[pyfunction]
pub fn ungron(py: Python<'_>, gron_str: &Bound<'_, PyString>) -> PyResult<Py<PyAny>> {
    let input = gron_str.to_string();

    // Release GIL during Rust processing
    let value: serde_json::Value = py
        .detach(|| ungron_to_value(&input))
        .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Ungron error: {e}")))?;

    json_to_py(py, &value)
}

/// Convert JSON to gron and return as bytes (zero-copy optimization).
///
/// Same as `gron()` but returns bytes for lower allocation overhead.
#[pyfunction]
#[pyo3(signature = (json, prefix=None, compact=false))]
pub fn gron_bytes(
    py: Python<'_>,
    json: &Bound<'_, PyAny>,
    prefix: Option<&str>,
    compact: bool,
) -> PyResult<Py<PyBytes>> {
    let result = gron(py, json, prefix, compact)?;
    Ok(PyBytes::new(py, result.as_bytes()).into())
}

/// Query JSON using gron-style paths with optional filtering.
///
/// # Arguments
/// * `json` - JSON string to query
/// * `query` - Query pattern (e.g., ".users[*].name", "json.data")
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// json = '{"users": [{"name": "Alice"}, {"name": "Bob"}]}'
/// result = fx.gron_query(json, ".users")
/// # Returns matching gron lines
/// ```
#[pyfunction]
pub fn gron_query(py: Python<'_>, json: &Bound<'_, PyAny>, query: &str) -> PyResult<String> {
    // Accept both str and bytes
    let json_str: String = if let Ok(s) = json.cast::<PyString>() {
        s.to_string()
    } else if let Ok(b) = json.cast::<PyBytes>() {
        String::from_utf8(b.as_bytes().to_vec())
            .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Invalid UTF-8: {e}")))?
    } else {
        return Err(PyErr::new::<pyo3::exceptions::PyTypeError, _>(
            "json must be str or bytes",
        ));
    };

    // Parse query
    let parsed_query = Query::parse(query)
        .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Invalid query: {e}")))?;

    let options = GronQueryOptions::default();

    py.detach(|| rust_gron_query(&json_str, &parsed_query, &options))
        .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Query error: {e}")))
}
