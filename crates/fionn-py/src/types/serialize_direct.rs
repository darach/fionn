// SPDX-License-Identifier: MIT OR Apache-2.0
//! Direct Python to JSON bytes serialization
//!
//! High-performance serialization that writes directly to a byte buffer,
//! bypassing `serde_json::Value` intermediary entirely.

use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};

use crate::compat::Fragment;
use crate::compat::options::DumpOptions;
use crate::exceptions::JSONEncodeError;

/// Maximum recursion depth (orjson limit is 254)
const MAX_RECURSION_DEPTH: usize = 254;

/// Direct Python to JSON serializer
pub struct DirectSerializer<'a> {
    buffer: Vec<u8>,
    options: &'a DumpOptions,
    default: Option<&'a Bound<'a, PyAny>>,
    indent_level: usize,
}

impl<'a> DirectSerializer<'a> {
    /// Create a new serializer with estimated capacity
    pub fn new(
        options: &'a DumpOptions,
        default: Option<&'a Bound<'a, PyAny>>,
        capacity: usize,
    ) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
            options,
            default,
            indent_level: 0,
        }
    }

    /// Serialize a Python object directly to JSON bytes
    pub fn serialize(mut self, py: Python<'_>, obj: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
        self.write_value(py, obj, 0)?;

        if self.options.append_newline {
            self.buffer.push(b'\n');
        }

        Ok(self.buffer)
    }

    fn write_value(
        &mut self,
        py: Python<'_>,
        obj: &Bound<'_, PyAny>,
        depth: usize,
    ) -> PyResult<()> {
        if depth > MAX_RECURSION_DEPTH {
            return Err(PyErr::new::<JSONEncodeError, _>(
                "Recursion limit exceeded (max 254 levels)",
            ));
        }

        // None
        if obj.is_none() {
            self.buffer.extend_from_slice(b"null");
            return Ok(());
        }

        // Boolean (must check before int, as bool is subclass of int in Python)
        if let Ok(b) = obj.cast::<PyBool>() {
            if b.is_true() {
                self.buffer.extend_from_slice(b"true");
            } else {
                self.buffer.extend_from_slice(b"false");
            }
            return Ok(());
        }

        // Integer
        if let Ok(i) = obj.cast::<PyInt>() {
            return self.write_int(i);
        }

        // Float
        if let Ok(f) = obj.cast::<PyFloat>() {
            return self.write_float(f);
        }

        // String
        if let Ok(s) = obj.cast::<PyString>() {
            return self.write_string(s);
        }

        // Fragment (pre-serialized JSON) - write directly
        if let Ok(fragment) = obj.extract::<Fragment>() {
            self.buffer.extend_from_slice(fragment.as_bytes());
            return Ok(());
        }

        // List
        if let Ok(list) = obj.cast::<PyList>() {
            return self.write_list(py, list, depth);
        }

        // Tuple (serialize as array)
        if let Ok(tuple) = obj.cast::<PyTuple>() {
            return self.write_tuple(py, tuple, depth);
        }

        // Dict
        if let Ok(dict) = obj.cast::<PyDict>() {
            return self.write_dict(py, dict, depth);
        }

        // Try default function if provided
        if let Some(default_fn) = self.default {
            let result = default_fn.call1((obj,))?;
            return self.write_value(py, &result, depth + 1);
        }

        // Unsupported type
        Err(PyErr::new::<JSONEncodeError, _>(format!(
            "Type is not JSON serializable: {}",
            obj.get_type().name()?
        )))
    }

    #[inline]
    fn write_int(&mut self, i: &Bound<'_, PyInt>) -> PyResult<()> {
        let val: i64 = i.extract()?;

        if self.options.strict_integer {
            const MAX_SAFE_INT: i64 = 9_007_199_254_740_991;
            const MIN_SAFE_INT: i64 = -9_007_199_254_740_991;
            if !(MIN_SAFE_INT..=MAX_SAFE_INT).contains(&val) {
                return Err(PyErr::new::<JSONEncodeError, _>(format!(
                    "Integer {val} exceeds 53-bit limit with OPT_STRICT_INTEGER"
                )));
            }
        }

        // Use itoa for fast integer formatting
        let mut itoa_buf = itoa::Buffer::new();
        self.buffer
            .extend_from_slice(itoa_buf.format(val).as_bytes());
        Ok(())
    }

    #[inline]
    fn write_float(&mut self, f: &Bound<'_, PyFloat>) -> PyResult<()> {
        let val: f64 = f.extract()?;

        if val.is_nan() || val.is_infinite() {
            return Err(PyErr::new::<JSONEncodeError, _>(
                "Cannot serialize NaN or Infinity",
            ));
        }

        // Use ryu for fast float formatting
        let mut ryu_buf = ryu::Buffer::new();
        self.buffer
            .extend_from_slice(ryu_buf.format(val).as_bytes());
        Ok(())
    }

    #[inline]
    fn write_string(&mut self, s: &Bound<'_, PyString>) -> PyResult<()> {
        let cow = s.to_cow()?;
        self.write_escaped_string(&cow);
        Ok(())
    }

    fn write_escaped_string(&mut self, s: &str) {
        self.buffer.push(b'"');

        for byte in s.bytes() {
            match byte {
                b'"' => self.buffer.extend_from_slice(b"\\\""),
                b'\\' => self.buffer.extend_from_slice(b"\\\\"),
                b'\n' => self.buffer.extend_from_slice(b"\\n"),
                b'\r' => self.buffer.extend_from_slice(b"\\r"),
                b'\t' => self.buffer.extend_from_slice(b"\\t"),
                // Control characters (0x00-0x1F)
                c if c < 0x20 => {
                    self.buffer.extend_from_slice(b"\\u00");
                    let high = (c >> 4) & 0xF;
                    let low = c & 0xF;
                    self.buffer.push(if high < 10 {
                        b'0' + high
                    } else {
                        b'a' + high - 10
                    });
                    self.buffer.push(if low < 10 {
                        b'0' + low
                    } else {
                        b'a' + low - 10
                    });
                }
                c => self.buffer.push(c),
            }
        }

        self.buffer.push(b'"');
    }

    fn write_list(
        &mut self,
        py: Python<'_>,
        list: &Bound<'_, PyList>,
        depth: usize,
    ) -> PyResult<()> {
        self.buffer.push(b'[');

        let mut first = true;
        for item in list.iter() {
            if !first {
                self.buffer.push(b',');
            }
            first = false;

            if self.options.indent.is_some() {
                self.write_newline_indent(depth + 1);
            }

            self.write_value(py, &item, depth + 1)?;
        }

        if !first && self.options.indent.is_some() {
            self.write_newline_indent(depth);
        }

        self.buffer.push(b']');
        Ok(())
    }

    fn write_tuple(
        &mut self,
        py: Python<'_>,
        tuple: &Bound<'_, PyTuple>,
        depth: usize,
    ) -> PyResult<()> {
        self.buffer.push(b'[');

        let mut first = true;
        for item in tuple.iter() {
            if !first {
                self.buffer.push(b',');
            }
            first = false;

            if self.options.indent.is_some() {
                self.write_newline_indent(depth + 1);
            }

            self.write_value(py, &item, depth + 1)?;
        }

        if !first && self.options.indent.is_some() {
            self.write_newline_indent(depth);
        }

        self.buffer.push(b']');
        Ok(())
    }

    fn write_dict(
        &mut self,
        py: Python<'_>,
        dict: &Bound<'_, PyDict>,
        depth: usize,
    ) -> PyResult<()> {
        self.buffer.push(b'{');

        if self.options.sort_keys {
            self.write_dict_sorted(py, dict, depth)
        } else {
            self.write_dict_unsorted(py, dict, depth)
        }
    }

    fn write_dict_unsorted(
        &mut self,
        py: Python<'_>,
        dict: &Bound<'_, PyDict>,
        depth: usize,
    ) -> PyResult<()> {
        let mut first = true;

        for (key, val) in dict.iter() {
            if !first {
                self.buffer.push(b',');
            }
            first = false;

            if self.options.indent.is_some() {
                self.write_newline_indent(depth + 1);
            }

            // Write key
            self.write_dict_key(&key)?;
            self.buffer.push(b':');

            if self.options.indent.is_some() {
                self.buffer.push(b' ');
            }

            // Write value
            self.write_value(py, &val, depth + 1)?;
        }

        if !first && self.options.indent.is_some() {
            self.write_newline_indent(depth);
        }

        self.buffer.push(b'}');
        Ok(())
    }

    fn write_dict_sorted(
        &mut self,
        py: Python<'_>,
        dict: &Bound<'_, PyDict>,
        depth: usize,
    ) -> PyResult<()> {
        // Collect and sort keys
        let mut items: Vec<(String, Bound<'_, PyAny>)> = Vec::with_capacity(dict.len());

        for (key, val) in dict.iter() {
            let key_str = self.extract_key_string(&key)?;
            items.push((key_str, val));
        }

        items.sort_by(|a, b| a.0.cmp(&b.0));

        let mut first = true;
        for (key_str, val) in items {
            if !first {
                self.buffer.push(b',');
            }
            first = false;

            if self.options.indent.is_some() {
                self.write_newline_indent(depth + 1);
            }

            // Write key
            self.write_escaped_string(&key_str);
            self.buffer.push(b':');

            if self.options.indent.is_some() {
                self.buffer.push(b' ');
            }

            // Write value
            self.write_value(py, &val, depth + 1)?;
        }

        if !first && self.options.indent.is_some() {
            self.write_newline_indent(depth);
        }

        self.buffer.push(b'}');
        Ok(())
    }

    fn write_dict_key(&mut self, key: &Bound<'_, PyAny>) -> PyResult<()> {
        if let Ok(s) = key.cast::<PyString>() {
            let cow = s.to_cow()?;
            self.write_escaped_string(&cow);
            Ok(())
        } else if self.options.non_str_keys {
            let s = key.str()?.to_cow()?.into_owned();
            self.write_escaped_string(&s);
            Ok(())
        } else {
            Err(PyErr::new::<JSONEncodeError, _>(
                "Dict keys must be strings (use OPT_NON_STR_KEYS to allow other types)",
            ))
        }
    }

    fn extract_key_string(&self, key: &Bound<'_, PyAny>) -> PyResult<String> {
        if let Ok(s) = key.cast::<PyString>() {
            Ok(s.to_cow()?.into_owned())
        } else if self.options.non_str_keys {
            Ok(key.str()?.to_cow()?.into_owned())
        } else {
            Err(PyErr::new::<JSONEncodeError, _>(
                "Dict keys must be strings (use OPT_NON_STR_KEYS to allow other types)",
            ))
        }
    }

    fn write_newline_indent(&mut self, level: usize) {
        self.buffer.push(b'\n');
        for _ in 0..level {
            self.buffer.extend_from_slice(b"  ");
        }
    }
}

/// Serialize Python object directly to JSON bytes (fast path)
pub fn py_to_json_bytes(
    py: Python<'_>,
    obj: &Bound<'_, PyAny>,
    default: Option<&Bound<'_, PyAny>>,
    options: &DumpOptions,
) -> PyResult<Vec<u8>> {
    let serializer = DirectSerializer::new(options, default, 128);
    serializer.serialize(py, obj)
}

#[cfg(test)]
mod tests {
    // Python-side tests in tests/test_compat.py
}
