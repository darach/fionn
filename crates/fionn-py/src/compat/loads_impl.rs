// SPDX-License-Identifier: MIT OR Apache-2.0
//! `loads()` - Parse JSON bytes to Python object
//!
//! orjson-compatible JSON deserialization with SIMD acceleration.

use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString};

use crate::exceptions::JSONDecodeError;
use crate::types::owned_value_to_py;

/// Threshold below which we skip GIL release (overhead > benefit for tiny payloads)
const SMALL_INPUT_THRESHOLD: usize = 256;

/// Parse JSON bytes to Python object.
///
/// Drop-in replacement for `orjson.loads()`.
///
/// # Arguments
/// * `data` - JSON bytes, bytearray, memoryview, or str
///
/// # Returns
/// Python object (dict, list, str, int, float, bool, or None)
///
/// # Raises
/// * `JSONDecodeError` - If the input is not valid JSON
///
/// # Examples
/// ```python
/// >>> fionn.loads(b'{"a": 1}')
/// {'a': 1}
/// >>> fionn.loads(b'[1, 2, 3]')
/// [1, 2, 3]
/// >>> fionn.loads(b'"hello"')
/// 'hello'
/// ```
#[pyfunction]
#[pyo3(signature = (data))]
pub fn loads(py: Python<'_>, data: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    // Extract bytes from various input types (bytes, bytearray, memoryview, str)
    let mut json_bytes: Vec<u8> = extract_bytes(data)?;

    // For small inputs: skip GIL release overhead (parsing is fast anyway)
    let value: simd_json::OwnedValue = if json_bytes.len() < SMALL_INPUT_THRESHOLD {
        simd_json::to_owned_value(&mut json_bytes)
            .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Invalid JSON: {e}")))?
    } else {
        // For larger inputs: release GIL during SIMD parse
        py.detach(|| {
            simd_json::to_owned_value(&mut json_bytes)
                .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Invalid JSON: {e}")))
        })?
    };

    // Convert simd-json OwnedValue directly to Python object (fast path)
    owned_value_to_py(py, &value)
}

/// Extract bytes from various Python types
fn extract_bytes(data: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    // Try bytes first (most common case)
    if let Ok(bytes) = data.cast::<PyBytes>() {
        return Ok(bytes.as_bytes().to_vec());
    }

    // Try str (convert to UTF-8 bytes)
    if let Ok(s) = data.cast::<PyString>() {
        let cow = s.to_cow()?;
        return Ok(cow.as_bytes().to_vec());
    }

    // Try buffer protocol (bytearray, memoryview)
    if let Ok(buffer) = data.extract::<Vec<u8>>() {
        return Ok(buffer);
    }

    Err(PyErr::new::<pyo3::exceptions::PyTypeError, _>(
        "loads() argument must be bytes, bytearray, memoryview, or str",
    ))
}

#[cfg(test)]
mod tests {
    // Python-side tests in tests/test_compat.py
}
