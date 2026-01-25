// SPDX-License-Identifier: MIT OR Apache-2.0
//! Stream processing pipeline
//!
//! Build pipelines for processing JSONL and ISONL streams.

use pyo3::prelude::*;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};

use crate::compat::options::DumpOptions;
use crate::exceptions::JSONDecodeError;
use crate::types::{json_to_py, py_to_json};

/// Stream processing pipeline for JSONL/ISONL.
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// pipeline = fx.Pipeline()
/// pipeline.filter(lambda x: x["active"])
/// pipeline.map(lambda x: {"id": x["id"], "score": x["score"] * 2})
///
/// # Process JSONL
/// pipeline.process_jsonl("input.jsonl", "output.jsonl")
///
/// # Process ISONL (11.9x faster)
/// pipeline.process_isonl("input.isonl", "output.isonl")
/// ```
#[pyclass]
pub struct Pipeline {
    stages: Vec<PipelineStage>,
}

enum PipelineStage {
    Filter(Py<PyAny>),
    Map(Py<PyAny>),
}

#[pymethods]
impl Pipeline {
    #[new]
    const fn new() -> Self {
        Self { stages: Vec::new() }
    }

    /// Add a filter stage.
    ///
    /// The predicate function should return True to keep the record, False to drop it.
    ///
    /// Args:
    ///     predicate: A callable that takes a record (dict) and returns bool
    ///
    /// Returns:
    ///     self for method chaining
    ///
    /// Example:
    /// ```python
    /// pipeline.filter(lambda x: x.get("active", False))
    /// ```
    fn filter(mut slf: PyRefMut<'_, Self>, predicate: Py<PyAny>) -> PyRefMut<'_, Self> {
        slf.stages.push(PipelineStage::Filter(predicate));
        slf
    }

    /// Add a map (transform) stage.
    ///
    /// The transform function takes a record and returns a new/modified record.
    ///
    /// Args:
    ///     transform: A callable that takes a record (dict) and returns a dict
    ///
    /// Returns:
    ///     self for method chaining
    ///
    /// Example:
    /// ```python
    /// pipeline.map(lambda x: {"id": x["id"], "score": x["score"] * 2})
    /// ```
    fn map(mut slf: PyRefMut<'_, Self>, transform: Py<PyAny>) -> PyRefMut<'_, Self> {
        slf.stages.push(PipelineStage::Map(transform));
        slf
    }

    /// Process JSONL file through pipeline.
    ///
    /// Args:
    ///     `input_path`: Path to input JSONL file
    ///     `output_path`: Path to output JSONL file
    ///
    /// Returns:
    ///     Number of records processed
    ///
    /// Example:
    /// ```python
    /// count = pipeline.process_jsonl("input.jsonl", "output.jsonl")
    /// print(f"Processed {count} records")
    /// ```
    fn process_jsonl(&self, py: Python<'_>, input_path: &str, output_path: &str) -> PyResult<u64> {
        let input_file = File::open(input_path).map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Cannot open input file: {e}"))
        })?;
        let output_file = File::create(output_path).map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Cannot create output file: {e}"))
        })?;

        let reader = BufReader::with_capacity(64 * 1024, input_file);
        let mut writer = BufWriter::with_capacity(64 * 1024, output_file);

        let mut count = 0u64;
        let mut line_buffer = String::with_capacity(4096);
        let opts = DumpOptions::default();

        for line_result in reader.lines() {
            let line = line_result.map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Read error: {e}"))
            })?;

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Parse JSON line
            let mut bytes = trimmed.as_bytes().to_vec();
            let value: serde_json::Value = simd_json::from_slice(&mut bytes)
                .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Invalid JSON: {e}")))?;

            // Convert to Python for pipeline stages
            let mut py_obj = json_to_py(py, &value)?;

            // Apply pipeline stages
            let mut keep = true;
            for stage in &self.stages {
                match stage {
                    PipelineStage::Filter(predicate) => {
                        let result = predicate.call1(py, (&py_obj,))?;
                        if !result.extract::<bool>(py)? {
                            keep = false;
                            break;
                        }
                    }
                    PipelineStage::Map(transform) => {
                        py_obj = transform.call1(py, (&py_obj,))?;
                    }
                }
            }

            if keep {
                // Convert back to JSON and write
                let result_value = py_to_json(py, py_obj.bind(py), None, &opts)?;
                let json_str = serde_json::to_string(&result_value).map_err(|e| {
                    PyErr::new::<crate::exceptions::JSONEncodeError, _>(format!(
                        "Serialization error: {e}"
                    ))
                })?;

                line_buffer.clear();
                line_buffer.push_str(&json_str);
                line_buffer.push('\n');
                writer.write_all(line_buffer.as_bytes()).map_err(|e| {
                    PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Write error: {e}"))
                })?;

                count += 1;
            }
        }

        writer.flush().map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Flush error: {e}"))
        })?;

        Ok(count)
    }

    /// Process ISONL file through pipeline (11.9x faster than JSONL).
    ///
    /// ISONL (Inline Schema Object Notation Lines) uses self-describing lines
    /// with embedded schema for faster parsing.
    ///
    /// Args:
    ///     `input_path`: Path to input ISONL file
    ///     `output_path`: Path to output file (ISONL format)
    ///
    /// Returns:
    ///     Number of records processed
    fn process_isonl(&self, py: Python<'_>, input_path: &str, output_path: &str) -> PyResult<u64> {
        // ISONL processing - parse the ISONL format and apply pipeline
        let input_file = File::open(input_path).map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Cannot open input file: {e}"))
        })?;
        let output_file = File::create(output_path).map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Cannot create output file: {e}"))
        })?;

        let reader = BufReader::with_capacity(64 * 1024, input_file);
        let mut writer = BufWriter::with_capacity(64 * 1024, output_file);

        let mut count = 0u64;

        for line_result in reader.lines() {
            let line = line_result.map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Read error: {e}"))
            })?;

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Parse ISONL line: table.name|field1:type|field2:type|val1|val2
            let parts: Vec<&str> = trimmed.split('|').collect();
            if parts.len() < 3 {
                continue;
            }

            // Find schema/data boundary (where type annotations end)
            let mut schema_end = 0;
            for (i, part) in parts.iter().enumerate() {
                if i == 0 {
                    schema_end = 1;
                    continue;
                }
                if part.contains(':') {
                    schema_end = i + 1;
                } else {
                    break;
                }
            }

            // Build Python dict from ISONL
            let dict = pyo3::types::PyDict::new(py);

            // Schema fields (skip table name)
            let fields: Vec<(&str, &str)> = parts[1..schema_end]
                .iter()
                .filter_map(|f| {
                    let (name, typ) = f.split_once(':')?;

                    Some((name, typ))
                })
                .collect();

            // Values
            let values = &parts[schema_end..];

            for (i, (name, typ)) in fields.iter().enumerate() {
                if i >= values.len() {
                    break;
                }
                let val_str = values[i];

                let py_val: Py<PyAny> = match *typ {
                    "int" | "i64" | "i32" => val_str.parse::<i64>().ok().map_or_else(
                        || py.None(),
                        |v| v.into_pyobject(py).unwrap().unbind().into_any(),
                    ),
                    "float" | "f64" | "f32" => val_str.parse::<f64>().ok().map_or_else(
                        || py.None(),
                        |v| pyo3::types::PyFloat::new(py, v).unbind().into_any(),
                    ),
                    "bool" => {
                        let b = val_str == "true" || val_str == "1";
                        b.into_pyobject(py).unwrap().to_owned().unbind().into_any()
                    }
                    _ => {
                        // Default to string
                        pyo3::types::PyString::new(py, val_str).unbind().into_any()
                    }
                };

                dict.set_item(*name, py_val)?;
            }

            // Apply pipeline stages
            let mut py_obj: Py<PyAny> = dict.into_any().unbind();
            let mut keep = true;

            for stage in &self.stages {
                match stage {
                    PipelineStage::Filter(predicate) => {
                        let result = predicate.call1(py, (&py_obj,))?;
                        if !result.extract::<bool>(py)? {
                            keep = false;
                            break;
                        }
                    }
                    PipelineStage::Map(transform) => {
                        py_obj = transform.call1(py, (&py_obj,))?;
                    }
                }
            }

            if keep {
                // Write back as ISONL
                // Reconstruct ISONL line from dict
                let result_dict = py_obj.bind(py);
                let mut output_parts = vec![parts[0].to_string()];

                // Add schema
                for field_schema in &parts[1..schema_end] {
                    output_parts.push(field_schema.to_string());
                }

                // Add values
                for (name, _) in &fields {
                    match result_dict.get_item(*name) {
                        Ok(val) => {
                            let s = val.str()?.to_string();
                            output_parts.push(s);
                        }
                        Err(_) => {
                            output_parts.push(String::new());
                        }
                    }
                }

                let line_out = output_parts.join("|");
                writeln!(writer, "{line_out}").map_err(|e| {
                    PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Write error: {e}"))
                })?;

                count += 1;
            }
        }

        writer.flush().map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Flush error: {e}"))
        })?;

        Ok(count)
    }

    /// Process with format conversion.
    ///
    /// Supports conversion between JSONL, ISONL, and other formats.
    ///
    /// Args:
    ///     `input_path`: Path to input file
    ///     `input_format`: Input format ("jsonl", "isonl", "json")
    ///     `output_path`: Path to output file
    ///     `output_format`: Output format ("jsonl", "isonl", "json")
    ///     `output_schema`: Optional list of fields to include in output
    ///
    /// Returns:
    ///     Number of records processed
    #[pyo3(signature = (input_path, input_format, output_path, output_format, output_schema=None))]
    fn process(
        &self,
        py: Python<'_>,
        input_path: &str,
        input_format: &str,
        output_path: &str,
        output_format: &str,
        output_schema: Option<Vec<String>>,
    ) -> PyResult<u64> {
        // Read input based on format
        let records = match input_format.to_lowercase().as_str() {
            "jsonl" => self.read_jsonl(py, input_path)?,
            "isonl" => self.read_isonl(py, input_path)?,
            "json" => self.read_json(py, input_path)?,
            _ => {
                return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(format!(
                    "Unsupported input format: {input_format}"
                )));
            }
        };

        // Apply pipeline stages to each record
        let mut processed: Vec<Py<PyAny>> = Vec::with_capacity(records.len());

        for record in records {
            let mut py_obj = record;
            let mut keep = true;

            for stage in &self.stages {
                match stage {
                    PipelineStage::Filter(predicate) => {
                        let result = predicate.call1(py, (&py_obj,))?;
                        if !result.extract::<bool>(py)? {
                            keep = false;
                            break;
                        }
                    }
                    PipelineStage::Map(transform) => {
                        py_obj = transform.call1(py, (&py_obj,))?;
                    }
                }
            }

            if keep {
                // Apply output schema if specified
                if let Some(ref schema) = output_schema {
                    let new_dict = pyo3::types::PyDict::new(py);
                    let source_bound = py_obj.bind(py);
                    // Try to downcast to PyDict for proper get_item semantics
                    if let Ok(source_dict) = source_bound.cast::<pyo3::types::PyDict>() {
                        for field in schema {
                            if let Some(v) = source_dict.get_item(field)? {
                                new_dict.set_item(field, v)?;
                            }
                        }
                    }
                    py_obj = new_dict.into_any().unbind();
                }
                processed.push(py_obj);
            }
        }

        // Write output based on format
        let count = processed.len() as u64;
        match output_format.to_lowercase().as_str() {
            "jsonl" => self.write_jsonl(py, output_path, &processed)?,
            "isonl" => self.write_isonl(py, output_path, &processed, &output_schema)?,
            "json" => self.write_json(py, output_path, &processed)?,
            _ => {
                return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(format!(
                    "Unsupported output format: {output_format}"
                )));
            }
        }

        Ok(count)
    }

    fn __repr__(&self) -> String {
        let stage_desc: Vec<&str> = self
            .stages
            .iter()
            .map(|s| match s {
                PipelineStage::Filter(_) => "filter",
                PipelineStage::Map(_) => "map",
            })
            .collect();
        format!("Pipeline(stages=[{}])", stage_desc.join(", "))
    }

    const fn __len__(&self) -> usize {
        self.stages.len()
    }
}

// Helper methods for Pipeline (not exposed to Python)
impl Pipeline {
    fn read_jsonl(&self, py: Python<'_>, path: &str) -> PyResult<Vec<Py<PyAny>>> {
        let file = File::open(path).map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Cannot open file: {e}"))
        })?;
        let reader = BufReader::new(file);
        let mut records = Vec::new();

        for line_result in reader.lines() {
            let line = line_result.map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Read error: {e}"))
            })?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let mut bytes = trimmed.as_bytes().to_vec();
            let value: serde_json::Value = simd_json::from_slice(&mut bytes)
                .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Invalid JSON: {e}")))?;

            records.push(json_to_py(py, &value)?);
        }

        Ok(records)
    }

    fn read_isonl(&self, py: Python<'_>, path: &str) -> PyResult<Vec<Py<PyAny>>> {
        let file = File::open(path).map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Cannot open file: {e}"))
        })?;
        let reader = BufReader::new(file);
        let mut records = Vec::new();

        for line_result in reader.lines() {
            let line = line_result.map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Read error: {e}"))
            })?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Parse ISONL format
            let parts: Vec<&str> = trimmed.split('|').collect();
            if parts.len() < 3 {
                continue;
            }

            let dict = pyo3::types::PyDict::new(py);

            // Find schema boundary
            let mut schema_end = 1;
            for (i, part) in parts.iter().enumerate().skip(1) {
                if part.contains(':') {
                    schema_end = i + 1;
                } else {
                    break;
                }
            }

            // Parse schema and values
            let fields: Vec<(&str, &str)> = parts[1..schema_end]
                .iter()
                .filter_map(|f| {
                    let mut split = f.splitn(2, ':');
                    Some((split.next()?, split.next()?))
                })
                .collect();

            let values = &parts[schema_end..];

            for (i, (name, typ)) in fields.iter().enumerate() {
                if i >= values.len() {
                    break;
                }
                let val_str = values[i];
                let py_val: Py<PyAny> = match *typ {
                    "int" | "i64" | "i32" => val_str.parse::<i64>().ok().map_or_else(
                        || py.None(),
                        |v| v.into_pyobject(py).unwrap().unbind().into_any(),
                    ),
                    "float" | "f64" | "f32" => val_str.parse::<f64>().ok().map_or_else(
                        || py.None(),
                        |v| pyo3::types::PyFloat::new(py, v).unbind().into_any(),
                    ),
                    "bool" => {
                        let b = val_str == "true" || val_str == "1";
                        b.into_pyobject(py).unwrap().to_owned().unbind().into_any()
                    }
                    _ => pyo3::types::PyString::new(py, val_str).unbind().into_any(),
                };
                dict.set_item(*name, py_val)?;
            }

            records.push(dict.into_any().unbind());
        }

        Ok(records)
    }

    fn read_json(&self, py: Python<'_>, path: &str) -> PyResult<Vec<Py<PyAny>>> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Cannot read file: {e}"))
        })?;

        let mut bytes = content.into_bytes();
        let value: serde_json::Value = simd_json::from_slice(&mut bytes)
            .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Invalid JSON: {e}")))?;

        // If it's an array, return elements; otherwise wrap in vec
        if let serde_json::Value::Array(arr) = value {
            arr.iter().map(|v| json_to_py(py, v)).collect()
        } else {
            Ok(vec![json_to_py(py, &value)?])
        }
    }

    fn write_jsonl(&self, py: Python<'_>, path: &str, records: &[Py<PyAny>]) -> PyResult<()> {
        let file = File::create(path).map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Cannot create file: {e}"))
        })?;
        let mut writer = BufWriter::new(file);
        let opts = DumpOptions::default();

        for record in records {
            let value = py_to_json(py, record.bind(py), None, &opts)?;
            let json_str = serde_json::to_string(&value).map_err(|e| {
                PyErr::new::<crate::exceptions::JSONEncodeError, _>(format!(
                    "Serialization error: {e}"
                ))
            })?;
            writeln!(writer, "{json_str}").map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Write error: {e}"))
            })?;
        }

        writer.flush().map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Flush error: {e}"))
        })?;

        Ok(())
    }

    fn write_isonl(
        &self,
        py: Python<'_>,
        path: &str,
        records: &[Py<PyAny>],
        schema: &Option<Vec<String>>,
    ) -> PyResult<()> {
        let file = File::create(path).map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Cannot create file: {e}"))
        })?;
        let mut writer = BufWriter::new(file);

        // Infer schema from first record if not provided
        let field_schema: Vec<(String, String)> = if let Some(fields) = schema {
            fields
                .iter()
                .map(|f| (f.clone(), "string".to_string()))
                .collect()
        } else if let Some(first) = records.first() {
            let first_bound = first.bind(py);
            if let Ok(first_dict) = first_bound.cast::<pyo3::types::PyDict>() {
                first_dict
                    .keys()
                    .iter()
                    .filter_map(|k| {
                        let key = k.str().ok()?.to_string();
                        // Infer type from value
                        let typ = if let Some(val) = first_dict.get_item(&key).ok().flatten() {
                            if val.is_instance_of::<pyo3::types::PyBool>() {
                                // Check bool before int since bool is subclass of int
                                "bool"
                            } else if val.is_instance_of::<pyo3::types::PyInt>() {
                                "int"
                            } else if val.is_instance_of::<pyo3::types::PyFloat>() {
                                "float"
                            } else {
                                "string"
                            }
                        } else {
                            "string"
                        };
                        Some((key, typ.to_string()))
                    })
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        for record in records {
            let record_bound = record.bind(py);
            let mut parts = vec!["table.data".to_string()];

            // Add schema fields
            for (name, typ) in &field_schema {
                parts.push(format!("{name}:{typ}"));
            }

            // Add values - try to downcast to PyDict
            if let Ok(record_dict) = record_bound.cast::<pyo3::types::PyDict>() {
                for (name, _) in &field_schema {
                    if let Some(val) = record_dict.get_item(name.as_str())? {
                        parts.push(val.str()?.to_string());
                    } else {
                        parts.push(String::new());
                    }
                }
            } else {
                // If not a dict, add empty values
                for _ in &field_schema {
                    parts.push(String::new());
                }
            }

            writeln!(writer, "{}", parts.join("|")).map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Write error: {e}"))
            })?;
        }

        writer.flush().map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Flush error: {e}"))
        })?;

        Ok(())
    }

    fn write_json(&self, py: Python<'_>, path: &str, records: &[Py<PyAny>]) -> PyResult<()> {
        let file = File::create(path).map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Cannot create file: {e}"))
        })?;
        let mut writer = BufWriter::new(file);
        let opts = DumpOptions::default();

        // Convert all records to JSON values
        let values: Vec<serde_json::Value> = records
            .iter()
            .map(|r| py_to_json(py, r.bind(py), None, &opts))
            .collect::<PyResult<Vec<_>>>()?;

        // Write as JSON array
        let json_str = serde_json::to_string_pretty(&values).map_err(|e| {
            PyErr::new::<crate::exceptions::JSONEncodeError, _>(format!("Serialization error: {e}"))
        })?;

        writer.write_all(json_str.as_bytes()).map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Write error: {e}"))
        })?;

        writer.flush().map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Flush error: {e}"))
        })?;

        Ok(())
    }
}
