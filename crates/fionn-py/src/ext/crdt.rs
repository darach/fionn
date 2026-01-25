// SPDX-License-Identifier: MIT OR Apache-2.0
//! CRDT operations - conflict-free replicated data types
//!
//! Distributed document merging without conflicts.

use fionn_core::MergeStrategy as CoreMergeStrategy;
use fionn_core::OperationValue;
use fionn_crdt::{
    OptimizedMergeProcessor, PreParsedValue, Winner, merge_additive_i64, merge_lww_fast,
    merge_max_i64, merge_min_i64,
};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::compat::options::DumpOptions;
use crate::exceptions::JSONDecodeError;
use crate::types::{json_to_py, py_to_json};

/// Merge strategy for CRDT operations.
#[pyclass(eq, eq_int)]
#[derive(Clone, PartialEq, Eq)]
pub enum MergeStrategy {
    /// Last-writer-wins based on timestamp
    LastWriterWins,
    /// Maximum value wins (for counters)
    Additive,
    /// Maximum value wins
    Max,
    /// Minimum value wins
    Min,
    /// Concatenate arrays
    Concat,
    /// Union of sets
    Union,
}

impl From<MergeStrategy> for CoreMergeStrategy {
    fn from(s: MergeStrategy) -> Self {
        match s {
            MergeStrategy::LastWriterWins => Self::LastWriteWins,
            MergeStrategy::Additive => Self::Additive,
            MergeStrategy::Max => Self::Max,
            MergeStrategy::Min => Self::Min,
            MergeStrategy::Concat | MergeStrategy::Union => Self::Union,
        }
    }
}

/// Get current timestamp in milliseconds.
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Convert Python value to `OperationValue`.
fn py_to_operation_value(py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<OperationValue> {
    let json_val = py_to_json(py, value, None, &DumpOptions::default())?;
    Ok(json_value_to_operation_value(&json_val))
}

/// Convert `serde_json::Value` to `OperationValue`.
fn json_value_to_operation_value(value: &serde_json::Value) -> OperationValue {
    match value {
        serde_json::Value::Null => OperationValue::Null,
        serde_json::Value::Bool(b) => OperationValue::BoolRef(*b),
        serde_json::Value::Number(n) => OperationValue::NumberRef(n.to_string()),
        serde_json::Value::String(s) => OperationValue::StringRef(s.clone()),
        serde_json::Value::Array(_) => OperationValue::ArrayRef { start: 0, end: 0 },
        serde_json::Value::Object(_) => OperationValue::ObjectRef { start: 0, end: 0 },
    }
}

/// Convert `PreParsedValue` to `serde_json::Value`.
#[allow(dead_code)] // Utility for future CRDT merge result serialization
fn pre_parsed_to_json(value: &PreParsedValue) -> serde_json::Value {
    match value {
        PreParsedValue::Integer(i) => serde_json::Value::Number((*i).into()),
        PreParsedValue::Float(f) => serde_json::Number::from_f64(*f)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        PreParsedValue::Timestamp(t) => serde_json::Value::Number((*t).into()),
        PreParsedValue::String(s) => serde_json::Value::String(s.clone()),
        PreParsedValue::Boolean(b) => serde_json::Value::Bool(*b),
        PreParsedValue::Null => serde_json::Value::Null,
        PreParsedValue::TapeRef { .. } => serde_json::Value::Null,
    }
}

/// Convert `AHashMap` to `serde_json::Map` for serialization.
fn timestamps_to_json(
    timestamps: &ahash::AHashMap<String, u64>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::new();
    for (k, v) in timestamps {
        map.insert(k.clone(), serde_json::Value::Number((*v).into()));
    }
    map
}

/// A CRDT-enabled document for conflict-free merging.
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// doc = fx.CrdtDocument({"counter": 0}, replica_id="node-1")
/// doc.set("counter", 10)
///
/// remote_doc = fx.CrdtDocument({"counter": 0}, replica_id="node-2")
/// remote_doc.set("counter", 5)
///
/// conflicts = doc.merge(remote_doc)
/// print(doc.value)  # Uses LWW resolution
/// ```
#[pyclass]
pub struct CrdtDocument {
    replica_id: String,
    processor: RwLock<OptimizedMergeProcessor>,
    document: RwLock<serde_json::Value>,
    timestamps: RwLock<ahash::AHashMap<String, u64>>,
}

#[pymethods]
impl CrdtDocument {
    #[new]
    #[pyo3(signature = (initial_data, replica_id))]
    fn new(py: Python<'_>, initial_data: &Bound<'_, PyAny>, replica_id: &str) -> PyResult<Self> {
        let doc = py_to_json(py, initial_data, None, &DumpOptions::default())?;
        let processor = OptimizedMergeProcessor::new();

        // Initialize timestamps for all fields
        let mut timestamps = ahash::AHashMap::new();
        let ts = current_timestamp();
        if let serde_json::Value::Object(map) = &doc {
            for key in map.keys() {
                timestamps.insert(key.clone(), ts);
            }
        }

        Ok(Self {
            replica_id: replica_id.to_string(),
            processor: RwLock::new(processor),
            document: RwLock::new(doc),
            timestamps: RwLock::new(timestamps),
        })
    }

    /// Set a value at the given path.
    fn set(&self, py: Python<'_>, path: &str, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let json_val = py_to_json(py, value, None, &DumpOptions::default())?;
        let ts = current_timestamp();

        // Update document
        let mut doc = self.document.write().map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Lock error: {e}"))
        })?;
        if let serde_json::Value::Object(ref mut map) = *doc {
            map.insert(path.to_string(), json_val);
        }

        // Update timestamp
        self.timestamps
            .write()
            .map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Lock error: {e}"))
            })?
            .insert(path.to_string(), ts);

        Ok(())
    }

    /// Delete a value at the given path.
    fn delete(&self, path: &str) -> PyResult<()> {
        let ts = current_timestamp();

        // Remove from document
        let mut doc = self.document.write().map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Lock error: {e}"))
        })?;
        if let serde_json::Value::Object(ref mut map) = *doc {
            map.remove(path);
        }

        // Update timestamp (tombstone)
        self.timestamps
            .write()
            .map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Lock error: {e}"))
            })?
            .insert(path.to_string(), ts);

        Ok(())
    }

    /// Merge with another document.
    ///
    /// Returns a list of conflicts (paths where both documents modified the same field).
    fn merge(&self, py: Python<'_>, other: &Self) -> PyResult<Py<PyAny>> {
        let conflicts = py.detach(|| {
            let mut conflicts = Vec::new();

            let other_doc = other.document.read().map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Lock error: {e}"))
            })?;
            let other_timestamps = other.timestamps.read().map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Lock error: {e}"))
            })?;

            let mut my_doc = self.document.write().map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Lock error: {e}"))
            })?;
            let mut my_timestamps = self.timestamps.write().map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Lock error: {e}"))
            })?;

            // For each field in other document
            if let (serde_json::Value::Object(my_map), serde_json::Value::Object(other_map)) =
                (&mut *my_doc, &*other_doc)
            {
                for (key, other_value) in other_map {
                    let other_ts = other_timestamps.get(key).copied().unwrap_or(0);
                    let my_ts = my_timestamps.get(key).copied().unwrap_or(0);

                    // LWW resolution
                    let winner = merge_lww_fast(my_ts, other_ts);

                    // Track conflict if both modified
                    if my_map.contains_key(key) && my_ts > 0 && other_ts > 0 && my_ts != other_ts {
                        conflicts.push(serde_json::json!({
                            "path": key,
                            "local_ts": my_ts,
                            "remote_ts": other_ts,
                            "winner": if winner == Winner::Remote { "remote" } else { "local" }
                        }));
                    }

                    // Apply winner
                    if winner == Winner::Remote {
                        my_map.insert(key.clone(), other_value.clone());
                        my_timestamps.insert(key.clone(), other_ts);
                    }
                }
            }
            Ok::<Vec<serde_json::Value>, PyErr>(conflicts)
        })?;

        json_to_py(py, &serde_json::Value::Array(conflicts))
    }

    /// Set merge strategy for a path pattern.
    fn set_strategy(&self, path_pattern: &str, strategy: MergeStrategy) -> PyResult<()> {
        let core_strategy: CoreMergeStrategy = strategy.into();
        self.processor
            .write()
            .map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Lock error: {e}"))
            })?
            .set_path_strategy(path_pattern, core_strategy);
        Ok(())
    }

    /// Export full state.
    fn export_state(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let doc = self.document.read().map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Lock error: {e}"))
        })?;
        let timestamps = self.timestamps.read().map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Lock error: {e}"))
        })?;

        let state = serde_json::json!({
            "replica_id": self.replica_id,
            "document": &*doc,
            "timestamps": serde_json::Value::Object(timestamps_to_json(&timestamps)),
        });

        json_to_py(py, &state)
    }

    /// Export delta since a version.
    fn export_delta(&self, py: Python<'_>, since_version: u64) -> PyResult<Py<PyAny>> {
        let doc = self.document.read().map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Lock error: {e}"))
        })?;
        let timestamps = self.timestamps.read().map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Lock error: {e}"))
        })?;

        // Collect fields modified since the given version
        let mut delta = serde_json::Map::new();
        let mut delta_timestamps = serde_json::Map::new();

        if let serde_json::Value::Object(ref map) = *doc {
            for (key, value) in map {
                if let Some(&ts) = timestamps.get(key)
                    && ts > since_version
                {
                    delta.insert(key.clone(), value.clone());
                    delta_timestamps.insert(key.clone(), serde_json::Value::Number(ts.into()));
                }
            }
        }

        let result = serde_json::json!({
            "replica_id": self.replica_id,
            "delta": serde_json::Value::Object(delta),
            "timestamps": serde_json::Value::Object(delta_timestamps),
        });

        json_to_py(py, &result)
    }

    /// Get current value.
    #[getter]
    fn value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let doc = self.document.read().map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Lock error: {e}"))
        })?;
        json_to_py(py, &doc)
    }

    /// Get replica ID.
    #[getter]
    fn replica_id_getter(&self) -> &str {
        &self.replica_id
    }

    /// Bulk merge from a JSONL file.
    ///
    /// This is significantly faster than one-by-one Python merges as it
    /// releases the GIL and uses native SIMD-accelerated parsing.
    fn merge_jsonl(&self, py: Python<'_>, path: &str) -> PyResult<()> {
        let path_buf = PathBuf::from(path);

        py.detach(|| {
            let file = File::open(&path_buf).map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyIOError, _>(format!("Failed to open {path}: {e}"))
            })?;
            let reader = BufReader::new(file);

            for (i, line) in reader.lines().enumerate() {
                let line_str = line.map_err(|e| {
                    PyErr::new::<pyo3::exceptions::PyIOError, _>(format!(
                        "Error reading line {i}: {e}"
                    ))
                })?;
                if line_str.trim().is_empty() {
                    continue;
                }

                let mut bytes = line_str.into_bytes();
                let value: serde_json::Value = simd_json::from_slice(&mut bytes).map_err(|e| {
                    PyErr::new::<JSONDecodeError, _>(format!("Invalid JSON in line {i}: {e}"))
                })?;

                if let serde_json::Value::Object(other_map) = value {
                    let mut my_doc = self.document.write().map_err(|e| {
                        PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
                            "Lock error: {e}"
                        ))
                    })?;
                    let mut my_timestamps = self.timestamps.write().map_err(|e| {
                        PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
                            "Lock error: {e}"
                        ))
                    })?;

                    if let serde_json::Value::Object(ref mut dest_map) = *my_doc {
                        // Assuming each object might have a 'ts' field for LWW
                        let remote_ts = other_map
                            .get("ts")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0);

                        for (key, other_value) in other_map {
                            if key == "ts" {
                                continue;
                            }

                            let my_ts = my_timestamps.get(&key).copied().unwrap_or(0);
                            let winner = merge_lww_fast(my_ts, remote_ts);

                            if winner == Winner::Remote {
                                dest_map.insert(key.clone(), other_value);
                                my_timestamps.insert(key, remote_ts);
                            }
                        }
                    }
                }
            }
            Ok(())
        })
    }
}

// =============================================================================
// Standalone CRDT merge functions
// =============================================================================

/// Perform LWW (Last-Writer-Wins) merge on two values.
///
/// # Arguments
/// * `local_value` - Local value
/// * `local_timestamp` - Local timestamp (ms since epoch)
/// * `remote_value` - Remote value
/// * `remote_timestamp` - Remote timestamp (ms since epoch)
///
/// # Returns
/// Tuple of (`winning_value`, `winner_str`)
#[pyfunction]
pub fn crdt_lww_merge(
    _py: Python<'_>,
    local_value: &Bound<'_, PyAny>,
    local_timestamp: u64,
    remote_value: &Bound<'_, PyAny>,
    remote_timestamp: u64,
) -> PyResult<(Py<PyAny>, String)> {
    let winner = merge_lww_fast(local_timestamp, remote_timestamp);
    let winner_str = if winner == Winner::Remote {
        "remote"
    } else {
        "local"
    };

    let winning_value = if winner == Winner::Remote {
        remote_value.clone().unbind()
    } else {
        local_value.clone().unbind()
    };

    Ok((winning_value, winner_str.to_string()))
}

/// Perform Max merge on two numeric values.
///
/// # Arguments
/// * `local_value` - Local numeric value
/// * `remote_value` - Remote numeric value
///
/// # Returns
/// Tuple of (`winning_value`, `winner_str`)
#[pyfunction]
pub fn crdt_max_merge(
    _py: Python<'_>,
    local_value: i64,
    remote_value: i64,
) -> PyResult<(i64, String)> {
    let (winner, value) = merge_max_i64(local_value, remote_value);
    let winner_str = if winner == Winner::Remote {
        "remote"
    } else {
        "local"
    };
    Ok((value, winner_str.to_string()))
}

/// Perform Min merge on two numeric values.
///
/// # Arguments
/// * `local_value` - Local numeric value
/// * `remote_value` - Remote numeric value
///
/// # Returns
/// Tuple of (`winning_value`, `winner_str`)
#[pyfunction]
pub fn crdt_min_merge(
    _py: Python<'_>,
    local_value: i64,
    remote_value: i64,
) -> PyResult<(i64, String)> {
    let (winner, value) = merge_min_i64(local_value, remote_value);
    let winner_str = if winner == Winner::Remote {
        "remote"
    } else {
        "local"
    };
    Ok((value, winner_str.to_string()))
}

/// Perform Additive merge on two numeric values.
///
/// # Arguments
/// * `local_value` - Local numeric value
/// * `remote_value` - Remote numeric value
///
/// # Returns
/// Sum of both values
#[pyfunction]
pub const fn crdt_additive_merge(
    _py: Python<'_>,
    local_value: i64,
    remote_value: i64,
) -> PyResult<i64> {
    Ok(merge_additive_i64(local_value, remote_value))
}

/// Batch merge multiple field updates using optimized processor.
///
/// # Arguments
/// * `local_state` - Dict of {path: (value, timestamp)}
/// * `remote_updates` - List of (path, value, timestamp) tuples
/// * `default_strategy` - Default merge strategy ("lww", "max", "min", "additive")
///
/// # Returns
/// Dict with merged values and conflict report
#[pyfunction]
#[pyo3(signature = (local_state, remote_updates, default_strategy="lww"))]
pub fn crdt_batch_merge(
    py: Python<'_>,
    local_state: &Bound<'_, PyDict>,
    remote_updates: &Bound<'_, PyList>,
    default_strategy: &str,
) -> PyResult<Py<PyAny>> {
    let mut processor = OptimizedMergeProcessor::new();

    // Set default strategy
    let strategy = match default_strategy {
        "max" => CoreMergeStrategy::Max,
        "min" => CoreMergeStrategy::Min,
        "additive" => CoreMergeStrategy::Additive,
        _ => CoreMergeStrategy::LastWriteWins,
    };
    processor.set_default_strategy(strategy);

    // Initialize local state
    let mut local_entries = Vec::new();
    for (key, value) in local_state.iter() {
        let path: String = key.extract()?;
        let tuple = value.cast::<pyo3::types::PyTuple>()?;
        if tuple.len() != 2 {
            return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                "Local state values must be (value, timestamp) tuples",
            ));
        }
        let val = tuple.get_item(0)?;
        let ts: u64 = tuple.get_item(1)?.extract()?;
        let op_val = py_to_operation_value(py, &val)?;
        local_entries.push((path, op_val, ts));
    }
    processor.init_local(local_entries.into_iter());

    // Process remote updates
    let mut remote_entries = Vec::new();
    for item in remote_updates.iter() {
        let tuple = item.cast::<pyo3::types::PyTuple>()?;
        if tuple.len() != 3 {
            return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                "Remote updates must be (path, value, timestamp) tuples",
            ));
        }
        let path: String = tuple.get_item(0)?.extract()?;
        let val = tuple.get_item(1)?;
        let ts: u64 = tuple.get_item(2)?.extract()?;
        let op_val = py_to_operation_value(py, &val)?;
        remote_entries.push((path, op_val, ts));
    }

    let results = py.detach(|| processor.merge_batch(remote_entries.into_iter()));

    // Build result
    let result = serde_json::json!({
        "local_wins": results.local_wins(),
        "remote_wins": results.remote_wins(),
        "merged_count": results.merged_count(),
        "total": results.len(),
    });

    json_to_py(py, &result)
}
