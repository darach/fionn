// SPDX-License-Identifier: MIT OR Apache-2.0
//! Python bindings for fionn Tape API
//!
//! Zero-copy SIMD-accelerated JSON tape for efficient path queries and modifications.

use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyFloat, PyList, PyString};
use std::collections::HashSet;

use fionn_pool::{PoolStrategy, PooledBuffer, SharedPool, TapePool as _};
use fionn_tape::{DsonTape, SimdValue};
use simd_json::value::tape::Node;
use std::sync::Arc;

use crate::exceptions::JSONDecodeError;

/// Schema for filtered parsing.
#[pyclass]
#[derive(Clone)]
pub struct Schema {
    fields: Vec<String>,
}

#[pymethods]
impl Schema {
    #[new]
    const fn new(fields: Vec<String>) -> Self {
        Self { fields }
    }

    fn __repr__(&self) -> String {
        format!("Schema(fields={:?})", self.fields)
    }
}

/// SIMD-accelerated JSON tape for zero-copy access.
///
/// The Tape provides O(1) access to JSON structure without full parsing.
/// Use for repeated queries on the same JSON document.
///
/// Example:
/// ```python
/// tape = Tape.parse(b'{"users": [{"name": "Alice"}, {"name": "Bob"}]}')
/// idx = tape.resolve_path("users[0].name")
/// value = tape.get_value(idx)  # "Alice"
/// ```
#[pyclass]
pub struct Tape {
    inner: Option<TapeInner>,
}

pub enum TapeInner {
    Owned(DsonTape<Vec<u8>>),
    Pooled(DsonTape<PooledBuffer>, Arc<SharedPool>),
}

impl TapeInner {
    fn nodes(&self) -> &[Node<'static>] {
        match self {
            Self::Owned(inner) => inner.nodes(),
            Self::Pooled(inner, _) => inner.nodes(),
        }
    }

    fn to_json_string(&self) -> fionn_core::Result<String> {
        match self {
            Self::Owned(inner) => inner.to_json_string(),
            Self::Pooled(inner, _) => inner.to_json_string(),
        }
    }

    fn resolve_path(&self, path: &str) -> fionn_core::Result<Option<usize>> {
        match self {
            Self::Owned(inner) => inner.resolve_path(path),
            Self::Pooled(inner, _) => inner.resolve_path(path),
        }
    }

    fn extract_value_simd(&self, index: usize) -> Option<SimdValue> {
        match self {
            Self::Owned(inner) => inner.extract_value_simd(index),
            Self::Pooled(inner, _) => inner.extract_value_simd(index),
        }
    }

    fn skip_value(&self, index: usize) -> fionn_core::Result<usize> {
        match self {
            Self::Owned(inner) => inner.skip_value(index),
            Self::Pooled(inner, _) => inner.skip_value(index),
        }
    }

    fn should_survive(&self, path: &str, schema: &HashSet<String>) -> bool {
        match self {
            Self::Owned(inner) => inner.simd_schema_match(path, schema),
            Self::Pooled(inner, _) => inner.simd_schema_match(path, schema),
        }
    }
}

impl Tape {
    const fn get_inner(&self) -> &TapeInner {
        self.inner.as_ref().expect("Tape has been dropped")
    }
}

impl Drop for Tape {
    fn drop(&mut self) {
        if let Some(TapeInner::Pooled(tape, pool)) = self.inner.take() {
            let buffer = tape.into_data();
            pool.release(buffer);
        }
    }
}

#[pymethods]
impl Tape {
    /// Parse JSON bytes or string into a tape.
    ///
    /// Args:
    ///     data: JSON bytes or string to parse
    ///     schema: Optional Schema to filter fields during parsing
    ///
    /// Returns:
    ///     Tape object for efficient access
    ///
    /// Raises:
    ///     `JSONDecodeError`: If JSON is malformed
    #[staticmethod]
    #[pyo3(signature = (data, schema=None))]
    fn parse(data: &Bound<'_, PyAny>, schema: Option<Schema>) -> PyResult<Self> {
        // Extract string from bytes or str
        let json_str: String = if let Ok(bytes) = data.cast::<PyBytes>() {
            String::from_utf8(bytes.as_bytes().to_vec())
                .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Invalid UTF-8: {e}")))?
        } else if let Ok(s) = data.cast::<PyString>() {
            s.to_cow()?.into_owned()
        } else {
            return Err(PyErr::new::<pyo3::exceptions::PyTypeError, _>(
                "data must be bytes or str",
            ));
        };

        let inner = data
            .py()
            .detach(|| DsonTape::parse(&json_str))
            .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Parse error: {e}")))?;

        // Apply schema filter if provided
        let inner = if let Some(schema) = schema {
            let schema_set: HashSet<String> = schema.fields.into_iter().collect();
            inner
                .filter_by_schema(&schema_set)
                .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Filter error: {e}")))?
        } else {
            inner
        };

        Ok(Self {
            inner: Some(TapeInner::Owned(inner)),
        })
    }

    /// Number of nodes in the tape.
    #[getter]
    fn node_count(&self) -> usize {
        self.get_inner().nodes().len()
    }

    /// Resolve a JSON path to a tape index.
    ///
    /// Args:
    ///     path: JSON path (e.g., "users[0].name", "data.items")
    ///
    /// Returns:
    ///     Tape index if path exists, None otherwise
    #[pyo3(signature = (path))]
    fn resolve_path(&self, path: &str) -> PyResult<Option<usize>> {
        self.get_inner()
            .resolve_path(path)
            .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Path error: {e}")))
    }

    /// Get value at a tape index.
    ///
    /// Args:
    ///     index: Tape index from `resolve_path()`
    ///
    /// Returns:
    ///     Python value (str, int, float, bool, or None)
    #[pyo3(signature = (index))]
    fn get_value(&self, py: Python<'_>, index: usize) -> PyResult<Py<PyAny>> {
        match self.get_inner().extract_value_simd(index) {
            Some(SimdValue::String(s)) => Ok(PyString::new(py, &s).into_any().unbind()),
            Some(SimdValue::Null) => Ok(py.None()),
            Some(SimdValue::Bool(b)) => Ok(b.into_pyobject(py)?.to_owned().unbind().into_any()),
            Some(SimdValue::Number(n)) => {
                // Try to parse as int first, then float
                if let Ok(i) = n.parse::<i64>() {
                    Ok(i.into_pyobject(py)?.unbind().into_any())
                } else if let Ok(f) = n.parse::<f64>() {
                    Ok(PyFloat::new(py, f).unbind().into_any())
                } else {
                    // Return as string if can't parse
                    Ok(PyString::new(py, &n).into_any().unbind())
                }
            }
            None => Ok(py.None()),
        }
    }

    /// Get a value by path (convenience method).
    ///
    /// Args:
    ///     path: JSON path (e.g., "users[0].name")
    ///
    /// Returns:
    ///     Python value if path exists, None otherwise
    #[pyo3(signature = (path))]
    fn get(&self, py: Python<'_>, path: &str) -> PyResult<Py<PyAny>> {
        match self.resolve_path(path)? {
            Some(idx) => self.get_value(py, idx),
            None => Ok(py.None()),
        }
    }

    /// Convert tape to Python object (full materialization).
    ///
    /// Returns:
    ///     Python dict/list representing the JSON
    ///
    /// Note: This uses direct tape-to-Python conversion, bypassing JSON
    /// string serialization and simd-json intermediary for better performance.
    fn to_object(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let inner = self.get_inner();
        match inner {
            TapeInner::Owned(inner) => tape_node_to_py(py, inner, 0),
            TapeInner::Pooled(inner, _) => tape_node_to_py(py, inner, 0),
        }
    }

    /// Convert tape back to JSON string.
    ///
    /// Returns:
    ///     JSON string representation
    fn to_json(&self) -> PyResult<String> {
        self.get_inner()
            .to_json_string()
            .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Serialization error: {e}")))
    }

    /// Filter tape by schema (keep only specified fields).
    ///
    /// Args:
    ///     fields: List of field paths to keep (e.g., ["name", "users.email"])
    ///
    /// Returns:
    ///     New filtered Tape
    #[pyo3(signature = (fields))]
    fn filter(&self, fields: Vec<String>) -> PyResult<Self> {
        let schema: HashSet<String> = fields.into_iter().collect();
        let filtered = match self.get_inner() {
            TapeInner::Owned(inner) => inner.filter_by_schema(&schema),
            TapeInner::Pooled(inner, _) => inner.filter_by_schema(&schema),
        }
        .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Filter error: {e}")))?;
        Ok(Self {
            inner: Some(TapeInner::Owned(filtered)),
        })
    }

    /// Query using JSONPath-like syntax.
    ///
    /// Supports:
    /// - `field` or `$.field` - direct field access
    /// - `field[n]` - array index
    /// - `field[*]` - all array elements
    /// - `..field` - recursive descent (find all occurrences of field)
    ///
    /// Args:
    ///     query: JSONPath-like query string
    ///
    /// Returns:
    ///     List of matched values (may be empty if no matches)
    ///
    /// Example:
    /// ```python
    /// tape = Tape.parse('{"users": [{"name": "Alice"}, {"name": "Bob"}]}')
    /// tape.query("users[*].name")  # ["Alice", "Bob"]
    /// tape.query("..name")  # ["Alice", "Bob"]
    /// ```
    fn query(&self, py: Python<'_>, query: &str) -> PyResult<Py<PyAny>> {
        let results = PyList::empty(py);
        let query = query.trim();

        // Strip leading $ or $. if present
        let query = query
            .strip_prefix("$.")
            .unwrap_or_else(|| query.strip_prefix('$').unwrap_or(query));

        if query.is_empty() {
            // Return root element
            let root = self.to_object(py)?;
            results.append(root)?;
            return Ok(results.into_any().unbind());
        }

        // Recursive descent: ..field
        if let Some(field) = query.strip_prefix("..") {
            let inner = self.get_inner();
            match inner {
                TapeInner::Owned(inner) => query_recursive_descent(py, inner, 0, field, &results)?,
                TapeInner::Pooled(inner, _) => {
                    query_recursive_descent(py, inner, 0, field, &results)?;
                }
            }
            return Ok(results.into_any().unbind());
        }

        // Parse path segments
        let segments = parse_query_segments(query);
        let inner = self.get_inner();
        match inner {
            TapeInner::Owned(inner) => query_segments(py, inner, 0, &segments, &results)?,
            TapeInner::Pooled(inner, _) => query_segments(py, inner, 0, &segments, &results)?,
        }

        Ok(results.into_any().unbind())
    }

    /// Skip to next sibling value (for manual tape iteration).
    ///
    /// Args:
    ///     index: Current tape index
    ///
    /// Returns:
    ///     Next sibling index
    #[pyo3(signature = (index))]
    fn skip_value(&self, index: usize) -> PyResult<usize> {
        self.get_inner()
            .skip_value(index)
            .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Skip error: {e}")))
    }

    /// Check if a path matches the given schema.
    ///
    /// Args:
    ///     path: Path to check
    ///     schema: List of schema paths
    ///
    /// Returns:
    ///     True if path should survive filtering
    #[pyo3(signature = (path, schema))]
    fn should_survive(&self, path: &str, schema: Vec<String>) -> bool {
        let schema_set: HashSet<String> = schema.into_iter().collect();
        self.get_inner().should_survive(path, &schema_set)
    }

    fn __repr__(&self) -> String {
        format!("Tape(nodes={})", self.get_inner().nodes().len())
    }

    fn __len__(&self) -> usize {
        self.get_inner().nodes().len()
    }
}

/// Pool for reusing tape allocations.
///
/// Provides pooled tape parsing for better memory efficiency when
/// processing many JSON documents.
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// pool = fx.TapePool(strategy="lru", max_tapes=100)
/// tape = pool.parse(json_bytes)
/// process(tape)
/// ```
#[pyclass]
pub struct TapePool {
    pool: Arc<SharedPool>,
}

#[pymethods]
impl TapePool {
    #[new]
    #[pyo3(signature = (strategy="lru", max_tapes=100))]
    fn new(strategy: &str, max_tapes: usize) -> Self {
        let pool_strategy = match strategy.to_lowercase().as_str() {
            "lru" => PoolStrategy::Lru { max_tapes },
            "size" => PoolStrategy::SizeLimited { max_tapes },
            _ => PoolStrategy::Lru { max_tapes },
        };
        Self {
            pool: Arc::new(SharedPool::new(pool_strategy)),
        }
    }

    /// Parse JSON bytes using pooled tape.
    fn parse(&self, data: &Bound<'_, PyAny>) -> PyResult<Tape> {
        // Extract bytes from pyany
        let bytes: Vec<u8> = if let Ok(py_bytes) = data.cast::<PyBytes>() {
            py_bytes.as_bytes().to_vec()
        } else if let Ok(py_str) = data.cast::<PyString>() {
            py_str.to_cow()?.as_bytes().to_vec()
        } else {
            return Err(PyErr::new::<pyo3::exceptions::PyTypeError, _>(
                "data must be bytes or str",
            ));
        };

        // Acquire buffer from pool
        let mut buffer = self.pool.acquire(bytes.len() + 64); // Add some padding for SIMD-JSON
        buffer.clear();
        buffer.extend_from_slice(&bytes);

        // Parse into pooled buffer
        let inner = data
            .py()
            .detach(|| DsonTape::from_raw(buffer))
            .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Parse error: {e}")))?;

        Ok(Tape {
            inner: Some(TapeInner::Pooled(inner, self.pool.clone())),
        })
    }

    fn __repr__(&self) -> String {
        format!("TapePool(buffers={})", self.pool.stats().buffers_in_pool)
    }
}

/// Parse JSON into a Tape for efficient repeated access.
///
/// This is the recommended way to work with JSON when you need to:
/// - Query the same document multiple times
/// - Extract specific paths without full parsing
/// - Filter by schema
///
/// Args:
///     json: JSON string or bytes to parse
///     fields: Optional list of fields to keep (schema filter)
///
/// Returns:
///     Tape object
///
/// Example:
///     tape = `fionn.ext.parse_tape`('{"name": "Alice", "age": 30}')
///     name = tape.get("name")  # "Alice"
#[pyfunction]
#[pyo3(signature = (json, fields=None))]
pub fn parse_tape(json: &Bound<'_, PyAny>, fields: Option<Vec<String>>) -> PyResult<Tape> {
    let schema = fields.map(Schema::new);
    Tape::parse(json, schema)
}

/// Batch resolve multiple paths on a tape.
///
/// More efficient than calling `get()` multiple times when you know
/// all the paths you need upfront.
///
/// Args:
///     tape: Tape object
///     paths: List of paths to resolve
///
/// Returns:
///     Dict mapping paths to their values (None for missing paths)
#[pyfunction]
#[pyo3(signature = (tape, paths))]
pub fn batch_resolve(py: Python<'_>, tape: &Tape, paths: Vec<String>) -> PyResult<Py<PyAny>> {
    let dict = PyDict::new(py);

    for path in paths {
        let value = tape.get(py, &path)?;
        dict.set_item(&path, value)?;
    }

    Ok(dict.into_any().unbind())
}

/// Query multiple JSON documents with the same path.
///
/// Optimized for applying the same query to many documents.
///
/// Args:
///     jsons: List of JSON strings
///     path: Path to extract from each
///
/// Returns:
///     List of values (None for documents where path doesn't exist)
#[pyfunction]
#[pyo3(signature = (jsons, path))]
pub fn batch_query(py: Python<'_>, jsons: Vec<String>, path: &str) -> PyResult<Py<PyAny>> {
    let list = PyList::empty(py);

    for json in jsons {
        let tape = DsonTape::parse(&json)
            .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Parse error: {e}")))?;

        let py_tape = Tape {
            inner: Some(TapeInner::Owned(tape)),
        };
        let value = py_tape.get(py, path)?;
        list.append(value)?;
    }

    Ok(list.into_any().unbind())
}

/// Convert a tape node directly to a Python object, recursively.
///
/// This bypasses JSON string serialization and `simd_json` `OwnedValue`,
/// building Python objects directly from the tape structure.
#[inline]
fn tape_node_to_py<S: AsRef<[u8]>>(
    py: Python<'_>,
    tape: &DsonTape<S>,
    index: usize,
) -> PyResult<Py<PyAny>> {
    use simd_json::value::tape::Node;

    let nodes = tape.nodes();
    if index >= nodes.len() {
        return Ok(py.None());
    }

    match &nodes[index] {
        Node::String(s) => Ok(PyString::new(py, s).into_any().unbind()),
        Node::Static(static_node) => match static_node {
            simd_json::StaticNode::Null => Ok(py.None()),
            simd_json::StaticNode::Bool(b) => {
                Ok((*b).into_pyobject(py)?.to_owned().unbind().into_any())
            }
            simd_json::StaticNode::I64(n) => Ok((*n).into_pyobject(py)?.unbind().into_any()),
            simd_json::StaticNode::U64(n) => Ok((*n).into_pyobject(py)?.unbind().into_any()),
            simd_json::StaticNode::F64(n) => Ok(PyFloat::new(py, *n).unbind().into_any()),
        },
        Node::Object { len, .. } => {
            let dict = PyDict::new(py);
            let mut current_idx = index + 1;

            for _ in 0..*len {
                if current_idx >= nodes.len() {
                    break;
                }

                // Get field name (key)
                if let Node::String(key) = &nodes[current_idx] {
                    // Intern the key for repeated access optimization
                    let key_interned = PyString::intern(py, key);
                    current_idx += 1;

                    // Get field value
                    if current_idx < nodes.len() {
                        let value = tape_node_to_py(py, tape, current_idx)?;
                        dict.set_item(key_interned, value)?;
                        current_idx = tape.skip_value(current_idx).map_err(|e| {
                            PyErr::new::<JSONDecodeError, _>(format!("Skip error: {e}"))
                        })?;
                    }
                } else {
                    current_idx += 1;
                }
            }

            Ok(dict.into_any().unbind())
        }
        Node::Array { len, .. } => {
            let list = PyList::empty(py);
            let mut current_idx = index + 1;

            for _ in 0..*len {
                if current_idx >= nodes.len() {
                    break;
                }

                let value = tape_node_to_py(py, tape, current_idx)?;
                list.append(value)?;
                current_idx = tape
                    .skip_value(current_idx)
                    .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Skip error: {e}")))?;
            }

            Ok(list.into_any().unbind())
        }
    }
}

// =============================================================================
// JSONPath Query Support
// =============================================================================

/// Segment of a `JSONPath` query
#[derive(Debug, Clone)]
enum QuerySegment {
    /// Field access: .field
    Field(String),
    /// Array index: [n]
    Index(usize),
    /// Array wildcard: [*]
    Wildcard,
}

/// Parse `JSONPath` query into segments
fn parse_query_segments(query: &str) -> Vec<QuerySegment> {
    let mut segments = Vec::new();
    let mut chars = query.chars().peekable();
    let mut current_field = String::new();

    while let Some(c) = chars.next() {
        match c {
            '.' => {
                if !current_field.is_empty() {
                    segments.push(QuerySegment::Field(current_field.clone()));
                    current_field.clear();
                }
            }
            '[' => {
                if !current_field.is_empty() {
                    segments.push(QuerySegment::Field(current_field.clone()));
                    current_field.clear();
                }

                // Parse bracket content
                let mut bracket_content = String::new();
                while let Some(&c) = chars.peek() {
                    if c == ']' {
                        chars.next();
                        break;
                    }
                    bracket_content.push(chars.next().unwrap());
                }

                if bracket_content == "*" {
                    segments.push(QuerySegment::Wildcard);
                } else if let Ok(n) = bracket_content.parse::<usize>() {
                    segments.push(QuerySegment::Index(n));
                }
            }
            _ => {
                current_field.push(c);
            }
        }
    }

    if !current_field.is_empty() {
        segments.push(QuerySegment::Field(current_field));
    }

    segments
}

/// Execute query segments recursively
fn query_segments<S: AsRef<[u8]>>(
    py: Python<'_>,
    tape: &DsonTape<S>,
    index: usize,
    segments: &[QuerySegment],
    results: &Bound<'_, PyList>,
) -> PyResult<()> {
    use simd_json::value::tape::Node;

    if segments.is_empty() {
        // Reached end of path - collect this value
        let value = tape_node_to_py(py, tape, index)?;
        results.append(value)?;
        return Ok(());
    }

    let nodes = tape.nodes();
    if index >= nodes.len() {
        return Ok(());
    }

    match &nodes[index] {
        Node::Object { len, .. } => {
            if let QuerySegment::Field(target_field) = &segments[0] {
                // Find field in object
                let mut current_idx = index + 1;
                for _ in 0..*len {
                    if current_idx >= nodes.len() {
                        break;
                    }

                    if let Node::String(key) = &nodes[current_idx] {
                        current_idx += 1;
                        if current_idx < nodes.len() {
                            if key == target_field {
                                // Found matching field - recurse with remaining segments
                                query_segments(py, tape, current_idx, &segments[1..], results)?;
                            }
                            current_idx = tape.skip_value(current_idx).map_err(|e| {
                                PyErr::new::<JSONDecodeError, _>(format!("Skip error: {e}"))
                            })?;
                        }
                    } else {
                        current_idx += 1;
                    }
                }
            }
        }
        Node::Array { len, .. } => {
            match &segments[0] {
                QuerySegment::Index(target_idx) => {
                    // Navigate to specific index
                    if *target_idx < *len {
                        let mut current_idx = index + 1;
                        for i in 0..=*target_idx {
                            if current_idx >= nodes.len() {
                                return Ok(());
                            }
                            if i == *target_idx {
                                query_segments(py, tape, current_idx, &segments[1..], results)?;
                                return Ok(());
                            }
                            current_idx = tape.skip_value(current_idx).map_err(|e| {
                                PyErr::new::<JSONDecodeError, _>(format!("Skip error: {e}"))
                            })?;
                        }
                    }
                }
                QuerySegment::Wildcard => {
                    // Iterate all array elements
                    let mut current_idx = index + 1;
                    for _ in 0..*len {
                        if current_idx >= nodes.len() {
                            break;
                        }
                        query_segments(py, tape, current_idx, &segments[1..], results)?;
                        current_idx = tape.skip_value(current_idx).map_err(|e| {
                            PyErr::new::<JSONDecodeError, _>(format!("Skip error: {e}"))
                        })?;
                    }
                }
                QuerySegment::Field(_) => {
                    // Try field access on each array element (for array of objects)
                    let mut current_idx = index + 1;
                    for _ in 0..*len {
                        if current_idx >= nodes.len() {
                            break;
                        }
                        // Recurse with same segments - field access will be handled by object branch
                        query_segments(py, tape, current_idx, segments, results)?;
                        current_idx = tape.skip_value(current_idx).map_err(|e| {
                            PyErr::new::<JSONDecodeError, _>(format!("Skip error: {e}"))
                        })?;
                    }
                }
            }
        }
        _ => {
            // Primitive value - no further navigation possible
        }
    }

    Ok(())
}

/// Recursive descent: find all occurrences of a field at any depth
fn query_recursive_descent<S: AsRef<[u8]>>(
    py: Python<'_>,
    tape: &DsonTape<S>,
    index: usize,
    target_field: &str,
    results: &Bound<'_, PyList>,
) -> PyResult<()> {
    use simd_json::value::tape::Node;

    let nodes = tape.nodes();
    if index >= nodes.len() {
        return Ok(());
    }

    match &nodes[index] {
        Node::Object { len, .. } => {
            let mut current_idx = index + 1;
            for _ in 0..*len {
                if current_idx >= nodes.len() {
                    break;
                }

                if let Node::String(key) = &nodes[current_idx] {
                    let key_matches = *key == target_field;
                    current_idx += 1;

                    if current_idx < nodes.len() {
                        // If key matches, collect the value
                        if key_matches {
                            let value = tape_node_to_py(py, tape, current_idx)?;
                            results.append(value)?;
                        }

                        // Also recurse into this value to find nested matches
                        query_recursive_descent(py, tape, current_idx, target_field, results)?;

                        current_idx = tape.skip_value(current_idx).map_err(|e| {
                            PyErr::new::<JSONDecodeError, _>(format!("Skip error: {e}"))
                        })?;
                    }
                } else {
                    current_idx += 1;
                }
            }
        }
        Node::Array { len, .. } => {
            let mut current_idx = index + 1;
            for _ in 0..*len {
                if current_idx >= nodes.len() {
                    break;
                }
                // Recurse into array elements
                query_recursive_descent(py, tape, current_idx, target_field, results)?;
                current_idx = tape
                    .skip_value(current_idx)
                    .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Skip error: {e}")))?;
            }
        }
        _ => {
            // Primitive - no recursion needed
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    // Python-side tests in tests/test_tape.py
}
