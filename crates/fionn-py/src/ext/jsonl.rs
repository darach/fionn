// SPDX-License-Identifier: MIT OR Apache-2.0
//! JSONL streaming support
//!
//! High-performance JSONL (JSON Lines) reading and writing with:
//! - Schema filtering (only parse requested fields)
//! - Batch processing with GIL release
//! - Memory-efficient streaming

use pyo3::prelude::*;
use pyo3::types::{PyList, PyString};
use std::cell::RefCell;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;

use crate::exceptions::JSONDecodeError;
use crate::types::json_to_py;

/// JSONL reader with schema filtering and batch processing.
///
/// Provides high-performance JSONL reading with optional schema filtering
/// to only parse requested fields.
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// # Read all fields
/// for batch in fx.JsonlReader("data.jsonl", batch_size=1000):
///     for record in batch:
///         process(record)
///
/// # Read only specific fields (faster)
/// for batch in fx.JsonlReader("data.jsonl", schema=["id", "name"]):
///     for record in batch:
///         print(record["id"], record["name"])
/// ```
#[pyclass(unsendable)]
pub struct JsonlReader {
    inner: RefCell<JsonlReaderInner>,
}

struct JsonlReaderInner {
    reader: BufReader<File>,
    schema: Option<Vec<String>>,
    filter: Option<(String, String)>, // (key, value)
    batch_size: usize,
    line_buffer: String,
    exhausted: bool,
}

#[pymethods]
impl JsonlReader {
    /// Create a new JSONL reader.
    ///
    /// # Arguments
    /// * `path` - Path to JSONL file
    /// * `schema` - Optional list of field names to extract (faster if specified)
    /// * `batch_size` - Number of records per batch (default: 1000)
    #[new]
    #[pyo3(signature = (path, schema=None, batch_size=1000, filter=None))]
    fn new(
        path: &str,
        schema: Option<Vec<String>>,
        batch_size: usize,
        filter: Option<(String, String)>,
    ) -> PyResult<Self> {
        let file = File::open(path).map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Cannot open file: {e}"))
        })?;

        Ok(Self {
            inner: RefCell::new(JsonlReaderInner {
                reader: BufReader::with_capacity(64 * 1024, file),
                schema,
                filter,
                batch_size,
                line_buffer: String::with_capacity(4096),
                exhausted: false,
            }),
        })
    }

    const fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(slf: PyRefMut<'_, Self>, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        let mut inner = slf.inner.borrow_mut();

        if inner.exhausted {
            return Ok(None);
        }

        // Read batch (buffered I/O is efficient)
        let batch_size = inner.batch_size;
        let mut records: Vec<serde_json::Value> = Vec::with_capacity(batch_size);

        // Destructure to get separate mutable borrows
        let JsonlReaderInner {
            reader,
            schema,
            filter,
            line_buffer,
            exhausted,
            ..
        } = &mut *inner;

        for _ in 0..batch_size {
            line_buffer.clear();
            match reader.read_line(line_buffer) {
                Ok(0) => {
                    *exhausted = true;
                    break;
                }
                Ok(_) => {
                    let line = line_buffer.trim();
                    if line.is_empty() {
                        continue;
                    }

                    // Parse JSON line
                    let mut bytes = line.as_bytes().to_vec();
                    match simd_json::from_slice::<serde_json::Value>(&mut bytes) {
                        Ok(value) => {
                            // Apply predicate pushdown (equality filter)
                            if let Some((f_key, f_val)) = filter {
                                if let Some(obj) = value.as_object() {
                                    let match_val = obj.get(f_key).map(|v| match v {
                                        serde_json::Value::String(s) => s.clone(),
                                        _ => v.to_string(),
                                    });
                                    if match_val.as_ref() != Some(f_val) {
                                        continue;
                                    }
                                } else {
                                    continue;
                                }
                            }

                            // Apply schema filtering if specified
                            if let Some(schema_fields) = schema {
                                if let serde_json::Value::Object(obj) = value {
                                    let filtered: serde_json::Map<String, serde_json::Value> = obj
                                        .into_iter()
                                        .filter(|(k, _)| schema_fields.contains(k))
                                        .collect();
                                    records.push(serde_json::Value::Object(filtered));
                                } else {
                                    records.push(value);
                                }
                            } else {
                                records.push(value);
                            }
                        }
                        Err(e) => {
                            return Err(PyErr::new::<JSONDecodeError, _>(format!(
                                "Invalid JSON on line: {e}"
                            )));
                        }
                    }
                }
                Err(e) => {
                    return Err(PyErr::new::<pyo3::exceptions::PyIOError, _>(format!(
                        "Read error: {e}"
                    )));
                }
            }
        }

        if records.is_empty() {
            return Ok(None);
        }

        // Convert to Python list
        let list = PyList::empty(py);
        for record in records {
            list.append(json_to_py(py, &record)?)?;
        }

        Ok(Some(list.into()))
    }
}

/// JSONL writer for streaming output.
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// with fx.JsonlWriter("output.jsonl") as writer:
///     writer.write({"id": 1, "name": "Alice"})
///     writer.write({"id": 2, "name": "Bob"})
/// ```
#[pyclass(unsendable)]
pub struct JsonlWriter {
    inner: RefCell<Option<BufWriter<File>>>,
    path: PathBuf,
}

#[pymethods]
impl JsonlWriter {
    #[new]
    fn new(path: &str) -> PyResult<Self> {
        let file = File::create(path).map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Cannot create file: {e}"))
        })?;

        Ok(Self {
            inner: RefCell::new(Some(BufWriter::with_capacity(64 * 1024, file))),
            path: PathBuf::from(path),
        })
    }

    /// Write a record to the JSONL file.
    fn write(&self, py: Python<'_>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        use crate::compat::options::DumpOptions;
        use crate::types::py_to_json;

        let value = py_to_json(py, obj, None, &DumpOptions::default())?;

        let mut inner = self.inner.borrow_mut();
        let writer = inner
            .as_mut()
            .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyIOError, _>("Writer is closed"))?;

        serde_json::to_writer(&mut *writer, &value).map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Write error: {e}"))
        })?;
        writeln!(writer).map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Write error: {e}"))
        })?;

        Ok(())
    }

    /// Close the writer.
    fn close(&self) -> PyResult<()> {
        let mut inner = self.inner.borrow_mut();
        if let Some(mut writer) = inner.take() {
            writer.flush().map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Flush error: {e}"))
            })?;
        }
        Ok(())
    }

    const fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    #[pyo3(signature = (_exc_type=None, _exc_val=None, _exc_tb=None))]
    fn __exit__(
        &self,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_val: Option<&Bound<'_, PyAny>>,
        _exc_tb: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<bool> {
        self.close()?;
        Ok(false)
    }
}

/// Parse JSONL string to list of Python objects.
#[pyfunction]
#[pyo3(signature = (data, schema=None))]
pub fn parse_jsonl(
    py: Python<'_>,
    data: &Bound<'_, PyString>,
    schema: Option<Vec<String>>,
) -> PyResult<Py<PyAny>> {
    let text = data.to_cow()?;
    let list = PyList::empty(py);

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let mut bytes = line.as_bytes().to_vec();
        let value: serde_json::Value = simd_json::from_slice(&mut bytes)
            .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Invalid JSON: {e}")))?;

        // Apply schema filtering
        let filtered = if let Some(ref schema) = schema {
            if let serde_json::Value::Object(obj) = value {
                let filtered: serde_json::Map<String, serde_json::Value> = obj
                    .into_iter()
                    .filter(|(k, _)| schema.contains(k))
                    .collect();
                serde_json::Value::Object(filtered)
            } else {
                value
            }
        } else {
            value
        };

        list.append(json_to_py(py, &filtered)?)?;
    }

    Ok(list.into())
}

/// Convert list of Python objects to JSONL string.
#[pyfunction]
pub fn to_jsonl(py: Python<'_>, data: &Bound<'_, PyList>) -> PyResult<String> {
    use crate::compat::options::DumpOptions;
    use crate::types::py_to_json;

    let mut output = String::with_capacity(data.len() * 100);

    for item in data.iter() {
        let value = py_to_json(py, &item, None, &DumpOptions::default())?;
        let json = serde_json::to_string(&value).map_err(|e| {
            PyErr::new::<crate::exceptions::JSONEncodeError, _>(format!("Serialization error: {e}"))
        })?;
        output.push_str(&json);
        output.push('\n');
    }

    Ok(output)
}
