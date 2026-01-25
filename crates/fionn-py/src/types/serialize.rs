// SPDX-License-Identifier: MIT OR Apache-2.0
//! Python to JSON conversion
//!
//! Convert Python objects to `serde_json::Value` with orjson-compatible semantics.

use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};
use serde_json::{Map, Number, Value};

use crate::compat::Fragment;
use crate::compat::options::DumpOptions;
use crate::exceptions::JSONEncodeError;

/// Maximum recursion depth (orjson limit is 254)
const MAX_RECURSION_DEPTH: usize = 254;

/// Convert Python object to `serde_json::Value`
pub fn py_to_json(
    py: Python<'_>,
    obj: &Bound<'_, PyAny>,
    default: Option<&Bound<'_, PyAny>>,
    options: &DumpOptions,
) -> PyResult<Value> {
    py_to_json_recursive(py, obj, default, options, 0)
}

fn py_to_json_recursive(
    py: Python<'_>,
    obj: &Bound<'_, PyAny>,
    default: Option<&Bound<'_, PyAny>>,
    options: &DumpOptions,
    depth: usize,
) -> PyResult<Value> {
    // Check recursion limit
    if depth > MAX_RECURSION_DEPTH {
        return Err(PyErr::new::<JSONEncodeError, _>(
            "Recursion limit exceeded (max 254 levels)",
        ));
    }

    // None
    if obj.is_none() {
        return Ok(Value::Null);
    }

    // Boolean (must check before int, as bool is subclass of int in Python)
    if let Ok(b) = obj.cast::<PyBool>() {
        return Ok(Value::Bool(b.is_true()));
    }

    // Integer
    if let Ok(i) = obj.cast::<PyInt>() {
        let val: i64 = i.extract()?;

        // Check strict integer limit (53-bit, JavaScript safe integers)
        if options.strict_integer {
            const MAX_SAFE_INT: i64 = 9_007_199_254_740_991; // 2^53 - 1
            const MIN_SAFE_INT: i64 = -9_007_199_254_740_991;
            if !(MIN_SAFE_INT..=MAX_SAFE_INT).contains(&val) {
                return Err(PyErr::new::<JSONEncodeError, _>(format!(
                    "Integer {val} exceeds 53-bit limit with OPT_STRICT_INTEGER"
                )));
            }
        }

        return Ok(Value::Number(Number::from(val)));
    }

    // Float
    if let Ok(f) = obj.cast::<PyFloat>() {
        let val: f64 = f.extract()?;
        if val.is_nan() || val.is_infinite() {
            return Err(PyErr::new::<JSONEncodeError, _>(
                "Cannot serialize NaN or Infinity",
            ));
        }
        return Ok(Value::Number(Number::from_f64(val).ok_or_else(|| {
            PyErr::new::<JSONEncodeError, _>("Invalid float value")
        })?));
    }

    // String
    if let Ok(s) = obj.cast::<PyString>() {
        return Ok(Value::String(s.to_cow()?.into_owned()));
    }

    // Fragment (pre-serialized JSON)
    if let Ok(fragment) = obj.extract::<Fragment>() {
        // Parse the fragment as JSON value
        let mut bytes = fragment.as_bytes().to_vec();
        let value: Value = simd_json::from_slice(&mut bytes)
            .map_err(|e| PyErr::new::<JSONEncodeError, _>(format!("Invalid Fragment JSON: {e}")))?;
        return Ok(value);
    }

    // List
    if let Ok(list) = obj.cast::<PyList>() {
        let mut arr = Vec::with_capacity(list.len());
        for item in list.iter() {
            arr.push(py_to_json_recursive(
                py,
                &item,
                default,
                options,
                depth + 1,
            )?);
        }
        return Ok(Value::Array(arr));
    }

    // Tuple (serialize as array)
    if let Ok(tuple) = obj.cast::<PyTuple>() {
        let mut arr = Vec::with_capacity(tuple.len());
        for item in tuple.iter() {
            arr.push(py_to_json_recursive(
                py,
                &item,
                default,
                options,
                depth + 1,
            )?);
        }
        return Ok(Value::Array(arr));
    }

    // Dict
    if let Ok(dict) = obj.cast::<PyDict>() {
        let mut map = Map::with_capacity(dict.len());

        // Collect keys for optional sorting
        let mut items: Vec<(String, Value)> = Vec::with_capacity(dict.len());

        for (key, val) in dict.iter() {
            // Convert key to string
            let key_str = if let Ok(s) = key.cast::<PyString>() {
                s.to_cow()?.into_owned()
            } else if options.non_str_keys {
                // Non-string keys allowed: convert to string representation
                key.str()?.to_cow()?.into_owned()
            } else {
                return Err(PyErr::new::<JSONEncodeError, _>(
                    "Dict keys must be strings (use OPT_NON_STR_KEYS to allow other types)",
                ));
            };

            let value = py_to_json_recursive(py, &val, default, options, depth + 1)?;
            items.push((key_str, value));
        }

        // Sort keys if requested
        if options.sort_keys {
            items.sort_by(|a, b| a.0.cmp(&b.0));
        }

        for (key, value) in items {
            map.insert(key, value);
        }

        return Ok(Value::Object(map));
    }

    // Try default function if provided
    if let Some(default_fn) = default {
        let result = default_fn.call1((obj,))?;
        return py_to_json_recursive(py, &result, None, options, depth + 1);
    }

    // Unsupported type
    Err(PyErr::new::<JSONEncodeError, _>(format!(
        "Type is not JSON serializable: {}",
        obj.get_type().name()?
    )))
}

#[cfg(test)]
mod tests {
    // Python-side tests in tests/test_compat.py
}
