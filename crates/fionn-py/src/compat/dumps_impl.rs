// SPDX-License-Identifier: MIT OR Apache-2.0
//! `dumps()` - Serialize Python object to JSON bytes
//!
//! orjson-compatible JSON serialization with all OPT_* flags.

use pyo3::prelude::*;
use pyo3::types::PyBytes;

use crate::types::py_to_json_bytes;

use super::options::DumpOptions;

/// Serialize Python object to JSON bytes.
///
/// Drop-in replacement for `orjson.dumps()`.
///
/// # Arguments
/// * `obj` - Python object to serialize
/// * `default` - Optional callable for non-serializable objects
/// * `option` - Optional bitwise OR of OPT_* flags
///
/// # Returns
/// JSON bytes
///
/// # Raises
/// * `JSONEncodeError` - If the object cannot be serialized
///
/// # Examples
/// ```python
/// >>> fionn.dumps({"a": 1})
/// b'{"a":1}'
/// >>> fionn.dumps({"a": 1}, option=fionn.OPT_INDENT_2)
/// b'{\n  "a": 1\n}'
/// >>> fionn.dumps({"b": 1, "a": 2}, option=fionn.OPT_SORT_KEYS)
/// b'{"a":2,"b":1}'
/// ```
#[pyfunction]
#[pyo3(signature = (obj, default=None, option=None))]
pub fn dumps(
    py: Python<'_>,
    obj: &Bound<'_, PyAny>,
    default: Option<&Bound<'_, PyAny>>,
    option: Option<u32>,
) -> PyResult<Py<PyBytes>> {
    let options = DumpOptions::from_flags(option.unwrap_or(0));

    // Serialize Python directly to JSON bytes (fast path, no serde intermediary)
    let bytes = py_to_json_bytes(py, obj, default, &options)?;

    // Return as Python bytes
    Ok(PyBytes::new(py, &bytes).into())
}

#[cfg(test)]
mod tests {
    // Python-side tests in tests/test_compat.py
}
