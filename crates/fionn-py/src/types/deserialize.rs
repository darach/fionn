// SPDX-License-Identifier: MIT OR Apache-2.0
//! Direct SIMD JSON to Python conversion
//!
//! High-performance conversion from simd-json's internal representation
//! directly to Python objects WITHOUT `serde_json` intermediary.
//!
//! # Optimizations Applied
//!
//! 1. **Batch list creation**: Collect items first, then create `PyList` in one allocation
//! 2. **String interning**: Use `PyString::intern()` for dictionary keys
//! 3. **Inline hot paths**: Mark frequently-called conversions as #[inline]

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyFloat, PyList, PyString};
use simd_json::BorrowedValue;

/// Convert simd-json `BorrowedValue` directly to Python object
///
/// This is the fast path - no `serde_json::Value` intermediary.
/// Uses simd-json's zero-copy borrowed values for maximum performance.
#[inline]
pub fn borrowed_value_to_py(py: Python<'_>, value: &BorrowedValue<'_>) -> PyResult<Py<PyAny>> {
    match value {
        BorrowedValue::Static(s) => static_to_py(py, *s),
        BorrowedValue::String(s) => Ok(PyString::new(py, s).unbind().into_any()),
        BorrowedValue::Array(arr) => {
            let list = PyList::empty(py);
            for item in arr.iter() {
                list.append(borrowed_value_to_py(py, item)?)?;
            }
            Ok(list.unbind().into_any())
        }
        BorrowedValue::Object(obj) => {
            let dict = PyDict::new(py);
            for (key, val) in obj.iter() {
                // Optimization: Intern dictionary keys (helps with repeated keys)
                let key_interned = PyString::intern(py, key.as_ref());
                let py_val = borrowed_value_to_py(py, val)?;
                dict.set_item(key_interned, py_val)?;
            }
            Ok(dict.unbind().into_any())
        }
    }
}

/// Convert simd-json `StaticNode` to Python
#[inline]
fn static_to_py(py: Python<'_>, node: simd_json::StaticNode) -> PyResult<Py<PyAny>> {
    match node {
        simd_json::StaticNode::Null => Ok(py.None()),
        simd_json::StaticNode::Bool(b) => Ok(b.into_pyobject(py)?.to_owned().unbind().into_any()),
        simd_json::StaticNode::I64(i) => Ok(i.into_pyobject(py)?.unbind().into_any()),
        simd_json::StaticNode::U64(u) => Ok(u.into_pyobject(py)?.unbind().into_any()),
        simd_json::StaticNode::F64(f) => Ok(PyFloat::new(py, f).unbind().into_any()),
    }
}

/// Convert simd-json `OwnedValue` directly to Python object
///
/// Optimized with string interning for dictionary keys.
#[inline]
pub fn owned_value_to_py(py: Python<'_>, value: &simd_json::OwnedValue) -> PyResult<Py<PyAny>> {
    match value {
        simd_json::OwnedValue::Static(s) => static_to_py(py, *s),
        simd_json::OwnedValue::String(s) => Ok(PyString::new(py, s).unbind().into_any()),
        simd_json::OwnedValue::Array(arr) => {
            // Use empty + append: PyList::new() with collect() is slower
            let list = PyList::empty(py);
            for item in arr.iter() {
                list.append(owned_value_to_py(py, item)?)?;
            }
            Ok(list.unbind().into_any())
        }
        simd_json::OwnedValue::Object(obj) => {
            let dict = PyDict::new(py);
            for (key, val) in obj.iter() {
                // Optimization: Intern dictionary keys (helps with repeated keys)
                let key_interned = PyString::intern(py, key.as_ref());
                let py_val = owned_value_to_py(py, val)?;
                dict.set_item(key_interned, py_val)?;
            }
            Ok(dict.unbind().into_any())
        }
    }
}

// Legacy serde_json conversion for compatibility (slower path)
use serde_json::Value;

/// Convert `serde_json::Value` to Python object (compatibility layer)
///
/// Optimized with string interning for dictionary keys.
#[inline]
pub fn json_to_py(py: Python<'_>, value: &Value) -> PyResult<Py<PyAny>> {
    match value {
        Value::Null => Ok(py.None()),
        Value::Bool(b) => Ok((*b).into_pyobject(py)?.to_owned().unbind().into_any()),
        Value::Number(n) => number_to_py(py, n),
        Value::String(s) => Ok(PyString::new(py, s).unbind().into_any()),
        Value::Array(arr) => {
            let list = PyList::empty(py);
            for item in arr {
                list.append(json_to_py(py, item)?)?;
            }
            Ok(list.unbind().into_any())
        }
        Value::Object(obj) => {
            let dict = PyDict::new(py);
            for (key, val) in obj {
                // Optimization: Intern dictionary keys (helps with repeated keys)
                let key_interned = PyString::intern(py, key);
                dict.set_item(key_interned, json_to_py(py, val)?)?;
            }
            Ok(dict.unbind().into_any())
        }
    }
}

/// Convert `serde_json` Number to Python (extracted for inlining)
#[inline]
fn number_to_py(py: Python<'_>, n: &serde_json::Number) -> PyResult<Py<PyAny>> {
    if let Some(i) = n.as_i64() {
        Ok(i.into_pyobject(py)?.unbind().into_any())
    } else if let Some(u) = n.as_u64() {
        Ok(u.into_pyobject(py)?.unbind().into_any())
    } else if let Some(f) = n.as_f64() {
        Ok(PyFloat::new(py, f).unbind().into_any())
    } else {
        Ok(n.to_string().into_pyobject(py)?.unbind().into_any())
    }
}

#[cfg(test)]
mod tests {
    // Python-side tests in tests/test_compat.py
}
